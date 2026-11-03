//! Path statement evaluation: AST items -> [`SceneElement`]s.
//!
//! The legacy evaluator walks AST path items and threads a builder with
//! pending shape state (`pendingCircleCenter`, `pendingGrid`, ...). This port
//! keeps that shape — a [`Builder`] plus a [`Pending`] slot — but consumes the
//! [`crate::items::PathItem`] AST produced by the front-end instead of
//! re-scanning statement text. Source spans are preserved exactly.
//!
//! Supported: coordinates (Cartesian/polar, absolute/relative), `--`, `|-`,
//! `-|`, `cycle`, `rectangle`, `circle`, `ellipse`, `arc`, `grid`, plus the
//! shape options `[radius=...]`, `[x radius=...]`, `[y radius=...]`,
//! `[start angle=...]`, `[end angle=...]`, `[step=...]`. Unsupported
//! keywords produce issues; nothing is dropped silently.

use crate::item_types::{CoordinateForm, CoordinateItem, RelativeCoordinatePrefix};
use crate::items::PathItem;
use crate::scene::{PathCmd, SceneElement, ScenePath};
use crate::semantic::pathcurve::{resolve_to_curve, to_curve_controls, ToCurveOptions};
use crate::semantic::pathscan::{
    parse_arc_shorthand, parse_coordinate, parse_length, parse_point, parse_radii, ArcParams,
    NamedCoordinates,
};
use crate::semantic::shapes::{emit_arc, emit_circle, emit_ellipse, emit_grid, emit_rectangle};
use crate::semantic::style::{Issue, StyleState};
use crate::span::Span;
use crate::units::Vec2;

/// Everything the evaluator needs beyond the item list.
pub struct PathContext<'a> {
    /// `data-source-id` for the elements this statement produces.
    pub source_id: String,
    /// Style state; mid-path option lists update it in place.
    pub state: StyleState,
    /// Named coordinates defined so far in the figure (mutated by `\node` /
    /// `\coordinate` statements, consumed when a bare `(name)` is used).
    pub named: &'a mut NamedCoordinates,
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

