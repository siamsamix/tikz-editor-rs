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

use crate::options::{parse_style_value_as_option_list, OptionEntry, OptionListAst};
use crate::scene::{
    ArrowTip, FontFamily, Paint, PathStyle, TextAlign, TextStyle, DEFAULT_TEXT_FONT_SIZE_PT,
};
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

    /// 1-based line and column (in chars) of [`Self::span`] `.from`.    /// 1-based line and column (in chars) of [`Self::span`] `.from`.
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

/// Option keys that legacy accepts but the port does not (yet) implement.
/// They are silently ignored so the rest of a statement keeps rendering.
const NON_STYLE_OPTION_KEYS: &[&str] = &[
    "anchor",
    "minimum width",
    "minimum height",
    "right",
    "above",
    "node distance",
    "text width",
    "text height",
    "text depth",
    "rounded corners",
    "sharp corners",
];

/// Option flags that legacy accepts but the port does not (yet) implement.
/// They are silently ignored so the rest of a statement keeps rendering.
const NON_STYLE_OPTION_FLAGS: &[&str] = &["circle", "ellipse", "rectangle"];

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
    /// `to`/curve out angle (degrees), set by `out=` / `bend`.
    pub to_out: Option<f64>,
    /// `to`/curve in angle (degrees), set by `in=` / `bend`.
    pub to_in: Option<f64>,
    /// `to`/curve looseness, set by `looseness=`.
    pub to_looseness: Option<f64>,
    /// `to`/curve out looseness, set by `out looseness=`.
    pub to_out_looseness: Option<f64>,
    /// `to`/curve in looseness, set by `in looseness=`.
    pub to_in_looseness: Option<f64>,
    /// `to`/curve signed bend angle: positive = `bend left`, negative = `bend right`.
    pub to_bend: Option<f64>,
    /// Whether `bend left`/`bend right` made the curve relative to the chord.
    pub to_relative: bool,
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
    /// Style definitions: name -> option list (for `.style` expansion).
    pub style_definitions: std::collections::HashMap<String, Vec<crate::options::OptionEntry>>,
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
            to_out: None,
            to_in: None,
            to_looseness: None,
            to_out_looseness: None,
            to_in_looseness: None,
            to_bend: None,
            to_relative: false,
            font_family: FontFamily::Serif,
            font_size: crate::units::DEFAULT_TEXT_FONT_SIZE_PT,
            font_weight: false,
            font_style: false,
            text_align: TextAlign::Center,
            text_fill: Paint::Black,
            text_opacity: 1.0,
            style_definitions: std::collections::HashMap::new(),
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
                OptionEntry::Unknown(unknown) => self.apply_unknown(unknown, issues),
            }
        }
    }

    fn apply_flag(&mut self, key: &str, span: Span, issues: &mut Vec<Issue>) {
        let key = key.strip_prefix("/tikz/").unwrap_or(key);

        // Expand a named style definition (e.g. `\node[box]` where `box/.style`
        // was defined earlier). Checked before colours so a defined style name
        // takes precedence over a same-named colour flag.
        if let Some(entries) = self.style_definitions.get(key) {
            let entries = entries.clone();
            for entry in entries {
                match entry {
                    crate::options::OptionEntry::KV(kv) => {
                        self.apply_kv(&kv.key, &kv.value_raw, kv.span, issues);
                    }
                    crate::options::OptionEntry::Flag(flag) => {
                        self.apply_flag(&flag.key, flag.span, issues);
                    }
                    crate::options::OptionEntry::Unknown(_) => {}
                }
            }
            return;
        }

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
            "bend left" | "bend right" => {
                // `bend left`/`bend right` default to a 30° bend, made relative
                // to the chord heading by `to` curve resolution. `bend angle=`
                // overrides the magnitude before resolution.
                let angle = self.to_bend.unwrap_or(30.0);
                self.to_bend = if key == "bend left" {
                    Some(angle.abs())
                } else {
                    Some(-angle.abs())
                };
                self.to_relative = true;
            }
            _ => {
                // Check for arrow shorthand: ->, <-, <->, -|, |-
                let (start_tip, end_tip) = parse_arrow_shorthand(key);
                if let Some(kind) = &start_tip {
                    self.style.arrow_start = Some(ArrowTip {
                        kind: kind.clone(),
                        length: crate::units::DEFAULT_ARROW_LENGTH_PT,
                        width: crate::units::DEFAULT_ARROW_WIDTH_PT,
                    });
                }
                if let Some(kind) = &end_tip {
                    self.style.arrow_end = Some(ArrowTip {
                        kind: kind.clone(),
                        length: crate::units::DEFAULT_ARROW_LENGTH_PT,
                        width: crate::units::DEFAULT_ARROW_WIDTH_PT,
                    });
                }
                if start_tip.is_none() && end_tip.is_none() {
                    // Arrow side specs: `>=Stealth`, `<latex`, `>`.
                    if let Some(kind) = parse_arrow_side_spec(key) {
                        let arrow = ArrowTip {
                            kind,
                            length: crate::units::DEFAULT_ARROW_LENGTH_PT,
                            width: crate::units::DEFAULT_ARROW_WIDTH_PT,
                        };
                        if key.starts_with('<') {
                            self.style.arrow_start = Some(arrow);
                        } else {
                            self.style.arrow_end = Some(arrow);
                        }
                        return;
                    }
                }
                if start_tip.is_none() && end_tip.is_none() {
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
                }
                if start_tip.is_none() && end_tip.is_none() {
                    // Flags legacy accepts but the port does not (yet) implement.
                    if NON_STYLE_OPTION_FLAGS.contains(&key) {
                        return;
                    }
                    issues.push(Issue::warning(
                        span,
                        format!("unsupported option flag `{key}`"),
                    ));
                }
            }
        }
    }

    /// Handle an unclassified option token: TeX font commands (`\small`),
    /// arrow side specs (`>=Stealth`), and `/tikz/`-prefixed keys all arrive
    /// here as `Unknown` because they are not alphabetic words.
    fn apply_unknown(&mut self, unknown: &crate::options::UnknownEntry, issues: &mut Vec<Issue>) {
        let raw = unknown.raw.trim();
        let text = raw.trim_start_matches('\\');
        let lower = text.to_ascii_lowercase();

        // TeX font size command: \small, \Large, etc.
        if raw.starts_with('\\') {
            let text = raw.trim_start_matches('\\');
            let font_size_factor = match text {
                "tiny" => 0.5,
                "scriptsize" => 0.7,
                "footnotesize" => 0.8,
                "small" => 0.9,
                "normalsize" => 1.0,
                "large" => 1.2,
                "Large" => 1.44,
                "LARGE" => 1.728,
                "huge" => 2.074,
                "Huge" => 2.488,
                _ => 1.0,
            };
            self.font_size = DEFAULT_TEXT_FONT_SIZE_PT * font_size_factor;
            return;
        }

        // Arrow side spec: `>=Stealth`, `<latex`, `>`, `<bar`.
        if lower.starts_with('>') || lower.starts_with('<') {
            if let Some(kind) = parse_arrow_side_spec(&lower) {
                let arrow = ArrowTip {
                    kind,
                    length: crate::units::DEFAULT_ARROW_LENGTH_PT,
                    width: crate::units::DEFAULT_ARROW_WIDTH_PT,
                };
                if lower.starts_with('<') {
                    self.style.arrow_start = Some(arrow);
                } else {
                    self.style.arrow_end = Some(arrow);
                }
                return;
            }
        }

        // Strip a `/tikz/` prefix and retry as a flag (e.g. `/tikz/blue`).
        let stripped = lower.strip_prefix("/tikz/").unwrap_or(&lower);
        self.apply_flag(stripped, unknown.span, issues);
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
            "out" | "in" | "looseness" | "out looseness" | "in looseness" | "bend angle"
            | "bend left" | "bend right" => match eval_number(value_trimmed) {
                Ok(degrees) => match key {
                    "out" => self.to_out = Some(degrees),
                    "in" => self.to_in = Some(degrees),
                    "looseness" => self.to_looseness = Some(degrees),
                    "out looseness" => self.to_out_looseness = Some(degrees),
                    "in looseness" => self.to_in_looseness = Some(degrees),
                    "bend angle" => self.to_bend = Some(degrees),
                    "bend left" => {
                        self.to_bend = Some(degrees.abs());
                        self.to_relative = true;
                    }
                    "bend right" => {
                        self.to_bend = Some(-degrees.abs());
                        self.to_relative = true;
                    }
                    _ => {}
                },
                Err(error) => issues.push(Issue::warning(span, format!("{key}: {error}"))),
            },
            "relative" => match parse_relative_boolean(value_trimmed) {
                Ok(value) => self.to_relative = value,
                Err(error) => issues.push(Issue::warning(span, format!("relative: {error}"))),
            },
            // Text font size commands: \tiny, \small, \Large, etc.
            key if key.starts_with('\\') => {
                let font_size_factor = match key {
                    "\\tiny" => 0.5,
                    "\\scriptsize" => 0.7,
                    "\\footnotesize" => 0.8,
                    "\\small" => 0.9,
                    "\\normalsize" => 1.0,
                    "\\large" => 1.2,
                    "\\Large" => 1.44,
                    "\\LARGE" => 1.728,
                    "\\huge" => 2.074,
                    "\\Huge" => 2.488,
                    _ => 1.0,
                };
                self.font_size = DEFAULT_TEXT_FONT_SIZE_PT * font_size_factor;
            }
            "text width" => match eval_len(value_trimmed, Unit::Cm) {
                Ok(width) => {
                    // Text width affects text box width but not the SVG output directly;
                    // store it for potential bounding box calculations.
                    let _ = width;
                }
                Err(error) => issues.push(Issue::warning(span, format!("text width: {error}"))),
            },
            "text height" => match eval_len(value_trimmed, Unit::Cm) {
                Ok(_) => {}
                Err(error) => issues.push(Issue::warning(span, format!("text height: {error}"))),
            },
            "text depth" => match eval_len(value_trimmed, Unit::Cm) {
                Ok(_) => {}
                Err(error) => issues.push(Issue::warning(span, format!("text depth: {error}"))),
            },
            "align" => match value_trimmed.to_ascii_lowercase().as_str() {
                "left" | "flush left" => self.text_align = TextAlign::Left,
                "center" | "flush center" => self.text_align = TextAlign::Center,
                "right" | "flush right" => self.text_align = TextAlign::Right,
                _ => issues.push(Issue::warning(
                    span,
                    format!("unsupported align value `{value_trimmed}`"),
                )),
            },
            "font" => {
                // Handle font family, weight, style specifications like \small, \bfseries, \itshape
                let value_lower = value_trimmed.to_ascii_lowercase();
                if value_lower.contains("bf") || value_lower == "bfseries" {
                    self.font_weight = true;
                }
                if value_lower.contains("it") || value_lower == "itshape" {
                    self.font_style = true;
                }
                // Font size commands are handled by the key.starts_with('\\') branch above
                // but font=\small passes through here too, so handle it:
                if value_lower.contains("small") {
                    self.font_size = DEFAULT_TEXT_FONT_SIZE_PT * 0.9;
                }
                if value_lower.contains("large") && !value_lower.contains("small") {
                    self.font_size = DEFAULT_TEXT_FONT_SIZE_PT * 1.2;
                }
                if value_lower.contains("large") && value_lower.contains("large") {
                    // \Large (capital L) - double large
                    self.font_size = DEFAULT_TEXT_FONT_SIZE_PT * 1.44;
                }
            }
            // Arrow tip: `>=Stealth`, `->stealth`, `stealth`, `latex`, etc.
            key if key.starts_with(">") || key.starts_with("<") => {
                apply_arrow_spec(self, key, value_trimmed, span, issues);
            }
            _ => {
                let key_stripped = key.strip_prefix("/tikz/").unwrap_or(key);

                // `.style` / `.append style` definitions: store the option list
                // under a name so later references (e.g. `\node[box]`) expand it.
                if let Some(name) = key_stripped.strip_suffix("/.style") {
                    let name = name.trim();
                    if !name.is_empty() {
                        if let Some(list) =
                            parse_style_value_as_option_list(value_trimmed, span.from)
                        {
                            self.style_definitions
                                .insert(name.to_string(), list.entries);
                        }
                    }
                    return;
                }
                if let Some(name) = key_stripped.strip_suffix("/.append style") {
                    let name = name.trim();
                    if !name.is_empty() {
                        if let Some(list) =
                            parse_style_value_as_option_list(value_trimmed, span.from)
                        {
                            self.style_definitions
                                .entry(name.to_string())
                                .or_default()
                                .extend(list.entries);
                        }
                    }
                    return;
                }

                // Keys legacy accepts but the port does not (yet) implement:
                // silently accepted so the rest of the statement keeps rendering.
                if NON_STYLE_OPTION_KEYS.contains(&key_stripped) {
                    return;
                }

                // Check if this key matches a defined style and expand it.
                if let Some(style_entries) = self.style_definitions.get(key_stripped) {
                    let entries = style_entries.clone();
                    for entry in entries {
                        match entry {
                            crate::options::OptionEntry::KV(kv) => {
                                self.apply_kv(&kv.key, &kv.value_raw, kv.span, issues);
                            }
                            crate::options::OptionEntry::Flag(flag) => {
                                self.apply_flag(&flag.key, flag.span, issues);
                            }
                            crate::options::OptionEntry::Unknown(_) => {
                                // Skip unknown entries in style definitions
                            }
                        }
                    }
                    return;
                }

                issues.push(Issue::warning(
                    span,
                    format!("unsupported option `{key_stripped}=...`"),
                ));
            }
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

/// Parse a boolean option value (`true`/`false`/`yes`/`no`/`on`/`off`/`1`/`0`).
/// Empty value means `true` (legacy `relative` with no value is true).
pub fn parse_relative_boolean(raw: &str) -> Result<bool, String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(true);
    }
    match trimmed.as_str() {
        "true" | "yes" | "on" | "1" => Ok(true),
        "false" | "no" | "off" | "0" => Ok(false),
        other => Err(format!("expected a boolean, got `{other}`")),
    }
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
pub fn parse_rotate_around(raw: &str) -> Result<(f64, Vec2), String> {
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

/// Parse an arrow specification like `>=Stealth`, `->`, `<-`, `stealth`, `latex`.
///
/// Returns the arrow tip kind (e.g. "stealth", "latex") or None if not an arrow spec.
fn parse_arrow_spec(raw: &str) -> Option<String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    // Strip leading/trailing dashes and angle brackets for the base name
    let trimmed = trimmed.trim_start_matches('-').trim_start_matches('>');
    let trimmed = trimmed.trim_end_matches('-').trim_end_matches('<');
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Known arrow tip names (from legacy Cobalt Strike arrow types)
    const ARROW_NAMES: &[&str] = &[
        "stealth",
        "latex",
        "triangle",
        "kite",
        "to",
        "implies",
        "rightarrow",
        "square",
        "circle",
        "bracket",
        "parenthesis",
        "diamond",
        "rectangle",
        "ellipse",
        "cap",
    ];
    if ARROW_NAMES.contains(&trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

/// Parse an arrow shorthand like `->`, `<-`, `<->`, `-|`, `|-`.
///
/// Returns (start_tip, end_tip) where each is the arrow tip kind or None.
fn parse_arrow_shorthand(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    match trimmed {
        "->" => (None, Some("stealth".to_string())),
        "<-" => (Some("stealth".to_string()), None),
        "<->" => (Some("stealth".to_string()), Some("stealth".to_string())),
        "-|" => (None, Some("bar".to_string())),
        "|-" => (Some("bar".to_string()), None),
        _ => (None, None),
    }
}

/// Parse an arrow side specification: `>` / `>=` / `<` / `<=` with an arrow
/// tip name, e.g. `>=Stealth`, `<latex`, `>`. Returns the arrow tip kind or
/// None when the token is not an arrow side spec.
///
/// Legacy lower-cases option keys before dispatch, so `key` is already lower.
fn parse_arrow_side_spec(key: &str) -> Option<String> {
    // Strip leading `<`/`>` and any `=`/`>`/`<` operators, then the arrow name.
    let trimmed = key.trim_start_matches(['<', '>', '=']).trim();
    let trimmed = trimmed.trim_end_matches(['<', '>', '=']);
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return None;
    }
    const ARROW_NAMES: &[&str] = &[
        "stealth",
        "latex",
        "triangle",
        "kite",
        "to",
        "implies",
        "rightarrow",
        "square",
        "circle",
        "bracket",
        "parenthesis",
        "diamond",
        "rectangle",
        "ellipse",
        "cap",
    ];
    if ARROW_NAMES.contains(&trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

/// Apply an arrow tip specification to the style state.
fn apply_arrow_spec(
    state: &mut StyleState,
    key: &str,
    value: &str,
    span: Span,
    issues: &mut Vec<Issue>,
) {
    // Handle `>=Stealth`, `->stealth`, etc.
    let kind = parse_arrow_spec(value);
    if let Some(kind) = kind {
        let arrow = ArrowTip {
            kind,
            length: crate::units::DEFAULT_ARROW_LENGTH_PT,
            width: crate::units::DEFAULT_ARROW_WIDTH_PT,
        };
        if key == ">" || key.ends_with(">") {
            state.style.arrow_end = Some(arrow);
        } else if key == "<" || key.ends_with("<") {
            state.style.arrow_start = Some(arrow);
        } else {
            // Default: apply to end
            state.style.arrow_end = Some(arrow);
        }
        return;
    }
    issues.push(Issue::warning(
        span,
        format!("unsupported arrow specification `{key}={value}`"),
    ));
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

    /// Legacy option application order: `dotted` reads the line width at the
    /// point it appears, so `line width=2pt, dotted` uses `2pt` while
    /// `dotted, line width=2pt` uses the default `0.4pt`.
    #[test]
    fn dotted_reads_line_width_at_apply_time() {
        let (state, _) = applied("[line width=2pt, dotted]");
        assert_eq!(state.style.dash, Some(vec![2.0, 2.0]));
        let (state, _) = applied("[dotted, line width=2pt]");
        assert_eq!(state.style.dash, Some(vec![0.4, 2.0]));
    }

    /// Font size commands scale `DEFAULT_TEXT_FONT_SIZE_PT` (\small → 0.9×).
    #[test]
    fn font_size_commands_scale_default_size() {
        let (state, _) = applied("[\\small]");
        assert!((state.font_size - DEFAULT_TEXT_FONT_SIZE_PT * 0.9).abs() < 1e-9);
        let (state, _) = applied("[\\Large]");
        assert!((state.font_size - DEFAULT_TEXT_FONT_SIZE_PT * 1.44).abs() < 1e-9);
        let (state, _) = applied("[\\normalsize]");
        assert!((state.font_size - DEFAULT_TEXT_FONT_SIZE_PT).abs() < 1e-9);
    }

    /// `font=` sets weight/style/size from a font spec string.
    #[test]
    fn font_key_sets_weight_and_style() {
        let (state, _) = applied("[font=\\bfseries]");
        assert!(state.font_weight);
        assert!(!state.font_style);
        let (state, _) = applied("[font=\\itshape]");
        assert!(!state.font_weight);
        assert!(state.font_style);
        let (state, _) = applied("[font=\\small]");
        assert!((state.font_size - DEFAULT_TEXT_FONT_SIZE_PT * 0.9).abs() < 1e-9);
    }

    /// `color=gray` paints stroke and fill when both are already set.
    #[test]
    fn color_touches_stroke_and_fill() {
        let (state, _) = applied("[fill=red, color=gray]");
        assert_eq!(state.style.stroke, Paint::Rgb(0x80, 0x80, 0x80));
        assert_eq!(state.style.fill, Paint::Rgb(0x80, 0x80, 0x80));
    }

    /// `>=Stealth` sets the end arrow tip; `<latex` sets the start arrow tip.
    #[test]
    fn arrow_side_specs_set_start_and_end() {
        let (state, _) = applied("[>=Stealth]");
        assert_eq!(
            state.style.arrow_end.as_ref().map(|a| a.kind.as_str()),
            Some("stealth")
        );
        let (state, _) = applied("[<latex]");
        assert_eq!(
            state.style.arrow_start.as_ref().map(|a| a.kind.as_str()),
            Some("latex")
        );
    }

    /// `->` / `->-` shorthand sets start and end tips.
    #[test]
    fn arrow_shorthand_sets_start_and_end() {
        let (state, _) = applied("[<->]");
        assert_eq!(
            state.style.arrow_start.as_ref().map(|a| a.kind.as_str()),
            Some("stealth")
        );
        assert_eq!(
            state.style.arrow_end.as_ref().map(|a| a.kind.as_str()),
            Some("stealth")
        );
    }

    /// A named style defined via `/.style` expands when referenced as a flag.
    #[test]
    fn style_definition_expands_on_reference() {
        let list = parse_option_list_raw("[box/.style={draw, fill=orange!30}]", 0);
        let mut state = StyleState::default();
        let mut issues = Vec::new();
        state.apply(&list, &mut issues);
        assert!(issues.is_empty(), "{issues:?}");
        assert!(state.style_definitions.contains_key("box"));

        // Reuse the same state so the definition persists.
        let list = parse_option_list_raw("[box]", 0);
        state.apply(&list, &mut issues);
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(state.style.stroke, Paint::Black); // from `draw`
        assert_eq!(state.style.fill, Paint::Rgb(0xff, 0xd9, 0xb3)); // orange!30
    }

    /// `/.append style` accumulates onto an existing definition.
    #[test]
    fn append_style_accumulates() {
        let mut state = StyleState::default();
        let mut issues = Vec::new();
        state.apply(
            &parse_option_list_raw("[box/.style={draw}]", 0),
            &mut issues,
        );
        state.apply(
            &parse_option_list_raw("[box/.append style={fill=blue}]", 0),
            &mut issues,
        );
        let entries = state.style_definitions.get("box").unwrap();
        assert_eq!(entries.len(), 2);
        let keys: Vec<&str> = entries.iter().map(|e| e.raw()).collect();
        assert!(keys.contains(&"draw"));
        assert!(keys.contains(&"fill=blue"));
    }

    /// `/tikz/` prefix is stripped before matching.
    #[test]
    fn tikz_prefix_is_stripped() {
        let (state, _) = applied("[/tikz/very thick]");
        assert_eq!(state.style.stroke_width, 1.2);
        let (state, _) = applied("[/tikz/blue]");
        assert_eq!(state.style.stroke, Paint::Rgb(0, 0, 255));
    }

    /// Keys legacy accepts but does not apply (node-positioning/geometry keys)
    /// are ignored without warning.
    #[test]
    fn non_style_option_keys_are_ignored() {
        for key in ["minimum width=8mm", "right=12mm", "anchor=north east"] {
            let (_, issues) = applied(&format!("[{key}]"));
            assert!(issues.is_empty(), "{key} produced {issues:?}");
        }
    }

    /// Flags legacy accepts but does not apply are ignored without warning.
    #[test]
    fn non_style_option_flags_are_ignored() {
        for key in ["circle", "ellipse", "rectangle"] {
            let (_, issues) = applied(&format!("[{key}]"));
            assert!(issues.is_empty(), "{key} produced {issues:?}");
        }
    }
}
