//! Move feedback from the moved bodies' own display triangles, without a scratch design. A move
//! changes no shape, so its preview needs no evaluation: the bodies' meshes are drawn moved by
//! the move's transform. The preview from a scratch design evaluated the move and meshed the
//! bodies again for every change, so while the triad was dragged the part trailed the arrow by
//! one evaluation, longer for a larger part.
//!
//! Where the view draws a layer ([`layered`]) and the bodies share one motion, the triangles are
//! not even moved here: they are built once where the bodies stand and the GPU draws them moved
//! ([`Layer`]), so a step of the triad's drag builds and uploads nothing. Building every moved
//! triangle again at each step, with the ghost of where the bodies were, took a large assembly
//! to ten frames a second.
use solidcraft_engine::geom::prelude::*;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::Vec3;
use solidcraft_engine::doc::{Document, IDENTITY, Mat, apply_point, apply_vector, expr::Kind, mat_inverse, mat_mul, rigid};
use solidcraft_engine::geom::{Aabb3, Mesh};

use crate::{
    SolveApp,
    gpu::GpuScene,
    preview::{Built, Colors, Layer},
};

/// Most triangles moved this way each frame; beyond them the scratch-design preview runs.
const MAX_TRIANGLES: usize = 2_000_000;

/// The most two bodies' motions may differ (per matrix entry, millimetres and rotation alike)
/// to be drawn as one layer.
const SAME_MOTION: f64 = 1e-9;

/// Tells one [`Cached`] from every other, in this run.
static CACHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Default)]
pub(crate) struct MovePreview {
    cached: Option<Cached>,
}

/// The world meshes of the bodies drawn (the listed bodies, and the bodies of the components
/// copied) and the frames that place them, for the design and revision they were taken from.
struct Cached {
    document: Arc<Document>,
    revision: u64,
    names: Vec<String>,
    meshes: Vec<Arc<Mesh>>,
    frames: Vec<Mat>,
    /// This cache and no other ([`CACHES`]): what a layer's scenes are built from.
    serial: u64,
    /// The points the meshes are drawn with where they stand, and the box around them: a
    /// layer's extent is theirs moved. Made when a layer first asks.
    rest: Option<(Aabb3, Vec<Vec3>)>,
}

/// Whether `commands` are a Move's preview (the one command this module draws).
pub(crate) fn is_move(commands: &[(String, Value)]) -> bool {
    match commands {
        [(id, _)] if id == "solid.move" => true,
        [(id, params)] if id == "timeline.redefine" => params.get("command").and_then(Value::as_str) == Some("solid.move"),
        _ => false,
    }
}

/// Whether the view draws a Move's bodies as a layer moved on the GPU
/// ([`crate::viewport::preview_layer_free`]); where not, they are built where they land.
pub(crate) fn layered(app: &SolveApp) -> bool {
    crate::viewport::preview_layer_free(app)
}

/// The motion all of `motions` are, within [`SAME_MOTION`]; none when they differ or there is none.
fn common(motions: &[Mat]) -> Option<Mat> {
    let first = motions.first()?;
    let same = |m: &Mat| m.iter().flatten().zip(first.iter().flatten()).all(|(a, b)| (a - b).abs() <= SAME_MOTION);
    motions.iter().all(same).then_some(*first)
}

/// What names a layer's scenes: the cache they are built from, the colours and whether the
/// bodies are copied (a copy leaves no ghost).
fn layer_key(serial: u64, colors: Colors, copy: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (serial, format!("{colors:?}"), copy).hash(&mut h);
    h.finish()
}

/// The points `draw` draws `mesh` with: each vertex of a triangle that has a normal, once.
fn drawn_points(mesh: &Mesh, out: &mut Vec<Vec3>) {
    let mut seen = vec![false; mesh.positions.len()];
    for k in mesh.triangles.iter().flatten() {
        let i = *k as usize;
        if let (Some(p), Some(_), Some(seen)) = (mesh.positions.get(i), mesh.normals.get(i), seen.get_mut(i))
            && !std::mem::replace(seen, true)
        {
            out.push(*p);
        }
    }
}

impl Cached {
    /// The box around the bodies moved by `motion`: the box of every drawn point moved, as
    /// building them moved finds it. A motion that turns nothing only shifts the box they have
    /// where they stand, which is the same box.
    fn moved_bounds(&mut self, motion: &Mat) -> Aabb3 {
        let meshes = &self.meshes;
        let (at_rest, points) = self.rest.get_or_insert_with(|| {
            let mut points = Vec::new();
            meshes.iter().for_each(|mesh| drawn_points(mesh, &mut points));
            let mut bounds = Aabb3::EMPTY;
            points.iter().for_each(|p| bounds.add(*p));
            (bounds, points)
        });
        let turns = (0..3).any(|c| (0..4).any(|r| motion[c][r] != IDENTITY[c][r])) || motion[3][3] != 1.0;
        let mut bounds = Aabb3::EMPTY;
        if turns {
            points.iter().for_each(|p| bounds.add(apply_point(motion, *p)));
        } else if !at_rest.is_empty() {
            bounds.add(apply_point(motion, at_rest.min));
            bounds.add(apply_point(motion, at_rest.max));
        }
        bounds
    }

