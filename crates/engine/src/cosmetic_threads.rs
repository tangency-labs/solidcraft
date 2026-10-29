//! Original schematic cylindrical hatching for evaluated cosmetic threads. The segments lie
//! on the displayed triangles, so neither a separate cylinder nor a helical B-rep is needed.
use std::collections::HashMap;

use solidcraft_doc::{ModelBody, ThreadInfo};
use solidcraft_geom::{Mesh, Vec3};

/// Shared upper bound for cosmetic strokes in one viewport/snapshot scene.
pub const MAX_SCENE_SEGMENTS: usize = 65_536;

/// Shared scene limits, including unsuccessful inspections. Camera-only frames reuse the
/// viewport's existing scene cache; scene rebuilds cannot restart this budget for each body.
pub struct Budget {
    work: usize,
    strokes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self { work: 500_000, strokes: MAX_SCENE_SEGMENTS }
    }
}

impl Budget {
    pub fn available(&self) -> bool {
        self.work > 0 && self.strokes > 0
    }
    fn spend(&mut self, n: usize) -> bool {
        if n > self.work {
            self.work = 0;
            return false;
        }
        self.work -= n;
        self.strokes > 0
    }
    /// Bounded preflight before requesting a body's potentially refined display mesh.
    pub fn has_thread(&mut self, name: &str, threads: &[ThreadInfo]) -> bool {
        for t in threads {
            if !self.spend(1) {
                return false;
            }
            if t.body == name {
                return true;
            }
        }
        false
    }
}

/// Depth-tested, section-clippable strokes. Use the same mesh as the visible surface to avoid
/// fighting a different tessellation. There are at most 32 bands and 32,768 segments per call.
/// This is a schematic marking, not a representation of the manufactured thread profile.
pub fn segments(body: &ModelBody, mesh: &Mesh, threads: &[ThreadInfo]) -> Vec<(Vec3, Vec3)> {
    segments_with_budget(body, mesh, threads, &mut Budget::default())
}

