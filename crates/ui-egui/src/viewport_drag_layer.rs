//! A component dragged with its Move triad, drawn as a layer of its own.
//!
//! Each step of the drag places the component anew, and drawing it moved every mesh of the
//! component, built the model scene again and uploaded it: much of each frame on a large
//! assembly. Instead the dragged bodies are drawn from the scene built where the drag found them,
//! moved on the GPU by the drag's motion, and the rest of the model stays as it was, so a step
//! meshes, builds and uploads no geometry. The model scene is whole again as soon as anything
//! else changes: the move captured, the design edited, a section or an analysis shown.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use solidcraft_engine::doc::{Document, Mat, ModelState};
use solidcraft_engine::geom::Aabb3;

use super::{GpuScene, SolveApp};
use crate::gpu::MovingLayer;

/// The most a dragged body's move may differ from the others' (per matrix entry, millimetres
/// and rotation alike) to be drawn with them.
const SAME_MOVE: f64 = 1e-7;

/// The world a model scene was built from, with its design and everything else it showed but its
/// display refinement ([`super::base_scene_key`]: a layer keeps the refinement it began with).
#[derive(Clone)]
pub(crate) struct SceneWorld {
    world: Arc<ModelState>,
    doc: Arc<Document>,
    base: u64,
}

/// A drag drawn as a layer: the model scene without the dragged bodies, and the dragged bodies'
/// own scene drawn moved by `delta`, both built from `from`.
pub(crate) struct DragLayer {
    from: SceneWorld,
    /// The world shown now, drawn by this layer.
    shown: Arc<ModelState>,
    /// The boxes of `from`'s bodies.
    boxes: Arc<Vec<(String, Aabb3)>>,
    /// For each body of `from`: dragged.
    moving: Vec<bool>,
    /// The layer's scenes: the same while the same bodies are dragged from the same world.
    key: u64,
    /// The drag's motion from `from` to the world shown now.
    delta: Mat,
    /// The model scene without the dragged bodies (built once).
    statics: Option<GpuScene>,
}

/// A whole model scene was built from the world shown now: a drag starting next draws from it.
pub(super) fn whole_scene_built(app: &mut SolveApp) {
    app.viewport.scene_world =
        Some(SceneWorld { world: crate::open_meshing::shown_world(app), doc: app.session.doc.clone(), base: super::base_scene_key(app) });
}

/// Follow a component's drag: whether this frame draws it as a layer.
pub(super) fn follow(app: &mut SolveApp) -> bool {
    let layer = app.viewport.drag.take();
    if drawable(app) {
        app.viewport.drag = next_layer(app, layer);
    }
    app.viewport.drag.is_some()
}

fn transform(delta: &Mat) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| delta[c][r] as f32))
}

/// A drag can be drawn as a layer: the model is drawn on the GPU, components are moved without
/// being captured, and no section or analysis is shown (their outlines and colours follow the
/// bodies' places).
fn drawable(app: &SolveApp) -> bool {
    app.viewport.gpu.is_some()
        && !app.session.pending_moves.is_empty()
        && app.session.analysis.is_none()
        && app.session.continuity.is_empty()
        && super::section_plane(app).is_none()
}

/// The layer drawing the world shown now: the one under way when its bodies are still dragged
/// (back where they began too), else one from the last whole scene's world; none when the world
/// is not that world with one rigid motion of some of its bodies, or nothing is dragged yet. The
/// world's body boxes are those of the layer's world, the dragged ones moved, so no step works out
/// a box from a mesh.
fn next_layer(app: &SolveApp, layer: Option<DragLayer>) -> Option<DragLayer> {
    let base = super::base_scene_key(app);
    let now = crate::open_meshing::shown_world(app);
    let valid = |w: &SceneWorld| w.base == base && Arc::ptr_eq(&w.doc, &app.session.doc) && !Arc::ptr_eq(&w.world, &now);
    // The same step of the drag (a frame without a move) draws as it did.
    let layer = match layer {
        Some(l) if Arc::ptr_eq(&l.shown, &now) && valid(&l.from) => return Some(l),
        other => other,
    };
    let (from, before) = match layer {
        Some(l) if valid(&l.from) => (l.from.clone(), Some(l)),
        _ => (app.viewport.scene_world.clone().filter(valid)?, None),
    };
    let moved = moved_bodies(&from.world, &now)?;
    let moved = match &before {
        Some(l) => still_dragged(moved, &l.moving),
        None if !moved.moving.contains(&true) => return None,
        None => moved,
    };
    let boxes = match &before {
        Some(l) => l.boxes.clone(),
        None => super::body_bounds(app, &from.world),
    };
    if let Some(now_boxes) = moved_boxes(&boxes, &moved) {
        super::known_body_bounds(app, &now, now_boxes);
    }
    let key = layer_key(&from, &moved.moving);
    let statics = before.filter(|l| l.key == key).and_then(|l| l.statics);
    Some(DragLayer { from, shown: now, boxes, moving: moved.moving, key, delta: moved.delta, statics })
}

