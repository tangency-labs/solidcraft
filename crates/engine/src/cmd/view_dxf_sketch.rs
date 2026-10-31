//! Shown sketches in the view drawing: the curves the viewport draws for each sketch it shows
//! (`view::sketch_shown`), as world polylines. Only profile curves are exported: drawn 2D
//! curves, text outlines and drawn 3D-sketch curves. Construction geometry, centerlines,
//! projected references, points, dimensions and constraints are viewport overlays and are left
//! out. The active sketch is drawn on top, as the viewport draws it.
use solidcraft_geom::{Plane, Vec3};
use solidcraft_sketch::Sketch;

use crate::Session;
use crate::mesh_lines::{Part, Stage};
use crate::view::{sketch_curve_visible, sketch_shown};

/// Sampled sketch points kept for one export, at most.
const MAX_POINTS: usize = 4_000_000;

fn profile_curve(sk: &Sketch, i: usize) -> bool {
    sketch_curve_visible(sk, i) && sk.curves.get(i).is_some_and(|c| !c.construction && !c.centerline && (c.link.is_none() || sk.is_text_curve(i)))
}

fn has_profile(sk: &Sketch) -> bool {
    (0..sk.curves.len()).any(|i| profile_curve(sk, i)) || sk.wires.iter().any(|w| w.fit.len() > 1 && w.pts.len() > 1)
}

/// Does the current view have anything `file.export_view_dxf` draws: a visible body or a shown
/// sketch with a profile curve? The export dialog asks this before choosing a file.
pub fn exportable(s: &Session) -> bool {
    let world = s.world_state();
    world.bodies.iter().any(|b| s.body_visible(&b.name)) || world.sketches.iter().any(|ss| sketch_shown(s, ss.feature) && has_profile(&ss.sketch))
}

/// A curve's boundary segments joined end to end, so a circle (two half arcs) is one run.
fn curve_parts(sk: &Sketch, i: usize, plane: &Plane, tolerance: f64) -> Vec<Vec<Vec3>> {
    let mut parts: Vec<Vec<Vec3>> = Vec::new();
    for seg in sk.segs(i) {
        let points: Vec<Vec3> = seg.polyline(tolerance).iter().map(|p| plane.to_world(*p)).collect();
        let Some(first) = points.first().copied() else { continue };
        match parts.last_mut() {
            Some(run) if run.last().is_some_and(|end| end.dist(first) <= tolerance * 1e-3) => run.extend(points.into_iter().skip(1)),
            _ => parts.push(points),
        }
    }
    parts
}

/// The shown sketches' profile curves, sampled at the export's chord `tolerance`.
pub(super) fn parts(s: &Session, tolerance: f64) -> Result<Vec<Part>, &'static str> {
    let world = s.world_state();
    let shown: Vec<_> = world.sketches.iter().filter(|ss| sketch_shown(s, ss.feature)).collect();
    let progress = Stage::new("dxf_projection", shown.len());
    let mut result = Vec::new();
    let mut total = 0usize;
    let mut add = |points: Vec<Vec3>, on_top: bool, result: &mut Vec<Part>| -> Result<(), &'static str> {
        if points.len() < 2 {
            return Ok(());
        }
        if points.iter().any(|p| !p.is_finite()) {
            return Err("sketch contains invalid coordinates");
        }
        total = total.saturating_add(points.len());
        if total > MAX_POINTS {
            return Err("view exceeds export memory budget; hide sketches or increase tolerance");
        }
        result.push(Part { points, on_top });
        Ok(())
    };
    for ss in shown {
        progress.check()?;
        let on_top = s.active_sketch == Some(ss.feature);
        let sk = &ss.sketch;
        for i in (0..sk.curves.len()).filter(|i| profile_curve(sk, *i)) {
            for points in curve_parts(sk, i, &ss.plane, tolerance) {
                add(points, on_top, &mut result)?;
            }
        }
        // 3D-sketch curves are in world coordinates already; included references are not drawn.
        for w in sk.wires.iter().filter(|w| w.fit.len() > 1) {
            add(w.pts.clone(), on_top, &mut result)?;
        }
        progress.advance(1)?;
    }
    Ok(result)
}
