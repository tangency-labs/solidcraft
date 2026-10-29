//! A joint drag drawn in layers (#6325).
//!
//! Each frame of a joint drag moves some occurrences (`Session::drag_preview`). Drawing that
//! frame from the world state placed every moved body's B-rep and mesh again, measured their
//! boxes, built the model scene and the highlight again and uploaded them all: most of each frame
//! on a large assembly. Instead the bodies under the occurrences the drag moves are split once
//! into rigid groups, one per moved occurrence path, each built into a scene of its own from the
//! world the drag began on and drawn as a layer moved on the GPU ([`MovingLayer`]); the model
//! scene is built without them, once. A frame then only works out each group's move from the
//! occurrences' new transforms. The design is placed exactly once, when the release applies the
//! drag; anything that needs the exact places meanwhile (a pick) still asks the session for them.
//!
//! Whatever the layers cannot draw as the world would (a section's outlines, an analysis, a
//! preview, a highlight or a sketch or construction moving with the parts) leaves the frame to the
//! world state, as before.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Document, IDENTITY, Mat, ModelState, apply_point, mat_inverse, mat_mul};
use solidcraft_engine::geom::Aabb3;

use super::{GpuScene, SolveApp};
use crate::gpu::{MovingLayer, SceneSlot};

/// The most two occurrence paths' moves may differ (per matrix entry, millimetres and rotation
/// alike) to be drawn as one layer.
const SAME_MOVE: f64 = 1e-7;

/// The bodies the drag moves alike: they move as one.
struct Group {
    /// The occurrence paths placing them, each down to the deepest occurrence the drag moves, and
    /// where each put its bodies when the drag began (in the world). The parts of a rigid link
    /// move alike however many occurrences the solve moved them by; the drag is split again when
    /// they come apart.
    members: Vec<(Vec<u64>, Mat)>,
    /// The boxes of its shown bodies where the drag began.
    boxes: Vec<Aabb3>,
    /// Its scene ([`MovingLayer::key`]) and where it goes to the GPU.
    key: u64,
    slot: SceneSlot,
    /// From where the drag began to where this frame draws it.
    delta: Mat,
}

/// A joint drag drawn in layers: the world it began on, its bodies split into the model's (still)
/// and the groups the drag moves.
pub(crate) struct DragLayers {
    /// The design and world the drag began on.
    doc: Arc<Document>,
    world: Arc<ModelState>,
    /// Each occurrence's transform in its parent when the drag began.
    transforms: HashMap<u64, Mat>,
    /// The occurrence path placing each body of `world`, and each body by name.
    paths: Vec<Vec<u64>>,
    by_name: HashMap<String, usize>,
    /// The occurrences the drag has moved so far: the groups are split by them.
    moved: BTreeSet<u64>,
    groups: Vec<Group>,
    /// For each body of `world`: its group, `None` for a body the model scene draws.
    layer_of: Vec<Option<usize>>,
    /// A shown sketch or construction moves with a group (drawn from the world, not as a layer).
    stale: bool,
    /// Names the split: the scenes built for it are the same while it stays.
    split: u64,
    /// The box round the still shown bodies.
    still_bounds: Aabb3,
    /// The still bodies' boxes, for the ground shadow.
    still_boxes: Vec<Aabb3>,
    /// The ground shadow under the moved parts, a layer of its own (its key names its box).
    shadow: Option<(u64, SceneSlot)>,
    /// This frame is drawn in layers.
    on: bool,
}

/// The world shown while a drag is drawn in layers: the one it began on (only for the design it
/// began on, while it lasts).
pub(crate) fn world(app: &SolveApp) -> Option<Arc<ModelState>> {
    let l = app.viewport.layers.as_ref().filter(|l| l.on)?;
    let (doc, _, _) = crate::joint_drag::began_on(app)?;
    (Arc::ptr_eq(doc, &app.session.doc) && Arc::ptr_eq(&l.doc, doc)).then(|| l.world.clone())
}

/// Follow the drag under way: whether this frame draws it in layers, and each group's move.
pub(crate) fn follow(app: &mut SolveApp) {
    let layers = app.viewport.layers.take();
    app.viewport.layers = next(app, layers);
}

