//! Style state and option-list application.
//!
//! Port of the style half of the legacy `semantic/style/*` modules: flags
//! (`dashed`, `thick`, colour names, ...) from `apply-flag.ts` and key/value
//! options (`draw=`, `fill=`, `line width=`, `shift=`, ...) from
//! `apply-kv.ts`. Only the subset the corpus needs is implemented; anything
//! else becomes a warning so the canvas keeps rendering the rest of the
//! statement instead of failing the document.
//!
//! ```
//! use tikz_core::options::parse_option_list_raw;
//! use tikz_core::semantic::style::StyleState;
//! use tikz_core::scene::Paint;
//!
//! let list = parse_option_list_raw("[very thick, dashed, blue!60!black]", 0);
//! let mut state = StyleState::default();
//! let mut issues = Vec::new();
//! state.apply(&list, &mut issues);
//! assert!(issues.is_empty());
//! assert_eq!(state.style.stroke_width, 1.2);
//! assert_eq!(state.style.dash, Some(vec![3.0, 3.0]));
//! assert_eq!(state.style.stroke, Paint::Rgb(0x00, 0x00, 0x99));
//! ```

use crate::options::{OptionEntry, OptionListAst};
use crate::scene::{FontFamily, Paint, PathStyle, TextAlign, TextStyle};
use crate::semantic::color::resolve_color;
use crate::semantic::expr::{eval_len, eval_number, Unit};
use crate::semantic::transform::Transform;
use crate::span::Span;
use crate::units::Vec2;

/// Severity of a semantic issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The document cannot be rendered.
    Error,
    /// The document renders, but a requested feature was not applied.
    Warning,
}

/// A problem found while evaluating a statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    /// Severity.
    pub severity: Severity,
    /// Offending region of the source.
    pub span: Span,
    /// Human-readable reason.
    pub message: String,
}

impl Issue {
    /// An issue that blocks rendering.
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            span,
            message: message.into(),
        }
    }

    /// An issue that only degrades rendering.
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            span,
            message: message.into(),
        }
    }

    /// Whether this issue blocks rendering.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// 1-based line and column (in chars) of [`Self::span`] `.from`.
    ///
    /// Byte offsets that fall inside a multi-byte character clamp to that
    /// character's start, so this never panics on malformed spans.
    ///
    /// ```
    /// use tikz_core::span::Span;
    /// use tikz_core::semantic::style::Issue;
    /// let at = |from| Issue::error(Span::new(from, from + 1), "bad");
    /// assert_eq!(at(6).line_col("ab\ncd\nef"), (3, 1));
    /// assert_eq!(at(3).line_col("ó\ncd"), (2, 1));
    /// assert_eq!(at(1).line_col("ó\ncd"), (1, 1)); // inside `ó`: clamps to its start
    /// ```
    pub fn line_col(&self, source: &str) -> (usize, usize) {
        let mut end = self.span.from.min(source.len());
        while end > 0 && !source.is_char_boundary(end) {
            end -= 1;
        }
        let before = &source[..end];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        (line, col)
    }
}

/// Paint state plus the parts of the legacy style state that affect geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleState {
    /// Stroke/fill paint, dash and opacity.
    pub style: PathStyle,
    /// Accumulated transform for the statement.
    pub transform: Transform,
    /// Legacy `drawExplicit`: whether `draw=` was given a colour.
    pub draw_explicit: bool,
    /// `radius=` value in pt, if given.
    pub radius: Option<f64>,
    /// Whether `radius=` had no explicit unit (so it scales with the frame).
    pub radius_uses_basis: bool,
    /// `x radius=` value in pt.
    pub x_radius: Option<f64>,
    /// Whether `x radius=` had no explicit unit.
    pub x_radius_uses_basis: bool,
    /// `y radius=` value in pt.
    pub y_radius: Option<f64>,
    /// Whether `y radius=` had no explicit unit.
    pub y_radius_uses_basis: bool,
    /// `start angle=` in degrees (arcs).
    pub start_angle: Option<f64>,
    /// `end angle=` in degrees (arcs).
    pub end_angle: Option<f64>,
    /// `step=` for `grid`, with its explicit-unit flag.
    pub grid_step: Option<(f64, bool)>,
    /// Resolved text style defaults inherited into `\node` text.
    pub font_family: FontFamily,
    /// Text font size in pt.
    pub font_size: f64,
    /// Bold weight.
    pub font_weight: bool,
    /// Italic style.
    pub font_style: bool,
    /// Horizontal alignment.
    pub text_align: TextAlign,
    /// Text fill color.
    pub text_fill: Paint,
    /// Text fill opacity.
    pub text_opacity: f64,
}

