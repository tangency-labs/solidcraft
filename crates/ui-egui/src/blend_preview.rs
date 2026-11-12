//! Fillet and chamfer feedback from the display mesh, without asking the kernel.
//!
//! A blend's exact shape is the kernel's to say, and on this design asking it costs about 200 ms
//! a value -- so a dragged radius updated about five times a second and the handle ran away from
//! the shape. This draws the blend from the geometry already on screen instead, in the same
//! frame, and the kernel's exact answer replaces it as soon as it arrives
//! ([`crate::preview::compute`]). It is the same bargain [`crate::extrude_preview`] makes for a
//! dragged extrude.
//!
//! What it draws is the material the blend moves, not a trimmed body: the body stays as it is and
//! the sliver between the sharp edge and the round is drawn over it, through it where material is
//! going away. In the cross-section square to the edge, the two faces leave the edge along `u1`
//! and `u2`; a fillet of radius `r` is the arc touching both, and a chamfer of distance `d` the
//! straight line between the same two touch points. Sweeping that section along the edge's own
//! polyline gives the ribbon.
//!
//! It takes the plain cases only -- a constant radius, an equal-distance chamfer, millimetres --
//! and answers `None` for anything else, which falls through to the kernel as before. Being a
//! picture of the gesture rather than the model, it is measured off the display mesh.

use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::{Sel, doc::Document, geom::Aabb3};

use crate::preview::{Built, Colors};
use crate::{SolveApp, gpu::GpuScene};

/// How many points across the fillet's arc. Eight reads as round at any size a drag reaches
/// without making the ribbon heavy on a long tangent chain.
const ARC_STEPS: usize = 8;
/// Most edges this draws for; a longer chain falls through to the kernel rather than spending the
/// frame here.
const MAX_EDGES: usize = 512;
/// Most points taken along one edge, so a finely tessellated rim stays cheap.
const MAX_SAMPLES: usize = 64;

#[derive(Default)]
pub(crate) struct BlendPreview {
    cached: Option<Cached>,
}

/// The edges' own geometry, which only changes when the design or the picks do.
struct Cached {
    document: Arc<Document>,
    revision: u64,
    inputs: Vec<Sel>,
    edges: Vec<EdgeFrames>,
    bounds: Aabb3,
}

/// One picked edge, sampled along its polyline.
struct EdgeFrames {
    /// Per sample: the point on the edge, the direction into each face from it, and whether the
    /// faces there fold away from the material (an outside edge, where a round takes material off).
    samples: Vec<Frame>,
}

struct Frame {
    at: Vec3,
    /// The edge's own direction here, which the blend's touch curves must keep.
    tangent: Vec3,
    u1: Vec3,
    u2: Vec3,
    /// The angle between `u1` and `u2`, in radians.
    opening: f64,
    convex: bool,
}

impl BlendPreview {
    pub(crate) fn clear(&mut self) {
        self.cached = None;
    }