/// Generate one body's strokes, charging all inspections and emitted segments to the scene.
pub fn segments_with_budget(body: &ModelBody, mesh: &Mesh, threads: &[ThreadInfo], budget: &mut Budget) -> Vec<(Vec3, Vec3)> {
    let mut out = Vec::new();
    // Spread a readable schematic density across ordinary assemblies before hitting a hard
    // scene cap. A large set of fine-pitch threads need not consume the budget on its first
    // few occurrences. This is deliberately independent of camera motion and scene caching.
    let bands = (768.0 / threads.len().max(1) as f64).clamp(4.0, 32.0);
    if body.body.is_mesh() {
        return out;
    }
    for t in threads {
        if !budget.spend(1) {
            return out;
        }
        if t.body != body.name {
            continue;
        }
        let length = t.end - t.start;
        if !t.axis_point.is_finite()
            || !t.axis.is_finite()
            || !length.is_finite()
            || length <= 0.0
            || !t.start.is_finite()
            || !t.pitch.is_finite()
            || t.pitch <= 0.0
            || !t.major_diameter.is_finite()
            || t.major_diameter <= 0.0
            || !t.radius.is_finite()
            || t.radius <= 0.0
        {
            continue;
        }
        let seed = if t.axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let Some(u) = t.axis.cross(seed).normalized() else { continue };
        let tilt = 0.18 * t.radius;
        let spacing = t.pitch.max((length + 2.0 * tilt) / bands);
        let tolerance = (t.radius * 1e-6).max(1e-7);
        let mut faces = HashMap::new();
        for (tri, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if !budget.spend(1) || out.len() >= 32_768 {
                return out;
            }
            let matched = if let Some(found) = faces.get(face) {
                *found
            } else {
                // face_surface traverses to `face`; charge that traversal too. No unbounded
                // face collection or full-mesh scan occurs before the budget is checked.
                if !budget.spend((*face as usize).saturating_add(1)) {
                    return out;
                }
                let surface = solidcraft_kernel::guard("cosmetic thread surface", || Ok(solidcraft_kernel::face_surface(&body.body, *face as usize)))
                    .ok()
                    .flatten();
                let found = match surface {
                    Some(solidcraft_kernel::FaceSurface::Cylinder { origin, axis, radius }) => {
                        let offset = origin - t.axis_point;
                        axis.dot(t.axis).abs() > 1.0 - 1e-6
                            && (offset - t.axis * offset.dot(t.axis)).len() < tolerance
                            && (radius - t.radius).abs() < tolerance
                    }
                    _ => false,
                };
                faces.insert(*face, found);
                found
            };
            if !matched {
                continue;
            }
            let Some(points) = mesh.tri(tri) else { continue };
            let [a, b, c] = points;
            let center = (a + b + c) / 3.0 - t.axis_point;
            let radial = center - t.axis * center.dot(t.axis);
            // Mesh winding distinguishes a bore from a shaft without rescanning the entire
            // mesh for every cylindrical face.
            let inward = (b - a).cross(c - a).dot(radial) < 0.0;
            if inward != t.internal {
                continue;
            }
            // Tilt bands across the placed cylindrical wall. The world-axis seed can change
            // their schematic orientation after rotation; it never changes their surface.
            let phase = points.map(|p| {
                let v = p - t.axis_point;
                (v.dot(t.axis) - t.start + 0.18 * v.dot(u)) / spacing
            });
            // A long face can extend far beyond a short threaded interval. Restrict the
            // candidate bands before the loop so its budget never hides that interval.
            let lo = phase.into_iter().fold(f64::INFINITY, f64::min).max(-tilt / spacing).ceil();
            let hi = phase.into_iter().fold(f64::NEG_INFINITY, f64::max).min((length + tilt) / spacing).floor();
            if !lo.is_finite() || !hi.is_finite() || lo.abs() > 1e9 || hi.abs() > 1e9 {
                continue;
            }
            for band in (lo as i64..=hi as i64).take(64) {
                if !budget.spend(1) || out.len() >= 32_768 {
                    return out;
                }
                let mut hits = Vec::with_capacity(2);
                for (i, j) in [(0, 1), (1, 2), (2, 0)] {
                    let (Some(&a), Some(&b), Some(&fa), Some(&fb)) = (points.get(i), points.get(j), phase.get(i), phase.get(j)) else { continue };
                    let level = band as f64;
                    if (fa <= level && fb > level) || (fb <= level && fa > level) {
                        hits.push(a + (b - a) * ((level - fa) / (fb - fa)));
                    }
                }
                if let [a, b] = hits.as_slice()
                    && let Some(pair) = trim(*a, *b, t)
                {
                    out.push(pair);
                    budget.strokes -= 1;
                }
            }
        }
    }
    out
}