    /// A layer's scenes: the ghost of where the bodies are (none for a copy, or with the cut
    /// tint off), which stays, and the bodies where they stand, which the GPU draws moved.
    fn rest_scenes(&self, colors: Colors, copy: bool) -> (GpuScene, GpuScene) {
        let (mut still, mut moving) = (GpuScene::default(), GpuScene::default());
        let removed = still.color(colors.removed);
        let mut bounds = Aabb3::EMPTY;
        for mesh in &self.meshes {
            draw(&mut moving, mesh, &IDENTITY, colors, &mut bounds);
            if !copy && colors.cut_tint {
                ghost(&mut still, mesh, removed);
            }
        }
        (still, moving)
    }
}

/// Where the components of `bodies` place them, the world for a body outside one.
fn placed_frames(app: &SolveApp, bodies: &[String]) -> Vec<Mat> {
    if app.session.doc.occurrences.is_empty() {
        return vec![IDENTITY; bodies.len()];
    }
    let frames = app.session.placed_document().body_frames(&app.session.model.state());
    bodies.iter().map(|name| frames.iter().find(|f| f.0 == *name).map_or(IDENTITY, |f| f.3)).collect()
}

impl MovePreview {
    pub(crate) fn clear(&mut self) {
        self.cached = None;
    }