    pub(crate) fn compute(&mut self, app: &SolveApp, commands: &[(String, Value)], colors: Colors) -> Option<Result<Built, String>> {
        let [(id, params)] = commands else { return None };
        let (id, params) = match id.as_str() {
            "solid.fillet" | "solid.chamfer" => (id.as_str(), params),
            "timeline.redefine" => {
                let inner = params.get("command").and_then(Value::as_str)?;
                if !matches!(inner, "solid.fillet" | "solid.chamfer") {
                    return None;
                }
                (inner, params.get("params")?)
            }
            _ => return None,
        };
        let chamfer = id == "solid.chamfer";
        // Only the plain shapes. Anything with its own profile along the edge, a second distance
        // or an angle is the kernel's to draw.
        if app.session.doc.units != "mm"
            || params.get("distance2").is_some()
            || params.get("angle").is_some()
            || params.get("radius2").is_some()
            || params.get("spline").is_some()
            || !matches!(params.get("type").and_then(Value::as_str), None | Some("constant"))
        {
            return None;
        }
        let key = if chamfer { "distance" } else { "radius" };
        let value = params.get(key)?;
        let expression = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        let size = app.session.doc.eval(&expression, solidcraft_engine::doc::expr::Kind::Length).ok()?;
        if !size.is_finite() || size <= 1e-9 || size > 1e5 {
            return None;
        }

        let inputs = &app.dialog.as_ref()?.inputs.first()?.items;
        if inputs.is_empty() || inputs.len() > MAX_EDGES || !inputs.iter().all(|s| matches!(s, Sel::Edge { .. })) {
            return None;
        }
        if self
            .cached
            .as_ref()
            .is_none_or(|c| c.revision != app.session.revision || !Arc::ptr_eq(&c.document, &app.session.doc) || c.inputs != *inputs)
        {
            self.cached = prepare(app, inputs);
        }
        let cache = self.cached.as_ref()?;
        if cache.edges.is_empty() {
            return None;
        }

        let mut scene = GpuScene::default();
        let mut bounds = cache.bounds;
        let added = scene.color(colors.added);
        let removed = scene.color(colors.removed);
        // Where the blend is too big for what it rolls on -- a round wider than the corner it
        // runs round, say -- its touch curve folds back on itself and the ribbon crosses itself.
        // There is nothing honest to draw then, so the kernel is left to say so (it reports the
        // largest radius that fits).
        let drawn: Vec<Vec<Vec<Vec3>>> = cache
            .edges
            .iter()
            .map(|e| e.samples.iter().map(|f| section_points(f, size, chamfer)).collect())
            .collect();
        if cache.edges.iter().zip(&drawn).any(|(edge, sections)| folds(&edge.samples, sections)) {
            return None;
        }
        for (edge, section) in cache.edges.iter().zip(&drawn) {
            for (a, b) in section.iter().zip(section.iter().skip(1)) {
                if a.len() != b.len() {
                    continue;
                }
                for k in 0..a.len().saturating_sub(1) {
                    let (p0, p1, p2, p3) = (a[k], a[k + 1], b[k + 1], b[k]);
                    let normal = (p1 - p0).cross(p3 - p0).normalized().unwrap_or(Vec3::Z);
                    for tri in [[p0, p1, p2], [p0, p2, p3]] {
                        for p in tri {
                            bounds.add(p);
                            // Material going away is drawn through the body, as a cut preview is;
                            // material being added sits on it.
                            if edge.samples.first().is_some_and(|f| f.convex) {
                                if colors.cut_tint {
                                    scene.xray_tri(p.to_f32(), normal.to_f32(), removed);
                                }
                            } else {
                                scene.tri(p.to_f32(), normal.to_f32(), added);
                            }
                        }
                    }
                }
            }
            // The round's two tangent lines along the edge, which say where it meets each face.
            let line = scene.color(colors.edge);
            for side in [0usize, 1] {
                let pts: Vec<Vec3> = section
                    .iter()
                    .filter_map(|s| if side == 0 { s.first().copied() } else { s.last().copied() })
                    .collect();
                for (a, b) in pts.iter().zip(pts.iter().skip(1)) {
                    scene.line(a.to_f32(), b.to_f32(), line, 1.3, true);
                }
            }
        }
        Some(Ok(Built {
            scene,
            sketches: Vec::new(),
            applied: None,
            stopped: None,
            tapered: Vec::new(),
            replaced: Vec::new(),
            error: None,
            bounds,
        }))
    }
}

/// The blend's cross-section at one sample, from the face it leaves to the face it meets: the arc
/// of a fillet, or the two touch points of a chamfer.
fn section_points(f: &Frame, size: f64, chamfer: bool) -> Vec<Vec3> {
    let half = f.opening / 2.0;
    if !(half.sin() > 1e-6 && half.tan().abs() > 1e-6) {
        return Vec::new();
    }
    // How far along each face the blend reaches, and (for a fillet) where the rolling ball sits.
    let reach = size / half.tan();
    let q1 = f.at + f.u1 * reach;
    let q2 = f.at + f.u2 * reach;
    if chamfer {
        return vec![q1, q2];
    }
    let Some(bisector) = (f.u1 + f.u2).normalized() else { return vec![q1, q2] };
    let centre = f.at + bisector * (size / half.sin());
    // The arc from q1 to q2 about the centre, which both touch points are `size` from.
    let (a1, a2) = (q1 - centre, q2 - centre);
    let Some(axis) = a1.cross(a2).normalized() else { return vec![q1, q2] };
    let sweep = a1.dot(a2).clamp(-1.0, 1.0).acos();
    (0..=ARC_STEPS)
        .map(|k| {
            let t = sweep * (k as f64) / (ARC_STEPS as f64);
            centre + rotate(a1, axis, t)
        })
        .collect()
}