/// The world shown now as the layer's world with some of its bodies moved.
struct Moved {
    /// For each body of the layer's world: moved.
    moving: Vec<bool>,
    /// For each body of the world shown now: its place in the layer's world.
    from: Vec<usize>,
    /// The moved bodies' common motion.
    delta: Mat,
}

/// Which bodies of `from` are moved in `now`, and their common motion (none moved: no motion);
/// none when a body is another or changed, or when the moved ones move differently. Bodies are
/// found by name: an opened design shows its bodies in the order they were meshed until it
/// changes, the session's world in the model's.
fn moved_bodies(from: &ModelState, now: &ModelState) -> Option<Moved> {
    if from.bodies.len() != now.bodies.len() {
        return None;
    }
    let order: Vec<usize> = if from.bodies.iter().zip(&now.bodies).all(|(a, b)| a.name == b.name) {
        (0..now.bodies.len()).collect()
    } else {
        let by_name: std::collections::HashMap<&str, usize> = from.bodies.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
        now.bodies.iter().map(|b| by_name.get(b.name.as_str()).copied()).collect::<Option<_>>()?
    };
    let mut delta: Option<Mat> = None;
    let mut moving = vec![false; from.bodies.len()];
    let mut seen = vec![false; from.bodies.len()];
    for (body, &i) in now.bodies.iter().zip(&order) {
        // Each body of `from` once (names told apart).
        if std::mem::replace(seen.get_mut(i)?, true) {
            return None;
        }
        let d = body.display_moved_from(from.bodies.get(i)?)?;
        if d == solidcraft_engine::doc::IDENTITY {
            continue;
        }
        match &delta {
            None => delta = Some(d),
            Some(common) if same_move(common, &d) => {}
            Some(_) => return None,
        }
        *moving.get_mut(i)? = true;
    }
    Some(Moved { moving, from: order, delta: delta.unwrap_or(solidcraft_engine::doc::IDENTITY) })
}

/// `moved`, with a layer's `dragged` bodies when none is moved: back where the layer's world has
/// them (the triad snapped back, or the drag came back to where it began), they are still its
/// bodies, drawn unmoved. A layer of nothing dragged would hand them all back to the model scene,
/// and the next step would take them again.
fn still_dragged(moved: Moved, dragged: &[bool]) -> Moved {
    if moved.moving.contains(&true) {
        return moved;
    }
    Moved { moving: dragged.to_vec(), ..moved }
}

/// The boxes of the world shown now from `boxes`, those of the layer's world: the moved bodies'
/// moved (each the box around its moved corners).
fn moved_boxes(boxes: &[(String, Aabb3)], moved: &Moved) -> Option<Vec<(String, Aabb3)>> {
    let shifted = |bb: &Aabb3| {
        let mut b = Aabb3::EMPTY;
        super::box_corners(bb).into_iter().for_each(|p| b.add(solidcraft_engine::doc::apply_point(&moved.delta, p)));
        b
    };
    moved
        .from
        .iter()
        .map(|&i| {
            let (name, bb) = boxes.get(i)?;
            let m = moved.moving.get(i).copied().unwrap_or(false);
            Some((name.clone(), if m && !bb.is_empty() { shifted(bb) } else { *bb }))
        })
        .collect()
}

fn same_move(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= SAME_MOVE)
}

fn layer_key(from: &SceneWorld, moving: &[bool]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    from.base.hash(&mut h);
    (Arc::as_ptr(&from.world) as usize).hash(&mut h);
    moving.hash(&mut h);
    h.finish()
}