fn trim(a: Vec3, b: Vec3, t: &ThreadInfo) -> Option<(Vec3, Vec3)> {
    let z = (a - t.axis_point).dot(t.axis);
    let dz = (b - a).dot(t.axis);
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    if dz.abs() < 1e-12 {
        if z < t.start || z > t.end {
            return None;
        }
    } else {
        let p = (t.start - z) / dz;
        let q = (t.end - z) / dz;
        lo = lo.max(p.min(q));
        hi = hi.min(p.max(q));
    }
    (hi > lo).then(|| (a + (b - a) * lo, a + (b - a) * hi))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::json;

    fn run(s: &mut Session, command: &str, params: serde_json::Value) {
        s.execute(command, &params).unwrap();
    }

    #[test]
    fn modeled_threads_do_not_receive_cosmetic_strokes() {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius":3,"height":4}));
        let before = s.world_state().bodies[0].mesh().triangles.len();
        run(&mut s, "solid.thread", json!({"face":[3,0,3],"designation":"M6","length":2,"modeled":true}));
        let st = s.world_state();
        assert!(st.threads.is_empty());
        assert_ne!(st.bodies[0].mesh().triangles.len(), before);
        assert!(segments(&st.bodies[0], &st.bodies[0].mesh(), &st.threads).is_empty());
    }

    #[test]
    fn downstream_enlargement_is_not_mistaken_for_the_threaded_wall() {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
        run(&mut s, "solid.hole", json!({"position":[10,10,10],"direction":[0,0,-1],"thread":"M6"}));
        run(&mut s, "solid.hole", json!({"position":[10,10,10],"direction":[0,0,-1],"diameter":8,"depth":3}));
        let st = s.world_state();
        let body = &st.bodies[0];
        let lines = segments(body, &body.mesh(), &st.threads);
        assert!(!lines.is_empty());
        for p in lines.iter().flat_map(|(a, b)| [a, b]) {
            assert!(p.z <= 7.0 + 1e-6, "enlarged smooth wall must be unmarked: {p:?}");
        }
    }

    #[test]
    fn many_occurrences_have_matching_metadata_and_share_work_and_output_limits() {
        let mut s = Session::default();
        let c = s.execute("component.create", &json!({"name":"Pegs"})).unwrap();
        run(&mut s, "solid.cylinder", json!({"radius":3,"height":20}));
        run(&mut s, "solid.thread", json!({"face":[3,0,10],"designation":"M6"}));
        for i in 1..96 {
            run(&mut s, "occurrence.copy", json!({"component":c["component"],"translate":[i*8,0,0]}));
        }
        let st = s.world_state();
        assert_eq!(st.bodies.len(), 96);
        assert_eq!(st.threads.len(), 96);
        let started = std::time::Instant::now();
        let mut budget = Budget::default();
        let mut count = 0;
        for b in &st.bodies {
            assert!(st.threads.iter().any(|t| t.body == b.name));
            assert!(!segments(b, &b.mesh(), &st.threads).is_empty(), "{}", b.name);
            if budget.available() {
                count += segments_with_budget(b, &b.mesh(), &st.threads, &mut budget).len();
            }
        }
        eprintln!("96 occurrences: {count} strokes; {} work units left; {:?} including per-occurrence verification", budget.work, started.elapsed());
        assert!(count > 1000 && count <= MAX_SCENE_SEGMENTS);
        let mut stale = st.threads.clone();
        for t in &mut stale {
            t.radius *= 2.0;
        }
        let mut budget = Budget { work: 10_000, strokes: MAX_SCENE_SEGMENTS };
        let mut seen = 0;
        for b in st.bodies.iter().cycle().take(1000) {
            if !budget.available() {
                break;
            }
            assert!(segments_with_budget(b, &b.mesh(), &stale, &mut budget).is_empty());
            seen += 1;
        }
        assert!(seen > 1);
        assert_eq!(budget.work, 0);
        assert_eq!(budget.strokes, MAX_SCENE_SEGMENTS);
    }

    #[test]
    fn tapped_hole_marks_survive_reopen_and_rotated_occurrences() {
        let mut s = Session::default();
        // This fixture rotates its first component; new assemblies ground that part by default.
        let c = s.execute("component.create", &json!({"name":"Plate","grounded":false})).unwrap();
        run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"PlateBody"}));
        run(&mut s, "solid.hole", json!({"position":[10,10,10],"direction":[0,0,-1],"thread":"M6","depth":7}));
        run(&mut s, "occurrence.move", json!({"occurrence":c["occurrence"],"translate":[30,20,10],"axis":[1,0,0],"angle":"90 deg","capture":true}));
        let saved = s.doc.to_json();
        let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
        let mut s = Session::new(reopened);
        let st = s.world_state();
        assert_eq!(st.threads.len(), 1);
        assert!(st.threads[0].axis.z.abs() < 1e-6);
        let body = &st.bodies[0];
        let lines = segments(body, &body.mesh(), &st.threads);
        assert!(lines.len() > 20);
        let t = &st.threads[0];
        for p in lines.iter().flat_map(|(a, b)| [a, b]) {
            let z = (*p - t.axis_point).dot(t.axis);
            assert!(z >= t.start - 1e-8 && z <= t.end + 1e-8);
        }
        let mut tiny = t.clone();
        tiny.pitch = 1e-12;
        assert!(segments(body, &body.mesh(), &[tiny]).len() <= 32_768);
        s.execute("solid.thread", &json!({"face":[0,0,0],"designation":"bad"})).unwrap_err();
        assert_eq!(s.doc.to_json(), saved);
    }

    #[test]
    fn cosmetic_internal_and_external_marks_preserve_geometry_and_partial_extent() {
        for internal in [false, true] {
            let mut s = Session::default();
            if internal {
                run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10,"body_name":"Part"}));
                run(&mut s, "solid.hole", json!({"position":[10,10,10],"diameter":5,"direction":[0,0,-1]}));
            } else {
                run(&mut s, "solid.cylinder", json!({"radius":3,"height":10,"body_name":"Part"}));
            }
            let before = s.world_state().bodies[0].mesh();
            let face = if internal { json!([12.5, 10, 9]) } else { json!([3, 0, 9]) };
            run(&mut s, "solid.thread", json!({"face":face,"designation":"M6","length":4,"modeled":false}));
            let st = s.world_state();
            let body = &st.bodies[0];
            assert_eq!(before.positions, body.mesh().positions);
            assert_eq!(before.triangles, body.mesh().triangles);
            let saved = s.doc.to_json();
            let lines = segments(body, &body.mesh(), &st.threads);
            assert!(lines.len() > 20, "{internal}: {}", lines.len());
            let t = &st.threads[0];
            for p in lines.iter().flat_map(|(a, b)| [a, b]) {
                let z = (*p - t.axis_point).dot(t.axis);
                assert!(z >= t.start - 1e-8 && z <= t.end + 1e-8);
                assert!(p.is_finite());
            }
            assert!(segments(body, &body.mesh(), &[]).is_empty());
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                for field in 0..5 {
                    let mut hostile = t.clone();
                    match field {
                        0 => hostile.pitch = value,
                        1 => hostile.start = value,
                        2 => hostile.end = value,
                        3 => hostile.axis_point.x = value,
                        _ => hostile.radius = value,
                    }
                    assert!(segments(body, &body.mesh(), &[hostile]).is_empty());
                }
            }
            assert_eq!(s.doc.to_json(), saved);
            let scene = crate::view::scene(&s, &crate::render::Camera::default());
            assert!(!scene.lines.iter().any(|l| l.width == 0.85), "ordinary snapshots preserve the default-off behavior");
            for distance in [25.0, 60.0] {
                let cam = crate::render::Camera {
                    target: if internal { Vec3::new(10., 10., 5.) } else { Vec3::new(0., 0., 5.) },
                    distance,
                    ..Default::default()
                };
                for section in [Vec::new(), vec![crate::SectionPlane { id: 1, origin: cam.target, normal: Vec3::X }]] {
                    let mut scene = crate::view::scene_with_thread_markings(
                        &s,
                        &cam,
                        &section,
                        crate::view::world_curve_parts,
                        &crate::view::Palette::DEFAULT,
                        Default::default(),
                        true,
                    );
                    assert!(scene.lines.iter().any(|l| l.width == 0.85 && !l.on_top));
                    let marked = crate::render::render(&scene, &cam, 320, 240).rgba;
                    scene.lines.retain(|l| l.width != 0.85);
                    let plain = crate::render::render(&scene, &cam, 320, 240).rgba;
                    let changed = marked.as_chunks::<4>().0.iter().zip(plain.as_chunks::<4>().0.iter()).filter(|(a, b)| a != b).count();
                    assert!(changed > 15, "internal={internal}, distance={distance}, section={section:?}: {changed}");
                }
            }
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(s.world_state().threads.is_empty());
        }
    }
}