    /// The preview of a `solid.move` (or its edit), or `None` to leave it to the scratch design:
    /// another command, values that don't evaluate, a body not shown, or too many triangles.
    pub(crate) fn compute(&mut self, app: &SolveApp, commands: &[(String, Value)], colors: Colors) -> Option<Result<Built, String>> {
        let [(id, params)] = commands else { return None };
        let params = match id.as_str() {
            "solid.move" => params,
            "timeline.redefine" if params.get("command").and_then(Value::as_str) == Some("solid.move") => params.get("params")?,
            _ => return None,
        };
        // A matrix is the engine's to read (`move_matrix_params`).
        if params.get("matrix").is_some() {
            return None;
        }
        let doc = &app.session.doc;
        let length = |v: &Value| match v {
            Value::Number(n) => n.as_f64(),
            Value::String(e) => doc.eval(e, Kind::Length).ok(),
            _ => None,
        };
        let translate = match params.get("translate") {
            Some(Value::Array(a)) if a.len() == 3 => Vec3::new(length(&a[0])?, length(&a[1])?, length(&a[2])?),
            None => Vec3::ZERO,
            _ => return None,
        };
        let origin = match params.get("origin") {
            Some(Value::Array(a)) if a.len() == 3 => Some(Vec3::new(length(&a[0])?, length(&a[1])?, length(&a[2])?)),
            None => None,
            _ => return None,
        };
        let given_axis = match params.get("axis") {
            Some(Value::Array(a)) if a.len() == 3 => Some(Vec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?)),
            None => None,
            _ => return None,
        };
        let axis = given_axis.unwrap_or(Vec3::Z);
        let angle = match params.get("angle") {
            None | Some(Value::Null) => 0.0,
            Some(Value::String(e)) => doc.eval(e, Kind::Angle).ok()?,
            Some(Value::Number(n)) => doc.eval(&n.to_string(), Kind::Angle).ok()?,
            _ => return None,
        };
        if !(translate.is_finite() && axis.is_finite() && angle.is_finite() && origin.is_none_or(|o| o.is_finite()))
            || (angle != 0.0 && axis.normalized().is_none())
        {
            return None;
        }
        let bodies: Vec<String> = params.get("bodies")?.as_array()?.iter().map(|b| b.as_str().map(str::to_string)).collect::<Option<_>>()?;
        // The components copied (`occurrences`): their bodies are drawn moved, as copies of them.
        let components: Vec<Vec<u64>> = match params.get("occurrences") {
            None => Vec::new(),
            Some(Value::Array(list)) => {
                list.iter().map(|path| path.as_array()?.iter().map(Value::as_u64).collect::<Option<Vec<u64>>>()).collect::<Option<_>>()?
            }
            Some(_) => return None,
        };
        if bodies.is_empty() && components.is_empty() {
            return None;
        }
        let copy = params.get("copy").and_then(Value::as_bool) == Some(true);
        // Only a copy takes components (the engine refuses them otherwise).
        if !components.is_empty() && !copy {
            return None;
        }
        let mut names = bodies.clone();
        for path in &components {
            names.extend(app.session.occurrence_bodies(path, true).ok()?);
        }
        if self.cached.as_ref().is_none_or(|c| c.revision != app.session.revision || !Arc::ptr_eq(&c.document, &app.session.doc) || c.names != names)
        {
            let state = crate::open_meshing::session_world(&app.session);
            let meshes: Vec<Arc<Mesh>> = names.iter().map(|n| state.body(n).map(|b| b.mesh())).collect::<Option<_>>()?;
            if meshes.iter().map(|m| m.triangles.len()).sum::<usize>() > MAX_TRIANGLES {
                return None;
            }
            // A component's copy moves in the world (`occurrence.move`), so its bodies take the world's.
            let mut frames = placed_frames(app, &bodies);
            frames.resize(names.len(), IDENTITY);
            self.cached = Some(Cached {
                document: app.session.doc.clone(),
                revision: app.session.revision,
                names: names.clone(),
                meshes,
                frames,
                serial: CACHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                rest: None,
            });
        }
        let cache = self.cached.as_mut()?;
        // The move in the world as a body in `frame` takes it: about the given origin and axis,
        // which are in the world; without an origin, about the frame's origin and, without an
        // axis too, its Z (`frames::map_local`). A component's copy has the world's frame.
        let world = |frame: &Mat| {
            let o = origin.unwrap_or_else(|| apply_point(frame, Vec3::ZERO));
            let a = given_axis.unwrap_or_else(|| if origin.is_some() { Vec3::Z } else { apply_vector(frame, Vec3::Z) });
            rigid(translate, o, a, angle)
        };
        // A solid.move splits by frame and takes the values into each body's own
        // (`frames::split_move`), so each body moves as `world` says. An edit takes them into the
        // first body's frame, and the feature applies that local move to every body in its own.
        let first = *cache.frames.first()?;
        let in_first = mat_mul(&mat_mul(&mat_inverse(&first)?, &world(&first)), &first);
        let edit = id == "timeline.redefine";
        let motion = |frame: &Mat| if edit { Some(mat_mul(&mat_mul(frame, &in_first), &mat_inverse(frame)?)) } else { Some(world(frame)) };
        let motions: Vec<Mat> = cache.frames.iter().map(motion).collect::<Option<_>>()?;
        let replaced = if copy { Vec::new() } else { bodies };
        let built = |scene: GpuScene, bounds: Aabb3, layer: Option<Layer>| Built {
            scene,
            sketches: Vec::new(),
            applied: None,
            stopped: None,
            placed: None,
            tapered: Vec::new(),
            replaced,
            error: None,
            bounds,
            layer,
        };
        // One motion for every body, in a view that draws a layer: nothing is built at this
        // change unless the bodies, the colours or the ghost changed since the scenes were made.
        if layered(app)
            && let Some(motion) = common(&motions)
        {
            let key = layer_key(cache.serial, colors, copy);
            let scenes = (app.preview.layer_shown() != Some(key)).then(|| cache.rest_scenes(colors, copy));
            let bounds = cache.moved_bounds(&motion);
            return Some(Ok(built(GpuScene::default(), bounds, Some(Layer { key, scenes, motion }))));
        }
        let mut scene = GpuScene::default();
        let removed = scene.color(colors.removed);
        let mut bounds = Aabb3::EMPTY;
        for (mesh, m) in cache.meshes.iter().zip(&motions) {
            draw(&mut scene, mesh, m, colors, &mut bounds);
            if !copy && colors.cut_tint {
                ghost(&mut scene, mesh, removed);
            }
        }
        Some(Ok(built(scene, bounds, None)))
    }
}

/// Where a body was, as the scratch-design preview shows a body moved away: `mesh` where it
/// stands, as removed material in the colour `removed`.
fn ghost(scene: &mut GpuScene, mesh: &Mesh, removed: crate::gpu::PaletteIndex) {
    for t in &mesh.triangles {
        for k in t {
            if let (Some(p), Some(n)) = (mesh.positions.get(*k as usize), mesh.normals.get(*k as usize)) {
                scene.ghost_tri(p.to_f32(), n.to_f32(), removed);
            }
        }
    }
}

