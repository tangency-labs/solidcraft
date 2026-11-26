//! A joint drag drawn by moving its bodies on the GPU (#6325).
//!
//! Each frame of a joint drag moves some occurrences (`Session::drag_preview`). Drawing that
//! frame from the world state placed every moved body's B-rep and mesh again, measured their
//! boxes, built the model scene and the highlight again and uploaded them all: most of each frame
//! on a large assembly. Instead the view keeps showing the world the drag began on, and each body
//! under an occurrence the drag moves is drawn moved by its slot of the renderer's motion table
//! (`crate::gpu::Motions`): a frame only works out each moved occurrence path's move and hands the
//! bodies' motions to the GPU. No body is placed, meshed, built or uploaded, at the drag's start
//! as little as in its frames, and whatever is drawn from a body's own triangles (a whole part's
//! highlight, its edges) moves with it. What lies under the bodies follows them: the ground
//! shadow is built again with the grid and the sketches, as it is when the view turns. The
//! design is placed exactly once, when the release
//! applies the drag; anything that needs the exact places meanwhile (a pick) still asks the
//! session for them.
//!
//! Whatever is worked out on the CPU from where the bodies are (a section's outlines, an
//! analysis, a preview, Wireframe's outlines, a face or an edge lit on a moving part, a sketch or
//! construction moving with the parts) leaves the frame to the world state, as before.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use solidcore_record::Vec3;
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{Document, IDENTITY, Mat, ModelState, apply_point, mat_inverse, mat_mul};
use solidcraft_engine::geom::Aabb3;

use super::SolveApp;
use crate::gpu::{MotionRows, Motions, UNMOVED_SLOT};

/// An occurrence path the drag moves bodies by: down to the deepest occurrence it moves.
struct Carrier {
    /// Where it put its bodies when the drag began (in the world).
    start: Mat,
    path: Vec<u64>,
    /// From where the drag began to where this frame draws its bodies.
    delta: Mat,
}

/// A shown body the drag moves.
struct Moving {
    /// Its carrier, by place in [`DragMotions::carriers`].
    carrier: usize,
    /// Its slot in the motion table (`BodySlots::motion`).
    slot: u32,
    /// Its box where the drag began.
    bounds: Aabb3,
}

/// A joint drag drawn by moving its bodies on the GPU: the world it began on, and the bodies the
/// drag moves with the occurrence paths that carry them.
pub(crate) struct DragMotions {
    /// The design and world the drag began on.
    doc: Arc<Document>,
    world: Arc<ModelState>,
    /// Each occurrence's transform in its parent when the drag began.
    transforms: HashMap<u64, Mat>,
    /// The occurrence path placing each body of `world`, and each body by name.
    paths: Vec<Vec<u64>>,
    by_name: HashMap<String, usize>,
    /// The occurrences the drag has moved so far: the bodies under them are the moving ones.
    moved: BTreeSet<u64>,
    carriers: Vec<Carrier>,
    /// For each body of `world`: the carrier that moves it, by place in `carriers`.
    carried: Vec<Option<usize>>,
    /// The shown bodies the drag moves.
    bodies: Vec<Moving>,
    /// What the view showed when the bodies were sorted ([`super::base_scene_key`]: which bodies,
    /// sketches and constructions are shown): `bodies`, the boxes and `stale` are for that.
    shown: u64,
    /// A shown sketch, construction or part's Origin moves with a carrier (drawn from the world,
    /// not moved).
    stale: bool,
    /// A shown moving body has no slot in the motion table (the table full, or its scene not
    /// built yet): it cannot be drawn moved, so the frame is the world's.
    slotless: bool,
    /// The model scene's key when the bodies were sorted: a slot comes only with a scene built
    /// again, so a drag without one sorts again only then.
    scene: u64,
    /// A shown moving body is see-through. Where the device blends see-through surfaces in the
    /// order they are sorted on the CPU, they are sorted where they were built, so the frame is
    /// the world's there.
    see_through: bool,
    /// The box round the still shown bodies, and each of their boxes (for the ground shadow).
    still_bounds: Aabb3,
    still_boxes: Vec<Aabb3>,
    /// This frame is drawn by moving the bodies.
    on: bool,
}

/// The world shown while a drag is drawn by moving its bodies: the one it began on (only for the
/// design it began on, while it lasts).
pub(crate) fn world(app: &SolveApp) -> Option<Arc<ModelState>> {
    drawn(app).map(|l| l.world.clone())
}

