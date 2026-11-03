//! Shape emitters for path statements: rectangle, circle, ellipse, arc, grid.
//!
//! Split out of [`super::pathbuild`] to keep both modules inside the port's
//! `<= 400` LOC budget. Every emitter takes the statement's [`PathContext`]
//! and pushes [`SceneElement`]s; nothing here re-reads the source text.

use crate::scene::{PathCmd, SceneElement, SceneEllipse, ScenePath};
use crate::semantic::pathbuild::{path_element, Builder, PathContext};
use crate::semantic::pathscan::{ellipse_from_basis, scale_radii_pair, steps, ArcParams};
use crate::semantic::style::Issue;
use crate::semantic::transform::Transform;
use crate::span::Span;
use crate::units::Vec2;

pub(crate) fn emit_rectangle(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    elements: &mut Vec<SceneElement>,
    corner: Vec2,
) {
    // Corners are computed in local space and transformed individually, so a
    // rotated frame turns the rectangle into a rotated quad (verified against
    // `goldens-slice3/geometry.svg`).
    let transform = ctx.state.transform;
    let local_first = builder.local.unwrap_or(Vec2::new(0.0, 0.0));
    let local_corners = [
        Vec2::new(corner.x, local_first.y),
        corner,
        Vec2::new(local_first.x, corner.y),
    ];
    let first = transform.apply(local_first);
    let mut cmds = Vec::new();
    if builder.current.is_some() && ctx.state.style.fill != crate::scene::Paint::None {
        // A filled rectangle opens a new subpath, so legacy emits a second
        // `M` at the same point (verified against `goldens-slice3/styled.svg`).
        cmds.push(PathCmd::Move(first));
    }
    cmds.push(PathCmd::Move(first));
    let mut bbox = Some((first, first));
    for local in local_corners {
        let point = transform.apply(local);
        cmds.push(PathCmd::Line(point));
        bbox = Some(match bbox {
            None => (point, point),
            Some((lo, hi)) => (
                Vec2::new(lo.x.min(point.x), lo.y.min(point.y)),
                Vec2::new(hi.x.max(point.x), hi.y.max(point.y)),
            ),
        });
    }
    cmds.push(PathCmd::Close);
    builder.current = Some(first);
    builder.base = Some(first);
    builder.local = Some(local_first);
    builder.local_base = Some(local_first);
    elements.push(path_element(ctx, cmds, bbox));
}

pub(crate) fn emit_circle(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    elements: &mut Vec<SceneElement>,
    radius: (f64, bool),
    span: Span,
    issues: &mut Vec<Issue>,
) {
    let center = builder.current.unwrap_or(Vec2::new(0.0, 0.0));
    let (rx, ry) = scale_radii_pair(&ctx.state.transform, radius, radius);
    if rx <= 0.0 || ry <= 0.0 {
        issues.push(Issue::error(span, "circle needs a positive radius"));
        return;
    }
    let right = Vec2::new(center.x + rx, center.y);
    let left = Vec2::new(center.x - rx, center.y);
    let bbox = Some((
        Vec2::new(center.x - rx, center.y - ry),
        Vec2::new(center.x + rx, center.y + ry),
    ));
    builder.current = Some(center);
    builder.base = Some(center);
    elements.push(path_element(
        ctx,
        vec![
            PathCmd::Move(center),
            PathCmd::Move(right),
            PathCmd::Arc {
                rx,
                ry,
                rotation: 0.0,
                large: false,
                sweep: true,
                to: left,
            },
            PathCmd::Arc {
                rx,
                ry,
                rotation: 0.0,
                large: false,
                sweep: true,
                to: right,
            },
            PathCmd::Close,
        ],
        bbox,
    ));
}