/// This frame's model scene and moving layer, for a drag drawn as a layer: its scenes are built
/// once, and afterwards only the ground shadow follows them.
pub(super) fn scenes(app: &mut SolveApp) -> Option<MovingLayer> {
    let mut layer = app.viewport.drag.take()?;
    let t0 = crate::now_ms();
    if layer.statics.is_none() {
        layer.statics = Some(build(app, &layer));
        app.viewport.build_ms = crate::now_ms() - t0;
    }
    let shadow = app.ui.ground_shadow && app.ui.visual_style != 2 && app.cam.pitch > 0.0;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    layer.key.hash(&mut h);
    if shadow {
        layer.delta.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
    }
    let key = h.finish();
    if key != app.viewport.key
        && let Some(statics) = &layer.statics
    {
        let mut sc = statics.clone();
        if shadow {
            let shown = layer.from.world.bodies.iter().zip(layer.boxes.iter()).enumerate();
            let corners = shown
                .filter(|(_, (b, (_, bb)))| !bb.is_empty() && app.body_visible(&b.name))
                .flat_map(|(i, (_, (_, bb)))| {
                    let moved = layer.moving.get(i).copied().unwrap_or(false);
                    super::box_corners(bb).map(|p| if moved { solidcraft_engine::doc::apply_point(&layer.delta, p) } else { p })
                })
                .collect::<Vec<_>>();
            super::ground_shadow_under(&mut sc, app, corners.into_iter());
        }
        if let Ok(mut slot) = app.viewport.slot.lock() {
            *slot = Some(sc);
        }
        app.viewport.key = key;
    }
    let moving = MovingLayer { key: app.viewport.moving_key, slot: app.viewport.moving_slot.clone(), transform: transform(&layer.delta) };
    app.viewport.drag = Some(layer);
    Some(moving)
}

/// The bodies a drag drawn as a layer moves, by name.
pub(super) fn dragged_names(app: &SolveApp) -> std::collections::HashSet<&str> {
    app.viewport
        .drag
        .iter()
        .flat_map(|layer| layer.from.world.bodies.iter().zip(&layer.moving).filter(|(_, moved)| **moved).map(|(b, _)| b.name.as_str()))
        .collect()
}