fn next(app: &SolveApp, layers: Option<DragLayers>) -> Option<DragLayers> {
    let (doc, world, pending) = crate::joint_drag::began_on(app)?;
    if !Arc::ptr_eq(doc, &app.session.doc) || *pending != app.session.pending_moves || app.viewport.gpu.is_none() {
        return None;
    }
    let mut l = match layers {
        Some(l) if Arc::ptr_eq(&l.world, world) && Arc::ptr_eq(&l.doc, doc) => l,
        _ => DragLayers::new(app, doc.clone(), world.clone(), pending)?,
    };
    let moves = app.session.drag_moves();
    let apart = |l: &DragLayers| {
        l.groups.iter().any(|g| {
            let mut deltas = g.members.iter().map(|(path, start)| delta(path, start, &l.transforms, moves));
            let first = deltas.next().unwrap_or(IDENTITY);
            deltas.any(|d| !same_move(&d, &first))
        })
    };
    if moves.keys().any(|o| !l.moved.contains(o)) || apart(&l) {
        l.moved.extend(moves.keys().copied());
        l.split(app, moves);
    }
    l.on = !l.groups.is_empty() && drawable(app, &l);
    if l.on {
        for g in &mut l.groups {
            g.delta = g.members.first().map_or(IDENTITY, |(path, start)| delta(path, start, &l.transforms, moves));
        }
    }
    Some(l)
}

/// The move of the bodies occurrence path `path` placed at `start` when the drag began, to where
/// this frame's `moves` place them.
fn delta(path: &[u64], start: &Mat, transforms: &HashMap<u64, Mat>, moves: &BTreeMap<u64, Mat>) -> Mat {
    path_world(path, transforms, moves).and_then(|now| mat_inverse(start).map(|inv| mat_mul(&now, &inv))).unwrap_or(IDENTITY)
}

fn same_move(a: &Mat, b: &Mat) -> bool {
    a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= SAME_MOVE)
}

/// The world transform of occurrence path `path`: each occurrence's transform in its parent,
/// `moves` over `transforms`.
fn path_world(path: &[u64], transforms: &HashMap<u64, Mat>, moves: &BTreeMap<u64, Mat>) -> Option<Mat> {
    let mut w = IDENTITY;
    for o in path {
        w = mat_mul(&w, moves.get(o).or_else(|| transforms.get(o))?);
    }
    Some(w)
}

impl DragLayers {
    fn new(app: &SolveApp, doc: Arc<Document>, world: Arc<ModelState>, pending: &BTreeMap<u64, Mat>) -> Option<DragLayers> {
        let paths = solidcraft_engine::doc::world_body_paths(&doc, &app.session.model.state());
        // The walk must be the world's own: one path per body, each placing the body's component.
        let component = |path: &Vec<u64>| path.last().and_then(|o| doc.occurrences.iter().find(|x| x.id == *o)).map_or(0, |o| o.component);
        if paths.len() != world.bodies.len() || world.bodies.iter().zip(&paths).any(|(b, p)| b.component != component(p)) {
            return None;
        }
        let transforms = doc.occurrences.iter().map(|o| (o.id, pending.get(&o.id).copied().unwrap_or(o.transform))).collect();
        let by_name = world.bodies.iter().enumerate().map(|(i, b)| (b.name.clone(), i)).collect();
        let layer_of = vec![None; world.bodies.len()];
        Some(DragLayers {
            doc,
            world,
            transforms,
            paths,
            by_name,
            moved: BTreeSet::new(),
            groups: Vec::new(),
            layer_of,
            stale: false,
            split: 0,
            still_bounds: Aabb3::EMPTY,
            still_boxes: Vec::new(),
            shadow: None,
            on: false,
        })
    }

