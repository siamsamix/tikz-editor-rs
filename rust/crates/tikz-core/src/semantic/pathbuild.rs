//! Path statement evaluation: text scan -> [`SceneElement`]s.
//!
//! The legacy evaluator walks AST path items and threads a builder with
//! pending shape state (`pendingCircleCenter`, `pendingGrid`, ...). This port
//! keeps that shape — a [`Builder`] plus a [`Pending`] slot — but scans the
//! statement text directly, because the Rust front-end does not (yet) build
//! path-item AST nodes for every keyword.
//!
//! Supported: coordinates (Cartesian/polar, absolute/relative), `--`, `|-`,
//! `-|`, `cycle`, `rectangle`, `circle`, `ellipse`, `arc`, `grid`, plus the
//! shape options `[radius=...]`, `[x radius=...]`, `[y radius=...]`,
//! `[start angle=...]`, `[end angle=...]`, `[step=...]`. Unsupported
//! keywords produce issues; nothing is dropped silently.

use crate::options::parse_option_list_raw;
use crate::scene::{PathCmd, SceneElement, ScenePath};
use crate::semantic::pathscan::{
    matching, parse_arc_shorthand, parse_coordinate, parse_length, parse_point, parse_radii,
    read_word, skip_optional_groups, skip_trivia, ArcParams,
};
use crate::semantic::shapes::{emit_arc, emit_circle, emit_ellipse, emit_grid, emit_rectangle};
use crate::semantic::style::{Issue, StyleState};
use crate::span::Span;
use crate::units::Vec2;

/// Everything the scanner needs beyond the text slice.
pub struct PathContext<'a> {
    /// Whole source (spans are absolute offsets into it).
    pub source: &'a str,
    /// `data-source-id` for the elements this statement produces.
    pub source_id: String,
    /// Style state; mid-path option lists update it in place.
    pub state: StyleState,
}

/// One in-progress path.
#[derive(Default)]
pub(crate) struct Builder {
    pub(crate) cmds: Vec<PathCmd>,
    pub(crate) bbox: Option<(Vec2, Vec2)>,
    pub(crate) current: Option<Vec2>,
    /// Reference point for `+` increments; only absolute/`++` move it.
    pub(crate) base: Option<Vec2>,
    pub(crate) start: Option<Vec2>,
    /// Current point in *local* (untransformed) coordinates.
    pub(crate) local: Option<Vec2>,
    /// Local reference point for `+` increments.
    pub(crate) local_base: Option<Vec2>,
}

impl Builder {
    pub(crate) fn touch(&mut self, p: Vec2) {
        self.bbox = Some(match self.bbox {
            None => (p, p),
            Some((lo, hi)) => (
                Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
                Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
            ),
        });
    }

    fn is_drawable(&self) -> bool {
        self.cmds.iter().any(|cmd| !matches!(cmd, PathCmd::Move(_)))
    }

    /// Continue the path at `point` (`Line`), or start it (`Move`).
    pub(crate) fn advance(&mut self, point: Vec2) {
        if self.current.is_none() {
            self.cmds.push(PathCmd::Move(point));
            self.start = Some(point);
        } else {
            self.cmds.push(PathCmd::Line(point));
        }
        self.current = Some(point);
        self.touch(point);
    }

    fn reset(&mut self) {
        self.cmds.clear();
        self.bbox = None;
        self.current = None;
        self.start = None;
        self.local = None;
        self.local_base = None;
    }
}

/// Shape keyword waiting for its parameters.
#[derive(Clone, Copy, PartialEq)]
enum Pending {
    None,
    Rectangle,
    Circle,
    Ellipse,
    Arc,
    Grid,
}

/// How a coordinate is anchored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Relative {
    /// No prefix.
    Absolute,
    /// `+` — relative, the reference point stays.
    Plus,
    /// `++` — relative and the reference point moves.
    PlusPlus,
}