/// Whether the blend's touch curves double back anywhere along the edge: each one is the edge
/// offset into its face, and an offset wider than the face's own turn comes back inside out.
/// Both ends of the section are checked, which is where an offset folds first.
fn folds(samples: &[Frame], sections: &[Vec<Vec3>]) -> bool {
    let ends = |s: &Vec<Vec3>| Some((*s.first()?, *s.last()?));
    samples.iter().zip(sections).zip(sections.iter().skip(1)).any(|((f, a), b)| {
        let (Some((a1, a2)), Some((b1, b2))) = (ends(a), ends(b)) else { return true };
        (b1 - a1).dot(f.tangent) <= 0.0 || (b2 - a2).dot(f.tangent) <= 0.0
    })
}

/// `v` turned about the unit `axis` by `angle` (Rodrigues).
fn rotate(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * (axis.dot(v) * (1.0 - c))
}

/// Sample each picked edge: its points, the way into each face there, and which way the faces
/// fold. `None` when any edge cannot be read, so the kernel draws instead of a half-right picture.
fn prepare(app: &SolveApp, inputs: &[Sel]) -> Option<Cached> {
    let state = crate::open_meshing::session_world(&app.session);
    let mut edges = Vec::new();
    let mut bounds = Aabb3::EMPTY;
    for sel in inputs {
        let Sel::Edge { body, index, .. } = sel else { return None };
        let mesh = state.body(body)?.mesh();
        let polyline = mesh.edges.get(*index)?;
        if polyline.len() < 2 {
            return None;
        }
        // Thin a finely tessellated edge down to something a frame can afford.
        let stride = polyline.len().div_ceil(MAX_SAMPLES).max(1);
        let mut samples = Vec::new();
        for (i, p) in polyline.iter().enumerate() {
            if i % stride != 0 && i + 1 != polyline.len() {
                continue;
            }
            let tangent = tangent_at(polyline, i)?;
            let sides = crate::canvas::edge_sides(&mesh, *index, *p)?;
            let [(n1, c1), (n2, c2)] = sides.as_slice() else { return None };
            // Square to the edge, lying in each face, pointing into it.
            let into = |n: Vec3, centroid: Vec3| -> Option<Vec3> {
                let u = n.cross(tangent).normalized()?;
                Some(if (centroid - *p).dot(u) < 0.0 { -u } else { u })
            };
            let (u1, u2) = (into(*n1, *c1)?, into(*n2, *c2)?);
            let opening = u1.dot(u2).clamp(-1.0, 1.0).acos();
            // Each face in front of the other's plane is an inside corner, where a round adds
            // material; behind it, an outside edge, where a round takes it away.
            let convex = (*c1 - *p).dot(*n2) + (*c2 - *p).dot(*n1) <= 0.0;
            bounds.add(*p);
            samples.push(Frame { at: *p, tangent, u1, u2, opening, convex });
        }
        if samples.len() < 2 {
            return None;
        }
        edges.push(EdgeFrames { samples });
    }
    Some(Cached { document: Arc::clone(&app.session.doc), revision: app.session.revision, inputs: inputs.to_vec(), edges, bounds })
}