    /// Split the bodies into the model's and the groups the moved occurrences carry: the bodies
    /// under each moved occurrence path, those that this frame's `moves` move alike together.
    fn split(&mut self, app: &SolveApp, moves: &BTreeMap<u64, Mat>) {
        let boxes = super::body_bounds(app, &self.world);
        let mut groups: Vec<Group> = Vec::new();
        // Each moved occurrence path once, with its group.
        let mut by_path: HashMap<Vec<u64>, Option<usize>> = HashMap::new();
        self.still_bounds = Aabb3::EMPTY;
        self.still_boxes.clear();
        for (i, path) in self.paths.iter().enumerate() {
            let prefix = path.iter().rposition(|o| self.moved.contains(o)).and_then(|k| path.get(..=k));
            let group = prefix.and_then(|prefix| {
                if let Some(at) = by_path.get(prefix) {
                    return *at;
                }
                let at = path_world(prefix, &self.transforms, &BTreeMap::new()).map(|start| {
                    let now = delta(prefix, &start, &self.transforms, moves);
                    match groups.iter().position(|g| same_move(&g.delta, &now)) {
                        Some(at) => {
                            if let Some(g) = groups.get_mut(at) {
                                g.members.push((prefix.to_vec(), start));
                            }
                            at
                        }
                        None => {
                            let slot = Arc::new(Mutex::new(None));
                            groups.push(Group { members: vec![(prefix.to_vec(), start)], boxes: Vec::new(), key: 0, slot, delta: now });
                            groups.len() - 1
                        }
                    }
                });
                by_path.insert(prefix.to_vec(), at);
                at
            });
            if let Some(slot) = self.layer_of.get_mut(i) {
                *slot = group;
            }
            let shown = self.world.bodies.get(i).is_some_and(|b| app.body_visible(&b.name));
            let bb = boxes.get(i).map_or(Aabb3::EMPTY, |(_, bb)| *bb);
            if !shown || bb.is_empty() {
                continue;
            }
            match group.and_then(|g| groups.get_mut(g)) {
                Some(g) => g.boxes.push(bb),
                None => {
                    self.still_bounds = self.still_bounds.union(&bb);
                    self.still_boxes.push(bb);
                }
            }
        }
        // Sketches and constructions are drawn from the world where their component's first
        // placement puts them: one that moves with a group keeps the frame to the world.
        let moves_with = |feature: u64| {
            let comp = self.doc.feature(feature).map_or(0, |f| f.component);
            first_path(&self.doc, comp).iter().any(|o| self.moved.contains(o))
        };
        let s = &app.session;
        self.stale = self.world.sketches.iter().any(|sk| solidcraft_engine::view::sketch_shown(s, sk.feature) && moves_with(sk.feature))
            || self.world.construct.iter().any(|c| s.construction_visible(&c.name) && moves_with(c.feature));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (Arc::as_ptr(&self.world) as usize).hash(&mut h);
        self.layer_of.hash(&mut h);
        self.split = h.finish();
        self.groups = groups;
    }

    /// The body `i` of the world the drag began on is drawn by the model scene.
    fn still(&self, i: usize) -> bool {
        self.layer_of.get(i).is_some_and(Option::is_none)
    }
}

/// The occurrence path of a component's first placement (where its sketches are drawn).
fn first_path(doc: &Document, comp: u64) -> Vec<u64> {
    let mut path = Vec::new();
    let mut c = comp;
    for _ in 0..solidcraft_engine::doc::MAX_PLACEMENT_DEPTH {
        if c == 0 {
            break;
        }
        let Some(o) = doc.occurrence_of(c) else { break };
        path.insert(0, o.id);
        c = o.parent;
    }
    path
}

/// The frame can be drawn in layers: nothing it shows besides the bodies follows them from the
/// world (a section's outlines, an analysis's colours, a preview, a highlight on a moving part,
/// a sketch or construction moving with one, the picture while a design opens).
fn drawable(app: &SolveApp, l: &DragLayers) -> bool {
    let section_dialog = app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Section { .. }));
    !l.stale
        && !section_dialog
        && app.session.section.is_none()
        && app.session.active_sketch.is_none()
        && app.session.analysis.is_none()
        && app.session.continuity.is_empty()
        && !app.preview.active
        && app.preview.effective_replaced().is_empty()
        && !crate::open_meshing::active(app)
        && app.edit_in_place.active.is_none()
        && !crate::render_workspace::active(app)
        && app.viewport.hover.is_none()
        && app.viewport.hover_feature.is_none()
        && app.viewport.hover_bodies.is_empty()
        && app.viewport.hover_plane.is_none()
        && app.highlighted().iter().all(|s| lit_still(l, s))
}