/// The layer's model scene without the dragged bodies (no ground shadow), and the dragged
/// bodies' scene put in the moving slot; both from the blocks the last scene built where they
/// come back the same.
fn build(app: &mut SolveApp, layer: &DragLayer) -> GpuScene {
    let world = &layer.from.world;
    let moved = |i: usize| layer.moving.get(i).copied().unwrap_or(false);
    let mut kept = app.viewport.blocks.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
    let (mut statics, mut made) = super::body_scene(app, world, |i| !moved(i), false, &mut kept);
    let (moving, made_moving) = super::body_scene(app, world, moved, false, &mut kept);
    made.extend(made_moving);
    super::keep_body_tris(app, &made);
    if let Ok(mut b) = app.viewport.blocks.lock() {
        *b = made;
    }
    if let Ok(mut slot) = app.viewport.moving_slot.lock() {
        *slot = Some(moving);
    }
    app.viewport.moving_key = layer.key;
    // The assembly around a part edited in place stays where it is.
    crate::edit_in_place::draw_context(app, &mut statics);
    statics
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A session with a body that stays and a placed copy of a part, moved off the part.
    fn two_parts() -> (solidcraft_engine::Session, u64) {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5, "body_name": "Stays"})).unwrap();
        let first = s.execute("component.create", &json!({"name": "Part"})).unwrap()["occurrence"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length": 2, "width": 3, "height": 4, "body_name": "Block", "operation": "new"})).unwrap();
        let dragged = s.execute("occurrence.copy", &json!({"occurrence": first})).unwrap()["occurrence"].as_u64().unwrap();
        s.execute("occurrence.move", &json!({"occurrence": dragged, "translate": [20, 0, 0]})).unwrap();
        (s, dragged)
    }

    fn with_gpu(app: &mut SolveApp) {
        app.viewport.gpu =
            Some(crate::gpu::GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
    }

    /// Whole bodies light by drawing the model's own triangles again, but a drag drawn as a layer
    /// moves its bodies' triangles on the GPU from where the drag found them: a selected dragged
    /// body lights its triangles where it stands, and a body that stays reuses the model's.
    #[test]
    fn a_selected_body_dragged_as_a_layer_lights_where_it_stands() {
        let (mut s, dragged) = two_parts();
        // Nothing faded (faded bodies are built apart): the design itself active.
        s.execute("component.activate", &json!({"component": 0})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.set_origin_shown(false);
        with_gpu(&mut app);
        let _ = super::super::build_scene(&app);
        whole_scene_built(&mut app);
        let before = app.session.world_state();
        let base = app.session.doc.occurrences.iter().find(|o| o.id == dragged).map(|o| o.transform).unwrap();
        app.run("occurrence.move", json!({"occurrence": dragged, "base_transform": base, "translate": [4, 0, 0], "capture": false})).unwrap();
        assert!(follow(&mut app));
        let world = app.session.world_state();
        let moving = world.bodies.iter().find(|b| before.body(&b.name).is_some_and(|w| w.mesh_bounds().min.x != b.mesh_bounds().min.x)).unwrap();
        let moving = moving.name.clone();
        let select = |app: &mut SolveApp, names: &[&str]| {
            app.session.selection = names.iter().map(|n| solidcraft_engine::Sel::Body { name: (*n).to_string() }).collect();
            super::super::build_highlight(app)
        };
        let stays = select(&mut app, &["Stays"]);
        assert_eq!(stays.selected_blocks.len(), 1, "a body that stays reuses the model's triangles");
        assert!(stays.merged().tris.is_empty());
        let lit = select(&mut app, &[moving.as_str()]);
        assert!(lit.selected_blocks.is_empty() && lit.selected_planned.is_empty() && lit.selected_instanced.is_empty());
        let tris = lit.merged().tris;
        assert!(!tris.is_empty(), "the dragged body lights its own triangles");
        let xs: Vec<f32> =
            tris.chunks_exact(crate::gpu::TRI_SIZE).filter_map(|v| v.get(..4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))).collect();
        assert!(xs.iter().all(|x| *x > 23.9), "lit where the drag put it: {xs:?}");
    }

    /// An opened design shows its bodies in the order they were meshed until it changes, and the
    /// session's world (a drag's first step) in the model's: the drag finds each body by name,
    /// and its boxes come in the order of the world shown now.
    #[test]
    fn a_drag_finds_its_bodies_by_name_in_another_order() {
        let (mut s, dragged) = two_parts();
        let mut meshed = (*s.world_state()).clone();
        meshed.bodies.reverse();
        let base = s.doc.occurrences.iter().find(|o| o.id == dragged).map(|o| o.transform).unwrap();
        s.execute("occurrence.move", &json!({"occurrence": dragged, "base_transform": base, "translate": [3, 0, 0], "capture": false})).unwrap();
        let now = s.world_state();
        assert!(now.bodies.len() >= 3 && now.bodies.iter().zip(&meshed.bodies).any(|(a, b)| a.name != b.name));

        let moved = moved_bodies(&meshed, &now).unwrap();
        let dragged_now =
            now.bodies.iter().position(|b| meshed.body(&b.name).is_some_and(|w| w.mesh_bounds().min.x != b.mesh_bounds().min.x)).unwrap();
        let dragged_meshed = meshed.bodies.iter().position(|b| b.name == now.bodies[dragged_now].name).unwrap();
        assert_eq!(moved.moving, (0..meshed.bodies.len()).map(|i| i == dragged_meshed).collect::<Vec<_>>());
        assert!(now.bodies.iter().zip(&moved.from).all(|(b, &i)| meshed.bodies[i].name == b.name));
        assert!((moved.delta[3][0] - 3.0).abs() < 1e-9 && moved.delta[3][1].abs() < 1e-9);

        let boxes: Vec<(String, Aabb3)> = meshed.bodies.iter().map(|b| (b.name.clone(), b.mesh_bounds())).collect();
        let shown = moved_boxes(&boxes, &moved).unwrap();
        for (i, ((name, bb), b)) in shown.iter().zip(&now.bodies).enumerate() {
            assert_eq!(name, &b.name);
            let was = boxes[moved.from[i]].1;
            let shift = if i == dragged_now { 3.0 } else { 0.0 };
            assert!((bb.min.x - was.min.x - shift).abs() < 1e-9 && (bb.max.x - was.max.x - shift).abs() < 1e-9, "{name}");
        }

        // Two bodies of one name: no telling them apart.
        let mut twins = meshed.clone();
        twins.bodies[1].name = twins.bodies[0].name.clone();
        assert!(moved_bodies(&twins, &now).is_none());
    }
}