impl Default for StyleState {
    fn default() -> Self {
        Self {
            style: PathStyle::default(),
            transform: Transform::default(),
            draw_explicit: false,
            radius: None,
            radius_uses_basis: true,
            x_radius: None,
            x_radius_uses_basis: true,
            y_radius: None,
            y_radius_uses_basis: true,
            start_angle: None,
            end_angle: None,
            grid_step: None,
            font_family: FontFamily::Serif,
            font_size: crate::units::DEFAULT_TEXT_FONT_SIZE_PT,
            font_weight: false,
            font_style: false,
            text_align: TextAlign::Center,
            text_fill: Paint::Black,
            text_opacity: 1.0,
        }
    }
}

impl StyleState {
    /// Apply one option list in order.
    pub fn apply(&mut self, list: &OptionListAst, issues: &mut Vec<Issue>) {
        for entry in &list.entries {
            match entry {
                OptionEntry::KV(kv) => self.apply_kv(&kv.key, &kv.value_raw, kv.span, issues),
                OptionEntry::Flag(flag) => self.apply_flag(&flag.key, flag.span, issues),
                OptionEntry::Unknown(unknown) => issues.push(Issue::warning(
                    unknown.span,
                    format!("unsupported option `{}`", unknown.raw.trim()),
                )),
            }
        }
    }

    fn apply_flag(&mut self, key: &str, span: Span, issues: &mut Vec<Issue>) {
        match key {
            "ultra thin" => self.style.stroke_width = 0.1,
            "very thin" => self.style.stroke_width = 0.2,
            "thin" => self.style.stroke_width = 0.4,
            "semithick" => self.style.stroke_width = 0.6,
            "thick" => self.style.stroke_width = 0.8,
            "very thick" => self.style.stroke_width = 1.2,
            "ultra thick" => self.style.stroke_width = 1.6,
            "solid" => {
                self.style.dash = None;
                self.style.dash_offset = 0.0;
            }
            "dashed" => self.style.dash = Some(vec![3.0, 3.0]),
            "densely dashed" => self.style.dash = Some(vec![4.0, 2.0]),
            "loosely dashed" => self.style.dash = Some(vec![6.0, 4.0]),
            "dotted" => self.style.dash = Some(vec![self.style.stroke_width, 2.0]),
            "densely dotted" => self.style.dash = Some(vec![self.style.stroke_width, 1.0]),
            "loosely dotted" => self.style.dash = Some(vec![self.style.stroke_width, 4.0]),
            "even odd rule" => self.style.even_odd = true,
            "nonzero rule" => self.style.even_odd = false,
            "draw" => self.draw_explicit = true,
            "help lines" => {
                self.style.stroke = Paint::Rgb(0x80, 0x80, 0x80);
                self.style.stroke_width = 0.2;
            }
            _ => {
                if let Ok(paint) = resolve_color(key) {
                    // Legacy colour flags touch the stroke only when a stroke
                    // is already visible, and the fill only when one is set.
                    if self.draw_explicit || self.style.stroke != Paint::None {
                        self.style.stroke = paint;
                    }
                    if self.style.fill != Paint::None {
                        self.style.fill = paint;
                    }
                    return;
                }
                issues.push(Issue::warning(
                    span,
                    format!("unsupported option flag `{key}`"),
                ));
            }
        }
    }