pub(crate) fn emit_ellipse(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    elements: &mut Vec<SceneElement>,
    rx: (f64, bool),
    ry: (f64, bool),
    span: Span,
    issues: &mut Vec<Issue>,
) {
    let (out_rx, out_ry) = scale_radii_pair(&ctx.state.transform, rx, ry);
    if ctx.state.transform.rotate_deg != 0.0 {
        issues.push(Issue::warning(
            span,
            "rotated `ellipse` is emitted axis-aligned",
        ));
    }
    if out_rx <= 0.0 || out_ry <= 0.0 {
        issues.push(Issue::error(span, "ellipse needs positive radii"));
        return;
    }
    let center = builder.current.unwrap_or(Vec2::new(0.0, 0.0));
    builder.current = Some(center);
    builder.base = Some(center);
    elements.push(SceneElement::Ellipse(SceneEllipse {
        source_id: ctx.source_id.clone(),
        center,
        rx: out_rx,
        ry: out_ry,
        style: ctx.state.style.clone(),
    }));
}

pub(crate) fn emit_arc(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    params: ArcParams,
    span: Span,
    issues: &mut Vec<Issue>,
) {
    let Some(from) = builder.current else {
        issues.push(Issue::error(span, "`arc` needs a current point"));
        return;
    };
    // A radius written without a unit follows the frame transform; one with
    // an explicit unit does not (legacy `usesBasis`).
    let basis = [params.rx, params.ry];
    let transform = if basis.iter().all(|radius| radius.1) {
        Transform::default()
    } else {
        ctx.state.transform
    };
    let (rx, ry) = (params.rx.0, params.ry.0);
    let start = params.start_angle.to_radians();
    let end = params.end_angle.to_radians();
    let start_vector = transform.apply_vector(Vec2::new(rx * start.cos(), ry * start.sin()));
    let end_vector = transform.apply_vector(Vec2::new(rx * end.cos(), ry * end.sin()));
    let center = Vec2::new(from.x - start_vector.x, from.y - start_vector.y);
    let endpoint = Vec2::new(center.x + end_vector.x, center.y + end_vector.y);
    let (out_rx, out_ry, rotation) = ellipse_from_basis(&transform, rx, ry);
    let delta = params.end_angle - params.start_angle;
    builder.cmds.push(PathCmd::Arc {
        rx: out_rx,
        ry: out_ry,
        rotation,
        large: delta.abs() > 180.0,
        sweep: delta >= 0.0,
        to: endpoint,
    });
    // Legacy bounds an arc by its whole ellipse box.
    builder.touch(Vec2::new(center.x - out_rx, center.y - out_ry));
    builder.touch(Vec2::new(center.x + out_rx, center.y + out_ry));
    builder.current = Some(endpoint);
    builder.base = Some(endpoint);
}

/// Emit a grid from the current point to `corner` (transformed by the frame).
///
/// Legacy draws one line element per grid line, all sharing the statement's
/// `data-source-id`: first the vertical lines in increasing x, then the
/// horizontal lines in increasing y.
pub(crate) fn emit_grid(
    builder: &mut Builder,
    ctx: &PathContext<'_>,
    elements: &mut Vec<SceneElement>,
    corner: Option<Vec2>,
) {
    let transform = ctx.state.transform;
    let origin = builder.local.unwrap_or(Vec2::new(0.0, 0.0));
    let corner = corner.unwrap_or(origin);
    let step = match ctx.state.grid_step {
        Some((value, _)) => value,
        None => crate::units::CM_TO_PT,
    };
    let (x0, x1) = (origin.x.min(corner.x), origin.x.max(corner.x));
    let (y0, y1) = (origin.y.min(corner.y), origin.y.max(corner.y));
    let mut push = |a: Vec2, b: Vec2| {
        let a = transform.apply(a);
        let b = transform.apply(b);
        elements.push(SceneElement::Path(ScenePath {
            source_id: ctx.source_id.clone(),
            cmds: vec![PathCmd::Move(a), PathCmd::Line(b)],
            style: ctx.state.style.clone(),
            bbox: Some((
                Vec2::new(a.x.min(b.x), a.y.min(b.y)),
                Vec2::new(a.x.max(b.x), a.y.max(b.y)),
            )),
        }));
    };
    for x in steps(x0, x1, step) {
        push(Vec2::new(x, y0), Vec2::new(x, y1));
    }
    for y in steps(y0, y1, step) {
        push(Vec2::new(x0, y), Vec2::new(x1, y));
    }
    builder.local = Some(corner);
    builder.local_base = Some(corner);
    builder.current = Some(transform.apply(corner));
    builder.base = Some(transform.apply(corner));
}