/// Build the elements of one path statement from its [`PathItem`] AST.
///
/// This replaces the legacy text scanner: every keyword/coordinate/option is
/// consumed from [`crate::items::PathItem`] nodes produced by the front-end,
/// keeping exact source spans and never re-reading statement text.
pub fn build_path(
    ctx: &mut PathContext<'_>,
    items: &[PathItem],
    issues: &mut Vec<Issue>,
) -> Vec<SceneElement> {
    let mut elements = Vec::new();
    let mut builder = Builder::default();
    let mut pending = Pending::None;
    // Item indices consumed by a preceding `|-`/`-|`/`to`/`edge` keyword so the
    // loop skips them as standalone coordinates.
    let mut consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for (item_index, item) in items.iter().enumerate() {
        if consumed.contains(&item_index) {
            continue;
        }
        match item {
            PathItem::Coordinate(coord) => {
                let relative = match coord.relative_prefix {
                    Some(RelativeCoordinatePrefix::PlusPlus) => Relative::PlusPlus,
                    Some(RelativeCoordinatePrefix::Plus) => Relative::Plus,
                    None => Relative::Absolute,
                };
                let inner = coordinate_inner(coord);
                let span = coord.span;
                match pending {
                    Pending::Rectangle => match parse_point(&inner) {
                        Ok(corner) => emit_rectangle(&mut builder, ctx, &mut elements, corner),
                        Err(error) => issues.push(Issue::error(span, error)),
                    },
                    Pending::Circle => match parse_length(&coord.x) {
                        Ok(radius) => {
                            emit_circle(&mut builder, ctx, &mut elements, radius, span, issues)
                        }
                        Err(error) => issues.push(Issue::error(span, error)),
                    },
                    Pending::Ellipse => match parse_radii(&inner) {
                        Ok((rx, ry)) => {
                            emit_ellipse(&mut builder, ctx, &mut elements, rx, ry, span, issues)
                        }
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
                    Pending::None => match parse_coordinate(&inner, Some(ctx.named)) {
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
            }
            PathItem::PathOption(opt) => {
                ctx.state.apply(&opt.options, issues);
                if matches!(pending, Pending::Circle | Pending::Ellipse | Pending::Arc) {
                    finish_pending_shape(
                        &mut builder,
                        ctx,
                        pending,
                        opt.span,
                        issues,
                        &mut elements,
                    );
                    pending = Pending::None;
                }
                // `grid [step=...] (corner)` keeps the pending flag: the corner
                // coordinate is what actually draws the lines.
            }
            PathItem::PathKeyword(keyword) => match keyword.keyword.as_str() {
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
                "--" | ".." => {
                    // Plain line connector: nothing to do — the next `Coordinate`
                    // item appends a `Line` (or `Move` if the path is empty).
                }
                "and" => {}
                "to" | "edge" | "-|" | "|-" => {
                    // Perpendicular connectors (`-|`, `|-`) draw a corner point
                    // then the target; `to`/`edge` draw a (possibly curved)
                    // segment from the current point to it.
                    // Mid-path `[...]` options appear as separate `PathOption`
                    // items in the AST and are handled before this keyword.
                    let is_perp = matches!(keyword.keyword.as_str(), "-|" | "|-");
                    // Expect a `(target)` coordinate.
                    if let Some(target_coord) = items[item_index + 1..].iter().find_map(|next| {
                        if let PathItem::Coordinate(coord) = next {
                            let inner = coordinate_inner(coord);
                            parse_coordinate(&inner, None)
                                .ok()
                                .map(|point| (coord.span, point))
                        } else {
                            None
                        }
                    }) {
                        consumed.insert(item_index + 1);
                        let (_coord_span, target) = target_coord;
                        if is_perp {
                            let vertical_first = keyword.keyword == "|-";
                            let current = builder.local.unwrap_or(target);
                            let local_corner = if vertical_first {
                                Vec2::new(current.x, target.y)
                            } else {
                                Vec2::new(target.x, current.y)
                            };
                            let transform = ctx.state.transform;
                            builder.local = Some(local_corner);
                            builder.advance(transform.apply(local_corner));
                            builder.local = Some(target);
                            builder.advance(transform.apply(target));
                            builder.base = Some(transform.apply(target));
                            builder.local_base = Some(target);
                        } else if let Some(current) = builder.current {
                            let transform = ctx.state.transform;
                            let local_delta = transform
                                .inverse_apply_vector(target - current)
                                .unwrap_or(target - current);
                            let base_heading = local_delta.y.atan2(local_delta.x).to_degrees();
                            let curve = resolve_to_curve(ToCurveOptions {
                                out: ctx.state.to_out,
                                in_angle: ctx.state.to_in,
                                looseness: ctx.state.to_looseness,
                                out_looseness: ctx.state.to_out_looseness,
                                in_looseness: ctx.state.to_in_looseness,
                                bend_signed: ctx.state.to_bend,
                                base_heading,
                                relative: ctx.state.to_relative,
                                has_explicit_in_or_out: ctx.state.to_out.is_some()
                                    || ctx.state.to_in.is_some(),
                                has_bend_direction: ctx.state.to_bend.is_some(),
                                bend_direction: None,
                            });
                            if let Some((c1, c2, end)) =
                                to_curve_controls(current, target, &transform, &curve)
                            {
                                builder.cmds.push(PathCmd::Cubic { c1, c2, to: end });
                                builder.current = Some(end);
                                builder.base = Some(end);
                                builder.local = Some(end);
                                builder.local_base = Some(end);
                                builder.touch(end);
                            } else {
                                builder.advance(target);
                            }
                        } else {
                            // No current point: start a fresh subpath at the target.
                            builder.advance(target);
                        }
                    } else {
                        issues.push(Issue::warning(
                            keyword.span,
                            format!("`{}` needs a coordinate target", keyword.keyword),
                        ));
                    }
                }
                "node" | "pic" => {
                    issues.push(Issue::warning(
                        keyword.span,
                        format!(
                            "`{}` is not rendered yet (text engine deferred)",
                            keyword.keyword
                        ),
                    ));
                }
                "plot" | "parabola" | "coordinates" | "let" | "svg" | "decorate" | "child"
                | "graph" => {
                    issues.push(Issue::error(
                        keyword.span,
                        format!("`{}` is not supported yet", keyword.keyword),
                    ));
                }
                "" => {}
                other => {
                    issues.push(Issue::error(
                        keyword.span,
                        format!("unsupported path item `{other}`"),
                    ));
                }
            },
            PathItem::PathComment(comment) => {
                let _ = comment;
                // Comments carry no geometry; skip them.
            }
            PathItem::Unknown(unknown) => {
                issues.push(Issue::error(
                    unknown.span,
                    format!("unsupported path item `{}`", unknown.raw),
                ));
            }
            PathItem::Node(_)
            | PathItem::PathForeach(_)
            | PathItem::GraphOperation(_)
            | PathItem::PlotOperation(_)
            | PathItem::PicOperation(_)
            | PathItem::ToOperation(_)
            | PathItem::EdgeOperation(_)
            | PathItem::ChildOperation(_)
            | PathItem::EdgeFromParentOperation(_)
            | PathItem::SvgOperation(_)
            | PathItem::LetOperation(_)
            | PathItem::DecorateOperation(_)
            | PathItem::CoordinateOperation(_) => {
                issues.push(Issue::warning(
                    item.span(),
                    "path item is not rendered yet (text engine deferred)",
                ));
            }
        }
    }

    flush(&mut builder, ctx, &mut elements);
    elements
}

/// Reconstruct the inner coordinate text (without parens) from a
/// [`CoordinateItem`], preserving cartesian `x,y`, polar `angle:radius`,
/// and bare-name forms.
fn coordinate_inner(coord: &CoordinateItem) -> String {
    match coord.form {
        CoordinateForm::Polar => format!("{}:{}", coord.x, coord.y),
        CoordinateForm::Cartesian => format!("{},{}", coord.x, coord.y),
        CoordinateForm::Named => coord.x.clone(),
        _ => {
            let raw = coord.raw.trim();
            if raw.starts_with('(') && raw.ends_with(')') && raw.len() >= 2 {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw.to_string()
            }
        }
    }
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
                .or_else(|| {
                    state
                        .x_radius
                        .map(|value| (value, !state.x_radius_uses_basis))
                });
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
                    ArcParams {
                        start_angle: start,
                        end_angle: end,
                        rx,
                        ry,
                    },
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

pub(crate) fn path_element(
    ctx: &PathContext<'_>,
    cmds: Vec<PathCmd>,
    bbox: Option<(Vec2, Vec2)>,
) -> SceneElement {
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