/// Build the elements of one path statement; `from`/`to` bound the slice
/// after the command name and the statement's leading option list.
pub fn build_path(
    ctx: &mut PathContext<'_>,
    from: usize,
    to: usize,
    issues: &mut Vec<Issue>,
) -> Vec<SceneElement> {
    let src = ctx.source;
    let mut elements = Vec::new();
    let mut builder = Builder::default();
    let mut pending = Pending::None;
    let mut at = from;

    while at < to {
        let byte = src.as_bytes()[at];
        if byte == b'%' {
            while at < to && src.as_bytes()[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if byte == b'[' {
            let Some(close) = matching(src, at, to, b'[', b']') else {
                issues.push(Issue::error(Span::new(at, to), "unterminated option list"));
                break;
            };
            let list = parse_option_list_raw(&src[at..=close], at);
            ctx.state.apply(&list, issues);
            if matches!(pending, Pending::Circle | Pending::Ellipse | Pending::Arc) {
                finish_pending_shape(&mut builder, ctx, pending, Span::new(at, close + 1), issues, &mut elements);
                pending = Pending::None;
            }
            // `grid [step=...] (corner)` keeps the pending flag: the corner
            // coordinate is what actually draws the lines.
            at = close + 1;
            continue;
        }
        if byte == b'(' {
            let Some(close) = matching(src, at, to, b'(', b')') else {
                issues.push(Issue::error(Span::new(at, to), "unterminated coordinate"));
                break;
            };
            let inner = src[at + 1..close].to_string();
            let span = Span::new(at, close + 1);
            let relative = relative_kind(src, at);
            match pending {
                Pending::Rectangle => match parse_point(&inner) {
                    Ok(corner) => emit_rectangle(&mut builder, ctx, &mut elements, corner),
                    Err(error) => issues.push(Issue::error(span, error)),
                },
                Pending::Circle => match parse_length(&inner) {
                    Ok(radius) => emit_circle(&mut builder, ctx, &mut elements, radius, span, issues),
                    Err(error) => issues.push(Issue::error(span, error)),
                },
                Pending::Ellipse => match parse_radii(&inner) {
                    Ok((rx, ry)) => emit_ellipse(&mut builder, ctx, &mut elements, rx, ry, span, issues),
                    Err(error) => issues.push(Issue::error(span, error)),
                },
                Pending::Arc => match parse_arc_shorthand(&inner) {
                    Ok(params) => emit_arc(&mut builder, ctx, params, span, issues),
                    Err(error) => issues.push(Issue::error(span, error)),
                },
                Pending::Grid => match parse_point(&inner) {
                    Ok(corner) => emit_grid(&mut builder, ctx, &mut elements, Some(corner)),
                    Err(error) => issues.push(Issue::error(span, error)),
                },
                Pending::None => match parse_coordinate(&inner) {
                    Ok(parsed) => {
                        let transform = ctx.state.transform;
                        let local = match relative {
                            Relative::Absolute => parsed,
                            Relative::Plus | Relative::PlusPlus => {
                                builder.local_base.unwrap_or(Vec2::new(0.0, 0.0)) + parsed
                            }
                        };
                        let point = transform.apply(local);
                        if relative != Relative::Plus {
                            builder.base = Some(point);
                            builder.local_base = Some(local);
                        }
                        builder.local = Some(local);
                        builder.advance(point);
                    }
                    Err(error) => issues.push(Issue::error(span, error)),
                },
            }
            pending = Pending::None;
            at = close + 1;
            continue;
        }
        if src[at..to].starts_with("--") {
            at += 2;
            continue;
        }
        if src[at..to].starts_with("|-") || src[at..to].starts_with("-|") {
            let vertical_first = src[at..to].starts_with("|-");
            let open = skip_trivia(src, at + 2, to);
            if src.as_bytes().get(open) == Some(&b'(') {
                if let Some(close) = matching(src, open, to, b'(', b')') {
                    match parse_coordinate(&src[open + 1..close]) {
                        Ok(target) => {
                            let transform = ctx.state.transform;
                            let local_from = builder.local.unwrap_or(target);
                            let local_corner = if vertical_first {
                                Vec2::new(local_from.x, target.y)
                            } else {
                                Vec2::new(target.x, local_from.y)
                            };
                            builder.local = Some(local_corner);
                            builder.advance(transform.apply(local_corner));
                            builder.local = Some(target);
                            builder.advance(transform.apply(target));
                            builder.base = Some(transform.apply(target));
                            builder.local_base = Some(target);
                        }
                        Err(error) => issues.push(Issue::error(Span::new(open, close + 1), error)),
                    }
                    at = close + 1;
                    continue;
                }
            }
            issues.push(Issue::error(Span::new(at, to), "`|-`/`-|` needs a coordinate"));
            break;
        }
        if byte == b';' {
            break;
        }
        if byte == b'+' {
            // `+`/`++` prefixes belong to the next coordinate.
            at += 1;
            continue;
        }

        let (word, after) = read_word(src, at, to);
        match word {
            "rectangle" => pending = Pending::Rectangle,
            "circle" => pending = Pending::Circle,
            "ellipse" => pending = Pending::Ellipse,
            "arc" => pending = Pending::Arc,
            "grid" => pending = Pending::Grid,
            "cycle" => {
                if let (Some(current), Some(start)) = (builder.current, builder.start) {
                    // Legacy closes by appending a line to the subpath start
                    // and then the `Z` command.
                    if current != start {
                        builder.cmds.push(PathCmd::Line(start));
                        builder.touch(start);
                    }
                    builder.cmds.push(PathCmd::Close);
                }
                if let Some(start) = builder.start {
                    builder.current = Some(start);
                    builder.base = Some(start);
                }
                flush(&mut builder, ctx, &mut elements);
            }
            "to" | "edge" => {
                issues.push(Issue::warning(
                    Span::new(at, after),
                    format!("`{word}` is drawn as a straight line (bends are not supported yet)"),
                ));
                at = skip_optional_groups(src, after, to, issues);
                continue;
            }
            "node" | "pic" => {
                issues.push(Issue::warning(
                    Span::new(at, after),
                    format!("`{word}` is not rendered yet (text engine deferred)"),
                ));
                at = skip_optional_groups(src, after, to, issues);
                continue;
            }
            "plot" | "parabola" | "coordinates" | "let" | "svg" | "decorate" | "child"
            | "graph" => {
                issues.push(Issue::error(
                    Span::new(at, after),
                    format!("`{word}` is not supported yet"),
                ));
                break;
            }
            "" => {
                at = (at + 1).min(to);
                continue;
            }
            other => {
                issues.push(Issue::error(
                    Span::new(at, after),
                    format!("unsupported path item `{other}`"),
                ));
                break;
            }
        }
        at = after;
    }
    flush(&mut builder, ctx, &mut elements);
    elements
}

/// Emit the shape a pending keyword described from its `[...]` options.
fn finish_pending_shape(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    pending: Pending,
    span: Span,
    issues: &mut Vec<Issue>,
    elements: &mut Vec<SceneElement>,
) {
    let state = &ctx.state;
    match pending {
        Pending::Circle => {
            let radius = state
                .radius
                .map(|value| (value, !state.radius_uses_basis))
                .or_else(|| state.x_radius.map(|value| (value, !state.x_radius_uses_basis)));
            match radius {
                Some(radius) => emit_circle(builder, ctx, elements, radius, span, issues),
                None => issues.push(Issue::error(span, "`circle` needs a radius")),
            }
        }
        Pending::Ellipse => {
            let rx = state
                .x_radius
                .map(|value| (value, !state.x_radius_uses_basis))
                .or_else(|| state.radius.map(|value| (value, !state.radius_uses_basis)));
            let ry = state
                .y_radius
                .map(|value| (value, !state.y_radius_uses_basis))
                .or_else(|| state.radius.map(|value| (value, !state.radius_uses_basis)));
            match (rx, ry) {
                (Some(rx), Some(ry)) => emit_ellipse(builder, ctx, elements, rx, ry, span, issues),
                _ => issues.push(Issue::error(span, "`ellipse` needs two radii")),
            }
        }
        Pending::Arc => {
            let rx = state
                .x_radius
                .map(|value| (value, !state.x_radius_uses_basis))
                .or_else(|| state.radius.map(|value| (value, !state.radius_uses_basis)));
            let ry = state
                .y_radius
                .map(|value| (value, !state.y_radius_uses_basis))
                .or_else(|| state.radius.map(|value| (value, !state.radius_uses_basis)));
            match (state.start_angle, state.end_angle, rx, ry) {
                (Some(start), Some(end), Some(rx), Some(ry)) => emit_arc(
                    builder,
                    ctx,
                    ArcParams { start_angle: start, end_angle: end, rx, ry },
                    span,
                    issues,
                ),
                _ => issues.push(Issue::error(
                    span,
                    "`arc` options need `start angle`, `end angle` and radii",
                )),
            }
        }
        Pending::Grid => emit_grid(builder, ctx, elements, None),
        Pending::Rectangle | Pending::None => {}
    }
}

pub(crate) fn path_element(ctx: &PathContext<'_>, cmds: Vec<PathCmd>, bbox: Option<(Vec2, Vec2)>) -> SceneElement {
    SceneElement::Path(ScenePath {
        source_id: ctx.source_id.clone(),
        cmds,
        style: ctx.state.style.clone(),
        bbox,
    })
}

fn flush(builder: &mut Builder, ctx: &PathContext<'_>, elements: &mut Vec<SceneElement>) {
    if builder.is_drawable() {
        let cmds = std::mem::take(&mut builder.cmds);
        elements.push(path_element(ctx, cmds, builder.bbox));
    }
    builder.reset();
}

fn relative_kind(src: &str, open: usize) -> Relative {
    if open >= 2 && &src[open - 2..open] == "++" {
        Relative::PlusPlus
    } else if open >= 1 && src.as_bytes()[open - 1] == b'+' {
        Relative::Plus
    } else {
        Relative::Absolute
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_prefixes_are_detected() {
        assert_eq!(relative_kind("++(1,1)", 2), Relative::PlusPlus);
        assert_eq!(relative_kind("+(1,1)", 1), Relative::Plus);
        assert_eq!(relative_kind("--(1,1)", 2), Relative::Absolute);
    }
}