    fn apply_kv(&mut self, key: &str, value: &str, span: Span, issues: &mut Vec<Issue>) {
        let value_trimmed = value.trim();
        match key {
            "fill" => match resolve_color(value_trimmed) {
                Ok(paint) => self.style.fill = paint,
                Err(error) => issues.push(Issue::warning(span, format!("fill: {error}"))),
            },
            "draw" => {
                if value_trimmed.eq_ignore_ascii_case("none") {
                    self.style.stroke = Paint::None;
                    self.draw_explicit = false;
                } else {
                    match resolve_color(value_trimmed) {
                        Ok(paint) => {
                            self.style.stroke = paint;
                            self.draw_explicit = true;
                        }
                        Err(error) => issues.push(Issue::warning(span, format!("draw: {error}"))),
                    }
                }
            }
            "color" => {
                if value_trimmed.eq_ignore_ascii_case("none") {
                    if self.draw_explicit || self.style.stroke != Paint::None {
                        self.style.stroke = Paint::None;
                    }
                    if self.style.fill != Paint::None {
                        self.style.fill = Paint::None;
                    }
                } else {
                    match resolve_color(value_trimmed) {
                        Ok(paint) => {
                            if self.draw_explicit || self.style.stroke != Paint::None {
                                self.style.stroke = paint;
                            }
                            if self.style.fill != Paint::None {
                                self.style.fill = paint;
                            }
                        }
                        Err(error) => issues.push(Issue::warning(span, format!("color: {error}"))),
                    }
                }
            }
            "line width" => match eval_len(value_trimmed, Unit::Pt) {
                Ok(width) if width >= 0.0 => self.style.stroke_width = width,
                Ok(_) => issues.push(Issue::warning(span, "negative line width")),
                Err(error) => issues.push(Issue::warning(span, format!("line width: {error}"))),
            },
            "opacity" => match clamp01(eval_number(value_trimmed)) {
                Some(value) => {
                    self.style.stroke_opacity = value;
                    self.style.fill_opacity = value;
                }
                None => issues.push(Issue::warning(span, "invalid opacity")),
            },
            "draw opacity" => match clamp01(eval_number(value_trimmed)) {
                Some(value) => self.style.stroke_opacity = value,
                None => issues.push(Issue::warning(span, "invalid draw opacity")),
            },
            "fill opacity" => match clamp01(eval_number(value_trimmed)) {
                Some(value) => self.style.fill_opacity = value,
                None => issues.push(Issue::warning(span, "invalid fill opacity")),
            },
            "transparent" => {
                self.style.stroke_opacity = 0.0;
                self.style.fill_opacity = 0.0;
            }
            "dash pattern" | "dash" => match parse_dash_pattern(value_trimmed) {
                Some(pattern) => self.style.dash = Some(pattern),
                None => issues.push(Issue::warning(
                    span,
                    format!("dash pattern: `{value_trimmed}`"),
                )),
            },
            "dash phase" => match eval_len(value_trimmed, Unit::Pt) {
                Ok(offset) => self.style.dash_offset = offset,
                Err(error) => issues.push(Issue::warning(span, format!("dash phase: {error}"))),
            },
            "scale" => match eval_number(value_trimmed) {
                Ok(factor) => {
                    if self.transform.scale(factor, factor).is_err() {
                        issues.push(Issue::warning(span, "invalid scale"));
                    }
                }
                Err(error) => issues.push(Issue::warning(span, format!("scale: {error}"))),
            },
            "xscale" | "yscale" => match eval_number(value_trimmed) {
                Ok(factor) => {
                    let result = if key == "xscale" {
                        self.transform.scale(factor, 1.0)
                    } else {
                        self.transform.scale(1.0, factor)
                    };
                    if result.is_err() {
                        issues.push(Issue::warning(span, format!("invalid {key}")));
                    }
                }
                Err(error) => issues.push(Issue::warning(span, format!("{key}: {error}"))),
            },
            "xshift" | "yshift" => match eval_len(value_trimmed, Unit::Cm) {
                Ok(length) => {
                    let delta = if key == "xshift" {
                        Vec2::new(length, 0.0)
                    } else {
                        Vec2::new(0.0, length)
                    };
                    self.transform.shift(delta);
                }
                Err(error) => issues.push(Issue::warning(span, format!("{key}: {error}"))),
            },
            "shift" => match parse_point(value_trimmed) {
                Ok(point) => self.transform.shift(point),
                Err(error) => issues.push(Issue::warning(span, format!("shift: {error}"))),
            },
            "rotate" => match eval_number(value_trimmed) {
                Ok(degrees) => {
                    if self.transform.rotate(degrees, Vec2::new(0.0, 0.0)).is_err() {
                        issues.push(Issue::warning(span, "invalid rotate"));
                    }
                }
                Err(error) => issues.push(Issue::warning(span, format!("rotate: {error}"))),
            },
            "rotate around" => match parse_rotate_around(value_trimmed) {
                Ok((degrees, pivot)) => {
                    if self.transform.rotate(degrees, pivot).is_err() {
                        issues.push(Issue::warning(span, "invalid rotate around"));
                    }
                }
                Err(error) => issues.push(Issue::warning(span, format!("rotate around: {error}"))),
            },
            "radius" => match eval_len_flagged(value_trimmed, Unit::Cm) {
                Ok((length, explicit)) => {
                    self.radius = Some(length);
                    self.radius_uses_basis = !explicit;
                }
                Err(error) => issues.push(Issue::warning(span, format!("radius: {error}"))),
            },
            "x radius" => match eval_len_flagged(value_trimmed, Unit::Cm) {
                Ok((length, explicit)) => {
                    self.x_radius = Some(length);
                    self.x_radius_uses_basis = !explicit;
                }
                Err(error) => issues.push(Issue::warning(span, format!("x radius: {error}"))),
            },
            "y radius" => match eval_len_flagged(value_trimmed, Unit::Cm) {
                Ok((length, explicit)) => {
                    self.y_radius = Some(length);
                    self.y_radius_uses_basis = !explicit;
                }
                Err(error) => issues.push(Issue::warning(span, format!("y radius: {error}"))),
            },
            "step" => match eval_len_flagged(value_trimmed, Unit::Cm) {
                Ok(step) => self.grid_step = Some(step),
                Err(error) => issues.push(Issue::warning(span, format!("step: {error}"))),
            },
            "start angle" | "end angle" => match eval_number(value_trimmed) {
                Ok(degrees) => {
                    if key == "start angle" {
                        self.start_angle = Some(degrees);
                    } else {
                        self.end_angle = Some(degrees);
                    }
                }
                Err(error) => issues.push(Issue::warning(span, format!("{key}: {error}"))),
            },
            _ => issues.push(Issue::warning(
                span,
                format!("unsupported option `{key}=...`"),
            )),
        }
    }
}

