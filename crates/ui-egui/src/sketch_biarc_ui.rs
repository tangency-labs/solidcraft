//! Place the points and tangent directions of a planar Biarc Spline.

use egui::{Painter, Stroke};
use serde_json::json;
use solidcore_ops::prism::chord;
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{BiarcSpline, MAX_BIARC_NODES, Sketch};
use solidcraft_ui_text::tr;

use crate::SolveApp;
use crate::tools::Tool;
use crate::viewport::Proj;

pub(crate) const COMMAND: &str = "sketch.spline.biarc";
pub(crate) const EDIT: &str = "sketch.spline.biarc.edit";

/// Resolve a selected piece or tangent guide to the span it edits.
pub(crate) fn selected_span<'a>(sketch: &'a Sketch, sketch_id: u64, selection: &[Sel]) -> Option<(&'a BiarcSpline, usize)> {
    let [Sel::SketchCurve { id, sketch: picked }] = selection else { return None };
    if picked.is_some_and(|picked| picked != sketch_id) {
        return None;
    }
    if let Some(spline) = sketch.biarc_spline(id) {
        let span = spline.spans.iter().position(|span| span.curves.contains(id)).or_else(|| (id == &spline.id).then_some(0))?;
        return Some((spline, span));
    }
    let spline = sketch.biarc_guide_spline(id)?;
    let node = spline.nodes.iter().position(|node| node.handle.as_ref().is_some_and(|handle| &handle.curve == id))?;
    Some((spline, node.min(spline.spans.len().checked_sub(1)?)))
}

fn needs_tangent(tool: &Tool) -> bool {
    !tool.pts.len().is_multiple_of(2)
}

pub(crate) fn hint(tool: &Tool) -> &'static str {
    if needs_tangent(tool) {
        "sketch.biarc.tangent"
    } else if tool.pts.len() >= 4 {
        "sketch.biarc.next"
    } else {
        "sketch.biarc.point"
    }
}

fn placements(tool: &Tool) -> impl Iterator<Item = (Vec2, Vec2, Option<&str>)> {
    tool.pts.as_chunks::<2>().0.iter().map(|[(point, reference), (handle, _)]| (*point, *handle, reference.as_deref()))
}

pub(crate) fn place(app: &mut SolveApp, tool: &mut Tool, p: (Vec2, Option<String>)) {
    if tool.pts.len() >= 2 * MAX_BIARC_NODES {
        app.set_status(tr("sketch.biarc.finish").to_string(), false);
        return;
    }
    let tangent = needs_tangent(tool);
    if tangent && tool.pts.last().is_some_and(|point| chord(point.0, p.0).1 <= 1e-9) {
        app.set_status(tr("sketch.biarc.tangent").to_string(), true);
        return;
    }
    tool.pts.push(if tangent { (p.0, None) } else { p });
    app.set_status(tr(hint(tool)).to_string(), false);
}

pub(crate) fn finish(app: &mut SolveApp, tool: &mut Tool) -> bool {
    if tool.pts.is_empty() {
        return false;
    }
    if needs_tangent(tool) || tool.pts.len() < 4 {
        app.set_status(tr(hint(tool)).to_string(), false);
        return true;
    }
    let nodes: Vec<_> = placements(tool)
        .map(|(p, handle, reference)| {
            let (t, _) = chord(p, handle);
            let mut node = json!({"point": [p.x, p.y], "tangent": [t.x, t.y]});
            if let Some(reference) = reference {
                node["reference"] = json!(reference);
            }
            node
        })
        .collect();
    let construction = crate::sketch_palette::mode(app) != 0;
    crate::tools::draw(app, tool, json!({"nodes": nodes, "construction": construction})).is_err()
}

pub(crate) fn preview(app: &SolveApp, tool: &Tool, plane: &Plane, painter: &Painter, proj: &Proj) {
    let to_screen = |p| proj.to_screen(plane.to_world(p));
    let tokens = solidcraft_theme::Tokens::view();
    let stroke = Stroke::new(1.5, tokens.sketch_accent);
    let guides = Stroke::new(1.0, tokens.sketch_construction);
    let mut nodes: Vec<_> = placements(tool).map(|(p, h, _)| (p, chord(p, h).0)).collect();
    let pending = if needs_tangent(tool) { tool.pts.last().map(|p| p.0) } else { None };
    let hover = tool.hover.as_ref().map(|(p, _)| *p);
    let exact_preview = |nodes: &[(Vec2, Vec2)]| {
        let mut sketch = Sketch::new();
        sketch.add_biarc_spline(nodes, None, false).ok()?;
        Some(sketch)
    };
    let mut curve = None;
    if let Some((p, handle)) = pending.zip(hover) {
        nodes.push((p, chord(p, handle).0));
        curve = exact_preview(&nodes);
        nodes.pop();
    }
    if let Some(curve) = curve.or_else(|| exact_preview(&nodes)) {
        let mode = u8::from(crate::sketch_palette::mode(app) != 0);
        crate::tools::paint_preview_curve(curve, plane, painter, proj, stroke, mode);
    }
    for (p, handle) in placements(tool).map(|(p, h, _)| (p, h)).chain(pending.zip(hover)) {
        if let Some((a, b)) = to_screen(p).zip(to_screen(handle)) {
            painter.arrow(a, b - a, guides);
            painter.circle_filled(a, 3.0, tokens.sketch_accent);
        }
    }
    if let Some(point) = pending.or(hover).and_then(to_screen) {
        painter.circle_stroke(point, 3.5, stroke);
    }
}