/// The move this frame draws body `i` of [`world`] by (world coordinates, after its placement):
/// `None` for a body that stays, or when the frame is not drawn by moving the bodies.
pub(crate) fn moved_by(app: &SolveApp, i: usize) -> Option<Mat> {
    let l = drawn(app)?;
    let carrier = (*l.carried.get(i)?)?;
    l.carriers.get(carrier).map(|c| c.delta)
}

/// The bodies of [`world`] this frame draws moved (see [`moved_by`]); none when the frame is
/// not drawn by moving the bodies.
pub(crate) fn moved_bodies(app: &SolveApp) -> Vec<usize> {
    drawn(app).map_or_else(Vec::new, |l| l.carried.iter().enumerate().filter_map(|(i, c)| c.map(|_| i)).collect())
}

/// Follow the drag under way: whether this frame draws it by moving its bodies, and each
/// carrier's move.
pub(crate) fn follow(app: &mut SolveApp) {
    let motions = app.viewport.joint_motions.take();
    app.viewport.joint_motions = next(app, motions);
}

/// The motions for this frame of the drag under way (`motions`: the last frame's), its bodies
/// sorted again when the drag moves an occurrence it had not or the view shows other things;
/// none without a drag whose frames are shown, on another design, with other uncaptured moves, or
/// without a GPU.
fn next(app: &SolveApp, motions: Option<DragMotions>) -> Option<DragMotions> {
    let (doc, world, pending) = crate::joint_drag::began_on(app)?;
    if !Arc::ptr_eq(doc, &app.session.doc) || *pending != app.session.pending_moves || app.viewport.gpu.is_none() {
        return None;
    }
    let mut l = match motions {
        Some(l) if Arc::ptr_eq(&l.world, world) && Arc::ptr_eq(&l.doc, doc) => l,
        _ => DragMotions::new(app, doc.clone(), world.clone(), pending)?,
    };
    let moves = app.session.drag_moves();
    // Sorted again too when what is shown changed (a body, sketch or construction hidden or shown
    // mid-drag), and when the scene a moving body waits for its slot in has been built again.
    let waits = l.slotless && l.scene != app.viewport.key;
    if moves.keys().any(|o| !l.moved.contains(o)) || waits || l.shown != super::base_scene_key(app) {
        l.moved.extend(moves.keys().copied());
        l.sort(app);
    }
    l.on = !l.bodies.is_empty() && !l.slotless && drawable(app, &l);
    if l.on {
        for c in &mut l.carriers {
            c.delta = delta(&c.path, &c.start, &l.transforms, moves);
        }
    }
    Some(l)
}

