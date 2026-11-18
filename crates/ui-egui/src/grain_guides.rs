//! Grain guides are display geometry: stripes clipped to the body's display triangles.

use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;

use crate::{SolveApp, dialogs_wood_grain};

pub fn draw(app: &SolveApp, scene: &mut crate::gpu::GpuScene) {
    let colour = scene.color(crate::viewport::c4(solidcraft_theme::Tokens::get().accent));
    for (a, b) in segments(app) {
        scene.line(a.to_f32(), b.to_f32(), colour, 1.4, false);
    }
}

pub fn segments(app: &SolveApp) -> Vec<(Vec3, Vec3)> {
    let preview = dialogs_wood_grain::preview(app);
    if !app.ui.show_grain && preview.is_none() {
        return Vec::new();
    }
    let mut scratch = app.session.scratch();
    let preview_body = preview.as_ref().and_then(|p| p.get("body")).and_then(serde_json::Value::as_str);
    if let Some(params) = &preview
        && scratch.execute("wood.grain", params).is_err()
    {
        return Vec::new();
    }
    let Ok(state) = scratch.world_state() else { return Vec::new() };
    let mut guides = Vec::new();
    for body in state.bodies.iter().filter(|b| app.body_visible(&b.name)) {
        if !app.ui.show_grain && preview_body != Some(body.name.as_str()) {
            continue;
        }
        let Ok(Some(direction)) = scratch.doc.grain_direction(body) else { continue };
        guides.extend(lines(&body.mesh(), direction));
    }
    guides
}

pub fn lines(mesh: &solidcraft_engine::geom::Mesh, grain: Vec3) -> Vec<(Vec3, Vec3)> {
    let mut faces = std::collections::BTreeMap::new();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, b, c]) = mesh.tri(triangle) else { continue };
        let Some(normal) = (b - a).cross(c - a).normalized() else { continue };
        let Some(across) = normal.cross(grain).normalized() else { continue };
        let (axis, min, max) = faces.entry(*face).or_insert((across, f64::INFINITY, f64::NEG_INFINITY));
        for p in [a, b, c] {
            let h = p.dot(*axis);
            *min = min.min(h);
            *max = max.max(h);
        }
    }
    let mut lines = Vec::new();
    for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, b, c]) = mesh.tri(triangle) else { continue };
        let Some(normal) = (b - a).cross(c - a).normalized() else { continue };
        let Some((across, min, max)) = faces.get(face) else { continue };
        let spacing = (max - min) / 4.0;
        if !spacing.is_finite() || spacing <= 1e-9 {
            continue;
        }
        for k in 1..=3 {
            let level = min + f64::from(k) * spacing;
            let mut ends = Vec::new();
            for (p, q) in [(a, b), (b, c), (c, a)] {
                let (hp, hq) = (p.dot(*across), q.dot(*across));
                if (hp <= level && level < hq) || (hq <= level && level < hp) {
                    ends.push(p.lerp(q, (level - hp) / (hq - hp)));
                }
            }
            if let [p, q] = ends.as_slice() {
                let offset = normal * (spacing * 1e-5);
                lines.push((*p + offset, *q + offset));
            }
        }
    }
    lines
}