fn clamp01(value: Result<f64, String>) -> Option<f64> {
    value
        .ok()
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 1.0))
}

impl StyleState {
    /// Resolve the node text style from this state.
    pub fn text_style(&self) -> TextStyle {
        TextStyle {
            fill: self.text_fill,
            fill_opacity: self.text_opacity,
            font_family: self.font_family,
            font_size: self.font_size,
            font_weight: self.font_weight,
            font_style: self.font_style,
            text_align: self.text_align,
        }
    }

    /// Estimated width of a single-line text in pt (legacy `charWidth = 0.7·fontSize`).
    pub fn text_width(&self, text: &str) -> f64 {
        text.len() as f64 * 0.7 * self.font_size
    }

    /// Estimated height of wrapped lines in pt (legacy `lineHeight = 1.05·fontSize`).
    pub fn text_height(&self, lines: &[String]) -> f64 {
        if lines.is_empty() {
            return 0.0;
        }
        lines.len() as f64 * 1.05 * self.font_size
    }
}

/// Evaluate a length and report whether the source carried an explicit unit.
pub fn eval_len_flagged(raw: &str, default_unit: Unit) -> Result<(f64, bool), String> {
    let trimmed = raw
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim();
    let explicit = trimmed.chars().any(|c| c.is_ascii_alphabetic());
    Ok((eval_len(trimmed, default_unit)?, explicit))
}

/// Parse `on 3pt off 2pt` (or `3pt 2pt`) into a dash array of alternating
/// on/off lengths.
pub fn parse_dash_pattern(raw: &str) -> Option<Vec<f64>> {
    let mut values = Vec::new();
    for token in raw.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "on" | "off" => continue,
            _ => values.push(eval_len(token, Unit::Pt).ok()?),
        }
    }
    (!values.is_empty()).then_some(values)
}

/// Parse `{(x,y)}` / `(x,y)` into a canvas-space point.
pub fn parse_point(raw: &str) -> Result<Vec2, String> {
    let inner = raw
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim();
    let inner = inner.trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 2 {
        return Err(format!("expected `(x,y)`, got `{raw}`"));
    }
    Ok(Vec2::new(
        eval_len(parts[0], Unit::Cm)?,
        eval_len(parts[1], Unit::Cm)?,
    ))
}