/// `mesh` moved by `m`, in the colour of geometry the feature adds, or the body's own where
/// the preference leaves new surface untinted (a move makes none either way), with its shaded
/// edges.
fn draw(scene: &mut GpuScene, mesh: &Mesh, m: &Mat, colors: Colors, bounds: &mut Aabb3) {
    let added = scene.color(if colors.tint_new { colors.added } else { colors.body });
    for t in &mesh.triangles {
        for k in t {
            let i = *k as usize;
            if let (Some(p), Some(n)) = (mesh.positions.get(i), mesh.normals.get(i)) {
                let p = apply_point(m, *p);
                bounds.add(p);
                scene.tri(p.to_f32(), apply_vector(m, *n).to_f32(), added);
            }
        }
    }
    let edge = scene.color(colors.edge);
    for (ei, e) in mesh.edges.iter().enumerate() {
        if !mesh.shaded_edge_visible(ei) {
            continue;
        }
        for w in e.windows(2) {
            scene.line(apply_point(m, w[0]).to_f32(), apply_point(m, w[1]).to_f32(), edge, 1.3, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, SolveApp};
    use serde_json::json;
    use solidcraft_engine::Session;

    fn app() -> SolveApp {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        SolveApp::new(s, Services::default())
    }

    fn colors(app: &SolveApp) -> Colors {
        let _ = app;
        Colors::from_tokens(&solidcraft_theme::Tokens::get())
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.dist(b) < 1e-9
    }

    /// `app` with its model drawn on a GPU: a Move's bodies are then a layer ([`layered`]).
    fn on_gpu(mut app: SolveApp) -> SolveApp {
        app.viewport.gpu =
            Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        app
    }

    /// Where the triangle vertices of `bytes` are (a scene's `tris` or `ghost`).
    fn positions(bytes: &[u8]) -> Vec<Vec3> {
        let read = |v: &[u8], at: usize| v.get(at..at + 4).map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        bytes.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().filter_map(|v| Some(Vec3::new(read(v, 0)?, read(v, 4)?, read(v, 8)?))).collect()
    }

    /// A plate and a pin beside it, both at the root.
    fn plate_and_pin() -> SolveApp {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 30, "width": 20, "height": 10, "body_name": "Plate"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius": 6, "height": 25, "base": [45, 10, 0], "operation": "new", "body_name": "Pin"})).unwrap();
        let mut app = SolveApp::new(s, Services::default());
        app.set_origin_shown(false);
        app
    }

    /// A turn about a point and a move, of both bodies of [`plate_and_pin`].
    fn turn_and_move() -> [(String, Value); 1] {
        let p = json!({"bodies": ["Plate", "Pin"], "origin": ["15 mm", "10 mm", "0 mm"], "axis": [0, 0, 1], "angle": "35 deg", "translate": ["12 mm", "-8 mm", "5 mm"]});
        [("solid.move".to_string(), p)]
    }

    /// On a GPU a Move's bodies are a layer: their triangles where they stand, drawn moved by the
    /// motion that puts them where building them moved did, with the same extent, and the ghost
    /// left where they are. A copy leaves no ghost, and neither does the cut tint turned off.
    #[test]
    fn a_move_on_a_gpu_is_a_layer_of_the_bodies_where_they_stand() {
        let (plain, gpu, c) = (plate_and_pin(), on_gpu(plate_and_pin()), colors(&plate_and_pin()));
        let cmds = turn_and_move();
        let built = MovePreview::default().compute(&plain, &cmds, c).expect("taken here").expect("built");
        assert!(built.layer.is_none() && !built.scene.tris.is_empty(), "without a GPU the bodies are built where they land");
        let layered = MovePreview::default().compute(&gpu, &cmds, c).expect("taken here").expect("built");
        assert_eq!(layered.replaced, built.replaced);
        assert!(
            near(layered.bounds.min, built.bounds.min) && near(layered.bounds.max, built.bounds.max),
            "{:?} vs {:?}",
            layered.bounds,
            built.bounds
        );
        let layer = layered.layer.expect("a layer");
        let (still, moving) = layer.scenes.expect("its scenes, as none are shown");
        let landed: Vec<Vec3> = positions(&moving.tris).iter().map(|p| apply_point(&layer.motion, *p)).collect();
        let expected = positions(&built.scene.tris);
        assert!(!landed.is_empty() && landed.len() == expected.len(), "{} vs {}", landed.len(), expected.len());
        assert!(landed.iter().zip(&expected).all(|(a, b)| a.dist(*b) < 1e-3), "every triangle lands where it was built");
        assert_eq!(moving.lines.len(), built.scene.lines.len(), "and every edge");
        assert!(!still.ghost.is_empty() && positions(&still.ghost) == positions(&built.scene.ghost), "the ghost stays where the bodies are");
        assert!(still.tris.is_empty() && still.lines.is_empty() && moving.ghost.is_empty());
        // A copy keeps the originals: no ghost, nothing replaced.
        let mut copy = turn_and_move();
        copy[0].1["copy"] = json!(true);
        let copied = MovePreview::default().compute(&gpu, &copy, c).expect("taken here").expect("built");
        let (still, moving) = copied.layer.expect("a layer").scenes.expect("its scenes");
        assert!(copied.replaced.is_empty() && still.ghost.is_empty() && !moving.tris.is_empty());
        // Removed material left transparent: no ghost.
        let (still, _) =
            MovePreview::default().compute(&gpu, &cmds, Colors { cut_tint: false, ..c }).unwrap().unwrap().layer.unwrap().scenes.unwrap();
        assert!(still.ghost.is_empty());
    }

    /// A move that turns nothing only shifts the bodies' box; the extent is the moved bodies'
    /// either way, and an empty selection of triangles has none.
    #[test]
    fn a_layers_extent_is_the_moved_bodies() {
        let gpu = on_gpu(plate_and_pin());
        let c = colors(&gpu);
        for p in [
            json!({"bodies": ["Plate", "Pin"], "translate": ["12 mm", "-8 mm", "5 mm"]}),
            json!({"bodies": ["Plate", "Pin"], "translate": [0, 0, 0]}),
            json!({"bodies": ["Pin"], "origin": [0, 0, 0], "axis": [1, 0, 0], "angle": "90 deg", "translate": [0, 0, 0]}),
            turn_and_move()[0].1.clone(),
        ] {
            let built = MovePreview::default().compute(&gpu, &[("solid.move".to_string(), p.clone())], c).expect("taken here").expect("built");
            assert!(built.layer.is_some(), "{p}");
            let mut s = gpu.session.scratch();
            s.execute("solid.move", &p).unwrap();
            let world = s.world_state().unwrap();
            let moved = p["bodies"]
                .as_array()
                .unwrap()
                .iter()
                .fold(Aabb3::EMPTY, |bb, n| bb.union(&world.body(n.as_str().unwrap()).unwrap().mesh().bounds()));
            assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{p}: {:?} vs {moved:?}", built.bounds);
        }
    }

    /// The scenes are made again only for what they are built from: other bodies, other colours,
    /// a copy. A change of the motion alone leaves them out, once the preview shows them.
    #[test]
    fn a_layers_scenes_are_built_once_for_their_bodies_and_colours() {
        let mut gpu = on_gpu(plate_and_pin());
        let c = colors(&gpu);
        let mut pv = MovePreview::default();
        let at = |x: f64| [("solid.move".to_string(), json!({"bodies": ["Plate", "Pin"], "translate": [x, 0, 0]}))];
        let layer = |pv: &mut MovePreview, app: &SolveApp, cmds: &[(String, Value)], c: Colors| {
            pv.compute(app, cmds, c).unwrap().unwrap().layer.expect("a layer")
        };
        let first = layer(&mut pv, &gpu, &at(10.0), c);
        assert!(first.scenes.is_some() && first.motion[3][0] == 10.0);
        // Not shown yet (the preview state has not taken them): built again for the same key.
        let again = layer(&mut pv, &gpu, &at(20.0), c);
        assert!(again.key == first.key && again.scenes.is_some());
        // Shown: a step is its motion alone.
        gpu.preview.show_layer_for_tests(first.key);
        let step = layer(&mut pv, &gpu, &at(30.0), c);
        assert!(step.key == first.key && step.scenes.is_none() && step.motion[3][0] == 30.0);
        // Other colours, a copy, other bodies: other scenes.
        let tinted = layer(&mut pv, &gpu, &at(30.0), Colors { tint_new: false, ..c });
        assert!(tinted.key != first.key && tinted.scenes.is_some());
        let mut copy = at(30.0);
        copy[0].1["copy"] = json!(true);
        let copied = layer(&mut pv, &gpu, &copy, c);
        assert!(copied.key != first.key && copied.scenes.is_some());
        let one = [("solid.move".to_string(), json!({"bodies": ["Pin"], "translate": [30, 0, 0]}))];
        let other = layer(&mut pv, &gpu, &one, c);
        assert!(other.key != first.key && other.scenes.is_some());
        // Back to the first bodies: a cache of its own, so never the first key (its scenes are gone).
        let back = layer(&mut pv, &gpu, &at(30.0), c);
        assert!(back.key != first.key && back.scenes.is_some());
    }

    /// Where the view draws no layer, or the bodies move differently, they are built where they
    /// land as before: without a GPU, with a component moved and not captured (its drag has the
    /// renderer's layer), with isocurves shown, and for bodies in differently placed components
    /// turned about their own origins.
    #[test]
    fn a_move_is_built_where_it_lands_when_it_cannot_be_a_layer() {
        let c = colors(&plate_and_pin());
        let cmds = turn_and_move();
        let built = |app: &SolveApp, cmds: &[(String, Value)]| MovePreview::default().compute(app, cmds, c).expect("taken here").expect("built");
        assert!(built(&plate_and_pin(), &cmds).layer.is_none(), "no GPU");
        let mut curves = on_gpu(plate_and_pin());
        assert!(layered(&curves) && built(&curves, &cmds).layer.is_some());
        curves.session.isocurves.push(solidcraft_engine::IsoPick { body: "Plate".into(), at: Vec3::new(15.0, 10.0, 10.0), count: 3 });
        assert!(!layered(&curves));
        let whole = built(&curves, &cmds);
        assert!(whole.layer.is_none() && !whole.scene.tris.is_empty() && !whole.scene.ghost.is_empty(), "isocurves shown");
        // Two placements of a part, one turned: a turn about each body's own origin differs.
        let mut s = Session::default();
        let part = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap()["occurrence"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let copy = s.execute("occurrence.copy", &json!({"occurrence": part})).unwrap()["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": copy, "axis": [0, 0, 1], "angle": "30 deg", "translate": [60, 0, 0]})).unwrap();
        let names: Vec<String> = [part, copy].iter().map(|o| s.occurrence_bodies(&[*o], true).unwrap()[0].clone()).collect();
        let app = on_gpu(SolveApp::new(s, Services::default()));
        let turned = [("solid.move".to_string(), json!({"bodies": names, "axis": [0, 1, 0], "angle": "25 deg"}))];
        if let Some(Ok(placed)) = MovePreview::default().compute(&app, &turned, c) {
            assert!(placed.layer.is_none() && !placed.scene.tris.is_empty(), "bodies that move differently are built where they land");
        }
        // The same bodies moved alike are a layer.
        let alike = [("solid.move".to_string(), json!({"bodies": names, "translate": [5, 0, 0]}))];
        if let Some(Ok(placed)) = MovePreview::default().compute(&app, &alike, c) {
            assert!(placed.layer.is_some(), "one motion for both");
        }
        // A component moved and not captured: its drag has, or can take, the renderer's layer.
        let mut dragged = on_gpu(SolveApp::new(app.session.scratch(), Services::default()));
        let base = dragged.session.doc.occurrences.iter().find(|o| o.id == copy).map(|o| o.transform).unwrap();
        dragged.run("occurrence.move", json!({"occurrence": copy, "base_transform": base, "translate": [4, 0, 0], "capture": false})).unwrap();
        assert!(!dragged.session.pending_moves.is_empty() && !layered(&dragged), "a pending component move keeps the layer");
    }

    /// Hostile commands never make a layer: what the preview cannot take is refused before one
    /// is thought of, on a GPU as without one.
    #[test]
    fn what_it_cannot_take_makes_no_layer_on_a_gpu() {
        let gpu = on_gpu(app());
        let c = colors(&gpu);
        let mut pv = MovePreview::default();
        for p in [
            json!({"bodies": ["Nope"], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "translate": ["width + ?", 0, 0]}),
            json!({"bodies": ["Plate"], "translate": [1e308, 1e308, f64::MAX]}),
            json!({"bodies": [], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "axis": [0, 0, 0], "angle": "90 deg"}),
            json!({"bodies": ["Plate"], "occurrences": [[1]], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "matrix": [0, -1, 0, 30, 1, 0, 0, 0, 0, 0, 1, 0]}),
        ] {
            let taken = pv.compute(&gpu, &[("solid.move".to_string(), p.clone())], c);
            assert!(
                !matches!(&taken, Some(Ok(b)) if b.layer.as_ref().is_some_and(|l| !l.motion.iter().flatten().all(|v| v.is_finite()))),
                "{p}: a motion that is not finite"
            );
        }
        assert!(pv.compute(&gpu, &[("solid.move".to_string(), json!({"bodies": ["Nope"], "translate": [1, 0, 0]}))], c).is_none());
        assert!(is_move(&[("solid.move".to_string(), json!({}))]));
        assert!(is_move(&[("timeline.redefine".to_string(), json!({"command": "solid.move"}))]));
        assert!(!is_move(&[("timeline.redefine".to_string(), json!({"command": "solid.fillet"}))]));
        assert!(!is_move(&[]) && !is_move(&[("solid.extrude".to_string(), json!({}))]));
        let two = [("solid.move".to_string(), json!({})), ("solid.move".to_string(), json!({}))];
        assert!(!is_move(&two), "one command only");
    }

    /// The layer looks as the bodies built where they land did: the same picture, but for the
    /// odd pixel on an outline (the GPU moves single-precision points). Skipped without a GPU
    /// adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_move_drawn_as_a_layer_looks_as_it_did_built_where_it_lands() {
        const SIZE: u32 = 192;
        let (mut plain, gpu) = (plate_and_pin(), on_gpu(plate_and_pin()));
        let c = colors(&plain);
        let cmds = turn_and_move();
        plain.fit_view();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE as f32, SIZE as f32));
        let proj = crate::viewport::projection(&plain, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |preview: GpuScene, moving: Option<(GpuScene, [[f32; 4]; 4])>| {
            crate::gpu::render_offscreen_preview(GpuScene::default(), preview, moving, proj.vp.to_f32(), proj.cam.back().to_f32(), eye, SIZE)
        };
        let built = MovePreview::default().compute(&plain, &cmds, c).unwrap().unwrap();
        let layer = MovePreview::default().compute(&gpu, &cmds, c).unwrap().unwrap().layer.expect("a layer");
        let (still, moving) = layer.scenes.expect("its scenes");
        let transform: [[f32; 4]; 4] = std::array::from_fn(|col| std::array::from_fn(|row| layer.motion[col][row] as f32));
        let (Some(none), Some(landed), Some(layered)) =
            (render(GpuScene::default(), None), render(built.scene, None), render(still, Some((moving, transform))))
        else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let differ = |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
        let drawn = differ(&none, &landed);
        assert!(drawn > 500, "the preview draws the moved bodies and their ghost: {drawn} pixels");
        let off = differ(&landed, &layered);
        assert!(off * 50 < drawn, "{off} of the {drawn} pixels the preview draws differ as a layer");
    }

    /// A move's preview is the body's own triangles moved as the Move feature moves the body:
    /// no scratch design, the original hidden (replaced), and where it lands is where the
    /// feature puts it.
    #[test]
    fn a_move_is_previewed_from_the_bodys_own_triangles() {
        let app = app();
        let c = colors(&app);
        let mut pv = MovePreview::default();
        let cmds =
            [("solid.move".to_string(), json!({"bodies": ["Plate"], "translate": ["30 mm", "0 mm", "0 mm"], "axis": [0, 0, 1], "angle": "90 deg"}))];
        let built = pv.compute(&app, &cmds, c).expect("taken here").expect("built");
        assert_eq!(built.replaced, vec!["Plate".to_string()]);
        // The Move feature on the same body: the preview's extent is the moved body's.
        let mut s = app.session.scratch();
        s.execute("solid.move", &cmds[0].1).unwrap();
        let moved = crate::open_meshing::session_world(&s).body("Plate").unwrap().mesh().bounds();
        assert!(near(built.bounds.min, moved.min) && near(built.bounds.max, moved.max), "{:?} vs {moved:?}", built.bounds);
        // A copy keeps the original shown.
        let copy = [("solid.move".to_string(), json!({"bodies": ["Plate"], "translate": [0, 0, 10], "copy": true}))];
        assert!(pv.compute(&app, &copy, c).unwrap().unwrap().replaced.is_empty());
    }

    /// A turn about a given origin is drawn about that point: the Move dialog names its pivot as
    /// `origin` and leaves `translate` alone, so a preview about the world origin drew the body
    /// far from where it lands. Checked at the root and on a body in a turned, placed component,
    /// where a given origin is in the world too.
    #[test]
    fn a_turn_about_a_given_origin_is_previewed_where_the_move_puts_it() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [100, 0, 0], "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "axis": [0, 0, 1], "angle": "30 deg", "translate": [-80, 20, 20]})).unwrap();
        let peg = s.occurrence_bodies(&[occurrence], true).unwrap()[0].clone();
        let app = SolveApp::new(s, Services::default());
        for body in ["Plate".to_string(), peg] {
            let p =
                json!({"bodies": [body], "origin": ["120 mm", "15 mm", "10 mm"], "axis": [1, 0, 0], "angle": "25 deg", "translate": ["5 mm", 0, 0]});
            let built =
                MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
            let mut s = app.session.scratch();
            s.execute("solid.move", &p).unwrap();
            let moved = s.world_state().unwrap().body(&body).unwrap().mesh().bounds();
            assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{body}: {:?} vs {moved:?}", built.bounds);
        }
    }

    /// A body in a turned, placed component turns about its component's origin, not the world's:
    /// previewed about the world's, it was drawn far from where the feature puts it.
    #[test]
    fn a_placed_body_is_previewed_where_the_feature_puts_it() {
        let mut s = Session::default();
        s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("occurrence.move", &json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "30 deg", "translate": [130, 20, 20]})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let app = SolveApp::new(s, Services::default());
        let cmds =
            [("solid.move".to_string(), json!({"bodies": ["Body1"], "translate": ["10 mm", "-5 mm", "0 mm"], "axis": [1, 0, 0], "angle": "25 deg"}))];
        let built = MovePreview::default().compute(&app, &cmds, colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        s.execute("solid.move", &cmds[0].1).unwrap();
        let moved = s.world_state().unwrap().body("Body1").unwrap().mesh().bounds();
        assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{:?} vs {moved:?}", built.bounds);
    }

    /// Anything it can't take is left to the scratch-design preview.
    #[test]
    fn what_it_cannot_take_goes_to_the_full_preview() {
        let app = app();
        let c = colors(&app);
        let mut pv = MovePreview::default();
        for p in [
            json!({"bodies": ["Nope"], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "translate": ["width + ?", 0, 0]}),
            json!({"bodies": ["Plate"], "translate": [1, 0]}),
            json!({"bodies": [], "translate": [1, 0, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": ["width + ?", 0, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": [1, 0]}),
            json!({"bodies": ["Plate"], "angle": "90 deg", "origin": "here"}),
            json!({"bodies": ["Plate"], "matrix": [0, -1, 0, 30, 1, 0, 0, 0, 0, 0, 1, 0]}),
        ] {
            assert!(pv.compute(&app, &[("solid.move".to_string(), p.clone())], c).is_none(), "{p}");
        }
        assert!(pv.compute(&app, &[("solid.extrude".to_string(), json!({}))], c).is_none());
    }

    /// A copy of a component is drawn where the copy lands: the component's bodies moved, the
    /// component itself left where it was, so the preview is not the scratch design's.
    #[test]
    fn a_component_copy_is_previewed_where_the_copy_lands() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        let app = SolveApp::new(s, Services::default());
        let c = colors(&app);
        let mut pv = MovePreview::default();
        // The component's box is 0..20 along X; the copy lands 50 mm along.
        let copy = [("solid.move".to_string(), json!({"bodies": [], "occurrences": [[occurrence]], "copy": true, "translate": [50, 0, 0]}))];
        let built = pv.compute(&app, &copy, c).expect("taken here").expect("built");
        assert!(built.replaced.is_empty(), "a copy keeps the original shown");
        assert!(near(Vec3::new(built.bounds.min.x, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0)), "{:?}", built.bounds);
        assert!(near(Vec3::new(built.bounds.max.x, 0.0, 0.0), Vec3::new(70.0, 0.0, 0.0)), "{:?}", built.bounds);
        // With bodies listed too, the component's copy is drawn alongside the body's: the
        // component's box is 15 mm tall, the Plate 5 mm, so the height shows it is there.
        let mixed = [("solid.move".to_string(), json!({"bodies": ["Plate"], "occurrences": [[occurrence]], "copy": true, "translate": [50, 0, 0]}))];
        let built = pv.compute(&app, &mixed, c).expect("taken here").expect("built");
        assert!(near(Vec3::new(built.bounds.min.x, 0.0, 0.0), Vec3::new(50.0, 0.0, 0.0)), "{:?}", built.bounds);
        assert!(near(Vec3::new(0.0, 0.0, built.bounds.max.z), Vec3::new(0.0, 0.0, 15.0)), "{:?}", built.bounds);
    }

    /// A turned copy of a placed component turns about the world origin, as `occurrence.move`
    /// turns it, not about the origin of the component's frame as a body of it would.
    #[test]
    fn a_turned_component_copy_is_previewed_where_the_copy_lands() {
        let mut s = Session::default();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "translate": [100, 0, 0]})).unwrap();
        let app = SolveApp::new(s, Services::default());
        let p = json!({"bodies": [], "occurrences": [[occurrence]], "copy": true, "translate": [10, 0, 0], "axis": [0, 0, 1], "angle": "90 deg"});
        let built = MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        let copy = s.execute("solid.move", &p).unwrap()["occurrences"][0].as_u64().unwrap();
        let mut landed = Aabb3::EMPTY;
        for name in s.occurrence_bodies(&[copy], true).unwrap() {
            landed = landed.union(&s.world_state().unwrap().body(&name).unwrap().mesh().bounds());
        }
        assert!(built.bounds.min.dist(landed.min) < 1e-6 && built.bounds.max.dist(landed.max) < 1e-6, "{:?} vs {landed:?}", built.bounds);
    }

    /// Bodies in differently turned frames move apart in the world (`frames::split_move`), each by
    /// the world vector: drawn with the first body's frame, the other moved along its own axes.
    #[test]
    fn bodies_in_differently_turned_frames_are_previewed_where_the_move_puts_them() {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 5, "body_name": "Plate"})).unwrap();
        let made = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 10, "height": 15, "body_name": "Peg"})).unwrap();
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        let occurrence = made["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": occurrence, "axis": [0, 0, 1], "angle": "90 deg", "translate": [100, 0, 0]})).unwrap();
        let peg = s.occurrence_bodies(&[occurrence], true).unwrap()[0].clone();
        let app = SolveApp::new(s, Services::default());
        let p = json!({"bodies": ["Plate", peg], "translate": [30, 0, 0]});
        let built = MovePreview::default().compute(&app, &[("solid.move".to_string(), p.clone())], colors(&app)).expect("taken here").expect("built");
        let mut s = app.session.scratch();
        s.execute("solid.move", &p).unwrap();
        let st = s.world_state().unwrap();
        let moved = st.body("Plate").unwrap().mesh().bounds().union(&st.body(&peg).unwrap().mesh().bounds());
        assert!(built.bounds.min.dist(moved.min) < 1e-6 && built.bounds.max.dist(moved.max) < 1e-6, "{:?} vs {moved:?}", built.bounds);
    }
}