/// The move of the bodies occurrence path `path` placed at `start` when the drag began, to where
/// this frame's `moves` place them.
fn delta(path: &[u64], start: &Mat, transforms: &HashMap<u64, Mat>, moves: &BTreeMap<u64, Mat>) -> Mat {
    path_world(path, transforms, moves).and_then(|now| mat_inverse(start).map(|inv| mat_mul(&now, &inv))).unwrap_or(IDENTITY)
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

impl DragMotions {
    /// The motions of a drag that began on `doc` and `world` with the uncaptured moves `pending`,
    /// nothing sorted yet; none when the world's bodies are not the ones the design's placements
    /// walk gives, one path each.
    fn new(app: &SolveApp, doc: Arc<Document>, world: Arc<ModelState>, pending: &BTreeMap<u64, Mat>) -> Option<DragMotions> {
        let paths = solidcraft_engine::doc::world_body_paths(&doc, &app.session.model.state());
        // The walk must be the world's own: one path per body, each placing the body's component.
        let placed: HashMap<u64, u64> = doc.occurrences.iter().map(|o| (o.id, o.component)).collect();
        let component = |path: &Vec<u64>| path.last().and_then(|o| placed.get(o)).copied().unwrap_or(0);
        if paths.len() != world.bodies.len() || world.bodies.iter().zip(&paths).any(|(b, p)| b.component != component(p)) {
            return None;
        }
        let transforms = doc.occurrences.iter().map(|o| (o.id, pending.get(&o.id).copied().unwrap_or(o.transform))).collect();
        let by_name = world.bodies.iter().enumerate().map(|(i, b)| (b.name.clone(), i)).collect();
        let carried = vec![None; world.bodies.len()];
        Some(DragMotions {
            doc,
            world,
            transforms,
            paths,
            by_name,
            moved: BTreeSet::new(),
            carriers: Vec::new(),
            carried,
            bodies: Vec::new(),
            shown: 0,
            stale: false,
            slotless: false,
            scene: 0,
            see_through: false,
            still_bounds: Aabb3::EMPTY,
            still_boxes: Vec::new(),
            on: false,
        })
    }

    /// Sort the bodies into the still ones and those the moved occurrences carry: the bodies under
    /// each moved occurrence path, each with its slot in the motion table.
    fn sort(&mut self, app: &SolveApp) {
        let boxes = super::body_bounds(app, &self.world);
        let arena = app.viewport.palette.lock().unwrap_or_else(|e| e.into_inner());
        let mut carriers: Vec<Carrier> = Vec::new();
        // Each moved occurrence path once, with its carrier.
        let mut by_path: HashMap<Vec<u64>, Option<usize>> = HashMap::new();
        self.bodies.clear();
        self.slotless = false;
        self.see_through = false;
        self.still_bounds = Aabb3::EMPTY;
        self.still_boxes.clear();
        for (i, path) in self.paths.iter().enumerate() {
            let prefix = path.iter().rposition(|o| self.moved.contains(o)).and_then(|k| path.get(..=k));
            let carrier = prefix.and_then(|prefix| {
                if let Some(at) = by_path.get(prefix) {
                    return *at;
                }
                let at = path_world(prefix, &self.transforms, &BTreeMap::new()).map(|start| {
                    carriers.push(Carrier { start, path: prefix.to_vec(), delta: IDENTITY });
                    carriers.len() - 1
                });
                by_path.insert(prefix.to_vec(), at);
                at
            });
            if let Some(slot) = self.carried.get_mut(i) {
                *slot = carrier;
            }
            let Some(body) = self.world.bodies.get(i) else { continue };
            let bounds = boxes.get(i).map_or(Aabb3::EMPTY, |(_, bb)| *bb);
            if !app.body_drawn(&body.name) || bounds.is_empty() {
                continue;
            }
            match carrier {
                Some(carrier) => {
                    let slot = arena.motion(&body.name);
                    self.slotless |= slot == UNMOVED_SLOT;
                    // A look of its own on a face may be see-through too; a painted face that is
                    // opaque is drawn as the body is.
                    self.see_through |= self.doc.model_body_opacity(body) * app.display_opacity(&body.name) < 0.999
                        || self.doc.face_colors(body).iter().any(|(_, l)| l.opacity < 0.999);
                    self.bodies.push(Moving { carrier, slot, bounds });
                }
                None => {
                    self.still_bounds = self.still_bounds.union(&bounds);
                    self.still_boxes.push(bounds);
                }
            }
        }
        drop(arena);
        // Sketches and constructions are drawn from the world where their component's first
        // placement puts them: one that moves with a carrier keeps the frame to the world.
        let moves_with = |feature: u64| {
            let comp = self.doc.feature(feature).map_or(0, |f| f.component);
            first_path(&self.doc, comp).iter().any(|o| self.moved.contains(o))
        };
        let s = &app.session;
        // So is a moving part's shown Origin: drawn where its placement is.
        let origin_moves = s.visibility.shown_component_origins.iter().any(|path| path.iter().any(|o| self.moved.contains(o)));
        self.stale = origin_moves
            || self.world.sketches.iter().any(|sk| solidcraft_engine::view::sketch_shown(s, sk.feature) && moves_with(sk.feature))
            || self.world.construct.iter().any(|c| s.construction_visible(&c.name) && moves_with(c.feature));
        self.carriers = carriers;
        self.shown = super::base_scene_key(app);
        self.scene = app.viewport.key;
    }

    /// The body `i` of the world the drag began on stays where it is.
    fn still(&self, i: usize) -> bool {
        self.carried.get(i).is_some_and(Option::is_none)
    }

    /// The corners of each shown body's box where this frame draws it: the still bodies' where
    /// the drag began, each moving body's moved by its carrier's move.
    fn drawn_corners(&self) -> impl Iterator<Item = [Vec3; 8]> + '_ {
        let still = self.still_boxes.iter().map(super::box_corners);
        let moved = self.bodies.iter().map(|b| {
            let delta = self.carriers.get(b.carrier).map_or(IDENTITY, |c| c.delta);
            super::box_corners(&b.bounds).map(|p| apply_point(&delta, p))
        });
        still.chain(moved)
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

/// The frame can be drawn by moving the bodies: nothing it shows besides them is worked out on
/// the CPU from where they are (a section's outlines, an analysis's colours, a preview, an open
/// dialog's feedback, kept print faces, a face or an edge lit on a moving part, a sketch or
/// construction moving with one, the picture while a design opens, Wireframe's outlines on
/// curved surfaces, #12789, see-through moving bodies where their order is sorted on the CPU).
fn drawable(app: &SolveApp, l: &DragMotions) -> bool {
    // See-through surfaces blended without sorting need nothing from the CPU.
    let sorted_on_the_cpu = !app.viewport.gpu.is_some_and(|g| g.oit);
    !l.stale
        && !(l.see_through && sorted_on_the_cpu)
        && app.ui.visual_style != 2
        // A dialog draws what it previews or picks where the world shown has it.
        && app.dialog.is_none()
        && !crate::print_face_ui::shown(app)
        && app.session.section.is_empty()
        && super::section_planes(app).is_empty()
        && app.session.active_sketch.is_none()
        && app.session.analysis.is_none()
        && app.session.continuity.is_empty()
        && app.session.isocurves.is_empty()
        && !app.preview.active
        && app.preview.effective_replaced().is_empty()
        && !crate::open_meshing::active(app)
        && app.edit_in_place.active.is_none()
        && !crate::render_workspace::active(app)
        && app.viewport.hover.is_none()
        && app.viewport.hover_feature.is_none()
        && app.viewport.hover_bodies.is_empty()
        && app.viewport.hover_plane.is_none()
        && app.highlighted().iter().all(|s| lit_moves(l, s))
}

/// Highlighting `s` draws nothing the motions leave behind. A whole body or occurrence is lit by
/// drawing the model's own triangles again, which move with their body. A face or an edge is
/// geometry of its own, built where the world shown has it: fine on a body that stays.
fn lit_moves(l: &DragMotions, s: &Sel) -> bool {
    match s {
        Sel::Body { .. } | Sel::Occurrence { .. } => true,
        Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } => l.by_name.get(name).is_some_and(|i| l.still(*i)),
        _ => false,
    }
}

/// The drag's motions while this frame is drawn by moving the bodies, for the design the drag
/// began on (a drag that ended, or another design's tab, has none).
fn drawn(app: &SolveApp) -> Option<&DragMotions> {
    let l = app.viewport.joint_motions.as_ref().filter(|l| l.on)?;
    let (doc, _, _) = crate::joint_drag::began_on(app)?;
    (Arc::ptr_eq(doc, &app.session.doc) && Arc::ptr_eq(&l.doc, doc)).then_some(l)
}

/// This frame's motions: each moving body's slot with its carrier's move; none when the frame is
/// not drawn by moving the bodies.
pub(crate) fn motions(app: &SolveApp) -> Motions {
    let Some(l) = drawn(app) else { return Motions::default() };
    let rows: Vec<MotionRows> = l.carriers.iter().map(|c| rows(&c.delta)).collect();
    Motions::new(l.bodies.iter().filter_map(|b| Some((b.slot, *rows.get(b.carrier)?))))
}

/// Names where this frame draws the moving bodies (0: not moved): what is built from their
/// places (the ground shadow) is built again when it changes.
pub(crate) fn key(app: &SolveApp) -> u64 {
    let Some(l) = drawn(app) else { return 0 };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in &l.carriers {
        c.delta.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
    }
    (l.carriers.len(), l.bodies.len(), l.shown).hash(&mut h);
    h.finish() | 1
}

/// Each shown body's box in the camera's Z-up frame where this frame draws it, for the ground
/// shadow; none when the frame is not drawn by moving the bodies.
pub(crate) fn ground_boxes(app: &SolveApp) -> Option<Vec<Aabb3>> {
    let l = drawn(app)?;
    Some(l.drawn_corners().map(|corners| super::frame_bounds(app, corners.into_iter())).collect())
}

/// The ground the ambient occlusion darkens under the bodies where this frame draws them; none
/// when the frame is not drawn by moving the bodies (the model scene's own then).
pub(crate) fn ground(app: &SolveApp) -> Option<solidcraft_viewport_gpu::view::GroundAo> {
    let l = drawn(app)?;
    super::ground_ao(app, super::frame_bounds(app, l.drawn_corners().flatten()))
}

/// The box round the shown bodies where a frame drawn by moving them shows them.
pub(crate) fn bodies_bounds(app: &SolveApp) -> Option<Aabb3> {
    let l = drawn(app)?;
    let mut b = l.still_bounds;
    for p in l.bodies.iter().flat_map(|m| {
        let delta = l.carriers.get(m.carrier).map_or(IDENTITY, |c| c.delta);
        super::box_corners(&m.bounds).map(move |p| apply_point(&delta, p))
    }) {
        b.add(p);
    }
    Some(b)
}

/// `m` as the motion table takes a move: its three rows, single precision.
fn rows(m: &Mat) -> MotionRows {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r] as f32))
}