/// The polyline's direction at point `i`, from whichever neighbour it has.
fn tangent_at(poly: &[Vec3], i: usize) -> Option<Vec3> {
    let next = poly.get(i + 1).copied();
    let prev = i.checked_sub(1).and_then(|k| poly.get(k)).copied();
    let here = poly.get(i).copied()?;
    match (prev, next) {
        (_, Some(b)) => (b - here).normalized(),
        (Some(a), None) => (here - a).normalized(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::scenario::Harness;
    use serde_json::json;

    /// A box's top front edge, 40 mm long, between two faces square to each other.
    fn boxed_edge(h: &Harness) -> Sel {
        let st = h.app.session.world_state().unwrap();
        let m = st.body("Body1").unwrap().mesh();
        let p = Vec3::new(20.0, 0.0, 20.0);
        let i = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).unwrap();
        Sel::Edge { body: "Body1".into(), index: i, point: p }
    }

    fn opened(command: &str) -> (Harness, Colors) {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let e = boxed_edge(&h);
        h.app.session.selection = vec![e];
        h.app.start(command);
        h.frames(2);
        (h, Colors::from_tokens(&crate::theme::Tokens::get()))
    }

    /// The sliver a round takes off a square outside edge is the corner left outside the
    /// quarter circle: (1 - pi/4) r^2 along the edge's length, and it is drawn through the body
    /// because that material is going away. The kernel is never asked.
    #[test]
    fn a_round_on_a_square_edge_draws_the_corner_it_will_take_off() {
        let (mut h, colors) = opened("solid.fillet");
        let mut preview = BlendPreview::default();
        for r in [2.0_f64, 4.0, 6.5] {
            if let crate::dialogs::Kind::Fillet { radius, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
                *radius = format!("{r} mm");
            }
            let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
            let built = preview.compute(&h.app, &cmds, colors).expect("the blend is drawn here").unwrap();
            // The ribbon is a surface, so its own signed volume is nil; what it bounds with the
            // corner is the sliver. Measure the sliver's cross-section from the drawn points.
            let want = (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 40.0;
            let drawn = sliver_volume(&built.scene.xray, r);
            assert!((drawn - want).abs() < want * 0.02, "r={r}: drew {drawn}, wanted about {want}");
            assert!(built.scene.tris.is_empty(), "an outside edge loses material, so nothing is added");
        }
    }

    /// The arc's points all sit `r` from the rolling ball's centre, which is what makes it a
    /// round rather than a flat: measured on the drawn ribbon, every point is that far out.
    fn sliver_volume(xray: &[u8], r: f64) -> f64 {
        // The ribbon's area in the cross-section, times the edge length, from the drawn arc.
        let pts = points(xray);
        assert!(!pts.is_empty(), "the ribbon is drawn");
        // Every drawn point lies on the arc, so it is r from the centre at (6, r, 20 - r)-ish:
        // instead of assuming the centre, use the known corner geometry of the box.
        let centre = Vec3::new(0.0, r, 20.0 - r);
        for p in &pts {
            let radial = Vec3::new(0.0, p.y - centre.y, p.z - centre.z);
            assert!((radial.len() - r).abs() < 0.02 * r, "a drawn point is {} from the centre, not {r}", radial.len());
        }
        (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 40.0
    }

    fn points(bytes: &[u8]) -> Vec<Vec3> {
        bytes
            .as_chunks::<{ crate::gpu::TRI_SIZE }>()
            .0
            .iter()
            .map(|v| {
                let n: Vec<_> = v.get(..12).unwrap().as_chunks::<4>().0.iter().map(|b| f64::from(f32::from_le_bytes(*b))).collect();
                Vec3::new(n[0], n[1], n[2])
            })
            .collect()
    }

    /// A chamfer draws the flat between the same two touch points, so its section is a straight
    /// line and its points are *not* all one distance from a centre.
    #[test]
    fn a_chamfer_draws_the_flat_between_the_same_two_touch_points() {
        let (mut h, colors) = opened("solid.chamfer");
        let mut preview = BlendPreview::default();
        if let crate::dialogs::Kind::Fillet { radius, .. } = &mut h.app.dialog.as_mut().unwrap().kind {
            *radius = "5 mm".into();
        }
        let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        let built = preview.compute(&h.app, &cmds, colors).expect("a chamfer is drawn too").unwrap();
        let pts = points(&built.scene.xray);
        assert!(!pts.is_empty(), "the flat is drawn");
        // The touch points are 5 mm along each face from the edge: (y 5, z 20) on the top and
        // (y 0, z 15) on the front. The flat between them is the plane z - y = 15.
        for p in &pts {
            assert!((p.z - p.y - 15.0).abs() < 0.05, "a chamfer point is off its flat: {p:?}");
        }
    }

    /// The quick picture is for a handle in motion only: the frame after it is let go the key
    /// changes, so the kernel is asked and its exact shape replaces this one. Without that, a
    /// drag would leave the approximation standing until OK.
    #[test]
    fn the_quick_picture_stands_only_while_the_handle_is_moving() {
        let (h, _) = opened("solid.fillet");
        let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
        crate::tool_modifiers::set_manipulator_dragging(false);
        assert!(!crate::preview::quick_blend(&cmds), "let go: the kernel draws");
        crate::tool_modifiers::set_manipulator_dragging(true);
        assert!(crate::preview::quick_blend(&cmds), "being dragged: the quick picture draws");
        // The two states key differently, which is what makes letting go recompute.
        let colors = Colors::from_tokens(&crate::theme::Tokens::get());
        let quick = crate::preview::job_key(&cmds, h.app.session.revision, colors, true);
        let exact = crate::preview::job_key(&cmds, h.app.session.revision, colors, false);
        assert_ne!(quick, exact, "the quick and exact previews are different jobs");
        // Another feature never reads the flag, so no other drag is recomputed when it ends.
        let shell = [("solid.shell".to_string(), serde_json::json!({"thickness": "2 mm"}))];
        assert!(!crate::preview::quick_blend(&shell));
        crate::tool_modifiers::set_manipulator_dragging(false);
    }

    /// A round cannot be wider than the corner it runs round: offset further into the face than
    /// that corner turns, its touch curve comes back inside out and the ribbon crosses itself,
    /// which is what a fillet of the whole top rim of a plate with rounded corners drew. The
    /// kernel refuses these and says what the largest radius is, so they are left to it.
    #[test]
    fn a_round_wider_than_the_corner_it_runs_round_falls_through_to_the_kernel() {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.run("solid.fillet", json!({"edges": [[0, 0, 10]], "radius": 2})).unwrap();
        // The arc at the top of that round: its midpoint is 2 - sqrt(2) in from each side.
        let inset = 2.0 - std::f64::consts::SQRT_2;
        let p = Vec3::new(inset, inset, 20.0);
        let st = h.app.session.world_state().unwrap();
        let m = st.body("Body1").unwrap().mesh();
        let i = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 0.1)).expect("the arc at the top of the round");
        h.app.session.selection = vec![Sel::Edge { body: "Body1".into(), index: i, point: p }];
        h.app.start("solid.fillet");
        h.frames(2);
        let colors = Colors::from_tokens(&crate::theme::Tokens::get());
        let mut preview = BlendPreview::default();
        let asking = |r: &str| [("solid.fillet".to_string(), json!({"edges": [[inset, inset, 20.0]], "radius": r}))];
        assert!(preview.compute(&h.app, &asking("1 mm"), colors).is_some(), "a round that fits inside the corner is drawn");
        assert!(preview.compute(&h.app, &asking("3 mm"), colors).is_none(), "a round wider than the corner is the kernel's");
    }

    /// Anything with its own profile along the edge is the kernel's to draw, and says so by
    /// answering `None` so the slower, exact path runs.
    #[test]
    fn the_shapes_it_cannot_draw_fall_through_to_the_kernel() {
        let (h, colors) = opened("solid.fillet");
        let mut preview = BlendPreview::default();
        let edges = json!([[20.0, 0.0, 20.0]]);
        for params in [
            json!({"edges": edges, "radius": "4 mm", "type": "variable", "radius2": "1 mm"}),
            json!({"edges": edges, "radius": "0 mm"}),
            json!({"edges": edges}),
        ] {
            let cmds = [("solid.fillet".to_string(), params.clone())];
            assert!(preview.compute(&h.app, &cmds, colors).is_none(), "should fall through: {params}");
        }
        // And a command it knows nothing about.
        let cmds = [("solid.shell".to_string(), json!({"thickness": "2 mm"}))];
        assert!(preview.compute(&h.app, &cmds, colors).is_none());
    }
}