/// Parse `45:(3,1)` into degrees and a pivot point.
fn parse_rotate_around(raw: &str) -> Result<(f64, Vec2), String> {
    let inner = raw
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim();
    let (degrees, point) = inner
        .split_once(':')
        .ok_or_else(|| format!("expected `angle:(x,y)`, got `{raw}`"))?;
    Ok((eval_number(degrees)?, parse_point(point)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::parse_option_list_raw;

    fn applied(raw: &str) -> (StyleState, Vec<Issue>) {
        let list = parse_option_list_raw(raw, 0);
        let mut state = StyleState::default();
        let mut issues = Vec::new();
        state.apply(&list, &mut issues);
        (state, issues)
    }

    #[test]
    fn width_and_dash_keywords_follow_legacy_table() {
        let (state, issues) = applied("[very thin, densely dashed]");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(state.style.stroke_width, 0.2);
        assert_eq!(state.style.dash, Some(vec![4.0, 2.0]));
        assert_eq!(state.style.half_stroke(), 0.1);
    }

    #[test]
    fn dotted_uses_the_line_width_at_apply_time() {
        let (state, _) = applied("[line width=2pt, dotted]");
        assert_eq!(state.style.dash, Some(vec![2.0, 2.0]));
        let (state, _) = applied("[dotted, line width=2pt]");
        assert_eq!(state.style.dash, Some(vec![0.4, 2.0]));
    }

    #[test]
    fn color_semantics_match_legacy_explicit_draw() {
        let (state, _) = applied("[blue]");
        assert_eq!(state.style.stroke, Paint::Rgb(0, 0, 255));
        assert_eq!(state.style.fill, Paint::None);

        let (state, _) = applied("[fill=yellow, fill opacity=.6, draw=black]");
        assert_eq!(state.style.fill, Paint::Rgb(255, 255, 0));
        assert_eq!(state.style.fill_opacity, 0.6);
        assert_eq!(state.style.stroke, Paint::Rgb(0, 0, 0));
        assert!(state.draw_explicit);

        let (state, _) = applied("[draw=none, fill=red]");
        assert_eq!(state.style.stroke, Paint::None);
        assert_eq!(state.style.fill, Paint::Rgb(255, 0, 0));

        let (state, _) = applied("[color=gray]");
        assert_eq!(state.style.stroke, Paint::Rgb(0x80, 0x80, 0x80));
    }

    #[test]
    fn transforms_accumulate_in_apply_order() {
        let (state, issues) = applied("[scale=0.6, xshift=5cm]");
        assert!(issues.is_empty(), "{issues:?}");
        let point = state.transform.apply(Vec2::new(0.0, 0.0));
        assert!((point.x - 3.0 * crate::units::CM_TO_PT).abs() < 1e-9);

        // Coordinates are centimetres: `(3,1)` is the pivot in points.
        let cm = crate::units::CM_TO_PT;
        let (state, _) = applied("[rotate around={45:(3,1)}]");
        let point = state.transform.apply(Vec2::new(2.5 * cm, 1.0 * cm));
        assert!((point.x / cm - 2.6464).abs() < 5e-4, "{point:?}");
        assert!((point.y / cm - 0.6464).abs() < 5e-4, "{point:?}");
    }

    #[test]
    fn unsupported_options_warn_instead_of_failing() {
        let (_, issues) = applied("[double, decoration={snake}]");
        assert_eq!(issues.len(), 2);
        assert!(issues
            .iter()
            .all(|issue| issue.severity == Severity::Warning));
    }

    #[test]
    fn radius_flags_track_explicit_units() {
        let (state, _) = applied("[radius=0.6]");
        assert_eq!(state.radius, Some(0.6 * crate::units::CM_TO_PT));
        assert!(state.radius_uses_basis);
        let (state, _) = applied("[radius=4pt]");
        assert_eq!(state.radius, Some(4.0));
        assert!(!state.radius_uses_basis);
    }

    #[test]
    fn dash_pattern_and_phase() {
        assert_eq!(parse_dash_pattern("on 3pt off 2pt"), Some(vec![3.0, 2.0]));
        assert_eq!(parse_dash_pattern("1pt 2pt 3pt"), Some(vec![1.0, 2.0, 3.0]));
        assert_eq!(parse_dash_pattern("on bogus"), None);
        let (state, _) = applied("[dash pattern=on 3pt off 2pt]");
        assert_eq!(state.style.dash, Some(vec![3.0, 2.0]));
    }
}