/// Highlighting `s` lights only bodies the model scene draws.
fn lit_still(l: &DragLayers, s: &Sel) -> bool {
    match s {
        Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => l.by_name.get(name).is_some_and(|i| l.still(*i)),
        Sel::Occurrence { path } => l.paths.iter().enumerate().all(|(i, p)| !p.starts_with(path) || l.still(i)),
        _ => false,
    }
}

/// What the model scene's key takes from the layers: the split drawn (0: none).
pub(crate) fn scene_key(app: &SolveApp) -> u64 {
    app.viewport.layers.as_ref().filter(|l| l.on).map_or(0, |l| l.split)
}

/// The model scene of a frame drawn in layers (`key`: its key): the bodies that stay, while the
/// groups' scenes go to their layers. `None` when the frame is not drawn in layers.
pub(crate) fn scenes(app: &mut SolveApp, key: u64) -> Option<GpuScene> {
    let (world, layer_of) = app.viewport.layers.as_ref().filter(|l| l.on).map(|l| (l.world.clone(), l.layer_of.clone()))?;
    // Built with the layers in place: what the build reads of the world shown is the drag's.
    let mut built = super::body_scenes(app, &world, &layer_of).into_iter();
    let still = built.next().unwrap_or_default();
    let l = app.viewport.layers.as_mut()?;
    // The GPU takes each layer's scene from its slot once: new scenes (and a new shadow, its
    // layer perhaps at another place) for a renderer that may have let the old ones go.
    l.shadow = None;
    for (i, (g, sc)) in l.groups.iter_mut().zip(built.chain(std::iter::repeat_with(GpuScene::default))).enumerate() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (key, i).hash(&mut h);
        g.key = h.finish();
        if let Ok(mut slot) = g.slot.lock() {
            *slot = Some(sc);
        }
    }
    Some(still)
}

/// This frame's layers: each group moved by its delta, and the ground shadow under them all.
pub(crate) fn frame(app: &mut SolveApp) -> Vec<MovingLayer> {
    if !app.viewport.layers.as_ref().is_some_and(|l| l.on) {
        return Vec::new();
    }
    let Some(mut l) = app.viewport.layers.take() else { return Vec::new() };
    let mut out: Vec<MovingLayer> =
        l.groups.iter().map(|g| MovingLayer { key: g.key, slot: g.slot.clone(), transform: transform(&g.delta) }).collect();
    if app.ui.ground_shadow && app.ui.visual_style != 2 && app.cam.pitch > 0.0 {
        let moved = l.groups.iter().flat_map(|g| g.boxes.iter().flat_map(|bb| super::box_corners(bb).map(|p| apply_point(&g.delta, p))));
        let corners = l.still_boxes.iter().flat_map(super::box_corners).chain(moved);
        let under = super::ground_shadow_box(app, corners);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        [under.min, under.max].iter().for_each(|p| [p.x, p.y, p.z].map(f64::to_bits).hash(&mut h));
        (app.cam.up, crate::theme::viewport_dark()).hash(&mut h);
        let key = h.finish();
        let slot = match l.shadow.take() {
            Some((k, slot)) if k == key => slot,
            _ => {
                let mut sc = GpuScene::default();
                super::ground_shadow_in(&mut sc, app, under);
                Arc::new(Mutex::new(Some(sc)))
            }
        };
        out.push(MovingLayer { key, slot: slot.clone(), transform: transform(&IDENTITY) });
        l.shadow = Some((key, slot));
    } else {
        l.shadow = None;
    }
    app.viewport.layers = Some(l);
    out
}

/// The box round the shown bodies a frame drawn in layers shows, where it shows them.
pub(crate) fn bodies_bounds(app: &SolveApp) -> Option<Aabb3> {
    let l = app.viewport.layers.as_ref().filter(|l| l.on)?;
    let mut b = l.still_bounds;
    for g in &l.groups {
        for p in g.boxes.iter().flat_map(super::box_corners) {
            b.add(apply_point(&g.delta, p));
        }
    }
    Some(b)
}

fn transform(m: &Mat) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| m[c][r] as f32))
}
