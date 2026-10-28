//! The 3D viewport: navigation, GPU (or CPU fallback) rendering, picking, overlays, the axis
//! view and the navigation bar.

use crate::ui_kit::{Container, Surface};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use serde_json::json;
use solidcraft_engine::Sel;
use solidcraft_engine::geom::{Vec2, Vec3};
use solidcraft_engine::render::{Camera, Mat4, Rgb};
use solidcraft_engine::sketch::CurveKind;
use solidcraft_engine::view::{GridFade, GridPlane, colors, grid_ext, grid_plane, grid_step, sketch_lines_with_sampler};
use std::hash::{Hash, Hasher};

use crate::gpu::{Frame, GpuScene, GpuTarget, SceneSlot, ViewportCallback};
use crate::navigation::{CameraInput, NavCaps, orbit_drag};
use crate::selection::{BoxSel, origin_quads, origin_size, ray_quad};
use crate::theme::Tokens;
use crate::{SolveApp, icons};

#[cfg(test)]
#[path = "viewport_appearance_tests.rs"]
mod appearance_tests;

#[cfg(test)]
#[path = "viewport_overlays_tests.rs"]
mod overlays_tests;

#[cfg(test)]
#[path = "viewport_removed_later_tests.rs"]
mod removed_later_tests;

#[cfg(test)]
#[path = "viewport_plane_target_tests.rs"]
mod plane_target_tests;

#[cfg(test)]
#[path = "viewport_refine_wait_tests.rs"]
mod refine_wait_tests;

#[path = "navigation_cube.rs"]
mod navigation_cube;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavMode {
    Orbit,
    Pan,
    Zoom,
}

#[derive(Default)]
pub struct ViewportState {
    /// A drag on the handles of the drawing being placed (Insert SVG / DXF).
    pub(crate) insert_drag: Option<crate::insert_handles::Drag>,
    /// A drag on the handles of the Transform window's box.
    pub(crate) transform_drag: Option<crate::sketch_transform_ui::Drag>,
    /// The arrow turning the view that is still down (`navigation::step_key_hold`).
    pub(crate) key_hold: Option<crate::navigation::KeyHold>,
    /// A two-point drawing tool's first point was placed by a press (here) that became a drag:
    /// the release places the second (#1630), unless it lands within [`DRAW_DRAG_MIN`] of the
    /// press. Such a drag was a click that moved on the way (#2448): the release places the
    /// first point instead and the shape goes on by clicks, rather than a sliver of a shape.
    pub(crate) draw_drag: Option<Pos2>,
    /// A tool's click moved on the way (#2448): egui takes a press that moves past its
    /// click distance, or is held past its click time, as a drag, so its release is the click.
    pub(crate) tool_click: bool,
    /// A Trim drag is drawing its path (`sketch_tools::trim_drag_*`).
    pub(crate) trim_drag: bool,
    /// Per-body boxes of one world state, held (not just its address, which a later state can
    /// reuse once the old one is freed, so stale boxes would come back).
    pick_bounds: std::sync::Mutex<Option<BodyBounds>>,
    /// The sketches' box in the world of one world state (held, as `pick_bounds` is). Every
    /// `projection` reads it, several times a frame, and each sketch's box samples its curves.
    sketch_bounds: std::sync::Mutex<Option<(std::sync::Arc<solidcraft_engine::doc::ModelState>, solidcraft_engine::geom::Aabb3)>>,
    /// The model scene's blocks of bodies by key, from the last scene built.
    pub(crate) blocks: std::sync::Mutex<std::collections::HashMap<u64, CachedBlock>>,
    /// The triangles of each mesh the last scene drew as instances, by the mesh's address (the
    /// mesh held, so its address stays its own).
    instanced_tris: std::sync::Mutex<InstancedTris>,
    /// The levels of detail of large instanced meshes ([`crate::mesh_lod`]).
    pub(crate) mesh_lods: crate::mesh_lod::MeshLods,
    /// View-dependent display refinement (#1320): which bodies are drawn finer than the chosen
    /// display quality, and their refined plans.
    pub(crate) lod: std::sync::Mutex<crate::display_lod::Lod>,
    /// The view and model the refinement was last chosen for.
    lod_view: u64,
    /// Refinement waits for the camera to stop ([`REFINE_STILL_MS`]).
    lod_rest: CameraRest,
    /// Each body mesh's box (the mesh held, so its address stays its own), from the last state.
    mesh_bounds: std::sync::Mutex<MeshBounds>,
    /// Each shown body mesh's section index for the cut's normal (the mesh held, so its address
    /// stays its own): a Section Analysis dragged along its normal cuts through it.
    section_index: std::sync::Mutex<
        std::collections::HashMap<
            (usize, [u64; 3]),
            (std::sync::Arc<solidcraft_engine::geom::Mesh>, std::sync::Arc<solidcraft_engine::view::SectionIndex>),
        >,
    >,
    /// Each body mesh's picking index (made the first time a pick reaches the body), kept while
    /// the mesh is part of the world state.
    pub(crate) pick_index: crate::pick_index::PickIndexes,
    /// The last click in the view was the middle button's (for the middle double-click).
    pub(crate) middle_clicked: bool,
    /// The last pick (its key and hits): reused while nothing it depends on changed.
    last_pick: Option<(u64, Vec<Hit>)>,
    pub rect: Option<Rect>,
    pub(crate) layout: Option<crate::viewport_layout::Layout>,
    pub(crate) palette_rect: Option<Rect>,
    pub(crate) dialog_rect: Option<Rect>,
    /// OK or Cancel clicked in the view's corner for the open dialog (#6709), which
    /// `dialogs::show` acts on in the same frame as its own buttons.
    pub(crate) corner: Option<crate::ui_kit::Action>,
    /// Whether the open dialog's OK was enabled when it was last drawn: the corner's check
    /// greys out with it.
    pub(crate) corner_ok: bool,
    pub gpu: Option<GpuTarget>,
    /// The GPU plan evaluation still compiling: `gpu.tess` once it is ready (#4860).
    pub(crate) tess_ready: Option<crate::gpu::TessReady>,
    /// What the GPU drew last frame, shared with the renderer (the frame-rate details, #2548).
    pub(crate) draw_stats: Option<crate::gpu::DrawStatsHandle>,
    /// The status line has said the view is drawn at a lower quality ([`DrawStats::lowered`]).
    ///
    /// [`DrawStats::lowered`]: crate::gpu::DrawStats::lowered
    lowered_noted: bool,
    /// The graphics backend and adapter, as the frame-rate details show them.
    pub(crate) adapter: String,
    slot: SceneSlot,
    pub(crate) key: u64,
    sketch_slot: SceneSlot,
    sketch_key: u64,
    physical_scale: f32,
    pub(crate) display: std::sync::Mutex<crate::sketch_display::DisplayCache>,
    pub(crate) hl_slot: SceneSlot,
    pub(crate) hl_key: u64,
    /// Browser/timeline body feedback, independent of the camera and reference widget scale.
    hover_block: std::sync::Mutex<Option<HoverBlock>>,
    /// Each shown body the model builds on the CPU: the block of the model scene that holds its
    /// triangles, and the body's index and vertex ranges in it (for whole-body highlights).
    body_tris: std::sync::Mutex<std::collections::HashMap<String, crate::gpu::BlockTris>>,
    /// The stable palette slots of the model scene (body faces, edges, placements).
    pub(crate) palette: std::sync::Mutex<crate::scene_palette::PaletteArena>,
    /// Each body's resolved look, kept while its geometry and the design's appearance state are
    /// the same: a theme or visibility rebuild does not resolve every face again.
    appearance: std::sync::Mutex<AppearanceCache>,
    pub nav: Option<NavMode>,
    /// The preset's navigation for the left-button drag under way (Alias: Shift+Alt), taken
    /// when the button went down, so the drag neither selects nor moves sketch geometry.
    pub primary_nav: Option<NavMode>,
    /// Button-free gesture and its last owned pointer position, kept through pauses.
    pub(crate) modifier_nav: Option<(NavMode, Pos2)>,
    /// What a click would pick now (after the active command's filter).
    pub hover: Option<Hit>,
    /// The centre of the circle or arc of the sketch being edited whose edge was hovered last
    /// (sketch, point id): it is drawn, and it takes the pointer from further away
    /// ([`WOKEN_CENTRE_PX`]), until another curve or point is hovered.
    pub woken_centre: Option<(u64, String)>,
    pub mouse: Option<Pos2>,
    /// Box selection being dragged.
    pub boxsel: Option<BoxSel>,
    /// Timeline item under the cursor: its bodies or resulting faces are highlighted.
    pub hover_feature: Option<u64>,
    /// Bodies whose browser row is under the cursor: highlighted the same way.
    pub hover_bodies: Vec<String>,
    /// The model point an orbit turns around (Preferences: orbit around the cursor).
    pub orbit_pivot: Option<Vec3>,
    /// The orbit drag under way, summed from where it began (see [`crate::navigation::orbit`]).
    pub orbit_from: Option<crate::navigation::OrbitFrom>,
    /// Continuous orbit: the view turns about the vertical until the next click or key.
    pub spin: bool,
    /// The 3D mouse's motion this frame, for the page showing the camera to apply with what its
    /// camera may do (`navigation::device`).
    pub device_motion: Option<crate::space_mouse::DeviceMotion>,
    /// A device app's motion since the page last took it (`space_mouse::bridge_frame`), applied
    /// the same way.
    pub bridge_motion: Option<crate::space_mouse::BridgeMotion>,
    /// After immediate zoom, ignore egui's outstanding smoothing until the gesture ends.
    pub(crate) wheel_zoom_raw_until_idle: bool,
    /// While trackpad scrolling runs, ignore egui's smoothing tail for zoom so
    /// two-finger input is not replayed as zoom after it pans.
    pub(crate) trackpad_until_idle: bool,
    /// Inside a bracketed trackpad scroll (`Start` without its `End` yet):
    /// point moves pan, bare moves are a mouse.
    pub(crate) trackpad_gesture: bool,
    /// The inertia after the fingers lift, which ends when the modifiers change (#7327).
    pub(crate) trackpad_coast: crate::navigation::TrackpadCoast,
    /// A primary press on the axis gizmo: the pointer position used by its last orbit step.
    pub axis_drag: Option<Pos2>,
    /// Unsnapped camera while Shift-orbit previews a standard or diagonal view.
    pub orbit_free: Option<Camera>,
    pub build_ms: f64,
    /// Adaptive supersampling (#1288): the view drawn last (its view-projection and pixels) and
    /// since when it has held still (app time, s).
    pub(crate) ss_view: (u64, f64),
    /// The right-click menu, open at this screen position.
    pub context_menu: Option<Pos2>,
    /// Select Other: its list, a press being held, and click cycling.
    pub select_other: crate::select_other::State,
    /// A sketch point or curve being dragged (its id, the undo depth when the drag began: the
    /// whole drag is one undo step, and for a curve where it was last under the cursor).
    pub point_drag: Option<(String, usize, Option<Vec2>)>,
    /// Checkpoint and source for a cancellable whole-text drag.
    pub(crate) text_drag: Option<(u64, String)>,
    /// A part that joints place, being dragged.
    pub(crate) joint_drag: Option<crate::joint_drag::Drag>,
    /// A free part being dragged to a new place (#2804).
    pub(crate) occurrence_drag: Option<crate::occurrence_drag::Drag>,
    /// Extrude by pulling a face or profile.
    pub(crate) drag_extrude: crate::drag_extrude::State,
    /// The plane the grid was last drawn on while Preferences ▸ Grid follows the view is on
    /// (#1470), kept so the choice is hysteretic near the thresholds (#1993).
    pub(crate) grid_plane: std::sync::Mutex<GridPlane>,
}

/// Something under the cursor.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    SketchPoint {
        sketch: u64,
        id: String,
        at: Vec2,
    },
    /// A selectable line midpoint, before any associated helper point has been made.
    SketchMidpoint {
        sketch: u64,
        id: String,
        at: Vec2,
    },
    /// A sketch curve; `straight` for lines (usable as an axis).
    SketchCurve {
        sketch: u64,
        id: String,
        straight: bool,
    },
    Vertex {
        body: String,
        point: Vec3,
    },
    Edge {
        body: String,
        index: usize,
        mid: Vec3,
    },
    Profile {
        sketch: u64,
        index: usize,
    },
    Face {
        body: String,
        index: usize,
        point: Vec3,
    },
    /// An origin plane (XY, XZ, YZ) or a construction plane (by name).
    Plane {
        name: String,
        point: Vec3,
    },
    /// A construction point (by feature name).
    ConstructionPoint {
        name: String,
    },
    /// An origin or construction axis.
    Axis {
        name: String,
    },
}

impl ViewportState {
    pub fn pixels_per_point(&self) -> f32 {
        if self.physical_scale > 0.0 { self.physical_scale } else { 1.0 }
    }
    #[cfg(test)]
    pub(crate) fn set_pixels_per_point(&mut self, scale: f32) {
        self.physical_scale = scale;
    }
}

fn display_proj(app: &SolveApp) -> Proj {
    projection(app, app.viewport.rect.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))))
}

/// World ↔ screen mapping for the current frame.
#[derive(Clone, Copy)]
pub struct Proj {
    pub cam: Camera,
    pub vp: Mat4,
    pub rect: Rect,
}

impl Proj {
    pub fn to_screen(&self, p: Vec3) -> Option<Pos2> {
        let c = self.vp.apply(p);
        if c[3] <= 1e-9 {
            return None;
        }
        let (x, y) = (c[0] / c[3], c[1] / c[3]);
        Some(pos2(
            self.rect.left() + ((x + 1.0) * 0.5 * self.rect.width() as f64) as f32,
            self.rect.top() + ((1.0 - y) * 0.5 * self.rect.height() as f64) as f32,
        ))
    }
    pub fn ray(&self, p: Pos2) -> (Vec3, Vec3) {
        self.cam.ray((p.x - self.rect.left()) as f64, (p.y - self.rect.top()) as f64, self.rect.width() as f64, self.rect.height() as f64)
    }
}

/// A projected 3D face (convex): an unfeathered triangle fan, outlined one edge at a time (no
/// joins between the edges). Unlike `Shape::convex_polygon`, it stays inside its outline when
/// the face turns edge-on.
pub(crate) fn projected_polygon(pts: &[Pos2], fill: Color32, stroke: Stroke) -> Vec<Shape> {
    let mut mesh = egui::Mesh::default();
    for (i, p) in (0u32..).zip(pts) {
        mesh.colored_vertex(*p, fill);
        if i >= 2 {
            mesh.add_triangle(0, i - 1, i);
        }
    }
    let mut out = vec![Shape::mesh(mesh)];
    out.extend(pts.iter().zip(pts.iter().skip(1).chain(pts.first())).map(|(a, b)| Shape::line_segment([*a, *b], stroke)));
    out
}

/// View directions all round, a few degrees apart, none exactly along an axis.
#[cfg(test)]
pub(crate) fn orbit_directions() -> impl Iterator<Item = Vec3> {
    (0..90).flat_map(|i| {
        (0..45).map(move |j| {
            let (yaw, pitch) = (f64::from(i) * 4.0f64.to_radians() + 0.003, f64::from(j) * 4.0f64.to_radians() - 1.5);
            Vec3::new(pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), pitch.sin())
        })
    })
}

/// Every vertex egui draws for `shapes`.
#[cfg(test)]
pub(crate) fn drawn_points(shapes: Vec<Shape>) -> Vec<Pos2> {
    let mut tess = egui::epaint::Tessellator::new(1.0, egui::epaint::TessellationOptions::default(), [1, 1], vec![]);
    let mut mesh = egui::Mesh::default();
    for shape in shapes {
        tess.tessellate_shape(shape, &mut mesh);
    }
    mesh.vertices.iter().map(|v| v.pos).collect()
}

fn rgba(c: Rgb) -> [u8; 4] {
    [c.0, c.1, c.2, 255]
}

fn scene_bounds(app: &SolveApp) -> solidcraft_engine::geom::Aabb3 {
    let mut b = model_bounds(app);
    if !app.preview.bounds.is_empty() {
        b = b.union(&app.preview.bounds);
    }
    b
}

pub fn scene_radius(app: &SolveApp) -> f64 {
    let b = scene_bounds(app);
    let r = if b.is_empty() { 50.0 } else { b.diagonal() * 0.5 + b.center().dist(app.cam.target) };
    r.max(app.cam.half_height() * 2.0).max(10.0)
}

pub fn projection(app: &SolveApp, rect: Rect) -> Proj {
    let mut cam = app.cam;
    // Render owns an explicit product camera, including its saved/custom FOV.
    if !crate::render_workspace::active(app) {
        cam.fov = if app.perspective_view() { 0.6 } else { 0.0 };
    }
    let aspect = rect.width() as f64 / rect.height().max(1.0) as f64;
    Proj { cam, vp: cam.view_proj_bounds(aspect, &scene_bounds(app)), rect }
}

/// The far plane the picture's projection uses: the grid's fade ends before that plane cuts the
/// ground (#4214). The scene's radius is not the radius the picture frames, so the planes of the
/// projection itself are the ones that count.
fn picture_far(app: &SolveApp, cam: &Camera) -> f64 {
    cam.clip_bounds(&scene_bounds(app)).1
}

/// Sketches drawn in the viewport: the active one, and finished ones not yet used by a feature.
pub(crate) fn visible_sketches(app: &SolveApp) -> Vec<u64> {
    let st = crate::open_meshing::shown_world(app);
    st.sketches.iter().filter(|s| solidcraft_engine::view::sketch_shown(&app.session, s.feature)).map(|s| s.feature).collect()
}

/// What Home, Fit and the orbit pivot frame: the bodies and sketches shown now, where they are
/// now (see [`solidcraft_engine::view::bounds`]).
pub fn view_bounds(app: &SolveApp) -> solidcraft_engine::geom::Aabb3 {
    // Bodies still being meshed after an open frame the view by their outlines.
    if crate::open_meshing::framing(app) {
        return solidcraft_engine::view::unmeshed_bounds(&app.session);
    }
    solidcraft_engine::view::bounds(&app.session)
}

/// The middle of the shown bodies the view shows part of: what a cursor orbit begun over empty
/// space turns about. The middle of everything shown can be far off screen when the part in
/// view is far from other geometry (a part far from the origin and a sketch at it, #2616), and
/// the part then swings out of view. `None` when no body is in view.
pub(crate) fn in_view_centre(app: &SolveApp, proj: &Proj) -> Option<Vec3> {
    let st = crate::open_meshing::shown_world(app);
    let b = body_bounds(app, &st)
        .iter()
        .filter(|(name, b)| !b.is_empty() && app.session.body_visible(name) && in_view(proj, b))
        .fold(solidcraft_engine::geom::Aabb3::EMPTY, |a, (_, b)| a.union(b));
    (!b.is_empty()).then(|| b.center())
}

/// Does the view show part of box `b`? A box partly behind the eye counts.
fn in_view(proj: &Proj, b: &solidcraft_engine::geom::Aabb3) -> bool {
    let mut seen = Rect::NOTHING;
    let mut behind = 0;
    for i in 0..8 {
        let corner = Vec3::new(
            if i & 1 == 0 { b.min.x } else { b.max.x },
            if i & 2 == 0 { b.min.y } else { b.max.y },
            if i & 4 == 0 { b.min.z } else { b.max.z },
        );
        match proj.to_screen(corner) {
            Some(p) => seen.extend_with(p),
            None => behind += 1,
        }
    }
    match behind {
        8 => false,
        0 => seen.intersects(proj.rect),
        _ => true,
    }
}

fn scene_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Not the session revision: selection and view-only commands bump it without changing the
    // model. The document identity catches edits, including appearance-only ones (whose world
    // state may be the same Arc).
    (std::sync::Arc::as_ptr(&app.session.doc) as usize).hash(&mut h);
    app.session.colored_components.hash(&mut h);
    (std::sync::Arc::as_ptr(&crate::open_meshing::shown_world(app)) as usize).hash(&mut h);
    app.ui.visual_style.hash(&mut h);
    app.ui.ground_shadow.hash(&mut h);
    solidcraft_engine::doc::display_quality::current().hash(&mut h);
    lod_generation(app).hash(&mut h);
    app.viewport.mesh_lods.generation().hash(&mut h);
    // How bodies are drawn: from their plans on the GPU, or built here (until the evaluation
    // shader has compiled, #4860).
    app.viewport.gpu.is_some_and(|g| g.tess).hash(&mut h);
    // The section plane (its cut outlines are part of the model scene).
    section_plane(app).map(|(o, n)| [o.x, o.y, o.z, n.x, n.y, n.z].map(f64::to_bits)).hash(&mut h);
    app.cam.up.hash(&mut h);
    (app.cam.pitch > 0.0).hash(&mut h);
    app.session.visibility.hidden_bodies.hash(&mut h);
    app.session.visibility.hidden_occurrences.hash(&mut h);
    app.session.visibility.hidden_sketches.hash(&mut h);
    app.session.visibility.shown_sketches.hash(&mut h);
    app.session.visibility.hidden_folders.hash(&mut h);
    app.session.visibility.hidden_profiles.hash(&mut h);
    app.session.active_component.hash(&mut h);
    app.session.active_occurrence_path.hash(&mut h);
    app.preview.effective_replaced().hash(&mut h);
    app.preview.rolled_back.hash(&mut h);
    app.preferences.cosmetic_threads.hash(&mut h);
    crate::theme::is_dark().hash(&mut h);
    crate::theme::vision().hash(&mut h);
    // Two themes may share a light/dark base while drawing different geometry colours.
    crate::theme_files::color_key(&Tokens::get()).hash(&mut h);
    app.session.active_sketch.hash(&mut h);
    // A reset palette arena (another design) invalidates every cached block's indices.
    app.viewport.palette.lock().map_or(0, |a| a.generation()).hash(&mut h);
    // Continuity seams are model-scene lines but are not part of the document or world state.
    for seam in &app.session.continuity {
        seam.body.hash(&mut h);
        seam.edge.hash(&mut h);
        seam.points.len().hash(&mut h);
        format!("{:?}", seam.verdict).hash(&mut h);
        seam.points.first().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).hash(&mut h);
    }
    h.finish()
}

fn sketch_scene_key(app: &SolveApp, proj: &Proj) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    scene_key(app).hash(&mut h);
    // The grid this scene draws follows the view when the preference says so (#1470): turning it
    // on or off redraws even while the camera holds still.
    app.ui.show_grid.hash(&mut h);
    app.preferences.grid_follows_view.hash(&mut h);
    // Finished sketches are drawn over the bodies when the preference says so.
    app.preferences.reference_geometry_through_bodies.hash(&mut h);
    app.preferences.sketch_halo.hash(&mut h);
    (std::sync::Arc::as_ptr(&crate::open_meshing::shown_world(app)) as usize).hash(&mut h);
    crate::sketch_display::projection_key(proj, app.viewport.pixels_per_point()).hash(&mut h);
    h.finish()
}

#[cfg(test)]
pub(crate) fn display_scene_keys(app: &SolveApp, proj: &Proj) -> (u64, u64) {
    (scene_key(app), sketch_scene_key(app, proj))
}

/// A sketch curve colour from the engine, in the current theme and colour vision mode.
fn sketch_color(t: &Tokens, c: Rgb) -> [u8; 4] {
    if c == colors::SKETCH {
        c4(t.sketch)
    } else if c == colors::SKETCH_DONE || c == colors::SKETCH_FIXED {
        c4(t.sketch_fixed)
    } else if c == colors::SKETCH_LOCKED {
        c4(t.sketch_locked)
    } else if c == colors::SKETCH_PROJECTED {
        c4(t.sketch_projected)
    } else if c == colors::SKETCH_LOST {
        c4(t.sketch_lost)
    } else if c == colors::SKETCH_CONSTRUCTION {
        c4(t.sketch_construction)
    } else if c == colors::SKETCH_CENTERLINE {
        c4(t.sketch_centerline)
    } else {
        rgba(c)
    }
}

/// With a component active, bodies outside it are drawn faded.
pub(crate) fn body_faded(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, active_bodies: &Option<Vec<String>>) -> bool {
    let s = &app.session;
    if let Some(names) = active_bodies {
        !names.contains(&b.name)
    } else {
        s.active_component != 0 && !s.doc.component_within(b.component, s.active_component)
    }
}

/// How many times the viewport's pixels to draw this frame each way (#1288; 1: off): the
/// preference, and with adaptive supersampling only once the view has held still for a moment,
/// so orbiting and zooming stay as quick as without it.
fn supersampling(app: &mut SolveApp, ctx: &egui::Context, view_proj: [[f32; 4]; 4], size_px: [f32; 2]) -> f32 {
    let factor = app.preferences.supersampling;
    if !(factor > 1.0) {
        return 1.0;
    }
    if !app.preferences.supersampling_adaptive {
        return factor;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    view_proj.iter().flatten().chain(&size_px).for_each(|x| x.to_bits().hash(&mut h));
    let view = h.finish();
    let now = ctx.input(|i| i.time);
    if view != app.viewport.ss_view.0 {
        app.viewport.ss_view = (view, now);
    }
    let still = now - app.viewport.ss_view.1;
    if still >= SS_SETTLE {
        factor
    } else {
        // Draw it supersampled once the view settles.
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SS_SETTLE - still + 0.01));
        1.0
    }
}

/// How long the view holds still before adaptive supersampling draws it at full quality (s).
const SS_SETTLE: f64 = 0.25;

/// How far (screen points) a press must be dragged for its release to draw a rectangle, circle
/// or polygon (#1630); a shorter drag is a click that moved on the way (#2448).
const DRAW_DRAG_MIN: f32 = 24.0;

/// The display plan the model scene draws `b` from on the GPU, or `None` when its faces are built
/// here from the reference mesh. Opaque shaded bodies (`opaque`: not faded, and no see-through
/// body or face) are drawn from their plans where the device evaluates them; faded and
/// see-through ones (sorted and blended per triangle) are built here. A body seen up close is
/// drawn from its refined plan ([`crate::display_lod`]).
fn drawn_plan(
    app: &SolveApp,
    b: &solidcraft_engine::doc::ModelBody,
    opaque: bool,
) -> Option<std::sync::Arc<solidcraft_engine::geom::tess::TessPlan>> {
    let shaded = app.ui.visual_style != 2;
    (shaded && opaque && app.viewport.gpu.is_some_and(|g| g.tess))
        .then(|| refined(app, b).map(|(plan, _)| plan).or_else(|| b.display_plan()))
        .flatten()
        .filter(|plan| plan.vertex_count() <= solidcraft_gpu::MAX_BATCH)
}

/// The refined plan and mesh `b` is drawn from, when the view has it drawn finer than the chosen
/// display quality (#1320).
fn refined(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody) -> crate::display_lod::Refined {
    app.viewport.lod.lock().ok()?.refined(b, solidcraft_engine::doc::display_quality::current())
}

/// Changes whenever a body's refinement changes (the scene and highlight keys).
fn lod_generation(app: &SolveApp) -> u64 {
    app.viewport.lod.lock().map_or(0, |l| l.generation)
}

/// The mesh whose triangles the model scene shows for `b`'s faces: the plan's (evaluated on the
/// CPU, the same triangles the GPU draws) or [`drawn_mesh`]. A face highlighted over the model
/// must use the same triangles: another triangulation of the same face fights it at equal depth
/// and shimmers.
fn shown_mesh(
    app: &SolveApp,
    b: &solidcraft_engine::doc::ModelBody,
    active_bodies: &Option<Vec<String>>,
) -> std::sync::Arc<solidcraft_engine::geom::Mesh> {
    if let Some((_, mesh)) = refined(app, b) {
        // Refined: faces drawn from the refined plan on the GPU or built from its mesh here.
        return mesh;
    }
    (app.viewport.gpu.is_some_and(|g| g.tess) && model_plan_class(app, b, active_bodies).is_some())
        .then(|| drawn_plan(app, b, true))
        .flatten()
        // A large body's plan mesh is made on a worker: until then its faces are lit on the
        // mesh drawn here (#3745).
        .and_then(|_| b.plan_mesh_soon())
        .unwrap_or_else(|| drawn_mesh(app, b))
}

/// The mesh the model scene builds `b` from on the CPU (its edges always, its faces when they
/// are not drawn from the plan on the GPU): a refined plan's mesh when the view has one for it
/// (#1320), the reference mesh at Normal display quality, else the plan's mesh at the chosen
/// quality (finer or coarser faces and edges alike), falling back to the reference mesh when the
/// body has no plan.
fn drawn_mesh(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody) -> std::sync::Arc<solidcraft_engine::geom::Mesh> {
    if crate::render_workspace::active(app) {
        return b.mesh();
    }
    use solidcraft_engine::doc::display_quality::{DisplayQuality, current};
    if let Some((_, mesh)) = refined(app, b) {
        return mesh;
    }
    if current() == DisplayQuality::Normal { b.mesh() } else { b.plan_mesh().unwrap_or_else(|| b.mesh()) }
}

/// How long (ms) the camera must stand still before display refinements are chosen and swapped in.
const REFINE_STILL_MS: f64 = 200.0;

/// The camera the last frame had and when it last differed from the one before (ms).
#[derive(Default)]
struct CameraRest {
    camera: u64,
    moved_at: f64,
}

impl CameraRest {
    /// Notes this frame's camera (a hash of the view) at `now` (ms); how much longer (ms) the
    /// camera has to stand still, 0 once it has for [`REFINE_STILL_MS`].
    fn wait(&mut self, camera: u64, now: f64) -> f64 {
        if camera != self.camera {
            *self = CameraRest { camera, moved_at: now };
        }
        (REFINE_STILL_MS - (now - self.moved_at)).max(0.0)
    }
}

/// Choose which bodies to draw finer than the chosen display quality for this view (#1320):
/// each shown body's screen-space error from its box on screen, refinements planned off the UI
/// thread and swapped in when ready. Keeps repainting while refinements are being planned.
fn update_refinement(app: &mut SolveApp, ctx: &egui::Context, proj: &Proj) {
    if app.viewport.gpu.is_none() || !crate::display_lod::enabled() {
        return; // The CPU picture draws the reference mesh.
    }
    let q = solidcraft_engine::doc::display_quality::current();
    // Nothing to choose again while the view, the model and the planning stand still.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    scene_key(app).hash(&mut h);
    crate::sketch_display::projection_key(proj, app.viewport.pixels_per_point()).hash(&mut h);
    let view = h.finish();
    // A moving camera is left alone: every refinement swapped in rebuilds and uploads the whole
    // model scene, which in a large assembly takes tens of milliseconds a time. The bodies keep
    // the step they have until the camera has stood still for [`REFINE_STILL_MS`].
    let camera = crate::sketch_display::projection_key(proj, app.viewport.pixels_per_point());
    let wait = app.viewport.lod_rest.wait(camera, crate::now_ms());
    if wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(wait as u64 + 5));
        return;
    }
    // While refinements are being planned, the view standing still: only collect finished ones.
    if view == app.viewport.lod_view {
        let (busy, finished) = app.viewport.lod.lock().map_or((false, false), |mut l| {
            let finished = l.collect();
            (l.busy(), finished)
        });
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        if !finished {
            return;
        }
    }
    let t0 = crate::now_ms();
    app.viewport.lod_view = view;
    let st = crate::open_meshing::shown_world(app);
    let ppp = f64::from(app.viewport.pixels_per_point());
    let lens = crate::display_lod::Lens::new(&proj.cam, f64::from(proj.rect.height()) * ppp);
    let bounds = body_bounds(app, &st);
    let mut live = std::collections::HashSet::with_capacity(st.bodies.len());
    let mut candidates = Vec::new();
    for (b, (name, bx)) in st.bodies.iter().zip(bounds.iter()) {
        let id = crate::display_lod::body_id(b).0;
        live.insert(id);
        if b.name != *name || b.body.is_mesh() {
            continue;
        }
        let shown = app.session.body_visible(&b.name) && !app.preview.effective_replaced().contains(&b.name);
        let on_screen = match screen_box(proj, bx, 0.0).filter(|_| shown) {
            Some(r) => r.intersects(proj.rect).then(|| f64::from(r.width().max(r.height())) * ppp),
            // A corner behind the eye: the view is inside or right by the box.
            None => (shown && proj.cam.fov > 0.0 && lens.in_front(bx)).then_some(f64::INFINITY),
        };
        let err_px = crate::display_lod::error_px(q, b.body.size(), bx, on_screen, &lens);
        candidates.push(crate::display_lod::Candidate { body: b, err_px, triangles: b.mesh_in_place().0.triangles.len() });
    }
    let busy = match app.viewport.lod.lock() {
        Ok(mut lod) => {
            lod.update(q, candidates, &live);
            lod.stats.choose_ms = crate::now_ms() - t0;
            lod.busy()
        }
        Err(_) => false,
    };
    if busy {
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }
}

/// A body's look as the scene resolves it: the component/body colours, the body opacity and the
/// faces with looks of their own.
#[derive(Clone)]
struct ResolvedAppearance {
    component: Option<Color32>,
    body: Option<Color32>,
    opacity: f64,
    face_looks: std::collections::HashMap<usize, solidcraft_engine::doc::appearance::Look>,
}

/// [`ViewportState::appearance`]: resolved bodies by name, with their geometry key and the
/// appearance state they were resolved from.
#[derive(Default)]
struct AppearanceCache {
    /// The document the face index below was built from, and whether components coloured bodies.
    doc: Option<(usize, bool)>,
    /// A hash of each body's face appearances, so a body's look is keyed without scanning them.
    faces: std::collections::HashMap<String, u64>,
    /// Each body's appearance key, reused while the document and its geometry are unchanged.
    keys: std::collections::HashMap<String, u64>,
    bodies: std::collections::HashMap<String, (u64, std::sync::Arc<ResolvedAppearance>)>,
    /// Bodies actually resolved (the test asserts an unrelated edit resolves only its body).
    #[cfg(test)]
    resolved: u32,
}

/// A hash per body of the face appearances that name it, in design order (a later assignment
/// wins). Rebuilt when the document changes.
fn face_appearance_hashes(doc: &solidcraft_engine::doc::Document) -> std::collections::HashMap<String, u64> {
    use std::hash::Hasher as _;
    let mut out: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for f in &doc.appearances.faces {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        out.get(&f.body).hash(&mut h);
        serde_json::to_string(f).ok().hash(&mut h);
        out.insert(f.body.clone(), h.finish());
    }
    out
}

fn hash_look(look: Option<&solidcraft_engine::doc::appearance::Look>, h: &mut impl std::hash::Hasher) {
    use std::hash::Hash as _;
    match look {
        Some(l) => {
            1u8.hash(h);
            l.name.hash(h);
            l.color.hash(h);
            l.opacity.to_bits().hash(h);
            match &l.pbr {
                Some(p) => {
                    1u8.hash(h);
                    p.metallic().to_bits().hash(h);
                    p.roughness().to_bits().hash(h);
                    p.dielectric_ior().to_bits().hash(h);
                }
                None => 0u8.hash(h),
            }
        }
        None => 0u8.hash(h),
    }
}

/// What a body's resolved look depends on: its geometry (imported paint), the body/component
/// looks, its material, its face appearances, and component colouring.
fn body_appearance_key(
    doc: &solidcraft_engine::doc::Document,
    b: &solidcraft_engine::doc::ModelBody,
    faces: &std::collections::HashMap<String, u64>,
    colored: bool,
) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.geometry_key().unwrap_or(0).hash(&mut h);
    let name = b.definition_name();
    hash_look(doc.appearances.bodies.get(name), &mut h);
    hash_look(doc.appearances.components.get(&b.component), &mut h);
    doc.materials.get(name).hash(&mut h);
    faces.get(name).copied().unwrap_or(0).hash(&mut h);
    colored.hash(&mut h);
    if colored {
        doc.component_identification_color(b.component).hash(&mut h);
    }
    h.finish()
}

fn resolve_appearance(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody) -> ResolvedAppearance {
    let s = &app.session;
    ResolvedAppearance {
        component: solidcraft_engine::view::component_color(s, b.component).map(|[r, g, b]| Color32::from_rgb(r, g, b)),
        body: body_color(&s.doc, b),
        opacity: s.doc.model_body_opacity(b),
        face_looks: if s.colored_components.is_none() { s.doc.face_colors(b).into_iter().collect() } else { Default::default() },
    }
}

/// How a body whose faces all share one class may be drawn from its plan: opaque on any GPU
/// path; faded or see-through only where the renderer blends them order-independently (the
/// sorted fallback needs the triangles on the CPU). `None`: faces of different classes, which
/// are built on the CPU. `faces` are the classes [`build_scene`] gives the body's faces.
fn plan_class(app: &SolveApp, faces: &[crate::gpu::TriClass]) -> Option<crate::gpu::TriClass> {
    let first = faces.first().copied().unwrap_or(crate::gpu::TriClass::Opaque);
    if faces.iter().any(|c| *c != first) {
        return None;
    }
    (first == crate::gpu::TriClass::Opaque || app.viewport.gpu.is_some_and(|g| g.oit)).then_some(first)
}

/// The classes [`build_scene`] gives `b`'s faces: faded outside the active component, else
/// see-through below full opacity (the body's, or a face's own look).
fn face_classes(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, active_bodies: &Option<Vec<String>>) -> Vec<crate::gpu::TriClass> {
    if body_faded(app, b, active_bodies) {
        return vec![crate::gpu::TriClass::Faded; b.body.face_count()];
    }
    let resolved = resolve_appearance(app, b);
    (0..b.body.face_count())
        .map(|f| {
            let opacity = resolved.face_looks.get(&f).map_or(resolved.opacity, |l| l.opacity);
            if opacity < 0.999 { crate::gpu::TriClass::Glass } else { crate::gpu::TriClass::Opaque }
        })
        .collect()
}

/// Whether the model scene draws `b` from its plan's GPU-evaluated triangles (and with which
/// class), as [`build_scene`] decides: a live preview stands in for none.
fn model_plan_class(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, active_bodies: &Option<Vec<String>>) -> Option<crate::gpu::TriClass> {
    if app.preview.effective_replaced().contains(&b.name) {
        return None;
    }
    plan_class(app, &face_classes(app, b, active_bodies))
}

/// A model scene block kept by the viewport: its bytes and the meshes they were built from
/// (held, so no other mesh can take their addresses while the block is kept).
pub(crate) struct CachedBlock {
    bytes: std::sync::Arc<crate::gpu::BlockBytes>,
    meshes: Vec<std::sync::Arc<solidcraft_engine::geom::Mesh>>,
    /// Each body's share of the block: its vertices, its opaque runs of them (whole-body
    /// highlights draw those again), and the face classes the runs were built from.
    ranges: Vec<BodyRange>,
}

/// One body's share of a block.
struct BodyRange {
    name: String,
    /// Its vertices: three per triangle, in mesh order (none when its faces are drawn elsewhere).
    vertices: std::ops::Range<u32>,
    opaque: Vec<std::ops::Range<u32>>,
    face_classes: Vec<crate::gpu::TriClass>,
}

/// [`ViewportState::mesh_bounds`]: each box by the shared mesh it was measured on (held, so its
/// address stays its own) and the placement that moved it ([`ModelBody::mesh_in_place`]).
///
/// [`ModelBody::mesh_in_place`]: solidcraft_engine::doc::ModelBody::mesh_in_place
type MeshBounds = std::collections::HashMap<(usize, [u64; 16]), (std::sync::Arc<solidcraft_engine::geom::Mesh>, solidcraft_engine::geom::Aabb3)>;

/// [`ViewportState::instanced_tris`].
type InstancedTris = std::collections::HashMap<usize, (std::sync::Arc<solidcraft_engine::geom::Mesh>, std::sync::Arc<crate::gpu::SharedTris>)>;

/// A body alone is drawn as an instance of its triangles (shared vertices, indexed) from this many
/// triangles: a large imported mesh. Smaller ones are drawn so only when placed more than once.
pub(crate) const INSTANCE_ALONE: usize = 50_000;

/// Shared vertices made ahead, off the window thread ([`warm_shared_tris`]), by mesh address
/// (the mesh held, so its address stays its own), until a scene takes them.
static WARMED: std::sync::Mutex<Vec<(std::sync::Arc<solidcraft_engine::geom::Mesh>, std::sync::Arc<crate::gpu::SharedTris>)>> =
    std::sync::Mutex::new(Vec::new());
/// Most meshes kept warmed and not taken yet (a design opened and closed before it was drawn).
const MAX_WARMED: usize = 8;

/// Make a large mesh's shared vertices ahead of its first scene, on the caller's thread (the
/// background mesher, after a file is opened or inserted): welding a scanned part takes a
/// noticeable part of a second, which the window thread would otherwise spend on the first frame
/// that shows it.
pub(crate) fn warm_shared_tris(mesh: &std::sync::Arc<solidcraft_engine::geom::Mesh>) {
    if mesh.triangles.len() < INSTANCE_ALONE || WARMED.lock().map_or(true, |w| w.iter().any(|(m, _)| std::sync::Arc::ptr_eq(m, mesh))) {
        return;
    }
    let tris = std::sync::Arc::new(crate::gpu::SharedTris::new(mesh));
    if let Ok(mut warmed) = WARMED.lock() {
        if warmed.len() >= MAX_WARMED {
            warmed.remove(0);
        }
        warmed.push((mesh.clone(), tris));
    }
}

/// The shared vertices made ahead for `mesh`, taken from the warmed ones.
fn taken_shared_tris(mesh: &std::sync::Arc<solidcraft_engine::geom::Mesh>) -> Option<std::sync::Arc<crate::gpu::SharedTris>> {
    let mut warmed = WARMED.lock().ok()?;
    let at = warmed.iter().position(|(m, _)| std::sync::Arc::ptr_eq(m, mesh))?;
    Some(warmed.remove(at).1)
}

/// One body as the model scene draws it. Its geometry lives in a block's canonical vertex
/// buffer; its colours are palette indices, so appearance edits never touch vertex bytes.
struct BodyDraw {
    /// The body's name in the world state, for the highlight's lookup of its triangles.
    name: String,
    /// Its mesh where it stands, or (with `place`) the shared mesh `place` moves there.
    mesh: std::sync::Arc<solidcraft_engine::geom::Mesh>,
    /// The move from `mesh` to where the body stands, for a body drawn as an instance.
    place: Option<solidcraft_engine::doc::Mat>,
    /// The palette index of each face's colour, and the body's fallback.
    face_colors: Vec<crate::gpu::PaletteIndex>,
    body_color: crate::gpu::PaletteIndex,
    /// The render class of each face (opaque, glass or faded).
    face_classes: Vec<crate::gpu::TriClass>,
    faded: bool,
    /// Its faces are drawn elsewhere, from its plan on the GPU or as an instance of shared
    /// triangles (only its edges are built here).
    on_gpu: bool,
    /// The body's first face slot: stable while the body exists, so cached block indices are.
    slot_base: u32,
    edge: crate::gpu::PaletteIndex,
    hidden_edge: crate::gpu::PaletteIndex,
}

impl BodyDraw {
    /// Everything [`BodyDraw::build`] reads. Colours, opacity and classes are deliberately left
    /// out: changing them must not change the block's vertices.
    fn key(&self, shaded: bool, edges: bool, hidden_edges: bool) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (std::sync::Arc::as_ptr(&self.mesh) as usize).hash(&mut h);
        self.place.iter().flatten().flatten().for_each(|v| v.to_bits().hash(&mut h));
        (self.faded, self.on_gpu, shaded, edges, hidden_edges, self.slot_base, self.edge.0, self.hidden_edge.0).hash(&mut h);
        h.finish()
    }

    fn build(&self, out: &mut crate::gpu::BlockBuilder, shaded: bool, edges: bool, hidden_edges: bool) {
        let m = &self.mesh;
        let color = |ti: usize| m.tri_face.get(ti).and_then(|f| self.face_colors.get(*f as usize)).copied().unwrap_or(self.body_color);
        let class = |ti: usize| {
            if self.faded {
                return crate::gpu::TriClass::Faded;
            }
            m.tri_face.get(ti).and_then(|f| self.face_classes.get(*f as usize)).copied().unwrap_or(crate::gpu::TriClass::Opaque)
        };
        if shaded && !self.on_gpu {
            let corner = |k: u32| {
                let i = k as usize;
                (m.positions.get(i).copied().unwrap_or(Vec3::ZERO).to_f32(), m.normals.get(i).copied().unwrap_or(Vec3::ZERO).to_f32())
            };
            // Exactly three vertices per triangle, in mesh order: a class change regenerates the
            // runs without touching the vertices (see `reindex_block`).
            for (ti, t) in m.triangles.iter().enumerate() {
                let [a, b, c] = t.map(corner);
                out.triangle([a.0, b.0, c.0], [a.1, b.1, c.1], color(ti), class(ti));
            }
        }
        let at = |p: Vec3| self.place.as_ref().map_or(p, |m| solidcraft_engine::doc::apply_point(m, p)).to_f32();
        for (ei, e) in m.edges.iter().enumerate().filter(|_| edges) {
            if !m.edge_visible(ei) || (shaded && !m.shaded_edge_visible(ei)) {
                continue;
            }
            for w in e.windows(2) {
                let (a, b) = (at(w[0]), at(w[1]));
                out.line(a, b, self.edge, 1.3);
                if hidden_edges {
                    out.hidden_line(a, b, self.hidden_edge, 1.0);
                }
            }
        }
    }
}

/// Add a block of bodies to the scene: the bytes built for the same bodies last time when
/// there were any, else built now.
fn push_block(
    sc: &mut GpuScene,
    kept: &mut std::collections::HashMap<u64, CachedBlock>,
    made: &mut std::collections::HashMap<u64, CachedBlock>,
    key: u64,
    draws: Vec<BodyDraw>,
    shaded: bool,
    build: impl Fn(&BodyDraw, &mut crate::gpu::BlockBuilder),
) {
    let same = |c: &CachedBlock| c.meshes.len() == draws.len() && c.meshes.iter().zip(&draws).all(|(m, d)| std::sync::Arc::ptr_eq(m, &d.mesh));
    let classes_same = |c: &CachedBlock| c.ranges.len() == draws.len() && c.ranges.iter().zip(&draws).all(|(r, d)| r.face_classes == d.face_classes);
    let mut block = match kept.remove(&key).or_else(|| made.remove(&key)).filter(same) {
        Some(c) if classes_same(&c) => c,
        Some(c) => reindex_block(c, &draws),
        None => build_block(&draws, shaded, &build),
    };
    // The body names are not part of a block's key (its bytes are the same whatever a body is
    // called): keep the ranges' names those of the bodies drawn from them this time.
    if block.ranges.len() == draws.len() {
        for (range, d) in block.ranges.iter_mut().zip(&draws) {
            if range.name != d.name {
                range.name.clone_from(&d.name);
            }
        }
    }
    sc.blocks.push(crate::gpu::SceneBlock { key, bytes: block.bytes.clone() });
    made.insert(key, block);
}

/// Write a body's colour into the stable arena and into the scene palette being built from it.
fn set_palette_color(
    arena: &mut crate::scene_palette::PaletteArena,
    palette: &mut std::sync::Arc<crate::gpu::Palette>,
    slot: crate::gpu::PaletteIndex,
    rgba: [u8; 4],
) {
    // The scene palette starts as a copy of the arena's entries: an unchanged slot is already
    // right, and most rebuilds (visibility, LOD, section) change none.
    if arena.get(slot) == Some(rgba) {
        return;
    }
    arena.set(slot, rgba);
    std::sync::Arc::make_mut(palette).set(slot, rgba);
}

fn build_block(draws: &[BodyDraw], shaded: bool, build: &impl Fn(&BodyDraw, &mut crate::gpu::BlockBuilder)) -> CachedBlock {
    let mut builder = crate::gpu::BlockBuilder::default();
    if shaded {
        builder.reserve_triangles(draws.iter().filter(|d| !d.on_gpu).map(|d| d.mesh.triangles.len()).sum());
    }
    let at = |i: usize| u32::try_from(i).unwrap_or(u32::MAX);
    let mut spans = Vec::with_capacity(draws.len());
    for d in draws {
        let v0 = builder.vertices_len();
        // Its box goes with its triangles, so a view without it leaves them out.
        builder.begin_body();
        build(d, &mut builder);
        builder.end_body();
        spans.push(at(v0)..at(builder.vertices_len()));
    }
    let ranges = draws
        .iter()
        .zip(spans)
        .map(|(d, vertices)| BodyRange {
            name: d.name.clone(),
            opaque: builder.runs_within(crate::gpu::TriClass::Opaque, vertices.clone()),
            vertices,
            face_classes: d.face_classes.clone(),
        })
        .collect();
    CachedBlock { bytes: builder.freeze(), meshes: draws.iter().map(|d| d.mesh.clone()).collect(), ranges }
}

/// Keep a block's vertex, line and hidden artifacts and regenerate only the class runs (an
/// opacity class change). A body's vertices are three per triangle in mesh order, so this reads
/// each triangle's face, not the mesh's geometry.
fn reindex_block(block: CachedBlock, draws: &[BodyDraw]) -> CachedBlock {
    use crate::gpu::TriClass;
    let mut runs: [Vec<std::ops::Range<u32>>; 3] = Default::default();
    for (range, d) in block.ranges.iter().zip(draws) {
        // The triangles this body wrote (none when its faces are drawn from a plan or as an
        // instance), never more than its mesh has.
        let written = ((range.vertices.end.saturating_sub(range.vertices.start)) / 3) as usize;
        for j in 0..written.min(d.mesh.triangles.len()) {
            let class = d.mesh.tri_face.get(j).and_then(|f| d.face_classes.get(*f as usize)).copied().unwrap_or(TriClass::Opaque);
            let slot = match class {
                TriClass::Opaque => 0,
                TriClass::Glass => 1,
                TriClass::Faded => 2,
            };
            let at = range.vertices.start.saturating_add(u32::try_from(j * 3).unwrap_or(u32::MAX));
            if let Some(list) = runs.get_mut(slot) {
                crate::gpu::extend_runs(list, at);
            }
        }
    }
    let [opaque, glass, faded] = runs;
    let ranges = block
        .ranges
        .iter()
        .zip(draws)
        .map(|(range, d)| BodyRange {
            name: d.name.clone(),
            vertices: range.vertices.clone(),
            opaque: crate::gpu::runs_within(&opaque, range.vertices.clone()),
            face_classes: d.face_classes.clone(),
        })
        .collect();
    let old = block.bytes;
    let bytes = std::sync::Arc::new(crate::gpu::BlockBytes {
        vertices: old.vertices.clone(),
        bodies: old.bodies.clone(),
        opaque: std::sync::Arc::new(opaque),
        glass: std::sync::Arc::new(glass),
        faded: std::sync::Arc::new(faded),
        lines: old.lines.clone(),
        hidden: old.hidden.clone(),
    });
    CachedBlock { bytes, meshes: draws.iter().map(|d| d.mesh.clone()).collect(), ranges }
}

/// The model scene's instanced meshes: the shared triangles drawn more than once among the shown
/// bodies (or large: [`INSTANCE_ALONE`]), and each body placed from them.
struct Instances {
    /// Each candidate body's shared mesh and placement, by body name.
    candidates: std::collections::HashMap<String, (std::sync::Arc<solidcraft_engine::geom::Mesh>, solidcraft_engine::doc::Mat)>,
    /// The meshes drawn as instances, in the order their first placement came, by address.
    meshes: Vec<(std::sync::Arc<solidcraft_engine::geom::Mesh>, crate::gpu::InstancedMesh)>,
    /// Where a mesh's look already is in `meshes`, so placing a body is a lookup, not a scan.
    by_mesh: std::collections::HashMap<(usize, crate::gpu::InstanceLook), usize>,
    kept: InstancedTris,
    lods: crate::mesh_lod::MeshLods,
}

impl Instances {
    /// The shown bodies' shared meshes, keeping those more than one body is drawn from and large
    /// ones.
    fn new(app: &SolveApp, shown: &[&solidcraft_engine::doc::ModelBody], shaded: bool) -> Instances {
        let kept = app.viewport.instanced_tris.lock().map(|mut k| std::mem::take(&mut *k)).unwrap_or_default();
        let mut candidates = std::collections::HashMap::new();
        if shaded {
            let mut uses: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
            let placed: Vec<_> = shown.iter().filter_map(|b| Some((b.name.clone(), shared_mesh(app, b)?))).collect();
            for (_, (mesh, _)) in &placed {
                *uses.entry(std::sync::Arc::as_ptr(mesh) as usize).or_default() += 1;
            }
            candidates = placed
                .into_iter()
                .filter(|(_, (m, _))| m.triangles.len() >= INSTANCE_ALONE || uses.get(&(std::sync::Arc::as_ptr(m) as usize)).is_some_and(|n| *n > 1))
                .collect();
        }
        Instances { candidates, meshes: Vec::new(), by_mesh: std::collections::HashMap::new(), kept, lods: app.viewport.mesh_lods.clone() }
    }

    /// Draw `b` as an instance when it is one, in colour `col` (its alpha for a see-through
    /// `look`): its shared mesh and placement.
    fn place(
        &mut self,
        b: &solidcraft_engine::doc::ModelBody,
        col: crate::gpu::PaletteIndex,
        look: crate::gpu::InstanceLook,
    ) -> Option<(std::sync::Arc<solidcraft_engine::geom::Mesh>, solidcraft_engine::doc::Mat)> {
        let (mesh, m) = self.candidates.get(&b.name)?.clone();
        let rows = [0, 1, 2].map(|r| [m[0][r] as f32, m[1][r] as f32, m[2][r] as f32, m[3][r] as f32]);
        let key = std::sync::Arc::as_ptr(&mesh) as usize;
        let at = match self.by_mesh.get(&(key, look)).copied() {
            Some(at) => at,
            None => {
                // The same triangles (and levels) for each look of a mesh.
                let made = self.meshes.iter().find(|(k, _)| std::sync::Arc::ptr_eq(k, &mesh)).map(|(_, i)| (i.tris.clone(), i.levels.clone()));
                let (tris, levels) = match made {
                    Some(made) => made,
                    None => (
                        match self.kept.get(&key).filter(|(k, _)| std::sync::Arc::ptr_eq(k, &mesh)) {
                            Some((_, t)) => t.clone(),
                            None => taken_shared_tris(&mesh).unwrap_or_else(|| std::sync::Arc::new(crate::gpu::SharedTris::new(&mesh))),
                        },
                        // A large mesh's levels of detail, once made (until then, drawn in full).
                        self.lods.levels(&mesh).map(|l| l.as_ref().clone()).unwrap_or_default(),
                    ),
                };
                self.meshes.push((mesh.clone(), crate::gpu::InstancedMesh { key: key as u64, tris, instances: Vec::new(), look, levels }));
                self.by_mesh.insert((key, look), self.meshes.len() - 1);
                self.meshes.len() - 1
            }
        };
        let (_, inst) = self.meshes.get_mut(at)?;
        inst.place(rows, col);
        Some((mesh, m))
    }

    /// The instanced meshes, keeping their triangles for the next scene.
    fn finish(self, app: &SolveApp) -> Vec<crate::gpu::InstancedMesh> {
        let kept: InstancedTris =
            self.meshes.iter().map(|(mesh, inst)| (std::sync::Arc::as_ptr(mesh) as usize, (mesh.clone(), inst.tris.clone()))).collect();
        self.lods.retain(&kept.keys().copied().collect());
        if let Ok(mut k) = app.viewport.instanced_tris.lock() {
            *k = kept;
        }
        self.meshes.into_iter().map(|(_, inst)| inst).collect()
    }
}

/// The shared mesh `b`'s faces are drawn from and the move that places it ([`drawn_mesh`]
/// without making the moved copy); `None` for a body drawn finer than its display quality.
fn shared_mesh(
    app: &SolveApp,
    b: &solidcraft_engine::doc::ModelBody,
) -> Option<(std::sync::Arc<solidcraft_engine::geom::Mesh>, solidcraft_engine::doc::Mat)> {
    use solidcraft_engine::doc::display_quality::{DisplayQuality, current};
    if refined(app, b).is_some() {
        return None;
    }
    let q = current();
    Some(if q == DisplayQuality::Normal { b.mesh_in_place() } else { b.plan_mesh_in_place_at(q).unwrap_or_else(|| b.mesh_in_place()) })
}

pub(crate) fn build_scene(app: &SolveApp) -> GpuScene {
    let tk = Tokens::get();
    let s = &app.session;
    // Bodies where their occurrences place them.
    let st = crate::open_meshing::shown_world(app);
    let active_bodies = s.active_occurrence_path.as_ref().and_then(|p| s.occurrence_bodies(p, true).ok());
    let (shaded, edges) = match app.ui.visual_style {
        1 => (true, false),
        2 => (false, true),
        _ => (true, true),
    };
    let edge_col = if shaded { c4(tk.body_edge) } else { c4(tk.text) };
    // Shaded with hidden edges: edges behind the model show faintly.
    let hidden_edges = app.ui.visual_style == 3;
    let hidden_col = c4(tk.text.gamma_multiply(0.35));
    let shown: Vec<&solidcraft_engine::doc::ModelBody> =
        st.bodies.iter().filter(|b| app.session.body_visible(&b.name) && !app.preview.effective_replaced().contains(&b.name)).collect();
    let mut arena = app.viewport.palette.lock().unwrap_or_else(|e| e.into_inner());
    let shown_names: Vec<&str> = shown.iter().map(|b| b.name.as_str()).collect();
    arena.sync(shown_names.iter().copied());
    // Allocate every body's slots before the scene's palette is copied, so the stable prefix
    // covers them; the colours themselves are written in the loop below, into both the arena
    // and the scene's palette.
    let slots: Vec<_> = shown.iter().map(|b| arena.body(&b.name, b.body.face_count())).collect();
    let mut sc = GpuScene::with_palette(crate::gpu::Palette::with_stable(arena.entries()));
    // Resolve looks once per body and appearance state: a theme, visibility or LOD rebuild does
    // not scan the whole design's face appearances again.
    let mut appearance = app.viewport.appearance.lock().unwrap_or_else(|e| e.into_inner());
    let doc = std::sync::Arc::as_ptr(&s.doc) as usize;
    let colored = s.colored_components.is_some();
    if appearance.doc != Some((doc, colored)) {
        appearance.doc = Some((doc, colored));
        appearance.faces = face_appearance_hashes(&s.doc);
        // A design or colouring edit can key any body differently; the resolved looks whose
        // key comes back unchanged are still reused.
        appearance.keys.clear();
    }
    // The blocks the last scene built, reused when their bodies come back as they were.
    let mut kept = app.viewport.blocks.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
    let mut made: std::collections::HashMap<u64, CachedBlock> = std::collections::HashMap::new();
    let mut run: Vec<BodyDraw> = Vec::new();
    let mut run_key = std::collections::hash_map::DefaultHasher::new();
    // Bodies drawn from the same triangles more than once (a part placed by many occurrences)
    // are drawn as instances of them: one copy of the triangles on the GPU, and none moved on
    // the CPU.
    let mut instances = Instances::new(app, &shown, shaded);
    let mut thread_budget = solidcraft_engine::cosmetic_threads::Budget::default();
    for (b, slots) in shown.into_iter().zip(slots) {
        let faded = body_faded(app, b, &active_bodies);
        let identity = match appearance.keys.get(&b.name).copied() {
            Some(key) => key,
            None => {
                let key = body_appearance_key(&s.doc, b, &appearance.faces, colored);
                appearance.keys.insert(b.name.clone(), key);
                key
            }
        };
        let resolved = match appearance.bodies.get(&b.name) {
            Some((key, resolved)) if *key == identity => resolved.clone(),
            _ => {
                let resolved = std::sync::Arc::new(resolve_appearance(app, b));
                appearance.bodies.insert(b.name.clone(), (identity, resolved.clone()));
                #[cfg(test)]
                {
                    appearance.resolved = appearance.resolved.saturating_add(1);
                }
                resolved
            }
        };
        set_palette_color(&mut arena, &mut sc.palette, slots.edge, edge_col);
        set_palette_color(&mut arena, &mut sc.palette, slots.hidden_edge, hidden_col);
        if app.preferences.cosmetic_threads
            && thread_budget.available()
            && shaded
            && !faded
            && thread_budget.has_thread(&b.name, &st.threads)
            && resolved.opacity >= 0.999
            && resolved.face_looks.values().all(|l| l.opacity >= 0.999)
        {
            let mesh = shown_mesh(app, b, &active_bodies);
            for (a, c) in solidcraft_engine::cosmetic_threads::segments_with_budget(b, &mesh, &st.threads, &mut thread_budget) {
                sc.line(a.to_f32(), c.to_f32(), slots.edge, 0.85, false);
            }
        }
        let base = resolved.component.or(resolved.body).unwrap_or(tk.body);
        let col = if faded { c4(base.gamma_multiply(0.35)) } else { c4(base) };
        set_palette_color(&mut arena, &mut sc.palette, slots.body, col);
        // Appearances: the body's opacity, and faces with looks of their own.
        let opacity = resolved.opacity;
        let face_looks = &resolved.face_looks;
        let faces = b.body.face_count();
        let mut face_colors = Vec::with_capacity(faces);
        let mut face_classes = Vec::with_capacity(faces);
        for f in 0..faces {
            let look = face_looks.get(&f);
            let color = look.map_or(base, |l| Color32::from_rgb(l.color[0], l.color[1], l.color[2]));
            let opacity = look.map_or(opacity, |l| l.opacity);
            let (rgba, class) = if faded {
                (c4(color.gamma_multiply(0.35)), crate::gpu::TriClass::Faded)
            } else if opacity < 0.999 {
                let mut rgba = c4(color);
                rgba[3] = (opacity.clamp(0.05, 1.0) * 255.0).round() as u8;
                (rgba, crate::gpu::TriClass::Glass)
            } else {
                (c4(color), crate::gpu::TriClass::Opaque)
            };
            let slot = slots.face(f);
            set_palette_color(&mut arena, &mut sc.palette, slot, rgba);
            face_colors.push(slot);
            face_classes.push(class);
        }
        let class = plan_class(app, &face_classes);
        let planned = class.and_then(|class| drawn_plan(app, b, true).map(|plan| (plan, class)));
        let on_gpu = planned.is_some();
        if let Some((plan, class)) = planned {
            sc.planned.push(crate::gpu::PlannedBody { plan, face_colors: face_colors.clone(), class });
        }
        // Opaque, faded and see-through bodies alike, in one colour (faces of their own colour
        // are built as before).
        let look = if faded {
            crate::gpu::InstanceLook::Faded
        } else if opacity < 0.999 {
            crate::gpu::InstanceLook::Glass
        } else {
            crate::gpu::InstanceLook::Opaque
        };
        let alpha = match look {
            crate::gpu::InstanceLook::Faded => col[3],
            crate::gpu::InstanceLook::Glass => (opacity.clamp(0.05, 1.0) * 255.0).round() as u8,
            crate::gpu::InstanceLook::Opaque => 255,
        };
        set_palette_color(&mut arena, &mut sc.palette, slots.instance, [col[0], col[1], col[2], alpha]);
        let instanced = (!on_gpu && face_looks.is_empty()).then(|| instances.place(b, slots.instance, look)).flatten();
        let name = b.name.clone();
        let body = match instanced {
            Some((mesh, place)) => BodyDraw {
                name,
                mesh,
                place: Some(place),
                face_colors,
                body_color: slots.body,
                face_classes,
                faded,
                on_gpu: true,
                slot_base: slots.faces,
                edge: slots.edge,
                hidden_edge: slots.hidden_edge,
            },
            None => BodyDraw {
                name,
                mesh: drawn_mesh(app, b),
                place: None,
                face_colors,
                body_color: slots.body,
                face_classes,
                faded,
                on_gpu,
                slot_base: slots.faces,
                edge: slots.edge,
                hidden_edge: slots.hidden_edge,
            },
        };
        let key = body.key(shaded, edges, hidden_edges);
        key.hash(&mut run_key);
        run.push(body);
        // Blocks end where a body's own key says, so a body added or removed changes only its
        // own block, not every block after it.
        if key.is_multiple_of(16) || run.len() >= 64 {
            let draws = std::mem::take(&mut run);
            let block_key = std::mem::replace(&mut run_key, std::collections::hash_map::DefaultHasher::new()).finish();
            push_block(&mut sc, &mut kept, &mut made, block_key, draws, shaded, |d, out| d.build(out, shaded, edges, hidden_edges));
        }
    }
    if !run.is_empty() {
        push_block(&mut sc, &mut kept, &mut made, run_key.finish(), run, shaded, |d, out| d.build(out, shaded, edges, hidden_edges));
    }
    // Where each CPU-built body's triangles are in the model's blocks, for whole-body
    // highlights (see `highlight_block_tris`).
    let mut body_tris = std::collections::HashMap::new();
    for (key, c) in &made {
        for range in &c.ranges {
            if !range.vertices.is_empty() {
                body_tris
                    .insert(range.name.clone(), crate::gpu::BlockTris { block: *key, tris: range.opaque.clone(), vertices: range.vertices.clone() });
            }
        }
    }
    if let Ok(mut m) = app.viewport.body_tris.lock() {
        *m = body_tris;
    }
    // Every shown body has its entries, so only a larger map holds bodies no longer shown.
    if appearance.bodies.len() > shown_names.len() || appearance.keys.len() > shown_names.len() {
        let names: std::collections::HashSet<&str> = shown_names.iter().copied().collect();
        appearance.bodies.retain(|name, _| names.contains(name.as_str()));
        appearance.keys.retain(|name, _| names.contains(name.as_str()));
    }
    if let Ok(mut b) = app.viewport.blocks.lock() {
        *b = made;
    }
    sc.instanced = instances.finish(app);
    if app.ui.ground_shadow && app.ui.visual_style != 2 && app.cam.pitch > 0.0 {
        ground_shadow(&mut sc, app, &st);
    }
    // Section: the outline of each body where the plane cuts it, in the view's text colour (#4146).
    if let Some((o, n)) = section_plane(app) {
        let col = sc.color(c4(Tokens::view().text.gamma_multiply(0.9)));
        let mut kept = app.viewport.section_index.lock().map(|mut k| std::mem::take(&mut *k)).unwrap_or_default();
        let mut made = std::collections::HashMap::with_capacity(kept.len());
        let normal = [n.x.to_bits(), n.y.to_bits(), n.z.to_bits()];
        for b in st.bodies.iter().filter(|b| app.session.body_visible(&b.name)) {
            let mesh = b.mesh();
            let key = (std::sync::Arc::as_ptr(&mesh) as usize, normal);
            let index = match kept.remove(&key).filter(|(m, i)| std::sync::Arc::ptr_eq(m, &mesh) && i.normal() == n) {
                Some((_, index)) => index,
                None => std::sync::Arc::new(solidcraft_engine::view::SectionIndex::new(&mesh, n)),
            };
            for (a, c) in index.segments(&mesh, o) {
                sc.line(a.to_f32(), c.to_f32(), col, 1.8, false);
            }
            made.insert(key, (mesh, index));
        }
        if let Ok(mut k) = app.viewport.section_index.lock() {
            *k = made;
        }
    }
    // Continuity analysis: each seam `inspect.continuity` measured, drawn over the model in the
    // colour of its worst verdict.
    for seam in &s.continuity {
        let col = sc.color(c4(continuity_color(&tk, seam.verdict)));
        for w in seam.points.windows(2) {
            if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                sc.line(a.to_f32(), b.to_f32(), col, 2.6, true);
            }
        }
    }

    sc
}

/// The colour of a measured seam, one per verdict from the theme's `continuity` tokens: within
/// every tolerance (G2), tangent with a curvature jump (G1), a crease (G0), and faces that do
/// not meet.
fn continuity_color(t: &Tokens, verdict: solidcraft_engine::kernel::SeamVerdict) -> Color32 {
    use solidcraft_engine::kernel::SeamVerdict as V;
    let [smooth, jump, sharp, gap] = t.continuity;
    match verdict {
        V::Smooth => smooth,
        V::CurvatureJump => jump,
        V::Sharp => sharp,
        V::Gap => gap,
    }
}

/// A soft dark ellipse on the ground under the shown bodies (darkest in the middle).
fn build_sketch_scene(app: &SolveApp, proj: &Proj) -> GpuScene {
    let tk = Tokens::get();
    let mut sc = GpuScene::default();
    let s = &app.session;
    let st = crate::open_meshing::shown_world(app);
    let mut fill_budget = 131_072;
    let sketch_plane = solidcraft_engine::view::open_sketch_plane(s);
    if sketch_plane.is_some() || (app.ui.show_grid && !active_view(app).hide_grid) {
        let (minor, major) = grid_step(app.cam.half_height());
        // An open sketch always has its grid on its own plane; otherwise the ground plane: XY with
        // Z up, XZ with Y up (laid out in the camera's Z-up frame), or the upright origin plane
        // facing the view when the ground is seen edge-on.
        let (c, u, v, slant, ext, far_reach, w3): (Vec3, Vec3, Vec3, f64, f64, f64, Box<dyn Fn(Vec3) -> [f32; 3]>) = if let Some(pl) = &sketch_plane {
            let (c, u, v) = solidcraft_engine::view::sketch_grid_frame(pl, app.cam.target, major);
            // How steeply the sketch's plane is seen: the grid reaches as far on it as the view
            // is wide, so the fade with the distance does too (#4214). The sketch's plane is in
            // the world, as the camera's direction is.
            let n = u.cross(v);
            let slant = app.cam.back().dot(n);
            let ext = grid_ext(app.cam.half_height(), major, slant);
            let far_reach = solidcraft_engine::view::grid_far_reach(&proj.cam, picture_far(app, &proj.cam), c, n);
            (c, u, v, slant, ext, far_reach, Box::new(|p| p.to_f32()))
        } else {
            let tf = app.cam.to_frame(app.cam.target);
            // On the ground (XY) unless the Grid follows the view preference is on (#1470); then on
            // the plane the last frame drew, so the choice is hysteretic near the thresholds (#1993).
            let (u, v) = if app.preferences.grid_follows_view {
                app.viewport.grid_plane.lock().map_or((Vec3::X, Vec3::Y), |mut last| {
                    *last = grid_plane(app.cam.to_frame(app.cam.back()), *last);
                    last.axes()
                })
            } else {
                (Vec3::X, Vec3::Y)
            };
            // Centred where the view's axis meets the ground, the part of the ground in the middle
            // of the view (#3087): the camera's target may lie off the ground (above a part, or
            // below the ground after an orbit), and zoomed in, a grid round the point under the
            // target was off screen. Seen at a slant the ground stretches across the view, so the
            // grid reaches further.
            let back = app.cam.to_frame(app.cam.back());
            let n = u.cross(v);
            let slant = back.dot(n);
            let ext = grid_ext(app.cam.half_height(), major, slant);
            let centre = grid_centre(tf, back, n, ext);
            let snap = |a: Vec3| a * ((centre.dot(a) / major).round() * major);
            let c = snap(u) + snap(v);
            // The far plane cuts the ground, and the grid's plane is the camera's frame's.
            let far_reach =
                solidcraft_engine::view::grid_far_reach(&proj.cam, picture_far(app, &proj.cam), app.cam.from_frame(c), app.cam.from_frame(n));
            (c, u, v, slant, ext, far_reach, Box::new(|p| app.cam.from_frame(p).to_f32()))
        };
        // The grid fades out with the distance from the middle of the view (#4214): it used to
        // stop at a fixed distance, so its edge showed as a cut across the picture. Each line is
        // now drawn in pieces, each as faint as the fade makes it there, and the grid is gone
        // before its own edge, which is therefore never seen.
        let fade = GridFade::new(app.cam.half_height(), slant, ext, far_reach);
        let n = ((fade.end / minor) as i64).clamp(1, 300);
        let (grid, grid_major) = (c4(tk.grid), c4(tk.grid_major));
        for i in -n..=n {
            let t = i as f64 * minor;
            let maj = (t / major).round() * major == t;
            let col = if maj { grid_major } else { grid };
            let wd = if maj { 1.0 } else { 0.7 };
            // Two lines through the point `c + u * t`: one along v, one along u.
            for (along, across) in [(v, u), (u, v)] {
                for (s0, s1, alpha) in fade.pieces(t) {
                    let col = sc.color(grid_colour(col, alpha));
                    sc.under_line(w3(c + across * t + along * s0), w3(c + across * t + along * s1), col, wd);
                }
            }
        }
    }
    for sid in visible_sketches(app) {
        let Some(ss) = st.sketch(sid) else { continue };
        let active = s.active_sketch == Some(sid);
        // Closed profiles are shaded (lifted off a face they may lie on).
        let lift = ss.plane.normal() * 0.01;
        let profiles = if app.session.visibility.hidden_profiles.contains(&sid) || ss.sketch.view.hide_profile { &[][..] } else { &ss.profiles[..] };
        let profile_fill = sc.color(c4(tk.profile_fill));
        for (index, _) in profiles.iter().enumerate() {
            let display = crate::sketch_display::profile_region(app, sid, index, &ss.plane, proj);
            for tri in display.as_ref().as_ref().map_or_else(Vec::new, |region| crate::sketch_display::fill_triangles(region, &mut fill_budget)) {
                for q in tri {
                    sc.trans_tri((ss.plane.to_world(q) + lift).to_f32(), [0.0; 3], profile_fill);
                }
            }
        }
        // Construction curves are dashed, centerlines dash-dot (lengths follow the grid).
        // When enabled, a finished sketch's own curves sit on a wider halo (#4862) to read on a
        // shaded face. Projected (reference) curves get none: they trace the face's own edges.
        // All the halos go first, so one curve's halo never cuts into another where they meet.
        let unit = grid_step(app.cam.half_height()).0;
        let mut strokes = Vec::new();
        for (pts, col, cons) in sketch_lines_with_sampler(&ss.sketch, &ss.plane, active, &ss.report.curve_determined, |_, index, plane| {
            crate::sketch_display::curve_sample(app, sid, index, plane, proj).parts.clone()
        }) {
            let segs = sketch_segments(&pts, sketch_line_mode(col, cons), unit);
            let haloed = app.preferences.sketch_halo && !active && col != colors::SKETCH_PROJECTED && col != colors::SKETCH_LOST;
            strokes.push((segs, sc.color(sketch_color(&tk, col)), if cons { 1.2 } else { 2.0 }, haloed));
        }
        // Active sketches retain their existing through-body display. Inactive sketches
        // can opt into the same behaviour in Preferences.
        let through = active || app.preferences.reference_geometry_through_bodies;
        let halo = sc.color(c4(tk.sketch_halo));
        for (segs, _, width, _) in strokes.iter().filter(|stroke| stroke.3) {
            for (a, b) in segs {
                sc.line(a.to_f32(), b.to_f32(), halo, width + SKETCH_HALO_WIDTH, through);
            }
        }
        for (segs, color, width, _) in &strokes {
            for (a, b) in segs {
                sc.line(a.to_f32(), b.to_f32(), *color, *width, through);
            }
        }
    }
    let guide_unit = grid_step(app.cam.half_height()).0;
    let warning = sc.color(c4(tk.warning));
    for points in crate::sketch_attachment_ui::display_guide_lines(app, proj) {
        for (a, b) in dashes(&points, &[guide_unit * 0.25, guide_unit * 0.15]) {
            sc.line(a.to_f32(), b.to_f32(), warning, 1.5, true);
        }
    }
    sc
}

fn ground_shadow(sc: &mut GpuScene, app: &SolveApp, st: &std::sync::Arc<solidcraft_engine::doc::ModelState>) {
    // The bodies' box in the camera's Z-up frame (the ground is its XY plane).
    let mut b = solidcraft_engine::geom::Aabb3::EMPTY;
    let boxes = body_bounds(app, st);
    for (x, (_, bb)) in st.bodies.iter().zip(boxes.iter()) {
        if bb.is_empty() || !app.session.body_visible(&x.name) {
            continue;
        }
        for i in 0..8 {
            let p = Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            b.add(app.cam.to_frame(p));
        }
    }
    if b.is_empty() {
        return;
    }
    let size = b.size();
    let c = b.center();
    let z = b.min.z - 1e-3 * (1.0 + b.diagonal());
    let (rx, ry) = (size.x * 0.62 + size.z * 0.15, size.y * 0.62 + size.z * 0.15);
    if !(rx > 0.0 && ry > 0.0) {
        return;
    }
    let dark = if crate::theme::viewport_dark() { 150 } else { 80 };
    // Rings from the middle out, fading to nothing.
    let rings = [(0.0, dark), (0.55, dark * 3 / 4), (0.85, dark / 3), (1.0, 0)];
    let n = 48;
    let at = |r: f64, k: usize| {
        let a = k as f64 / n as f64 * std::f64::consts::TAU;
        app.cam.from_frame(Vec3::new(c.x + rx * r * a.cos(), c.y + ry * r * a.sin(), z)).to_f32()
    };
    for w in rings.windows(2) {
        let ((r0, a0), (r1, a1)) = (w[0], w[1]);
        let (c0, c1) = (sc.color([0, 0, 0, a0 as u8]), sc.color([0, 0, 0, a1 as u8]));
        for k in 0..n {
            let (p00, p01, p10, p11) = (at(r0, k), at(r0, k + 1), at(r1, k), at(r1, k + 1));
            sc.shadow_tri(p00, c0);
            sc.shadow_tri(p10, c1);
            sc.shadow_tri(p11, c1);
            if r0 > 0.0 {
                sc.shadow_tri(p00, c0);
                sc.shadow_tri(p11, c1);
                sc.shadow_tri(p01, c0);
            }
        }
    }
}

/// The [`sketch_segments`] line type of a [`sketch_lines_with_sampler`] polyline: solid,
/// construction (dashed) or centerline (dash-dot).
fn sketch_line_mode(col: Rgb, cons: bool) -> u8 {
    if !cons {
        0
    } else if col == colors::SKETCH_CENTERLINE {
        2
    } else {
        1
    }
}

/// Sketch line types share the same world-space dash phase in the scene and overlays.
pub(crate) fn sketch_segments(pts: &[Vec3], mode: u8, unit: f64) -> Vec<(Vec3, Vec3)> {
    match mode {
        // Construction, and infinite lines (#1359), which are construction too.
        1 | 3 => dashes(pts, &[0.3 * unit, 0.2 * unit]),
        2 => dashes(pts, &[0.6 * unit, 0.15 * unit, 0.05 * unit, 0.15 * unit]),
        _ => pts.windows(2).map(|w| (w[0], w[1])).collect(),
    }
}

/// The drawn pieces of a polyline in a dash pattern (on, off, on, off… lengths).
pub(crate) fn dashes(pts: &[Vec3], pattern: &[f64]) -> Vec<(Vec3, Vec3)> {
    let mut out = Vec::new();
    if pattern.is_empty() || pattern.iter().any(|l| !(l.is_finite() && *l > 0.0)) {
        return pts.windows(2).map(|w| (w[0], w[1])).collect();
    }
    let (mut k, mut left) = (0usize, pattern[0]);
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = (b - a).len();
        // Cap the pieces per segment so a huge zoom-out can't explode the scene.
        let mut guard = 0;
        while len > 1e-12 && guard < 10_000 {
            guard += 1;
            let step = left.min(len);
            let q = a + (b - a) * (step / len);
            if k % 2 == 0 {
                out.push((a, q));
            }
            left -= step;
            len -= step;
            a = q;
            if left <= 1e-12 {
                k = (k + 1) % pattern.len();
                left = pattern.get(k).copied().unwrap_or(1.0);
            }
        }
    }
    out
}

/// Colour as straight (unpremultiplied) sRGBA bytes for the GPU.
fn c4(c: Color32) -> [u8; 4] {
    c.to_srgba_unmultiplied()
}

/// Origin planes and axes currently shown, sized for the view: (name, normal, quad).
pub fn origin_planes(app: &SolveApp) -> Vec<(&'static str, Vec3, [Vec3; 4])> {
    if !app.origin_visible() {
        return Vec::new();
    }
    // While the Origin is hidden, a dialog picking a plane brings the planes back: then every
    // plane; otherwise only those not hidden one by one.
    let creating = !app.origin_shown() && app.picking_plane();
    origin_quads(origin_size(app.cam.half_height()))
        .into_iter()
        .filter(|(n, _, _)| creating || !app.session.visibility.hidden_origin.iter().any(|h| h == n))
        .collect()
}

/// A point on origin plane `name`'s square that a click at its screen point picks: the middle,
/// or else the middle of a quarter the other planes don't cover from this view (in an iso view
/// the XZ square hides the middle of the XY one). The middle when no candidate is picked.
pub fn origin_plane_target(app: &SolveApp, proj: &Proj, name: &str) -> Option<Vec3> {
    let (_, _, q) = origin_planes(app).into_iter().find(|(n, _, _)| *n == name)?;
    let mid = (q[0] + q[1] + q[2] + q[3]) / 4.0;
    let picked = |w: &Vec3| proj.to_screen(*w).is_some_and(|p| matches!(pick(app, proj, p).first(), Some(Hit::Plane { name: n, .. }) if n == name));
    Some(std::iter::once(mid).chain(q.iter().map(|c| (mid + *c) / 2.0)).find(picked).unwrap_or(mid))
}

pub fn origin_axes(app: &SolveApp) -> Vec<(&'static str, Vec3)> {
    if !app.origin_visible() {
        return Vec::new();
    }
    [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)]
        .into_iter()
        .filter(|(n, _)| !app.session.visibility.hidden_origin.iter().any(|h| h == n))
        .collect()
}

/// Construction axes shown, as segments: across the model where the axis passes it (a little
/// past it either side), at least the origin axes' length around the axis's origin: (name,
/// from, to).
pub fn construction_axis_segments(app: &SolveApp) -> Vec<(String, Vec3, Vec3)> {
    let axes: Vec<_> =
        solidcraft_engine::view::construction_axes(&app.session).into_iter().filter(|(_, n, _, _)| app.session.construction_visible(n)).collect();
    if axes.is_empty() {
        return Vec::new();
    }
    let bb = view_bounds(app);
    axes.into_iter()
        .map(|(_, name, o, d)| {
            let (a, b) = axis_span(app, &bb, o, d);
            (name, a, b)
        })
        .collect()
}

/// How far an axis through `o` along `d` is drawn: across the model's box `bb` where it passes
/// it (a little past it either side), at least the origin axes' length around `o`.
fn axis_span(app: &SolveApp, bb: &solidcraft_engine::geom::Aabb3, o: Vec3, d: Vec3) -> (Vec3, Vec3) {
    let h = origin_size(app.cam.half_height()) * AXIS_LEN;
    let (mut lo, mut hi) = (-h, h);
    if !bb.is_empty() {
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            let t = (c - o).dot(d);
            lo = lo.min(t - h * 0.3);
            hi = hi.max(t + h * 0.3);
        }
    }
    (o + d * lo, o + d * hi)
}

/// What a round edge or face stands for in the open dialog's active input, drawn as construction
/// geometry while it is under the cursor or picked: the axis an axis input takes (a dashed line
/// through a circle's centre along its normal, or along a cylinder's, cone's or torus's axis),
/// or the centre a point input takes from a circular edge (a small star).
pub(crate) fn round_pick_feedback(app: &SolveApp) -> (Vec<(Vec3, Vec3)>, Vec<Vec3>) {
    let (mut lines, mut centres): (Vec<(Vec3, Vec3, Vec3, Vec3)>, Vec<Vec3>) = (Vec::new(), Vec::new());
    let Some(d) = app.dialog.as_ref() else { return (Vec::new(), centres) };
    let Some(inp) = d.active_input() else { return (Vec::new(), centres) };
    let s = &app.session;
    let hovered = app.viewport.hover.as_ref().and_then(|h| d.candidate(s, h));
    if inp.accept == crate::selection::AXIS_PICKS {
        let bb = view_bounds(app);
        for sel in hovered.iter().chain(inp.items.iter()) {
            if let Some((o, dir)) = crate::dialogs::round_axis(s, sel)
                && !lines.iter().any(|(_, _, q, e)| q.dist(o) < 1e-9 && e.cross(dir).len() < 1e-9)
            {
                let (a, b) = axis_span(app, &bb, o, dir);
                lines.push((a, b, o, dir));
            }
        }
    } else if let (Some(Hit::Edge { body, index, .. }), Some(sel)) = (app.viewport.hover.as_ref(), hovered.as_ref()) {
        // Where the round edge is taken as its centre (`Dialog::circle_centre_pick`, Move's points).
        if (matches!(sel, Sel::Vertex { .. }) || d.edge_is_centre(inp))
            && let Some((c, ..)) = crate::dialogs::edge_circle(s, body, *index)
        {
            centres.push(c);
        }
    }
    (lines.into_iter().map(|(a, b, ..)| (a, b)).collect(), centres)
}

/// Construction points shown: (name, position).
pub fn construction_points(app: &SolveApp) -> Vec<(String, Vec3)> {
    solidcraft_engine::view::construction_points(&app.session)
        .into_iter()
        .filter(|(_, n, _)| app.session.construction_visible(n))
        .map(|(_, n, p)| (n, p))
        .collect()
}

/// Construction planes drawn as squares around their origin: (name, normal, quad).
pub fn construction_quads(app: &SolveApp) -> Vec<(String, Vec3, [Vec3; 4])> {
    let h = origin_size(app.cam.half_height()) * 1.2;
    solidcraft_engine::view::construction_planes(&app.session)
        .into_iter()
        .filter(|(_, n, _)| app.session.construction_visible(n))
        .map(|(_, name, pl)| {
            let (o, x, y) = (pl.origin, pl.x * h, pl.y * h);
            (name, pl.normal(), [o - x - y, o + x - y, o + x + y, o - x + y])
        })
        .collect()
}

/// Pick what is under `pos`: sketch points, sketch curves, vertices, edges and axes first, then
/// the surfaces along the ray (profiles, faces, planes) nearest first.
/// Bounding boxes of the shown model's bodies (cached until the model changes).
pub(crate) fn body_bounds(
    app: &SolveApp,
    st: &std::sync::Arc<solidcraft_engine::doc::ModelState>,
) -> std::sync::Arc<Vec<(String, solidcraft_engine::geom::Aabb3)>> {
    if let Ok(mut g) = app.viewport.pick_bounds.lock() {
        if let Some((k, b)) = g.as_ref()
            && std::sync::Arc::ptr_eq(k, st)
        {
            return b.clone();
        }
        // Boxes of meshes the last state had too are kept (each body's box walks its mesh). A
        // placed part's box is its shared mesh's points moved, so no moved copy is made.
        let mut known = app.viewport.mesh_bounds.lock().map(|mut k| std::mem::take(&mut *k)).unwrap_or_default();
        let mut now: MeshBounds = std::collections::HashMap::with_capacity(st.bodies.len());
        let b = std::sync::Arc::new(
            st.bodies
                .iter()
                .map(|b| {
                    let (mesh, m) = b.mesh_in_place();
                    let key = (std::sync::Arc::as_ptr(&mesh) as usize, std::array::from_fn(|i| m[i / 4][i % 4].to_bits()));
                    let bounds = match known.remove(&key).or_else(|| now.get(&key).cloned()) {
                        Some((_, bounds)) => bounds,
                        None => b.mesh_bounds(),
                    };
                    now.insert(key, (mesh, bounds));
                    (b.name.clone(), bounds)
                })
                .collect::<Vec<_>>(),
        );
        // Pick indexes are kept for the meshes this state is picked from (a placed part's
        // shared mesh).
        let live: std::collections::HashSet<usize> = now.keys().map(|(mesh, _)| *mesh).collect();
        app.viewport.pick_index.retain(|k| live.contains(&k));
        if let Ok(mut k) = app.viewport.mesh_bounds.lock() {
            *k = now;
        }
        *g = Some((st.clone(), b.clone()));
        return b;
    }
    std::sync::Arc::new(Vec::new())
}

type BodyBounds = (std::sync::Arc<solidcraft_engine::doc::ModelState>, std::sync::Arc<Vec<(String, solidcraft_engine::geom::Aabb3)>>);

/// The model's extent: bodies (from the cached boxes) and sketches.
pub fn model_bounds(app: &SolveApp) -> solidcraft_engine::geom::Aabb3 {
    let st = crate::open_meshing::shown_world(app);
    let bodies = body_bounds(app, &st).iter().fold(solidcraft_engine::geom::Aabb3::EMPTY, |a, (_, x)| a.union(x));
    let mut b = bodies.union(&sketch_bounds(app, &st));
    for point in crate::sketch_attachment_ui::guide_lines(app).into_iter().flatten() {
        b.add(point);
    }
    b
}

/// The box of every sketch of `st` in the world, kept for that state: a design with hundreds of
/// sketches spent most of each frame sampling their splines again for `projection`.
fn sketch_bounds(app: &SolveApp, st: &std::sync::Arc<solidcraft_engine::doc::ModelState>) -> solidcraft_engine::geom::Aabb3 {
    if let Ok(g) = app.viewport.sketch_bounds.lock()
        && let Some((k, b)) = g.as_ref()
        && std::sync::Arc::ptr_eq(k, st)
    {
        return *b;
    }
    let mut b = solidcraft_engine::geom::Aabb3::EMPTY;
    for ss in &st.sketches {
        if let Some((lo, hi)) = ss.sketch.bounds() {
            for p in [lo, hi] {
                b.add(ss.plane.to_world(p));
            }
        }
    }
    if let Ok(mut g) = app.viewport.sketch_bounds.lock() {
        *g = Some((std::sync::Arc::clone(st), b));
    }
    b
}

/// Where a ray enters a box (None: it misses).
pub(crate) fn ray_box(o: Vec3, d: Vec3, b: &solidcraft_engine::geom::Aabb3, pad: f64) -> Option<f64> {
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for (oo, dd, lo, hi) in [(o.x, d.x, b.min.x, b.max.x), (o.y, d.y, b.min.y, b.max.y), (o.z, d.z, b.min.z, b.max.z)] {
        let (lo, hi) = (lo - pad, hi + pad);
        if dd.abs() < 1e-15 {
            if oo < lo || oo > hi {
                return None;
            }
        } else {
            let (a, c) = ((lo - oo) / dd, (hi - oo) / dd);
            t0 = t0.max(a.min(c));
            t1 = t1.min(a.max(c));
        }
    }
    (t0 <= t1).then_some(t0)
}

/// The point of segment `a`–`b` nearest the ray from `o` along `d`.
pub(crate) fn ray_segment_closest(o: Vec3, d: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let u = b - a;
    let w = a - o;
    let (dd, du, uu) = (d.dot(d), d.dot(u), u.dot(u));
    let den = dd * uu - du * du;
    // Parallel or degenerate: every point is as near, so the start stands for the segment.
    if !(den.is_finite() && den > 1e-12 * dd * uu) {
        return a;
    }
    let s = (du * d.dot(w) - dd * u.dot(w)) / den;
    if !s.is_finite() {
        return a;
    }
    a + u * s.clamp(0.0, 1.0)
}

/// A box's outline on screen, grown by `px` (None: behind the eye).
pub(crate) fn screen_box(proj: &Proj, b: &solidcraft_engine::geom::Aabb3, px: f32) -> Option<Rect> {
    let mut r = Rect::NOTHING;
    for i in 0..8 {
        let p = Vec3::new(
            if i & 1 == 0 { b.min.x } else { b.max.x },
            if i & 2 == 0 { b.min.y } else { b.max.y },
            if i & 4 == 0 { b.min.z } else { b.max.z },
        );
        r.extend_with(proj.to_screen(p)?);
    }
    Some(r.expand(px))
}

/// Mesh outlines have no B-rep identity, but their points and lengths can be measured.
/// Keep them out of modeling operands and invalidate hover picks when Measure opens/closes.
fn measuring_mesh_edges(app: &SolveApp) -> bool {
    app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::Measure { .. }))
}

fn edge_pickable(app: &SolveApp, mesh: &solidcraft_engine::geom::Mesh, index: usize) -> bool {
    mesh.edge_pickable(index) || (measuring_mesh_edges(app) && mesh.display_only_edges && mesh.edge_visible(index))
}

/// What is under `pos`, reusing the last answer while the cursor, the camera and the model are
/// unchanged.
pub(crate) fn pick_cached(app: &mut SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    crate::render_workspace::active(app).hash(&mut h);
    pos.x.to_bits().hash(&mut h);
    pos.y.to_bits().hash(&mut h);
    for v in [app.cam.yaw, app.cam.pitch, app.cam.roll, app.cam.distance, app.cam.target.x, app.cam.target.y, app.cam.target.z] {
        v.to_bits().hash(&mut h);
    }
    app.session.revision.hash(&mut h);
    (std::sync::Arc::as_ptr(&crate::open_meshing::shown_world(app)) as usize).hash(&mut h);
    app.session.active_sketch.hash(&mut h);
    app.tool.as_ref().map(|tool| tool.cmd).hash(&mut h);
    app.dialog.is_none().hash(&mut h);
    measuring_mesh_edges(app).hash(&mut h);
    app.preferences.prefer_free_sketch_points.hash(&mut h);
    app.viewport.woken_centre.hash(&mut h);
    crate::sketch3d::drawing(app).hash(&mut h);
    app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept).hash(&mut h);
    crate::hole_place::snapping(app).hash(&mut h);
    crate::drag_snap::suppressed().hash(&mut h);
    app.session.visibility.hidden_bodies.hash(&mut h);
    app.session.visibility.hidden_occurrences.hash(&mut h);
    origin_point_shown(app).hash(&mut h);
    app.perspective_view().hash(&mut h);
    (proj.rect.width().to_bits(), proj.rect.height().to_bits()).hash(&mut h);
    crate::sketch_display::projection_key(proj, app.viewport.pixels_per_point()).hash(&mut h);
    let key = h.finish();
    if let Some((k, hits)) = &app.viewport.last_pick
        && *k == key
    {
        return hits.clone();
    }
    // Hover feedback mirrors what a 3D bridge click will snap to; clicks still go through `pick`.
    let hits = pick_with_sketch_points(app, proj, pos, crate::sketch3d::drawing(app));
    app.viewport.last_pick = Some((key, hits.clone()));
    hits
}

/// What is under `pos`. Sketch points come from the active sketch only (from every shown sketch
/// while a dialog input takes points, while idle, or while projecting), so sketch tools never get
/// another sketch's local point and id. The 3D bridge snap asks for cross-sketch points explicitly with [`pick_3d`].
pub fn pick(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    pick_with_sketch_points(app, proj, pos, false)
}

/// Operand picking excludes idle MID selection, including while a tool is temporarily taken
/// out of `app.tool` to handle its click.
pub(crate) fn pick_for_tool(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    pick_hits(app, proj, pos, false, false, false)
}

/// Like [`pick`], but also returns points of every visible sketch: only for the 3D drawing snap,
/// which converts them to world space through their own sketch's plane.
pub fn pick_3d(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    pick_with_sketch_points(app, proj, pos, true)
}

/// Everything under `pos` (Select Other): like [`pick`], but every sketch point, sketch curve,
/// vertex, edge, axis and profile near the cursor, and the nearest face of every body along the
/// ray, instead of the nearest of each kind. Each kind is nearest first.
pub fn pick_all(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Hit> {
    pick_hits(app, proj, pos, false, true, true)
}

fn pick_with_sketch_points(app: &SolveApp, proj: &Proj, pos: Pos2, other_sketch_points: bool) -> Vec<Hit> {
    let mut hits = pick_hits(app, proj, pos, other_sketch_points, false, true);
    // A Single hole's face place snaps to the face's centre and its round edges' centres.
    crate::hole_place::snap_hits(app, proj, pos, &mut hits);
    hits
}

/// The same thing picked twice (one row per target in [`pick_all`]).
pub fn same_target(a: &Hit, b: &Hit) -> bool {
    match (a, b) {
        (Hit::SketchMidpoint { sketch: s, id: i, .. }, Hit::SketchMidpoint { sketch: t, id: j, .. })
        | (Hit::SketchPoint { sketch: s, id: i, .. }, Hit::SketchPoint { sketch: t, id: j, .. })
        | (Hit::SketchCurve { sketch: s, id: i, .. }, Hit::SketchCurve { sketch: t, id: j, .. }) => s == t && i == j,
        (Hit::Vertex { body: a, point: p }, Hit::Vertex { body: b, point: q }) => a == b && p.dist(*q) < 1e-9,
        (Hit::Edge { body: a, index: i, .. }, Hit::Edge { body: b, index: j, .. })
        | (Hit::Face { body: a, index: i, .. }, Hit::Face { body: b, index: j, .. }) => a == b && i == j,
        (Hit::Profile { sketch: s, index: i }, Hit::Profile { sketch: t, index: j }) => s == t && i == j,
        (Hit::Plane { name: a, .. }, Hit::Plane { name: b, .. })
        | (Hit::Axis { name: a }, Hit::Axis { name: b })
        | (Hit::ConstructionPoint { name: a }, Hit::ConstructionPoint { name: b }) => a == b,
        _ => false,
    }
}

/// The candidates of one kind: the nearest only, or with `all` every target (nearest first).
struct Near<T> {
    all: bool,
    v: Vec<(T, Hit)>,
}

impl<T: PartialOrd + Copy> Near<T> {
    fn new(all: bool) -> Self {
        Near { all, v: Vec::new() }
    }
    /// Whether a candidate at `d` would be kept.
    fn wants(&self, d: T) -> bool {
        self.all || self.v.first().is_none_or(|(b, _)| d < *b)
    }
    fn offer(&mut self, d: T, h: Hit) {
        if !self.all {
            if self.wants(d) {
                self.v = vec![(d, h)];
            }
            return;
        }
        match self.v.iter_mut().find(|(_, x)| same_target(x, &h)) {
            Some(e) if d < e.0 => *e = (d, h),
            Some(_) => {}
            None => self.v.push((d, h)),
        }
    }
    fn sorted(mut self) -> Vec<(T, Hit)> {
        self.v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        self.v
    }
    fn hits(self) -> impl Iterator<Item = Hit> {
        self.sorted().into_iter().map(|x| x.1)
    }
}

fn pick_hits(app: &SolveApp, proj: &Proj, pos: Pos2, other_sketch_points: bool, all: bool, include_midpoints: bool) -> Vec<Hit> {
    let s = &app.session;
    let st = crate::open_meshing::shown_world(app);
    let bounds = body_bounds(app, &st);
    let mut hits = Vec::new();
    let (o, d) = proj.ray(pos);
    // MID is an idle sketch selection target. Tools and dialogs keep their existing picks.
    // Put it before a materialized helper point so later constraints still refer to the edge.
    if include_midpoints
        && app.tool.is_none()
        && app.dialog.is_none()
        && let Some(ss) = s.active_sketch.and_then(|sid| st.sketch(sid))
    {
        let mut mids = Near::new(all);
        for (i, curve) in ss.sketch.curves.iter().enumerate() {
            if !solidcraft_engine::view::sketch_curve_visible(&ss.sketch, i) {
                continue;
            }
            if let Some(at) = crate::selection::line_midpoint(&ss.sketch, i)
                && let Some(screen) = proj.to_screen(ss.plane.to_world(at))
            {
                let distance = screen.distance(pos);
                if distance < 8.0 {
                    mids.offer(distance, Hit::SketchMidpoint { sketch: ss.feature, id: curve.id.clone(), at });
                }
            }
        }
        hits.extend(mids.hits());
    }
    // Sketch points of the active sketch (of every shown sketch while a command takes points,
    // while idle or projecting, and while 3D drawing snaps to points on other visible sketches).
    let wants_points = app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::POINTS != 0);
    let external_points =
        wants_points || other_sketch_points || (app.dialog.is_none() && app.tool.as_ref().is_none_or(|tool| tool.cmd == "sketch.project"));
    let point_sketches: Vec<u64> = if crate::render_workspace::active(app) {
        Vec::new()
    } else if external_points {
        visible_sketches(app)
    } else {
        s.active_sketch.into_iter().collect()
    };
    // A face place (the Single hole's) snaps only to points their sketches show, so a nearer
    // point that isn't shown (a letter's outline) neither takes the pick nor hides a shown one.
    let snapping = app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| crate::dialogs::snaps_to_points(i.accept));
    let prefer_free = app.preferences.prefer_free_sketch_points && include_midpoints && app.tool.is_none() && app.dialog.is_none();
    let mut best = Near::new(all);
    // A woken centre's extra reach (past the usual 8) yields to a curve under the pointer, so a
    // click on a small circle's edge still takes the circle.
    let mut woken_far: Option<(f32, Hit)> = None;
    for ss in point_sketches.iter().filter_map(|sid| st.sketch(*sid)) {
        let sid = ss.feature;
        let shown = snapping.then(|| crate::dialogs::SnapPoints::of(&ss.sketch));
        let visible = solidcraft_engine::view::SketchPointVisibility::new(&ss.sketch);
        for (p, locked) in ss.sketch.points.iter().zip(ss.sketch.locked_points()) {
            if !visible.takes(p) || shown.as_ref().is_some_and(|f| !f.takes(p)) {
                continue;
            }
            if let Some(sp) = proj.to_screen(ss.plane.to_world(p.pos)) {
                let dd = sp.distance(pos) - if s.active_sketch == Some(sid) { 2.0 } else { 0.0 };
                // On a distance tie, grab a movable point before the origin or a linked point.
                let rank = (dd, prefer_free && locked);
                let woken = app.viewport.woken_centre.as_ref().is_some_and(|(k, id)| *k == sid && *id == p.id);
                if dd < 8.0 && best.wants(rank) {
                    best.offer(rank, Hit::SketchPoint { sketch: sid, id: p.id.clone(), at: p.pos });
                } else if woken && dd < WOKEN_CENTRE_PX && woken_far.as_ref().is_none_or(|(d0, _)| dd < *d0) {
                    woken_far = Some((dd, Hit::SketchPoint { sketch: sid, id: p.id.clone(), at: p.pos }));
                }
            }
        }
    }
    for (name, point) in construction_points(app) {
        if let Some(sp) = proj.to_screen(point) {
            let dd = sp.distance(pos);
            let rank = (dd, prefer_free);
            if dd < 8.0 && best.wants(rank) {
                best.offer(rank, Hit::ConstructionPoint { name });
            }
        }
    }
    hits.extend(best.hits());
    // Sketch curves: the active sketch's own before projected and other sketches' ones, then
    // nearest (the active sketch wins ties).
    let mut bestc = Near::new(all);
    for sid in visible_sketches(app).into_iter().filter(|_| !crate::render_workspace::active(app)) {
        let Some(ss) = st.sketch(sid) else { continue };
        let active = s.active_sketch == Some(sid);
        let bonus = if active { 2.0 } else { 0.0 };
        let tier = |own: bool| u8::from(!(active && own));
        for (i, c) in ss.sketch.curves.iter().enumerate() {
            if !solidcraft_engine::view::sketch_curve_visible(&ss.sketch, i) {
                continue;
            }
            let straight = matches!(c.kind, CurveKind::Line { .. });
            let tier = tier(c.link.is_none() || ss.sketch.is_text_curve(i));
            // Construction geometry lying on a regular curve yields to it (#6396); Select Other
            // still offers both.
            let helper = if c.construction || c.centerline { 0.5 } else { 0.0 };
            for part in &crate::sketch_display::curve_sample(app, sid, i, &ss.plane, proj).parts {
                let pts: Vec<Pos2> = part.iter().filter_map(|q| proj.to_screen(*q)).collect();
                for w in pts.windows(2) {
                    let dd = seg_dist(pos, w[0], w[1]) - bonus + helper;
                    if dd < 6.0 && bestc.wants((tier, dd)) {
                        bestc.offer((tier, dd), Hit::SketchCurve { sketch: sid, id: c.id.clone(), straight });
                    }
                }
            }
        }
        // 3D sketch curves (paths for sweeps and pipes).
        for w in &ss.sketch.wires {
            let straight = w.fit.len() == 2;
            let tier = tier(w.link.is_none());
            let pts: Vec<Pos2> = w.pts.iter().filter_map(|q| proj.to_screen(*q)).collect();
            for seg in pts.windows(2) {
                let dd = seg_dist(pos, seg[0], seg[1]) - bonus;
                if dd < 6.0 && bestc.wants((tier, dd)) {
                    bestc.offer((tier, dd), Hit::SketchCurve { sketch: sid, id: w.id.clone(), straight });
                }
            }
        }
    }
    let before_curves = hits.len();
    hits.extend(bestc.hits());
    if let Some((_, far)) = woken_far
        && hits.len() == before_curves
        && !hits.iter().any(|h| matches!(h, Hit::SketchPoint { .. }))
    {
        hits.push(far);
    }
    // A section hides the material its normal points at: nothing there is picked, and where the
    // plane crosses a body its cap hides what lies behind it (#6328), as `view.pick` answers.
    let section = section_plane(app);
    let kept = |p: Vec3| section.is_none_or(|(so, sn)| sn.dot(p) <= sn.dot(so) + crate::view_pick::SECTION_EPS);
    let walk_eps = 1e-6 * (1.0 + scene_radius(app));
    // Nearest face along the ray (hides what is behind it); with `all`, every body's nearest.
    let mut bestf: Option<(f64, Hit)> = None;
    let mut faces = Near::new(all);
    // Bounds retain the state's body order. Looking each one up by name made both passes
    // quadratic for assemblies with many small parts.
    for (i, b) in st.bodies.iter().enumerate() {
        if !app.session.body_visible(&b.name) {
            continue;
        }
        // Bodies whose box the ray misses (or enters behind the best hit so far) are skipped.
        if let Some((_, bb)) = bounds.get(i) {
            let pad = bb.diagonal() * 1e-6 + 1e-9;
            match ray_box(o, d, bb, pad) {
                None => continue,
                Some(t0) if !all && bestf.as_ref().is_some_and(|(bt, _)| t0 > *bt) => continue,
                _ => {}
            }
        }
        // A placed part is picked in its own frame, from its shared mesh and index: the ray is
        // moved there (a rigid move keeps distances along it), so no moved copy is made.
        let (m, place) = b.mesh_in_place();
        let index = app.viewport.pick_index.of(&m);
        // The nearest kept face: past any the section removes (a bounded walk along the ray).
        let mut nearest = None;
        let mut from = 0.0f64;
        for _ in 0..64 {
            let Some((lo, ld)) = local_ray(&place, o + d * from, d) else { break };
            let Some((dt, ti)) = crate::pick_index::raycast(index.as_deref(), &m, lo, ld) else { break };
            let t = from + dt;
            if kept(o + d * t) {
                nearest = Some((t, ti));
                break;
            }
            from = t + walk_eps;
        }
        if let Some((t, ti)) = nearest {
            let fi = m.tri_face.get(ti).copied().unwrap_or(0) as usize;
            let hit = Hit::Face { body: b.name.clone(), index: fi, point: o + d * t };
            if bestf.as_ref().is_none_or(|(bt, _)| t < *bt) {
                bestf = Some((t, hit.clone()));
            }
            if all {
                faces.offer(t, hit);
            }
        }
    }
    if crate::render_workspace::active(app) {
        return if all { faces.sorted().into_iter().map(|(_, hit)| hit).collect() } else { bestf.into_iter().map(|(_, hit)| hit).collect() };
    }
    // The cut cap, where the plane crosses a body nearer than any kept face: it hides that face
    // and everything behind it, and is no face itself.
    let cap = section.and_then(|(so, sn)| {
        let den = sn.dot(d);
        let t_cap = (sn.dot(so) - sn.dot(o)) / den;
        let nearer = den.abs() > 1e-12 && t_cap > 0.0 && bestf.as_ref().is_none_or(|(t, _)| t_cap < *t);
        let replaced = app.preview.effective_replaced();
        let shown_bodies = &None;
        (nearer && crate::view_pick::body_cut_at(app, &st, &bounds, o, d, t_cap, true, replaced, shown_bodies).is_some()).then_some(t_cap)
    });
    if let Some(t_cap) = cap {
        bestf = None;
        faces.v.retain(|(t, _)| *t < t_cap);
    }
    let hidden_from = cap.or(bestf.as_ref().map(|(t, _)| *t));
    let slack = scene_radius(app) * 0.02;
    // Wireframe draws no faces, so nothing hides an edge or a vertex.
    let wireframe = app.ui.visual_style == 2;
    let visible = |p: Vec3| kept(p) && (wireframe || hidden_from.is_none_or(|t| (p - o).dot(d) <= t + slack));
    // Vertices (edge end points) and edges, within 7 and 6 points of the cursor.
    let reach = crate::pick_index::reach(proj, pos, 8.0);
    let mut bestv = Near::new(all);
    let mut beste = Near::new(all);
    for (i, b) in st.bodies.iter().enumerate() {
        if !app.session.body_visible(&b.name) {
            continue;
        }
        // Only bodies whose outline on screen is near the cursor can have an edge under it.
        if let Some((_, bb)) = bounds.get(i)
            && screen_box(proj, bb, 8.0).is_some_and(|r| !r.contains(pos))
        {
            continue;
        }
        // In the part's own frame, as for faces; its points are moved back to where it stands.
        let (m, place) = b.mesh_in_place();
        let Some((lo, ld)) = local_ray(&place, o, d) else { continue };
        let moved = !solidcraft_engine::doc::is_identity(&place);
        let at = |p: Vec3| if moved { solidcraft_engine::doc::apply_point(&place, p) } else { p };
        // Only the segments that can come near the cursor, from the index, in the order a scan
        // of every edge would meet them, so ties go the same way.
        let near = crate::pick_index::segments_near(app.viewport.pick_index.of(&m).as_deref(), &m, lo, ld, reach);
        let mut last_edge = None;
        for (ei, si) in near {
            let Some(e) = m.edges.get(ei) else { continue };
            if !edge_pickable(app, &m, ei) {
                continue;
            }
            if last_edge != Some(ei) {
                last_edge = Some(ei);
                for v in [e.first(), e.last()].into_iter().flatten().map(|v| at(*v)) {
                    if let Some(sp) = proj.to_screen(v) {
                        let dd = sp.distance(pos);
                        if dd < 7.0 && visible(v) && bestv.wants(dd) {
                            bestv.offer(dd, Hit::Vertex { body: b.name.clone(), point: v });
                        }
                    }
                }
            }
            let (Some(p0), Some(p1)) = (e.get(si).map(|p| at(*p)), e.get(si + 1).map(|p| at(*p))) else { continue };
            let (Some(a), Some(c)) = (proj.to_screen(p0), proj.to_screen(p1)) else { continue };
            let dd = seg_dist(pos, a, c);
            // Visible where the cursor is: the edge's point under the cursor, not an end or
            // the middle, which can lie deeper than the face the cursor is on.
            if dd < 6.0 && beste.wants(dd) && visible(ray_segment_closest(o, d, p0, p1)) {
                beste.offer(dd, Hit::Edge { body: b.name.clone(), index: ei, mid: at(polyline_mid(e)) });
            }
        }
    }
    // The origin point, for an input that takes it: a vertex of no body, drawn over everything.
    if app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::ORIGIN != 0)
        && origin_point_shown(app)
        && let Some(sp) = proj.to_screen(Vec3::ZERO)
    {
        let dd = sp.distance(pos);
        if dd < 7.0 && bestv.wants(dd) {
            bestv.offer(dd, Hit::Vertex { body: String::new(), point: Vec3::ZERO });
        }
    }
    hits.extend(bestv.hits());
    hits.extend(beste.hits());
    // Origin axes.
    let size = origin_size(app.cam.half_height());
    let mut besta = Near::new(all);
    for (name, dir) in origin_axes(app) {
        if let (Some(a), Some(b)) = (proj.to_screen(Vec3::ZERO), proj.to_screen(dir * (size * AXIS_LEN))) {
            let dd = seg_dist(pos, a, b);
            if dd < 5.0 && besta.wants(dd) {
                besta.offer(dd, Hit::Axis { name: name.into() });
            }
        }
    }
    // Construction axes, picked like the origin axes.
    for (name, a, b) in construction_axis_segments(app) {
        if let (Some(a), Some(b)) = (proj.to_screen(a), proj.to_screen(b)) {
            let dd = seg_dist(pos, a, b);
            if dd < 5.0 && besta.wants(dd) {
                besta.offer(dd, Hit::Axis { name });
            }
        }
    }
    hits.extend(besta.hits());
    // Surfaces by depth: profiles (slightly preferred over a face they lie on), faces, planes.
    let mut surf: Vec<(f64, Hit)> = Vec::new();
    let mut bestp: Option<(f64, f64, Hit)> = None;
    let mut profiles_all: Vec<(f64, f64, Hit)> = Vec::new();
    for sid in visible_sketches(app) {
        let Some(ss) = st.sketch(sid) else { continue };
        let n = ss.plane.normal();
        let den = n.dot(d);
        if den.abs() < 1e-12 {
            continue;
        }
        let t = n.dot(ss.plane.origin - o) / den;
        if t < 0.0 {
            continue;
        }
        let lp = ss.plane.to_local(o + d * t);
        // Show Profile off: the sketch being edited offers no profiles.
        let profiles = if ss.sketch.view.hide_profile && s.active_sketch == Some(sid) { &[][..] } else { &ss.profiles[..] };
        for (pi, p) in profiles.iter().enumerate() {
            if !p.region.contains(lp) {
                continue;
            }
            if all {
                profiles_all.push((t, p.area, Hit::Profile { sketch: sid, index: pi }));
            } else if bestp.as_ref().is_none_or(|(_, a, _)| p.area < *a) {
                bestp = Some((t, p.area, Hit::Profile { sketch: sid, index: pi }));
            }
        }
    }
    // Smallest profile first.
    profiles_all.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (t, _, h) in bestp.into_iter().chain(profiles_all) {
        surf.push((t - slack, h));
    }
    if all {
        surf.extend(faces.sorted());
    } else if let Some(f) = bestf {
        surf.push(f);
    }
    // Planes give way to a face or profile at the same depth (a floor on the XZ plane).
    for (name, _, q) in origin_planes(app) {
        if let Some(t) = ray_quad(o, d, &q) {
            surf.push((t + slack, Hit::Plane { name: name.into(), point: o + d * t }));
        }
    }
    for (name, _, q) in construction_quads(app) {
        if let Some(t) = ray_quad(o, d, &q) {
            surf.push((t + slack, Hit::Plane { name, point: o + d * t }));
        }
    }
    surf.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.extend(surf.into_iter().map(|x| x.1));
    hits
}

/// The section cut shown now: the open Section Analysis dialog's plane, else the session's.
/// The active sketch's display options (defaults when not sketching).
pub fn active_view(app: &SolveApp) -> solidcraft_engine::sketch::SketchView {
    let st = crate::open_meshing::shown_world(app);
    app.session.active_sketch.and_then(|s| st.sketch(s)).map(|ss| ss.sketch.view.clone()).unwrap_or_default()
}

pub fn section_plane(app: &SolveApp) -> Option<(Vec3, Vec3)> {
    if let Some(d) = app.dialog.as_ref().filter(|d| matches!(d.kind, crate::dialogs::Kind::Section { .. })) {
        let cmds = crate::dialogs::apply_commands(app, d).ok()?;
        let (_, p) = cmds.into_iter().next()?;
        let mut scratch = app.session.scratch();
        scratch.execute("inspect.section", &p).ok()?;
        return scratch.section;
    }
    // Slice (Sketch Palette): cut the model at the plane of the sketch being edited.
    if app.session.section.is_none() {
        let st = crate::open_meshing::shown_world(app);
        if let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)).filter(|ss| ss.sketch.view.slice) {
            return Some((ss.plane.origin, ss.plane.normal()));
        }
    }
    app.session.section
}

/// Whether the section [`section_plane`] shows has hatched cut faces: an open Section Analysis
/// dialog's Hatch option, else the section's own.
pub fn section_hatch(app: &SolveApp) -> bool {
    match app.dialog.as_ref().map(|d| &d.kind) {
        Some(crate::dialogs::Kind::Section { hatch, .. }) => *hatch,
        _ => app.session.section_hatch,
    }
}

/// Visible geometry belonging to a timeline feature. Body creators retain their whole-body
/// feedback; modifiers use persistent face names, which survive later edits and placement.
/// Do not expand these faces into surface patches: neighbouring faces can belong to another
/// feature even when they share a surface.
fn feature_geometry(app: &SolveApp, id: u64) -> Vec<Sel> {
    feature_parts(app, id, true)
}

/// A feature's geometry as [`feature_geometry`] finds it; `shown_only` leaves out hidden bodies
/// and bodies a dialog's preview stands in for.
fn feature_parts(app: &SolveApp, id: u64, shown_only: bool) -> Vec<Sel> {
    if !app.session.model.result(id).is_some_and(|r| !r.skipped && r.error.is_none()) {
        return Vec::new();
    }
    let st = crate::open_meshing::shown_world(app);
    let mut out = Vec::new();
    for b in &st.bodies {
        if shown_only && (!app.session.body_visible(&b.name) || app.preview.effective_replaced().contains(&b.name)) {
            continue;
        }
        if b.feature == id {
            out.push(Sel::Body { name: b.name.clone() });
            continue;
        }
        let names = solidcraft_engine::doc::naming::face_names(b);
        let m = b.mesh();
        for (index, _) in names.iter().enumerate().filter(|(_, n)| solidcraft_engine::doc::persistent_name::made_by(n) == Some(id)) {
            if let Some(point) = m.triangles.iter().zip(&m.tri_face).find_map(|(t, f)| {
                if *f as usize != index {
                    return None;
                }
                m.tri(t).map(|[a, b, c]| (a + b + c) / 3.0)
            }) {
                out.push(Sel::Face { body: b.name.clone(), index, point });
            }
        }
    }
    out
}

/// The sketches picked among `items` (a sketch row in the Browser or the timeline) that the
/// highlight draws, shown or hidden (#7156). The sketch being edited shows its own selection.
fn selected_sketches(app: &SolveApp, items: &[Sel]) -> Vec<u64> {
    let st = crate::open_meshing::shown_world(app);
    let mut out = Vec::new();
    for item in items {
        if let Sel::Feature { id } = item
            && app.session.active_sketch != Some(*id)
            && st.sketch(*id).is_some()
            && !out.contains(id)
        {
            out.push(*id);
        }
    }
    out
}

fn displayed_selection(app: &SolveApp, item: &Sel) -> Vec<Sel> {
    match item {
        Sel::Feature { id } => feature_geometry(app, *id),
        Sel::Occurrence { path } => app
            .session
            .occurrence_bodies(path, true)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| app.session.body_visible(name))
            .map(|name| Sel::Body { name })
            .collect(),
        _ => grouped_selection(&app.session, item),
    }
}

/// The box around what is selected (bodies, faces, edges, vertices, sketch curves and the
/// bodies of selected features), where it is shown.
pub fn selection_bounds(app: &SolveApp) -> Option<solidcraft_engine::geom::Aabb3> {
    bounds_of(app, app.session.selection.iter().flat_map(|item| displayed_selection(app, item)))
}

/// The box around the geometry a feature made (its bodies, or its faces on bodies later features
/// changed), also where a dialog's preview stands in for it: a pattern of the feature previews
/// the body the feature is on. None when the feature made nothing.
pub(crate) fn feature_bounds(app: &SolveApp, id: u64) -> Option<solidcraft_engine::geom::Aabb3> {
    bounds_of(app, feature_parts(app, id, false).into_iter())
}

fn bounds_of(app: &SolveApp, items: impl Iterator<Item = Sel>) -> Option<solidcraft_engine::geom::Aabb3> {
    let st = crate::open_meshing::shown_world(app);
    let mut b = solidcraft_engine::geom::Aabb3::EMPTY;
    for s in items {
        match &s {
            Sel::Body { name } => {
                if let Some(x) = st.body(name) {
                    b = b.union(&x.mesh().bounds());
                }
            }
            Sel::Face { body, index, .. } => {
                if let Some(x) = st.body(body) {
                    let m = x.mesh();
                    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
                        if *f as usize == *index {
                            for p in m.tri(t).into_iter().flatten() {
                                b.add(p);
                            }
                        }
                    }
                }
            }
            Sel::Edge { body, index, .. } => {
                if let Some(e) = st.body(body).and_then(|x| x.mesh().edges.get(*index).cloned()) {
                    for p in e {
                        b.add(p);
                    }
                }
            }
            Sel::Vertex { point, .. } => b.add(*point),
            Sel::SketchMidpoint { id } => {
                if let Some(ss) = app.session.active_sketch.and_then(|sid| st.sketch(sid))
                    && let Some(line) = ss.sketch.curve_index(id)
                    && let Some(at) = crate::selection::line_midpoint(&ss.sketch, line)
                {
                    b.add(ss.plane.to_world(at));
                }
            }
            Sel::ExternalSketchPoint { sketch, id } => {
                if let Some(ss) = st.sketch(*sketch)
                    && let Some(p) = ss.sketch.resolve_point(id).and_then(|i| ss.sketch.point(i))
                {
                    b.add(ss.plane.to_world(p));
                }
            }
            Sel::ExternalSketchCurve { sketch, id } => {
                if let Some(ss) = st.sketch(*sketch)
                    && let Some(i) = ss.sketch.curve_index(id)
                {
                    for p in ss.sketch.polyline(i) {
                        b.add(ss.plane.to_world(p));
                    }
                }
            }
            Sel::SketchCurve { id, sketch } => {
                for ss in st.sketches.iter().filter(|ss| sketch.is_none_or(|k| ss.feature == k)) {
                    if let Some(i) = ss.sketch.curve_index(id) {
                        for q in ss.sketch.polyline(i) {
                            b.add(ss.plane.to_world(q));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (!b.is_empty()).then_some(b)
}

/// The section as the shader's clip plane (normal, d), zero when there is none.
/// The surface analysis for the shader (see `gpu::Frame::analysis`).
fn analysis_uniform(app: &SolveApp) -> [f32; 8] {
    solidcraft_gpu::view::analysis_uniform(app.session.analysis.map(|a| a.mode()), crate::access::map(app).as_ref())
}

/// While Draft, Accessibility or Curvature Map analysis is on, what each of its colours means and
/// the values it stands for, at the bottom left of the viewport (#973). The colours alone don't
/// say, and with colour vision loss they can be hard to tell apart.
fn analysis_legend(app: &SolveApp, painter: &egui::Painter, rect: Rect) {
    use solidcraft_engine::SurfaceAnalysis as A;
    let t = Tokens::get();
    let tr = crate::i18n::tr_label;
    let col = |c: [f32; 3]| {
        let u = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(u(c[0]), u(c[1]), u(c[2]))
    };
    let dir = |v: Vec3| format!("({:.2}, {:.2}, {:.2})", v.x, v.y, v.z);
    let a = &t.analysis;
    // (title, the direction it measures along, rows of colour, text, hatched).
    let (title, along, rows): (&str, Option<String>, Vec<(Color32, String, bool)>) = match app.session.analysis {
        Some(A::Draft { pull, angle }) => (
            tr("Draft Analysis"),
            Some(format!("{} {}", tr("Pull direction"), dir(pull))),
            vec![
                (col(a.draft[0]), format!("≥ {angle:.1}°"), false),
                (col(a.draft[1]), format!("−{angle:.1}° … {angle:.1}°"), false),
                (col(a.draft[2]), format!("≤ −{angle:.1}°"), a.hatch),
            ],
        ),
        Some(A::Access { dir: d }) => (
            tr("Accessibility Analysis"),
            Some(format!("{} {}", tr("Tool direction"), dir(d))),
            vec![(col(a.access[0]), tr("Reachable").to_string(), false), (col(a.access[1]), tr("Blocked").to_string(), a.hatch)],
        ),
        // The shader's scale reaches its middle at twice the analysis radius.
        Some(A::Curvature { radius }) => (
            tr("Curvature Map Analysis"),
            None,
            vec![
                (col(a.curvature[0]), tr("ui.flat").to_string(), false),
                (col(a.curvature[1]), format!("{} {}", tr("Radius"), crate::prefs::show_mm(app, 2.0 * radius, 1)), false),
                (col(a.curvature[2]), format!("{} ≤ {}", tr("Radius"), crate::prefs::show_mm(app, radius, 1)), false),
            ],
        ),
        _ => return,
    };
    let (pad, row) = (8.0, 18.0);
    let h = pad * 2.0 + 18.0 + along.as_ref().map_or(0.0, |_| row) + row * rows.len() as f32;
    let r = Rect::from_min_size(pos2(rect.left() + 12.0, rect.bottom() - 12.0 - h), vec2(230.0, h));
    painter.rect(r, t.radius_md, t.overlay, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    crate::scenario::publish_target("analysis:legend", r);
    let mut y = r.top() + pad;
    painter.text(pos2(r.left() + pad, y), Align2::LEFT_TOP, title, FontId::proportional(t.font_button), t.text);
    y += 18.0;
    if let Some(along) = along {
        painter.text(pos2(r.left() + pad, y), Align2::LEFT_TOP, along, FontId::proportional(t.font_caption), t.text_dim);
        y += row;
    }
    for (c, text, hatched) in rows {
        let sw = Rect::from_min_size(pos2(r.left() + pad, y + 2.0), vec2(12.0, 12.0));
        painter.rect_filled(sw, t.radius_sm / 2, c);
        if hatched {
            // The stripes the viewport adds to these faces (colour vision modes).
            for k in [3.0, 7.0, 11.0] {
                painter.line_segment(
                    [pos2(sw.left() + k - 3.0, sw.bottom()), pos2(sw.left() + k + 1.0, sw.top())],
                    Stroke::new(1.2, Color32::from_black_alpha(110)),
                );
            }
        }
        painter.text(pos2(sw.right() + 8.0, y), Align2::LEFT_TOP, text, FontId::proportional(t.font_body), t.text);
        y += row;
    }
}

fn section_clip(app: &SolveApp) -> [f32; 4] {
    match section_plane(app) {
        Some((o, n)) => [n.x as f32, n.y as f32, n.z as f32, n.dot(o) as f32],
        None => [0.0; 4],
    }
}

/// Space kept free at the top right of the viewport for the axis view (docked dialogs start below).
pub(crate) fn axis_view_size(style: crate::prefs::NavigatorStyle) -> f32 {
    match style {
        crate::prefs::NavigatorStyle::Axes => 116.0,
        crate::prefs::NavigatorStyle::Cube => navigation_cube::SIZE,
    }
}
// Keep command panels anchored independently of the compact cube's display size.
pub const AXIS_VIEW_CLEARANCE: f32 = 168.0;

/// Axis length as a multiple of the origin plane size (solid part 1, dashed beyond).
const AXIS_LEN: f64 = 1.8;

/// The hit as a plain selection (no command filter).
pub fn hit_sel(h: &Hit) -> Option<Sel> {
    Some(match h {
        Hit::SketchMidpoint { id, .. } => Sel::SketchMidpoint { id: id.clone() },
        Hit::SketchPoint { id, .. } => Sel::SketchPoint { id: id.clone() },
        Hit::SketchCurve { id, .. } => Sel::SketchCurve { id: id.clone(), sketch: None },
        Hit::Vertex { body, point } => Sel::Vertex { body: body.clone(), point: *point },
        Hit::Edge { body, index, mid } => Sel::Edge { body: body.clone(), index: *index, point: *mid },
        Hit::Profile { sketch, index } => Sel::Profile { sketch: *sketch, index: *index },
        Hit::Face { body, index, point } => Sel::Face { body: body.clone(), index: *index, point: *point },
        Hit::Plane { name, .. } => Sel::Plane { name: name.clone() },
        Hit::Axis { name } => Sel::Axis { name: name.clone() },
        Hit::ConstructionPoint { name } => Sel::ConstructionPoint { name: name.clone() },
    })
}

/// What lights up under the pointer while a tool runs: the nearest hit its click takes.
fn tool_hover(app: &SolveApp, hits: &[Hit]) -> Option<Hit> {
    let cmd = app.tool.as_ref()?.cmd;
    hits.iter().find(|h| crate::sketch_tools::takes(cmd, h)).cloned()
}

/// A sketch annotation under either a left-click or a context-menu click. Dimension text
/// wins over geometry; dimension lines yield to points and curves, and glyphs to midpoints.
pub(crate) fn sketch_constraint_at(app: &SolveApp, pos: Pos2, hit: Option<&Hit>) -> Option<Sel> {
    app.session.active_sketch?;
    let off = |kind: &str| app.ui.pick_off.iter().any(|x| x == kind);
    let on_geometry = matches!(hit, Some(Hit::SketchPoint { .. } | Hit::SketchMidpoint { .. } | Hit::SketchCurve { .. }));
    let dimension =
        crate::dim_view::hit_text(pos).or_else(|| if on_geometry { None } else { crate::dim_view::hit(pos) }).filter(|_| !off("dimensions"));
    let glyph = crate::sketch_tools::glyph_at(pos).filter(|_| !off("constraints") && !matches!(hit, Some(Hit::SketchMidpoint { .. })));
    dimension.or(glyph).map(|id| Sel::SketchConstraint { id })
}

/// What a click would pick: the first hit the open dialog's active input accepts, or with no
/// dialog the first hit (sketch entities only belong to the active sketch).
pub fn candidate(app: &SolveApp, hits: &[Hit]) -> Option<(Hit, Sel)> {
    if let Some(d) = app.dialog.as_ref().filter(|d| d.wants_picks()) {
        let snap = d.face_snap(&app.session, hits);
        return hits.iter().find_map(|h| d.candidate_on(&app.session, h, snap.as_ref()).map(|s| (h.clone(), s)));
    }
    hits.iter().filter(|h| pick_allowed(app, h)).find_map(|h| hit_candidate(app, h))
}

/// Every hit a click could pick, in order, each with its selection (Select Other).
pub fn candidates(app: &SolveApp, hits: &[Hit]) -> Vec<(Hit, Sel)> {
    if let Some(d) = app.dialog.as_ref().filter(|d| d.wants_picks()) {
        // The face under the cursor is looked up once for every sketch point a face place may
        // snap to, before Select Other caps its rows.
        let snap = d.face_snap(&app.session, hits);
        return hits.iter().filter_map(|h| d.candidate_on(&app.session, h, snap.as_ref()).map(|s| (h.clone(), s))).collect();
    }
    hits.iter().filter(|h| pick_allowed(app, h)).filter_map(|h| hit_candidate(app, h)).collect()
}

/// What the selection filter lets a click pick with no dialog.
fn pick_allowed(app: &SolveApp, h: &Hit) -> bool {
    let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
    match h {
        Hit::SketchCurve { .. } => !off("sketch") && !off("sketch_curves"),
        Hit::SketchPoint { .. } | Hit::SketchMidpoint { .. } => !off("sketch") && !off("sketch_points"),
        Hit::Profile { .. } => !off("sketch"),
        Hit::Vertex { .. } => !off("vertices") && !app.ui.pick_bodies && !app.ui.pick_components,
        Hit::Edge { .. } => !off("edges") && !app.ui.pick_bodies && !app.ui.pick_components,
        Hit::Face { .. } => !off("faces") || app.ui.pick_bodies || app.ui.pick_components,
        Hit::Plane { .. } | Hit::Axis { .. } | Hit::ConstructionPoint { .. } => true,
    }
}

/// The selection a click on `h` makes with no dialog (sketch entities only belong to the active sketch).
fn hit_candidate(app: &SolveApp, h: &Hit) -> Option<(Hit, Sel)> {
    match h {
        Hit::SketchCurve { sketch, id, .. } if app.session.active_sketch != Some(*sketch) => {
            Some((h.clone(), Sel::ExternalSketchCurve { sketch: *sketch, id: id.clone() }))
        }
        Hit::SketchPoint { sketch, id, .. } if app.session.active_sketch != Some(*sketch) => {
            Some((h.clone(), Sel::ExternalSketchPoint { sketch: *sketch, id: id.clone() }))
        }
        Hit::Face { body, .. } if app.ui.pick_components => Some((h.clone(), component_selection(app, body))),
        Hit::Face { body, .. } if app.ui.pick_bodies => Some((h.clone(), Sel::Body { name: body.clone() })),
        _ => hit_sel(h).map(|s| (h.clone(), s)),
    }
}

/// Use the Browser's exact occurrence identity, including nested/repeated placements.
/// Root bodies have no occurrence row, so retain their body identity.
fn component_selection(app: &SolveApp, body: &str) -> Sel {
    let doc = app.session.placed_document();
    let state = app.session.world_state();
    if let Some(b) = state.body(body) {
        let component = doc.model_body_component(b);
        let model = app.session.model.state();
        if let Some((_, path, _)) = doc
            .placements()
            .into_iter()
            .filter(|(c, _, _)| *c == component)
            .find(|(_, path, _)| doc.world_body_name(&model, b.definition_name(), component, path) == body)
            .filter(|(_, path, _)| !path.is_empty())
        {
            return Sel::Occurrence { path };
        }
    }
    Sel::Body { name: body.to_string() }
}

/// A pick for the open dialog's active input (a click, or a Select Other row).
pub(crate) fn dialog_pick(app: &mut SolveApp, hit: &Hit, sel: Sel) {
    // While OK computes, the dialog's inputs hold.
    if app.applying.is_some() {
        return;
    }
    // Press Pull's first pick of a curved face or an edge reopens it as an offset or a fillet.
    if let Some(d) = &app.dialog
        && d.press_pull
        && d.inputs.first().is_some_and(|i| i.items.is_empty())
        && match &sel {
            Sel::Edge { .. } => true,
            Sel::Face { body, index, .. } => crate::dialogs::planar_face(&app.session, body, *index).is_none(),
            _ => false,
        }
    {
        crate::dialogs::cancel(app);
        app.session.selection = vec![sel];
        app.start("solid.press_pull");
        return;
    }
    let Some(mut d) = app.dialog.take() else { return };
    // With holes moving by clicking (Preferences), a Single hole picked again on its face moves there.
    if app.preferences.hole_moves && crate::hole_place::pick_moves(&mut d, &app.session, &sel) {
        d.focus = true;
    } else {
        d.pick(&app.session, sel);
    }
    if let crate::dialogs::Kind::PathPlane { sketch, .. } = &mut d.kind {
        *sketch = match hit {
            Hit::SketchCurve { sketch, .. } => Some(*sketch),
            _ => None,
        };
        d.extra.remove("path");
    }
    // Split Body's sketch line keeps the sketch it was picked in (curve ids repeat across sketches).
    if matches!(d.kind, crate::dialogs::Kind::Split) {
        if let Hit::SketchCurve { sketch, .. } = hit {
            d.extra.insert("tool_sketch".into(), serde_json::json!(sketch));
        } else {
            d.extra.remove("tool_sketch");
        }
    }
    app.dialog = Some(d);
}

/// The edge a selection refers to now: by index if it still passes through the stored point,
/// otherwise the edge nearest the point.
fn edge_of(m: &solidcraft_engine::geom::Mesh, index: usize, point: Vec3) -> Option<&Vec<Vec3>> {
    let tol = m.bounds().diagonal().max(1.0) * 1e-6;
    let near = |e: &Vec<Vec3>| e.windows(2).map(|w| point.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
    if let Some(e) = m.edges.get(index)
        && near(e) < tol
    {
        return Some(e);
    }
    m.edges.iter().min_by(|a, b| near(a).total_cmp(&near(b))).filter(|e| near(e) < tol * 1e3)
}

fn face_tris(sc: &mut GpuScene, m: &solidcraft_engine::geom::Mesh, face: usize, col: [u8; 4], lit: bool) {
    mesh_tris(sc, m, col, lit, |f| f as usize == face);
}

/// Emit a set of faces in one mesh pass. Whole-body feedback must not scan every triangle
/// again for each face of a detailed imported part.
fn mesh_tris(sc: &mut GpuScene, m: &solidcraft_engine::geom::Mesh, col: [u8; 4], lit: bool, takes: impl Fn(u32) -> bool) {
    let col = sc.color(col);
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if !takes(*f) {
            continue;
        }
        for k in t {
            let i = *k as usize;
            if let (Some(p), Some(n)) = (m.positions.get(i), m.normals.get(i)) {
                sc.tri(p.to_f32(), if lit { n.to_f32() } else { [0.0; 3] }, col);
            }
        }
    }
}

/// How much wider a sketch curve's halo is than the curve (#4862): a 1 px rim each side.
const SKETCH_HALO_WIDTH: f32 = 2.0;

/// A highlighted edge: a halo and a core, and where the model hides it, the core dimmed (so a
/// pick on the far side still shows).
/// The palette entries of a highlighted edge: its halo, its core, and its dimmed part where the
/// model hides it. Interned once for every edge drawn in them.
#[derive(Clone, Copy)]
struct EdgeColours {
    halo: crate::gpu::PaletteIndex,
    core: crate::gpu::PaletteIndex,
    hidden: crate::gpu::PaletteIndex,
}

impl EdgeColours {
    fn new(sc: &mut GpuScene, core: Color32, halo: Color32) -> EdgeColours {
        EdgeColours { halo: sc.color(c4(halo)), core: sc.color(c4(core)), hidden: sc.color(c4(core.gamma_multiply(0.45))) }
    }
}

fn edge_lines(sc: &mut GpuScene, e: &[Vec3], cols: EdgeColours, w: f32, normals: &[[[f32; 3]; 2]]) {
    let segments = e.len().saturating_sub(1);
    sc.reserve_lines(2 * segments, segments);
    for (col, width) in [(cols.halo, w + 3.0), (cols.core, w)] {
        // Segments without their faces' normals share one width word.
        let flat = crate::gpu::lifted_width(width, [[0.0; 3]; 2]);
        for (i, p) in e.windows(2).enumerate() {
            let word = normals.get(i).map_or(flat, |n| crate::gpu::lifted_width(width, *n));
            sc.lifted_line_word(p[0].to_f32(), p[1].to_f32(), col, word);
        }
    }
    for p in e.windows(2) {
        sc.hidden_line(p[0].to_f32(), p[1].to_f32(), cols.hidden, (w * 0.7).max(1.0));
    }
}

/// The normals of the faces beside an edge, for each of its segments (the two most different
/// ones found at the segment's ends; zero where none is found), so its highlight can be lifted
/// over a face seen at a grazing angle (`GpuScene::lifted_line`). One pass over the mesh's
/// vertices: the tessellation shares the edge's points with the faces on either side.
fn edge_normals(m: &solidcraft_engine::geom::Mesh, e: &[Vec3]) -> Vec<[[f32; 3]; 2]> {
    let tol = m.bounds().diagonal().max(1.0) * 1e-7;
    let key = |p: Vec3| ((p.x / tol).round() as i64, (p.y / tol).round() as i64, (p.z / tol).round() as i64);
    let mut at: std::collections::HashMap<(i64, i64, i64), Vec<Vec3>> = e.iter().map(|p| (key(*p), Vec::new())).collect();
    for (p, n) in m.positions.iter().zip(&m.normals) {
        if let Some(list) = at.get_mut(&key(*p))
            && list.len() < 8
        {
            list.push(*n);
        }
    }
    e.windows(2)
        .map(|w| {
            let mut found: Vec<Vec3> = Vec::new();
            for p in w {
                found.extend(at.get(&key(*p)).into_iter().flatten().copied());
            }
            let Some(first) = found.first().copied() else { return [[0.0; 3]; 2] };
            let other = found.iter().copied().min_by(|a, b| a.dot(first).total_cmp(&b.dot(first))).unwrap_or(first);
            [first.to_f32(), other.to_f32()]
        })
        .collect()
}

/// The outline of a highlighted face where the model hides it (dimmed).
fn hidden_face_outline(sc: &mut GpuScene, m: &solidcraft_engine::geom::Mesh, face: usize, col: Color32) {
    let col = sc.color(c4(col.gamma_multiply(0.45)));
    for (ei, e) in m.edges.iter().enumerate() {
        let on = m.edge_faces.get(ei).is_some_and(|f| f.iter().any(|x| *x as usize == face));
        if on && m.edge_pickable(ei) {
            for p in e.windows(2) {
                sc.hidden_line(p[0].to_f32(), p[1].to_f32(), col, 1.4);
            }
        }
    }
}

pub(crate) fn profile_fill(sc: &mut GpuScene, app: &SolveApp, sketch: u64, index: usize, fill: Color32, edge: Color32) {
    let st = crate::open_meshing::shown_world(app);
    let Some(ss) = st.sketch(sketch) else { return };
    let Some(p) = ss.profiles.get(index) else { return };
    let plane = match crate::dialogs::emboss_profile_plane(app, sketch) {
        Ok(Some(plane)) => plane,
        Ok(None) => ss.plane,
        Err(_) => return,
    };
    // Lift toward the eye so a profile on a face shows over it.
    let lift = app.cam.back() * (app.cam.half_height() * 0.002);
    let w = |q: solidcraft_engine::geom::Vec2| (plane.to_world(q) + lift).to_f32();
    let col = sc.color(c4(fill));
    let edge = sc.color(c4(edge));
    // In the translucent pass (after the grid and edges) so it covers them.
    let proj = display_proj(app);
    let display = crate::sketch_display::profile_region(app, sketch, index, &plane, &proj);
    for [a, b, c] in display.as_ref().as_ref().map_or_else(Vec::new, |region| crate::sketch_display::fill_triangles(region, &mut 65_536)) {
        sc.trans_tri(w(a), [0.0; 3], col);
        sc.trans_tri(w(b), [0.0; 3], col);
        sc.trans_tri(w(c), [0.0; 3], col);
    }
    for lp in std::iter::once(&p.region.outer).chain(&p.region.holes) {
        for part in crate::sketch_display::boundary_parts(lp, &plane, &proj, app.viewport.pixels_per_point()) {
            for pair in part.windows(2) {
                sc.line((pair[0] + lift).to_f32(), (pair[1] + lift).to_f32(), edge, 2.0, false);
            }
        }
    }
}

/// An origin or construction plane: reference geometry, which a section does not cut (#1229).
/// A reference plane's square. `through` draws it over the model (Create Sketch: a plane inside
/// a body stays visible and pickable; or the Preferences setting to show reference geometry
/// through bodies), else it is depth-tested like the rest of the scene.
fn quad(sc: &mut GpuScene, q: &[Vec3; 4], fill: Color32, edge: Color32, through: bool) {
    let col = sc.color(c4(fill));
    for i in [0, 1, 2, 0, 2, 3] {
        if let Some(p) = q.get(i) {
            if through {
                sc.xray_tri(p.to_f32(), [0.0; 3], col);
            } else {
                sc.guide_tri(p.to_f32(), col);
            }
        }
    }
    for i in 0..4 {
        let (a, b) = (q[i].to_f32(), q[(i + 1) % 4].to_f32());
        reference_line(sc, a, b, c4(edge), 1.2, through);
    }
}

/// A reference line (plane edge, axis): over the model when `through` (as [`quad`]), else depth-tested and
/// left whole by a section.
fn reference_line(sc: &mut GpuScene, a: [f32; 3], b: [f32; 3], col: [u8; 4], width: f32, through: bool) {
    let col = sc.color(col);
    if through {
        sc.line(a, b, col, width, true);
    } else {
        sc.guide_line(a, b, col, width);
    }
}

/// What the highlight draws of the hovered item. A face or a plane is lit the same wherever on it
/// the pointer is, so the point under the pointer is left out: the highlight (a whole scanned
/// mesh is one face) is not built again at every move of the pointer across it.
fn hover_drawn(hover: &Option<Hit>) -> String {
    match hover {
        Some(Hit::Face { body, index, .. }) => format!("face {body:?} {index}"),
        Some(Hit::Plane { name, .. }) => format!("plane {name:?}"),
        other => format!("{other:?}"),
    }
}

fn highlight_geometry_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let selected = app.highlighted();
    app.session.revision.hash(&mut h);
    (std::sync::Arc::as_ptr(&crate::open_meshing::shown_world(app)) as usize).hash(&mut h);
    serde_json::to_string(&selected).unwrap_or_default().hash(&mut h);
    hover_drawn(&app.viewport.hover).hash(&mut h);
    app.viewport.hover_feature.hash(&mut h);
    app.viewport.hover_bodies.hash(&mut h);
    // Which mesh lights a face follows how the model draws its body (see `shown_mesh`), and at
    // which display quality its plan and edges are made.
    solidcraft_engine::doc::display_quality::current().hash(&mut h);
    lod_generation(app).hash(&mut h);
    // Whole-body highlights redraw the model scene's own blocks: they must be the scene's
    // current ones (their keys and the bodies' ranges in them).
    scene_key(app).hash(&mut h);
    // And the mode the model draws bodies in: from their plans on the GPU, or built here.
    app.viewport.gpu.is_some_and(|g| g.tess).hash(&mut h);
    app.session.active_component.hash(&mut h);
    app.session.active_occurrence_path.hash(&mut h);
    app.ui.visual_style.hash(&mut h);
    app.ui.pick_bodies.hash(&mut h);
    app.ui.pick_components.hash(&mut h);
    crate::render_workspace::active(app).hash(&mut h);
    app.session.active_sketch.hash(&mut h);
    (app.dialog.is_some(), app.tool.is_some()).hash(&mut h);
    // Which input is active decides what a round edge or face under the cursor stands for.
    app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept).hash(&mut h);
    crate::theme::is_dark().hash(&mut h);
    crate::theme::vision().hash(&mut h);
    // Two themes may share a light/dark base while drawing different geometry colours.
    crate::theme_files::color_key(&Tokens::get()).hash(&mut h);
    app.preview.effective_replaced().hash(&mut h);
    app.preview.rolled_back.hash(&mut h);
    app.origin_visible().hash(&mut h);
    app.picking_plane().hash(&mut h);
    app.preferences.reference_geometry_through_bodies.hash(&mut h);
    app.session.visibility.hidden_origin.hash(&mut h);
    app.session.visibility.hidden_bodies.hash(&mut h);
    app.session.visibility.hidden_occurrences.hash(&mut h);
    app.session.visibility.hidden_folders.hash(&mut h);
    h.finish()
}

pub(crate) fn highlight_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    highlight_geometry_key(app).hash(&mut h);
    let selected = app.highlighted();
    app.cam.half_height().to_bits().hash(&mut h);
    // Body triangles and outlines live in world space; the GPU applies the new camera.
    // Only sketch feedback needs projected tessellation and the profile's lift toward the eye.
    let projected = selected.iter().any(|s| matches!(s, Sel::Profile { .. } | Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. }))
        || matches!(app.viewport.hover, Some(Hit::Profile { .. }))
        || !selected_sketches(app, &selected).is_empty();
    if projected {
        crate::sketch_display::projection_key(&display_proj(app), app.viewport.pixels_per_point()).hash(&mut h);
    }
    h.finish()
}

#[derive(Clone)]
struct HoverBlock {
    key: u64,
    state: std::sync::Arc<solidcraft_engine::doc::ModelState>,
    doc: std::sync::Arc<solidcraft_engine::doc::Document>,
    /// The colours the block's palette indices refer to.
    palette: std::sync::Arc<crate::gpu::Palette>,
    block: crate::gpu::SceneBlock,
    /// Whole bodies the model draws from their plans: reused like the block when the camera
    /// moves (they are the model's own evaluated triangles, not a copy of them).
    hover_planned: Vec<std::sync::Arc<solidcraft_engine::geom::tess::TessPlan>>,
    selected_planned: Vec<std::sync::Arc<solidcraft_engine::geom::tess::TessPlan>>,
    /// CPU-built bodies: their triangles in the model scene's blocks, reused the same way.
    hover_blocks: Vec<crate::gpu::BlockTris>,
    selected_blocks: Vec<crate::gpu::BlockTris>,
    /// Bodies the model draws as placements of shared triangles: the same placements with the
    /// highlight colour.
    hover_instanced: Vec<crate::gpu::InstancedMesh>,
    selected_instanced: Vec<crate::gpu::InstancedMesh>,
    /// Faded or see-through bodies in the model's blocks, drawn again in this palette.
    recoloured: Vec<crate::gpu::BlockTris>,
}

/// The plan the model draws a whole highlighted body from, when it draws one on the GPU. The
/// highlight then redraws those evaluated triangles in the highlight colour instead of building
/// a copy of every triangle on the CPU: lighting a whole assembly's faces that way took seconds
/// (every triangle of every body, again), while the model already had them all on the GPU.
fn highlight_plan(
    app: &SolveApp,
    b: &solidcraft_engine::doc::ModelBody,
    active_bodies: &Option<Vec<String>>,
) -> Option<std::sync::Arc<solidcraft_engine::geom::tess::TessPlan>> {
    if crate::render_workspace::active(app) {
        return None;
    }
    drawn_plan(app, b, model_plan_class(app, b, active_bodies).is_some())
}

/// Whether the model draws a whole body opaque on the GPU, rather than building it on the CPU:
/// a live preview stands in for none, faded and see-through bodies stay on the CPU, and face
/// opacities matter only where faces are shaded.
fn highlight_opaque(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, active_bodies: &Option<Vec<String>>) -> bool {
    if crate::render_workspace::active(app) || app.preview.effective_replaced().contains(&b.name) {
        return false;
    }
    let doc = &app.session.doc;
    let faces_opaque = app.ui.visual_style == 2 || doc.face_colors(b).iter().all(|(_, l)| l.opacity >= 0.999);
    !body_faded(app, b, active_bodies) && doc.model_body_opacity(b) >= 0.999 && faces_opaque
}

/// Whole highlighted bodies the model draws as placements of shared triangles, collected as one
/// [`InstancedMesh`] per mesh (a placement and its colour each): the model scene already holds
/// the shared vertices, so a whole assembly under the pointer draws them again, not a copy.
#[derive(Default)]
struct HighlightInstances {
    meshes: Vec<crate::gpu::InstancedMesh>,
    /// The index in `meshes` of each mesh, by its address.
    by_mesh: std::collections::HashMap<usize, usize>,
}

impl HighlightInstances {
    /// Add the body at `color`, when the model draws it as an instanced placement of an opaque
    /// shared mesh; `false` when it does not (the caller builds its feedback on the CPU).
    fn add(&mut self, app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, color: crate::gpu::PaletteIndex) -> bool {
        // A live preview stands in for the body; a body with face appearances of its own is built
        // on the CPU, never as an instance. A faded or see-through placement is lit like an opaque
        // one: the highlight colour is the placement's own here, not the model's.
        if app.preview.effective_replaced().contains(&b.name) || !app.session.doc.face_colors(b).is_empty() {
            return false;
        }
        let Some((mesh, m)) = shared_mesh(app, b) else { return false };
        let key = std::sync::Arc::as_ptr(&mesh) as usize;
        let at = match self.by_mesh.get(&key) {
            Some(i) => *i,
            None => {
                // The model scene's shared triangles (made ahead, or with its own instances).
                let tris = app
                    .viewport
                    .instanced_tris
                    .lock()
                    .ok()
                    .and_then(|c| c.get(&key).filter(|(held, _)| std::sync::Arc::ptr_eq(held, &mesh)).map(|(_, t)| t.clone()))
                    .or_else(|| taken_shared_tris(&mesh));
                let Some(tris) = tris else { return false };
                self.meshes.push(crate::gpu::InstancedMesh {
                    key: key as u64,
                    tris,
                    instances: Vec::new(),
                    look: crate::gpu::InstanceLook::Opaque,
                    levels: Vec::new(),
                });
                self.by_mesh.insert(key, self.meshes.len() - 1);
                self.meshes.len() - 1
            }
        };
        let rows = [0, 1, 2].map(|r| [m[0][r] as f32, m[1][r] as f32, m[2][r] as f32, m[3][r] as f32]);
        match self.meshes.get_mut(at) {
            Some(inst) => {
                inst.place(rows, color);
                true
            }
            None => false,
        }
    }

    /// The meshes with their placements (none when nothing was added).
    fn finish(self) -> Vec<crate::gpu::InstancedMesh> {
        self.meshes
    }
}

/// The model scene's triangles for a whole highlighted body it builds on the CPU (no display
/// plan): the block holding them and the body's vertex range in it. Drawing those again tinted
/// costs no CPU work, where copying every triangle did (a whole assembly hovered at once stalled
/// the window). See-through and faded bodies have no range here: they are not in the block's
/// opaque triangles, and the CPU feedback colours them.
fn highlight_block_tris(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody) -> Option<crate::gpu::BlockTris> {
    if crate::render_workspace::active(app) || app.preview.effective_replaced().contains(&b.name) {
        return None;
    }
    let map = app.viewport.body_tris.lock().ok()?;
    map.get(&b.name).cloned()
}

/// How a whole body is lit: hovered or selected, and in which colour (where the model's own
/// colour is not kept: placements, and faded or see-through bodies).
#[derive(Clone, Copy)]
struct WholeBody {
    selected: bool,
    colour: [u8; 4],
}

/// Light the whole of `b` from what the model already has on the GPU, copying no triangle: its
/// plan or its opaque runs (tinted over the model's own colours), its block's vertices recoloured
/// in the highlight's palette (faded and see-through bodies), or its placements. `false` when
/// none applies (wireframe, a live preview standing in for the body): the caller copies it.
fn light_whole_body(
    app: &SolveApp,
    b: &solidcraft_engine::doc::ModelBody,
    active_bodies: &Option<Vec<String>>,
    lit: WholeBody,
    body_slots: &crate::scene_palette::PaletteArena,
    feedback: &mut GpuScene,
    instances: &mut HighlightInstances,
) -> bool {
    if let Some(plan) = highlight_plan(app, b, active_bodies) {
        if lit.selected {
            feedback.selected_planned.push(plan)
        } else {
            feedback.hover_planned.push(plan)
        }
        return true;
    }
    let opaque = highlight_opaque(app, b, active_bodies);
    if let Some(tris) = highlight_block_tris(app, b) {
        if opaque && !tris.tris.is_empty() {
            if lit.selected {
                feedback.selected_blocks.push(tris)
            } else {
                feedback.hover_blocks.push(tris)
            }
            return true;
        }
        // Faded or see-through: the body's every vertex again, its slots in this palette holding
        // the highlight colour (as the copy it replaces was drawn).
        if !opaque && let Some(slots) = body_slots.slots(&b.name) {
            let palette = std::sync::Arc::make_mut(&mut feedback.palette);
            palette.set(slots.body, lit.colour);
            for f in 0..b.body.face_count() {
                palette.set(slots.face(f), lit.colour);
            }
            feedback.recoloured.push(tris);
            return true;
        }
    }
    let colour = feedback.color(lit.colour);
    instances.add(app, b, colour)
}

/// The highlight scene: origin widget, construction planes, selection and hover.
pub(crate) fn build_highlight(app: &SolveApp) -> GpuScene {
    let began = crate::now_ms();
    let t = Tokens::get();
    // The model scene's stable palette slots first: whole faded or see-through bodies are drawn
    // again from the model's own vertices in this palette, their slots set to the highlight's
    // colour (see `light_whole_body`); this scene's own colours follow.
    // The key first: it reads the arena (`scene_key`), which is held below.
    let key = highlight_geometry_key(app);
    let body_slots = app.viewport.palette.lock().unwrap_or_else(|e| e.into_inner());
    let mut sc = GpuScene::with_palette(crate::gpu::Palette::with_stable(body_slots.entries()));
    let st = crate::open_meshing::shown_world(app);
    let s = &app.session;
    let active_bodies = s.active_occurrence_path.as_ref().and_then(|p| s.occurrence_bodies(p, true).ok());
    // Faces lit over the model use the triangles the model shows them with.
    let product = crate::render_workspace::active(app);
    let shown = |b: &solidcraft_engine::doc::ModelBody| if product { b.mesh() } else { shown_mesh(app, b, &active_bodies) };
    let items = app.highlighted();
    let sketches = selected_sketches(app, &items);
    let mut sel = Vec::new();
    for item in &items {
        for piece in displayed_selection(app, item).into_iter().filter(|s| !product || matches!(s, Sel::Body { .. } | Sel::Face { .. })) {
            if !sel.iter().any(|x| crate::selection::same_item(x, &piece)) {
                sel.push(piece);
            }
        }
    }
    // Component hover expands through the same occurrence selection as a Browser row.
    let component_hover = app
        .viewport
        .hover
        .as_ref()
        .filter(|h| app.ui.pick_components && matches!(h, Hit::Face { .. }))
        .and_then(|h| hit_candidate(app, h))
        .map(|(_, s)| displayed_selection(app, &s));
    let hover = app.viewport.hover.as_ref().filter(|_| component_hover.is_none());
    let size = origin_size(app.cam.half_height());
    if !product {
        // Origin planes and construction planes.
        let plane_state = |name: &str| {
            let selected = sel.iter().any(|x| matches!(x, Sel::Plane { name: n } if n == name));
            let hovered = matches!(hover, Some(Hit::Plane { name: n, .. }) if n == name);
            (selected, hovered)
        };
        let mut planes: Vec<(String, [Vec3; 4], Color32)> =
            origin_planes(app).into_iter().map(|(n, _, q)| (n.to_string(), q, t.origin_plane)).collect();
        planes.extend(construction_quads(app).into_iter().map(|(n, _, q)| (n, q, t.construction_plane)));
        let through = app.picking_plane() || app.preferences.reference_geometry_through_bodies;
        for (name, q, base) in &planes {
            let (selected, hovered) = plane_state(name);
            let fill = if hovered {
                t.origin_plane_hover
            } else if selected {
                Color32::from_rgba_unmultiplied(t.sel_face.r(), t.sel_face.g(), t.sel_face.b(), 150)
            } else {
                *base
            };
            quad(&mut sc, q, fill, if selected { t.sel_edge } else { t.origin_plane_edge }, through);
        }
        // Origin axes: solid near the origin, dashed beyond.
        for (name, dir) in origin_axes(app) {
            let col = match name {
                "X" => t.axis_x,
                "Y" => t.axis_y,
                _ => t.axis_z,
            };
            let on = sel.iter().any(|x| matches!(x, Sel::Axis { name: n } if n == name)) || matches!(hover, Some(Hit::Axis { name: n }) if n == name);
            let w = if on { 4.0 } else { 2.0 };
            reference_line(&mut sc, [0.0; 3], (dir * size).to_f32(), c4(col), w, through);
            let dash = size * 0.08;
            let mut a = size + dash;
            while a < size * AXIS_LEN {
                reference_line(&mut sc, (dir * a).to_f32(), (dir * (a + dash)).to_f32(), c4(col), w * 0.75, through);
                a += dash * 2.0;
            }
        }
        // Construction axes (dashed, the length of the origin axes either side) and points (a
        // small star), in the construction colour.
        for (name, a, b) in construction_axis_segments(app) {
            let on =
                sel.iter().any(|x| matches!(x, Sel::Axis { name: n } if *n == name)) || matches!(hover, Some(Hit::Axis { name: n }) if *n == name);
            let col = c4(if on { t.sel_edge } else { t.construction_geom });
            let w = if on { 3.0 } else { 1.6 };
            let len = a.dist(b);
            let dash = size * 0.08;
            let mut s = 0.0;
            while s < len {
                let e = (s + dash).min(len);
                reference_line(&mut sc, a.lerp(b, s / len).to_f32(), a.lerp(b, e / len).to_f32(), col, w, through);
                s += dash * 1.6;
            }
        }
        for (name, p) in construction_points(app) {
            let on = sel.iter().any(|x| matches!(x, Sel::ConstructionPoint { name: n } if *n == name))
                || matches!(hover, Some(Hit::ConstructionPoint { name: n }) if *n == name);
            let col = sc.color(c4(if on { t.sel_edge } else { t.construction_geom }));
            let r = size * 0.04;
            for d in [Vec3::X, Vec3::Y, Vec3::Z] {
                sc.line((p - d * r).to_f32(), (p + d * r).to_f32(), col, 2.0, true);
            }
        }
    }
    // A round edge or face under the cursor (or picked) in an axis input shows its axis, and a
    // circle in a point input its centre, as construction geometry. The axis runs through the
    // solid it belongs to, so it is drawn over the model.
    let (round_axes, round_centres) = round_pick_feedback(app);
    let construction = sc.color(c4(t.construction_geom));
    for (a, b) in round_axes {
        let len = a.dist(b);
        let dash = size * 0.08;
        let mut s = 0.0;
        while s < len {
            let e = (s + dash).min(len);
            sc.line(a.lerp(b, s / len).to_f32(), a.lerp(b, e / len).to_f32(), construction, 2.0, true);
            s += dash * 1.6;
        }
    }
    for p in round_centres {
        let r = size * 0.04;
        for d in [Vec3::X, Vec3::Y, Vec3::Z] {
            sc.line((p - d * r).to_f32(), (p + d * r).to_f32(), construction, 2.0, true);
        }
    }
    // Reference widgets follow zoom; the often much larger world-space feedback does not.
    let projected = sel.iter().any(|s| matches!(s, Sel::Profile { .. } | Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. }))
        || matches!(hover, Some(Hit::Profile { .. }))
        || !sketches.is_empty();
    if !projected
        && let Some(cached) = app.viewport.hover_block.lock().ok().and_then(|cache| {
            cache.as_ref().filter(|c| c.key == key && std::sync::Arc::ptr_eq(&c.state, &st) && std::sync::Arc::ptr_eq(&c.doc, &s.doc)).cloned()
        })
    {
        sc.palette = cached.palette;
        sc.blocks.push(cached.block);
        sc.hover_planned = cached.hover_planned;
        sc.selected_planned = cached.selected_planned;
        sc.hover_blocks = cached.hover_blocks;
        sc.selected_blocks = cached.selected_blocks;
        sc.recoloured = cached.recoloured;
        sc.hover_instanced = cached.hover_instanced;
        sc.selected_instanced = cached.selected_instanced;
        return sc;
    }
    let mut feedback = GpuScene { palette: std::sync::Arc::new((*sc.palette).clone()), ..Default::default() };
    let (hover_edge, selected_edge) =
        (EdgeColours::new(&mut feedback, t.hover_edge, t.hover_edge_halo), EdgeColours::new(&mut feedback, t.sel_edge, t.sel_edge_rim));
    let mut hover_instances = HighlightInstances::default();
    let mut selected_instances = HighlightInstances::default();
    // Selected things.
    for x in &sel {
        match x {
            Sel::Face { body, index, .. } => {
                // A live preview stands in for the body: its old faces would fight with it.
                if let Some(b) = st.body(body).filter(|_| app.session.body_visible(body) && !app.preview.effective_replaced().contains(body)) {
                    face_tris(&mut feedback, &shown(b), *index, c4(t.sel_face), false);
                    hidden_face_outline(&mut feedback, &drawn_mesh(app, b), *index, t.sel_edge);
                }
            }
            Sel::Body { name } => {
                if let Some(b) = st.body(name)
                    && app.session.body_visible(name)
                {
                    let lit = WholeBody { selected: true, colour: c4(t.body_selected) };
                    if !light_whole_body(app, b, &active_bodies, lit, &body_slots, &mut feedback, &mut selected_instances) {
                        let m = shown(b);
                        let count = b.body.face_count();
                        mesh_tris(&mut feedback, &m, c4(t.body_selected), true, |f| (f as usize) < count);
                    }
                }
            }
            Sel::Edge { body, index, point } => {
                if let Some(b) = st.body(body)
                    && app.session.body_visible(body)
                {
                    let m = drawn_mesh(app, b);
                    if let Some(e) = edge_of(&m, *index, *point) {
                        edge_lines(&mut feedback, e, selected_edge, 3.0, &edge_normals(&m, e));
                    }
                }
            }
            Sel::Profile { sketch, index } => profile_fill(&mut feedback, app, *sketch, *index, t.sel_profile, t.sel_profile_edge),
            // A 3D sketch curve, in the sketch it names, or whichever sketch has it.
            Sel::SketchCurve { id, sketch } if st.sketches.iter().any(|ss| ss.sketch.wire_index(id).is_some()) => {
                let sel_edge = feedback.color(c4(t.sel_edge));
                for ss in st.sketches.iter().filter(|ss| sketch.is_none_or(|k| ss.feature == k)) {
                    if let Some(w) = ss.sketch.wire_index(id).and_then(|i| ss.sketch.wires.get(i)) {
                        for q in w.pts.windows(2) {
                            feedback.line(q[0].to_f32(), q[1].to_f32(), sel_edge, 3.5, true);
                        }
                    }
                }
            }
            // A curve picked into a dialog from another sketch than the active one names it.
            Sel::ExternalSketchCurve { sketch, id } | Sel::SketchCurve { id, sketch: Some(sketch) } => {
                let sel_edge = feedback.color(c4(t.sel_edge));
                if let Some(ss) = st.sketch(*sketch)
                    && let Some(ci) = ss.sketch.curve_index(id)
                {
                    for pts in &crate::sketch_display::curve_sample(app, *sketch, ci, &ss.plane, &display_proj(app)).parts {
                        let mode = ss.sketch.curves.get(ci).map_or(0, |c| if c.centerline { 2 } else { u8::from(c.construction) });
                        for (a, b) in sketch_segments(pts, mode, grid_step(app.cam.half_height()).0) {
                            feedback.line(a.to_f32(), b.to_f32(), sel_edge, 3.5, true);
                        }
                    }
                } else if let Some(w) = st.sketch(*sketch).and_then(|ss| ss.sketch.wire_index(id).and_then(|i| ss.sketch.wires.get(i))) {
                    // A 3D sketch curve of that sketch.
                    for q in w.pts.windows(2) {
                        feedback.line(q[0].to_f32(), q[1].to_f32(), sel_edge, 3.5, true);
                    }
                }
            }
            Sel::SketchCurve { id, .. } => {
                if let Some(ss) = app.session.active_sketch.and_then(|sid| st.sketch(sid))
                    && let Some(ci) = ss.sketch.curve_index(id)
                {
                    let sel_edge = feedback.color(c4(t.sel_edge));
                    for pts in &crate::sketch_display::curve_sample(app, ss.feature, ci, &ss.plane, &display_proj(app)).parts {
                        let mode = ss.sketch.curves.get(ci).map_or(0, |c| if c.centerline { 2 } else { u8::from(c.construction) });
                        for (a, b) in sketch_segments(pts, mode, grid_step(app.cam.half_height()).0) {
                            feedback.line(a.to_f32(), b.to_f32(), sel_edge, 3.5, true);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // A sketch picked in the Browser or the timeline: all its curves in their line types, over
    // the model, also while the sketch is hidden or inside a body, so it is easy to find.
    let (unit, proj) = (grid_step(app.cam.half_height()).0, display_proj(app));
    let sel_edge = feedback.color(c4(t.sel_edge));
    for ss in sketches.iter().filter_map(|id| st.sketch(*id)) {
        for (pts, col, cons) in sketch_lines_with_sampler(&ss.sketch, &ss.plane, false, &[], |_, index, plane| {
            crate::sketch_display::curve_sample(app, ss.feature, index, plane, &proj).parts.clone()
        }) {
            for (a, b) in sketch_segments(&pts, sketch_line_mode(col, cons), unit) {
                feedback.line(a.to_f32(), b.to_f32(), sel_edge, 3.5, true);
            }
        }
    }
    // Timeline modifiers light only their resulting faces; browser rows light whole bodies.
    let mut hovered = app.viewport.hover_feature.map(|id| feature_geometry(app, id)).unwrap_or_default();
    hovered.extend(app.viewport.hover_bodies.iter().map(|name| Sel::Body { name: name.clone() }));
    hovered.extend(component_hover.into_iter().flatten());
    // By name, once: a whole assembly holds thousands of bodies, and scanning the list for
    // each of them is quadratic (as is a body lookup by name in the world).
    let hovered_whole: std::collections::HashSet<&str> =
        hovered.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    let selected_whole: std::collections::HashSet<&str> =
        sel.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.as_str()) } else { None }).collect();
    let mut hovered_faces: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    let mut selected_faces: std::collections::HashMap<&str, std::collections::HashSet<usize>> = std::collections::HashMap::new();
    for x in &hovered {
        if let Sel::Face { body, index, .. } = x {
            hovered_faces.entry(body.as_str()).or_default().push(*index);
        }
    }
    for x in &sel {
        if let Sel::Face { body, index, .. } = x {
            selected_faces.entry(body.as_str()).or_default().insert(*index);
        }
    }
    for b in st.bodies.iter().filter(|b| app.session.body_visible(&b.name) && !app.preview.effective_replaced().contains(&b.name)) {
        let whole = hovered_whole.contains(b.name.as_str());
        let faces: Vec<usize> =
            if whole { (0..b.body.face_count()).collect() } else { hovered_faces.get(b.name.as_str()).cloned().unwrap_or_default() };
        if faces.is_empty() {
            continue;
        }
        let m = drawn_mesh(app, b);
        let base = body_color(&s.doc, b).unwrap_or(t.body);
        let k = t.hover_face_lift;
        let hover_col = [base.r().saturating_add(k), base.g().saturating_add(k), base.b().saturating_add(k), 255];
        let selected_body = selected_whole.contains(b.name.as_str());
        if !selected_body {
            // A whole body the model draws from a plan is lit by redrawing the plan's own
            // evaluated triangles, not by building every triangle again (see `highlight_plan`).
            let lit = WholeBody { selected: false, colour: hover_col };
            if whole && light_whole_body(app, b, &active_bodies, lit, &body_slots, &mut feedback, &mut hover_instances) {
                // Drawn again from what the model already has on the GPU.
            } else {
                let lit = shown(b);
                let selected_faces = selected_faces.get(b.name.as_str());
                let hovered_faces: std::collections::HashSet<usize> = faces.iter().copied().collect();
                let keeps = |f: u32| {
                    let f = f as usize;
                    hovered_faces.contains(&f) && selected_faces.is_none_or(|s| !s.contains(&f))
                };
                mesh_tris(&mut feedback, &lit, hover_col, true, keeps);
            }
        }
        for (ei, e) in m.edges.iter().enumerate() {
            if m.edge_pickable(ei) && (whole || m.edge_faces.get(ei).is_some_and(|fs| fs.iter().any(|f| faces.contains(&(*f as usize))))) {
                edge_lines(&mut feedback, e, hover_edge, 1.5, &[]);
            }
        }
    }
    // Hover (pre-highlight).
    match hover {
        // Body selection: the whole body lights up.
        Some(Hit::Face { body, .. }) if app.ui.pick_bodies && app.dialog.is_none() && app.tool.is_none() => {
            if let Some(b) = st.body(body) {
                let k = t.hover_face_lift;
                let c = body_color(&s.doc, b).unwrap_or(t.body);
                let hover_col = [c.r().saturating_add(k), c.g().saturating_add(k), c.b().saturating_add(k), 255];
                let lit = WholeBody { selected: false, colour: hover_col };
                if !light_whole_body(app, b, &active_bodies, lit, &body_slots, &mut feedback, &mut hover_instances) {
                    let m = shown(b);
                    let count = b.body.face_count();
                    mesh_tris(&mut feedback, &m, hover_col, true, |f| (f as usize) < count);
                }
            }
        }
        Some(Hit::Face { body, index, .. })
            if !app.preview.effective_replaced().contains(body)
                && !sel.iter().any(|x| matches!(x, Sel::Face { body: b, index: i, .. } if b == body && i == index)) =>
        {
            if let Some(b) = st.body(body) {
                let k = t.hover_face_lift;
                let c = body_rgb(app, body);
                let m = drawn_mesh(app, b);
                let patch = m.surface_patch(*index);
                let lit = shown(b);
                for &f in &patch {
                    face_tris(&mut feedback, &lit, f, [c.0.saturating_add(k), c.1.saturating_add(k), c.2.saturating_add(k), 255], true);
                }
                for (e, _) in m.edge_faces.iter().enumerate().filter(|(_, fs)| fs.iter().any(|f| patch.contains(&(*f as usize)))) {
                    if let Some(p) = m.edges.get(e)
                        && m.edge_pickable(e)
                    {
                        edge_lines(&mut feedback, p, hover_edge, 1.5, &[]);
                    }
                }
            }
        }
        Some(Hit::Edge { body, index, mid }) => {
            if let Some(b) = st.body(body) {
                // Lit over the drawn edges (finer away from Normal display quality).
                let m = drawn_mesh(app, b);
                for i in m.circular_rim(*index) {
                    let selected = sel.iter().any(|x| matches!(x, Sel::Edge { body: b, index: j, .. } if b == body && *j == i));
                    if !selected && let Some(e) = if i == *index { edge_of(&m, i, *mid) } else { m.edges.get(i) } {
                        edge_lines(&mut feedback, e, hover_edge, 2.0, &edge_normals(&m, e));
                    }
                }
            }
        }
        Some(Hit::Profile { sketch, index })
            if !sel.iter().any(|x| matches!(x, Sel::Profile { sketch: s2, index: i2 } if s2 == sketch && i2 == index)) =>
        {
            profile_fill(&mut feedback, app, *sketch, *index, t.hover_profile, t.hover_profile_edge);
        }
        _ => {}
    }
    feedback.hover_instanced = hover_instances.finish();
    feedback.selected_instanced = selected_instances.finish();
    // The feedback palette extends the scene's own: set it before sharing the geometry, and keep
    // the indices valid on the cache hit above.
    sc.palette = feedback.palette.clone();
    if projected {
        sc.tris.append(&mut feedback.tris);
        sc.trans.append(&mut feedback.trans);
        sc.lines.append(&mut feedback.lines);
        sc.hidden.append(&mut feedback.hidden);
        sc.overlay.append(&mut feedback.overlay);
        sc.hover_planned.append(&mut feedback.hover_planned);
        sc.selected_planned.append(&mut feedback.selected_planned);
        sc.hover_blocks.append(&mut feedback.hover_blocks);
        sc.selected_blocks.append(&mut feedback.selected_blocks);
        sc.recoloured.append(&mut feedback.recoloured);
        sc.hover_instanced.append(&mut feedback.hover_instanced);
        sc.selected_instanced.append(&mut feedback.selected_instanced);
    } else {
        // The feedback triangles become a block's canonical vertices, in one opaque list.
        let vertices = std::mem::take(&mut feedback.tris);
        let count = u32::try_from(vertices.len() / crate::gpu::TRI_SIZE).unwrap_or(0);
        let block = crate::gpu::SceneBlock {
            key,
            bytes: std::sync::Arc::new(crate::gpu::BlockBytes {
                vertices: std::sync::Arc::new(vertices),
                opaque: std::sync::Arc::new(std::iter::once(0..count).filter(|r| !r.is_empty()).collect()),
                lines: std::sync::Arc::new(std::mem::take(&mut feedback.lines)),
                hidden: std::sync::Arc::new(std::mem::take(&mut feedback.hidden)),
                ..Default::default()
            }),
        };
        let palette = feedback.palette.clone();
        let (hover_planned, selected_planned) = (feedback.hover_planned, feedback.selected_planned);
        let (hover_blocks, selected_blocks) = (feedback.hover_blocks, feedback.selected_blocks);
        let (hover_instanced, selected_instanced) = (feedback.hover_instanced, feedback.selected_instanced);
        let recoloured = feedback.recoloured;
        if let Ok(mut cache) = app.viewport.hover_block.lock() {
            *cache = Some(HoverBlock {
                key,
                state: st.clone(),
                doc: s.doc.clone(),
                palette,
                block: block.clone(),
                hover_planned: hover_planned.clone(),
                selected_planned: selected_planned.clone(),
                hover_blocks: hover_blocks.clone(),
                selected_blocks: selected_blocks.clone(),
                hover_instanced: hover_instanced.clone(),
                selected_instanced: selected_instanced.clone(),
                recoloured: recoloured.clone(),
            });
        }
        sc.blocks.push(block);
        sc.hover_planned = hover_planned;
        sc.selected_planned = selected_planned;
        sc.hover_blocks = hover_blocks;
        sc.selected_blocks = selected_blocks;
        sc.hover_instanced = hover_instanced;
        sc.selected_instanced = selected_instanced;
        sc.recoloured = recoloured;
    }

    if solidcraft_engine::geom::env::var_os("PROFILE_FRAMES").is_some() {
        let in_blocks = |f: fn(&crate::gpu::SceneBlock) -> usize| sc.blocks.iter().map(f).sum::<usize>();
        let tris = sc.tris.len() + in_blocks(|b| b.bytes.vertices.len());
        let lines = sc.lines.len() + in_blocks(|b| b.bytes.lines.len());
        let hidden = sc.hidden.len() + in_blocks(|b| b.bytes.hidden.len());
        eprintln!(
            "SolidCraft highlight: {:.0} ms, hovered {} bodies, planned {} hover + {} selected, blocks {} hover + {} selected, instanced {} hover + {} selected, CPU triangles {tris} B, lines {lines} B, hidden {hidden} B",
            crate::now_ms() - began,
            hovered.len(),
            sc.hover_planned.len(),
            sc.selected_planned.len(),
            sc.hover_blocks.len(),
            sc.selected_blocks.len(),
            sc.hover_instanced.len(),
            sc.selected_instanced.len(),
        );
    }
    sc
}

/// A body's display colour (appearance, material or imported colour), if it has one.
fn body_color(doc: &solidcraft_engine::doc::Document, b: &solidcraft_engine::doc::ModelBody) -> Option<Color32> {
    let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    doc.body_color(b).map(|[r, g, b]| Color32::from_rgb(to8(r), to8(g), to8(b)))
}

/// A body's display colour (else the theme's) as RGB.
fn body_rgb(app: &SolveApp, name: &str) -> (u8, u8, u8) {
    let st = crate::open_meshing::shown_world(app);
    let c = st.body(name).and_then(|b| body_color(&app.session.doc, b)).unwrap_or(Tokens::get().body);
    (c.r(), c.g(), c.b())
}

/// The ray `o` + t `d` in the frame of a mesh that `place` (a rigid move) puts in the world, with
/// the same t along it; `None` for a move with no inverse.
pub(crate) fn local_ray(place: &solidcraft_engine::doc::Mat, o: Vec3, d: Vec3) -> Option<(Vec3, Vec3)> {
    if solidcraft_engine::doc::is_identity(place) {
        return Some((o, d));
    }
    let back = solidcraft_engine::doc::mat_inverse(place)?;
    Some((solidcraft_engine::doc::apply_point(&back, o), solidcraft_engine::doc::apply_vector(&back, d)))
}

pub(crate) fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

pub(crate) fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_sq();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Point on the active sketch plane under the cursor (sketch coordinates), snapped to sketch
/// points within a few pixels and otherwise to a fine grid. Returns the snapped point and the
/// id of the sketch point it snapped to.
pub fn sketch_point_at(app: &SolveApp, proj: &Proj, pos: Pos2) -> Option<(Vec2, Option<String>)> {
    let sid = app.session.active_sketch?;
    let st = crate::open_meshing::shown_world(app);
    let ss = st.sketch(sid)?;
    for h in pick_for_tool(app, proj, pos) {
        if let Hit::SketchPoint { sketch, id, at } = h
            && sketch == sid
        {
            return Some((at, Some(id)));
        }
    }
    let (o, d) = proj.ray(pos);
    let w = ss.plane.intersect_ray(o, d)?;
    let lp = ss.plane.to_local(w);
    if crate::drag_snap::suppressed() {
        return Some((lp, None));
    }
    let (minor, _) = grid_step(app.cam.half_height());
    let step = minor / 10.0;
    if let Some(p) = crate::sketch_tools::refine_snap(app, proj, &ss.sketch, &ss.plane, lp) {
        return Some(p);
    }
    // With Snap off (Sketch Palette) the point is where the cursor is.
    if ss.sketch.view.no_snap {
        return Some((lp, None));
    }
    Some((Vec2::new((lp.x / step).round() * step, (lp.y / step).round() * step), None))
}

pub fn delete_selection(app: &mut SolveApp) {
    crate::delete::delete_selection(app);
}

/// Whether Primary should hide the live command preview so obscured geometry can be reselected.
///
/// Shared by [`show`] so tests can lock the same path: a manipulator drag never rolls the
/// preview back, even when Primary (preview-reselect) is held.
pub(crate) fn preview_should_roll_back(app: &SolveApp, held: crate::tool_modifiers::Held, manipulator_dragging: bool) -> bool {
    app.dialog.as_ref().is_some_and(|d| d.previews()) && app.preview.active && crate::tool_modifiers::preview_reselect(held, manipulator_dragging)
}

pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let rect = ui.max_rect();
    app.viewport.rect = Some(rect);
    refresh_layout(app, ui.ctx());
    let resp = ui.interact(rect, ui.id().with("viewport"), Sense::click_and_drag());
    let flat = crate::dialogs_sheet::flat_active(app);
    if !flat {
        app.flat_view.fitted = false;
    }
    let input =
        if flat { crate::dialogs_sheet::navigate(app, ui, rect, &resp) } else { crate::navigation::handle(app, ui, rect, &resp, NavCaps::MODEL) };
    let CameraInput {
        hover,
        scroll,
        mods,
        middle,
        secondary_down,
        primary_down,
        primary_pressed,
        delta,
        inside,
        axes,
        pinch,
        pan,
        primary_nav,
        mode,
    } = input;
    app.viewport.physical_scale = ui.ctx().pixels_per_point();
    // Suppress-snap chord and manipulator-free prefs; Shift midpoint targeting while sketching.
    // Preview reselect uses Primary but is skipped while a manipulator drag is under way.
    crate::drag_snap::set_suppressed(crate::prefs::suppress_sketch_snap_held(app, mods));
    crate::tool_modifiers::set_manipulator_free(crate::prefs::manipulator_free_held(app, mods));
    let manipulator_dragging = crate::tool_modifiers::manipulator_dragging();
    crate::tool_modifiers::set_manipulator_dragging(false);
    {
        let held = crate::tool_modifiers::Held::from_modifiers(mods);
        let mid = if let Some(cmd) = app.tool.as_ref().map(|t| t.cmd) {
            let live = crate::tool_modifiers::resolve(
                &[crate::tool_modifiers::Scope::SketchDraw, crate::tool_modifiers::Scope::SketchDrag],
                crate::tool_modifiers::descriptor(cmd),
                held,
                crate::tool_modifiers::Trigger::Live,
            );
            live.contains(crate::tool_modifiers::ModAction::TargetMidpoint)
        } else {
            false
        };
        crate::tool_modifiers::set_target_midpoint(mid);
        app.preview.rolled_back = preview_should_roll_back(app, held, manipulator_dragging);
    }
    let proj = projection(app, rect);
    if let Ok(mut cache) = app.viewport.display.lock() {
        cache.prepare(crate::open_meshing::shown_world(app), &proj, app.viewport.pixels_per_point());
    }
    // Suppress hover for the whole navigation gesture, including frames between pointer
    // events. Testing delta made the highlight reappear whenever a held drag paused.
    // Resolve it before rendering so the GPU scene and screen overlays use the same hit.
    // A 3D mouse moving the view is not such a gesture: the pointer hand is free, and hover
    // and clicks keep picking while the other hand turns the view.
    let navigating = scroll != 0.0
        || pinch != 1.0
        || pan != egui::Vec2::ZERO
        || middle
        || secondary_down
        || (primary_nav.is_some() && primary_down)
        || mode.is_some()
        || app.viewport.axis_drag.is_some()
        || app.viewport.spin
        || app.cam_anim.is_some();
    // A part dragged along its joint owns the pointer: nothing under it lights up (#5137).
    let hits = hover
        .filter(|_| inside && app.viewport.boxsel.is_none() && !crate::joint_drag::active(app) && !navigating)
        .map(|p| pick_cached(app, &proj, p))
        .unwrap_or_default();
    // The Fillet and Chamfer window resolves its own corners: the nearest sketch point to a
    // corner drawn from the origin is the fixed origin, which ends no two curves.
    let corner = hover.filter(|_| inside && !navigating).and_then(|p| crate::sketch_modify_ui::corner_hit(app, &proj, p));
    let cand = if app.tool.is_some() {
        tool_hover(app, &hits).map(|h| (h, None))
    } else if let Some((hit, sel)) = corner {
        Some((hit, Some(sel)))
    } else {
        candidate(app, &hits).map(|(h, s)| (h, Some(s)))
    };
    app.viewport.hover = cand.as_ref().map(|c| c.0.clone());
    wake_centre(app, &hits);
    // Select Other: a press held still opens the list of everything under it; while the list is
    // open its row under the pointer is what lights up.
    let held =
        crate::select_other::track_hold(app, ui.ctx(), &proj, inside, primary_pressed, primary_down, hover, primary_nav.is_some() || mode.is_some());
    crate::select_other::hover_override(app);
    let painter = ui.painter_at(rect);
    // Background gradient.
    let (top, bot) = (t.viewport_top, t.viewport_bottom);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bot);
    mesh.colored_vertex(rect.left_bottom(), bot);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));

    // ---- model ----
    crate::frame_watch::step(app, "viewport before the model");
    update_refinement(app, ui.ctx(), &proj);
    // Levels of detail being made: keep frames coming to draw them when they are.
    if app.viewport.mesh_lods.busy() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    crate::frame_watch::step(app, "display refinement");
    let key = scene_key(app);
    let t0 = crate::now_ms();
    // Drawn on the GPU; without one (the headless harness, tests) the model is not drawn.
    if app.viewport.gpu.is_some() {
        if key != app.viewport.key {
            let sc = build_scene(app);
            if let Ok(mut slot) = app.viewport.slot.lock() {
                *slot = Some(sc);
            }
            app.viewport.key = key;
            app.viewport.build_ms = crate::now_ms() - t0;
            crate::frame_watch::step(app, "model scene");
        }
        let sk = sketch_scene_key(app, &proj);
        if sk != app.viewport.sketch_key {
            let scene = build_sketch_scene(app, &proj);
            if let Ok(mut slot) = app.viewport.sketch_slot.lock() {
                *slot = Some(scene);
            }
            app.viewport.sketch_key = sk;
        }
        let hk = highlight_key(app);
        if hk != app.viewport.hl_key {
            let hl = build_highlight(app);
            if let Ok(mut slot) = app.viewport.hl_slot.lock() {
                *slot = Some(hl);
            }
            app.viewport.hl_key = hk;
        }
        crate::frame_watch::step(app, "highlight scene");
        let ppp = ui.ctx().pixels_per_point();
        let size_px = [rect.width() * ppp, rect.height() * ppp];
        let ssaa = supersampling(app, ui.ctx(), proj.vp.to_f32(), size_px);
        let cb = ViewportCallback(Frame {
            ssaa,
            product: false,
            key: app.viewport.key,
            slot: app.viewport.slot.clone(),
            sketch_key: app.viewport.sketch_key,
            sketch_slot: app.viewport.sketch_slot.clone(),
            hl_key: app.viewport.hl_key,
            hl_slot: app.viewport.hl_slot.clone(),
            pv_key: app.preview.key,
            pv_slot: app.preview.slot.clone(),
            pv_hide: app.preview.rolled_back,
            clip: section_clip(app),
            cap: {
                let [r, g, b, _] = t.section_cap.to_normalized_gamma_f32();
                [r, g, b, if section_hatch(app) { 1.0 } else { 0.0 }]
            },
            view_proj: proj.vp.to_f32(),
            back: proj.cam.back().to_f32(),
            size_px,
            images: crate::ref_images::gpu_images(app),
            analysis: analysis_uniform(app),
            analysis_colors: crate::gpu::analysis_colors(&t.analysis),
            access: crate::access::map(app),
            outline: matches!(app.ui.visual_style, 0 | 3),
            ao_radius: if app.preferences.ambient_occlusion && app.ui.visual_style != 2 {
                (model_bounds(app).diagonal() * 0.04).clamp(1e-4, 1e6) as f32
            } else {
                0.0
            },
            eye: {
                let e = proj.cam.eye().to_f32();
                [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }]
            },
            // Whole-body highlights redraw the model's own plan triangles: the hover lifts each
            // body's colour the same amount `mesh_tris` used to, and a selection replaces it.
            hover_tint: [f32::from(t.hover_face_lift) / 255.0; 3],
            selection_tint: {
                let c = t.body_selected;
                [f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0]
            },
        });
        painter.add(egui_wgpu::Callback::new_paint_callback(rect, cb));
        // The graphics card had no memory for the view's frame (#7296): say once why it looks
        // coarser.
        if !app.viewport.lowered_noted && app.viewport.draw_stats.as_ref().is_some_and(|s| s.lock().is_ok_and(|s| s.lowered)) {
            app.viewport.lowered_noted = true;
            app.set_status(app.tr("ui.gpu_memory_lowered"), false);
        }
    }
    let overlay_rect = crate::shell::bottom_overlay_rect(app, rect);
    analysis_legend(app, &painter, overlay_rect);
    crate::frame_rate::paint(app, ui, &painter, overlay_rect);

    // ---- interaction ----
    // A tool's click that moved on the way (#2448) clicks where the button is let go.
    if app.viewport.tool_click && resp.drag_stopped() {
        app.viewport.tool_click = false;
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()).filter(|p| rect.contains(*p))
            && app.tool.is_some()
        {
            crate::tools::on_hover(app, &proj, p, mods.shift);
            crate::tools::on_click(app, &proj, p, mods.shift);
        }
    }
    // A two-point shape being dragged out (#1630) follows the pointer and ends where it is let go,
    // even over the shape's own dimension box, which rides along beside the pointer.
    if let Some(from) = app.viewport.draw_drag {
        let at = ui.input(|i| i.pointer.latest_pos()).filter(|p| rect.contains(*p) && !crate::shell::covered(app, *p));
        if resp.drag_stopped() || !primary_down {
            app.viewport.draw_drag = None;
            if let Some(p) = at
                && app.tool.as_ref().is_some_and(|t| t.pts.len() == 1)
            {
                // Let go close to the press, it was a click that moved on the way (#2448): the
                // first point goes where it was let go, as for the other tools, and the shape
                // goes on by clicks.
                if p.distance(from) < DRAW_DRAG_MIN
                    && let Some(t) = app.tool.as_mut()
                {
                    t.pts.clear();
                    crate::sketch_dims::reset(t);
                }
                crate::tools::on_hover(app, &proj, p, mods.shift);
                crate::tools::on_click(app, &proj, p, mods.shift);
            }
        } else if let Some(p) = at {
            crate::tools::on_hover(app, &proj, p, mods.shift);
        }
    }
    // A Trim drag's path follows the pointer; letting go trims what it crossed. A path that
    // crossed nothing was a click that moved on the way (#2448): the release clicks.
    if app.viewport.trim_drag {
        let at = ui.input(|i| i.pointer.latest_pos()).filter(|p| rect.contains(*p));
        if resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            app.viewport.trim_drag = false;
            if !crate::sketch_tools::trim_drag_end(app)
                && let Some(p) = at
                && app.tool.is_some()
            {
                crate::tools::on_hover(app, &proj, p, mods.shift);
                crate::tools::on_click(app, &proj, p, mods.shift);
            }
        } else if let Some(p) = at {
            crate::sketch_tools::trim_drag_to(app, &proj, p);
        }
    }
    // Selection chords via resolve (add-only Shift is a preference; default both toggle).
    let held_mods = crate::tool_modifiers::Held::from_modifiers(mods);
    let (add, allow_toggle) = crate::tool_modifiers::selection_click(held_mods, app.preferences.selection_shift_add_only);
    crate::sketch_text::set_one_letter(mods.shift);
    if let Some(p) = hover.filter(|_| inside) {
        if app.tool.is_some() {
            crate::tools::on_hover(app, &proj, p, mods.shift);
        }
        // Trim also trims by press and drag across the pieces to remove (its own release handling,
        // so not a click that moved on the way below).
        if app.tool.as_ref().is_some_and(|t| t.cmd == "sketch.trim")
            && !held
            && primary_nav.is_none()
            && mode.is_none()
            && resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(a) = ui.input(|i| i.pointer.press_origin())
        {
            app.viewport.trim_drag = crate::sketch_tools::trim_drag_start(app, &proj, a);
            if app.viewport.trim_drag {
                crate::sketch_tools::trim_drag_to(app, &proj, p);
            }
        }
        // Rectangles, circles and polygons also draw by press, drag and release (#1630): the press
        // places the first point as a click there would, the release the second.
        let two_point_start = app.tool.as_ref().is_some_and(|t| matches!(t.kind, crate::tools::Kind::Draw(2)) && t.pts.is_empty());
        if two_point_start
            && !held
            && primary_nav.is_none()
            && mode.is_none()
            && resp.drag_started_by(egui::PointerButton::Primary)
            && let Some(a) = ui.input(|i| i.pointer.press_origin())
        {
            crate::tools::on_click(app, &proj, a, mods.shift);
            app.viewport.draw_drag = app.tool.as_ref().is_some_and(|t| t.pts.len() == 1).then_some(a);
        } else if app.tool.is_some()
            && !app.viewport.trim_drag
            && !held
            && primary_nav.is_none()
            && mode.is_none()
            && resp.drag_started_by(egui::PointerButton::Primary)
        {
            // Any other press of a tool that turns into a drag is a click that moved on the way
            // (#2448): its release clicks.
            app.viewport.tool_click = true;
        }
        // While acquiring a midpoint dimension's second entity, geometry takes precedence
        // over existing dimension lines ending on it. Dimension text can still supply a value.
        let midpoint_geometry = crate::dim_view::live() == Some(false)
            && app.tool.as_ref().is_some_and(|t| t.kind == crate::tools::Kind::Dimension && t.picks.len() == 1 && !t.point_picks.is_empty())
            && matches!(cand.as_ref().map(|c| &c.0), Some(Hit::SketchPoint { .. } | Hit::SketchCurve { .. }))
            && crate::dim_view::hit_text(p).is_none();
        let reference_pick = (resp.clicked() || resp.double_clicked())
            && !midpoint_geometry
            && !held
            && app.dialog.is_none()
            && crate::dim_view::pick_reference(ui.ctx(), p, resp.double_clicked());
        // Capture the press too: text editors lose focus before the click is released. A
        // running tool keeps the keyboard for its own typed values.
        if !held
            && app.tool.is_none()
            && (resp.clicked() || (resp.contains_pointer() && ui.input(|i| i.pointer.primary_pressed())))
            && let Some(dialog) = app.dialog.as_mut()
        {
            dialog.restore_value_focus = true;
        }
        if resp.clicked() && !held && !reference_pick {
            if app.tool.is_some() {
                crate::tools::on_click(app, &proj, p, mods.shift);
            } else if app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
                if let Some((hit, Some(sel))) = cand {
                    dialog_pick(app, &hit, sel);
                } else if let Some(mut dialog) = app.dialog.take() {
                    let state = crate::open_meshing::shown_world(app);
                    if let Some(sketch) = app.session.active_sketch.and_then(|id| state.sketch(id)) {
                        let (origin, direction) = proj.ray(p);
                        if let Some(point) = sketch.plane.intersect_ray(origin, direction) {
                            crate::sketch_modify_ui::pick_position(&mut dialog, &app.session, point);
                        }
                    }
                    app.dialog = Some(dialog);
                }
            } else {
                let annotation = sketch_constraint_at(app, p, cand.as_ref().map(|c| &c.0));
                if crate::sketch_text_handles::over(app, &proj, p) {
                    // A text's resize or rotation handle: the text stays selected.
                } else if !crate::sketch3d::click(app, &proj, p) {
                    match annotation {
                        Some(sel) => select(app, Some(sel), add, allow_toggle),
                        // Clicking again on the same spot selects the next thing under it (not
                        // the second click of a double-click, which has its own actions).
                        None if !add && !resp.double_clicked() && crate::select_other::cycle_click(app, &proj, p) => {}
                        None => {
                            let sel = cand.and_then(|c| c.1);
                            select(app, sel.clone(), add, allow_toggle);
                            crate::select_other::note_click(app, p, if add { None } else { sel });
                        }
                    }
                }
            }
        }
        // Double-click a dimension: edit its value in place (a finished sketch's shown dimension
        // too, without editing the sketch).
        let dim_edit = resp.double_clicked()
            && !held
            && !crate::dim_view::editing()
            && app.tool.is_none()
            && app.dialog.is_none()
            && (crate::sketch_modify_ui::double_click_pattern(app, p)
                || crate::dim_view::double_click(app, p)
                || crate::dim_view::model_double_click(app, p));
        // Double-click a text (an outline or inside a letter; the first click selected it): edit it.
        if resp.double_clicked()
            && !held
            && !dim_edit
            && app.tool.is_none()
            && app.dialog.is_none()
            && let Some(entity) = crate::sketch_text::selected_entity(app)
        {
            crate::sketch_text::open(app, &entity);
        }
        // Double-click a sketch curve: select the chain joined to it end to end.
        if resp.double_clicked()
            && !held
            && !dim_edit
            && app.tool.is_none()
            && app.dialog.is_none()
            && let Some(Hit::SketchCurve { sketch, id, .. }) = app.viewport.hover.clone()
            && app.session.active_sketch == Some(sketch)
            && let Ok(v) = app.session.execute("sketch.chain", &json!({ "curve": id }))
        {
            let items: Vec<Sel> = v["curves"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str())
                .map(|id| Sel::SketchCurve { id: id.into(), sketch: None })
                .collect();
            let _ = app.run("select.set", json!({ "items": items, "add": add }));
        }
        // Double-click an edge: select the edges that continue it smoothly (the loop).
        if resp.double_clicked()
            && !held
            && !dim_edit
            && app.tool.is_none()
            && app.dialog.is_none()
            && let Some(Hit::Edge { body, index, .. }) = app.viewport.hover.clone()
            && let Some(b) = crate::open_meshing::shown_world(app).body(&body).cloned()
        {
            let m = b.mesh();
            let items: Vec<Sel> = m
                .tangent_chain(index, 2f64.to_radians())
                .into_iter()
                .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                .collect();
            let _ = app.run("select.set", json!({ "items": items, "add": add }));
        }
        // A right-click, unless the same press opened the mouse gestures' ring.
        if resp.secondary_clicked() && delta == egui::Vec2::ZERO && !crate::mouse_gesture::took_press(app) {
            if app.tool.is_some() {
                crate::tools::finish(app);
            } else if app.dialog.is_none() {
                crate::context_menu::open(app, p, held_mods);
            }
        }
    }
    // Dragging a sketch point of the active sketch moves it; the solver keeps the constraints.
    if app.tool.is_none() && app.dialog.is_none() && primary_nav.is_none() && mode.is_none() && app.session.active_sketch.is_some() && !held {
        let on_plane = |app: &SolveApp, p: Pos2| {
            let ss = app.session.active_sketch.and_then(|sid| crate::open_meshing::shown_world(app).sketch(sid).cloned())?;
            let (o, d) = proj.ray(p);
            ss.plane.intersect_ray(o, d).map(|w| ss.plane.to_local(w))
        };
        // What a drag starts on: the 3D triad, then a dimension's text, then sketch geometry
        // (before the dimension's lines, which end on the geometry they measure), as for clicks.
        let grabbed = resp
            .drag_started_by(egui::PointerButton::Primary)
            .then(|| ui.input(|i| i.pointer.press_origin()))
            .flatten()
            .filter(|a| !crate::sketch_text_handles::drag_start(app, &proj, *a) && !crate::sketch3d::drag_start(app, &proj, *a))
            .map(|a| (a, pick(app, &proj, a).into_iter().next()));
        if let Some((a, hit)) = &grabbed
            && !crate::sketch_text_handles::dragging(app)
            && let Some(q) = on_plane(app, *a)
        {
            let on_geometry = matches!(hit, Some(Hit::SketchPoint { .. } | Hit::SketchMidpoint { .. } | Hit::SketchCurve { .. }));
            crate::dim_view::drag_start(app, *a, q, on_geometry);
        }
        if crate::sketch3d::dragging()
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(p) = hover
        {
            crate::sketch3d::drag_to(app, &proj, p);
        }
        if crate::sketch_text_handles::dragging(app)
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(p) = hover
        {
            crate::sketch_text_handles::drag_to(app, &proj, p, mods.shift);
        }
        if crate::dim_view::dragging()
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(q) = hover.and_then(|p| on_plane(app, p))
        {
            crate::dim_view::drag_to(app, q);
        }
        // A point or a curve of the sketch being edited follows a drag that starts on it.
        if let Some((a, hit)) = grabbed
            && !crate::dim_view::dragging()
            && !crate::sketch3d::dragging()
            && !crate::sketch_text_handles::dragging(app)
        {
            let active = app.session.active_sketch;
            match hit {
                Some(Hit::SketchPoint { sketch, id, .. }) if Some(sketch) == active => {
                    // A point dragged with the rest of a selection slides with it, from where it
                    // was grabbed, rather than going to the cursor.
                    let grab = if dragged_with(app, &id).is_empty() { None } else { on_plane(app, a) };
                    app.viewport.point_drag = Some((id, app.session.undo.len(), grab))
                }
                Some(Hit::SketchCurve { sketch, id, .. } | Hit::SketchMidpoint { sketch, id, .. }) if Some(sketch) == active => {
                    app.viewport.point_drag = on_plane(app, a).map(|q| (id, app.session.undo.len(), Some(q)));
                }
                _ => {}
            }
            if let Some((id, _, _)) = &app.viewport.point_drag {
                let link = app.session.active_sketch.and_then(|sid| app.session.doc.sketch(sid).ok()).and_then(|sk| {
                    let link = sk
                        .curve_index(id)
                        .and_then(|i| sk.curves.get(i))
                        .and_then(|c| c.link.as_deref())
                        .or_else(|| sk.resolve_point(id).and_then(|i| sk.points.get(i)).and_then(|p| p.link.as_deref()))?;
                    sk.link(link).filter(|l| l.kind == solidcraft_engine::sketch::LinkKind::Text).map(|l| l.id.clone())
                });
                if let Some(link) = link
                    && let Some(q) = on_plane(app, a)
                {
                    if let Some((_, _, grab)) = &mut app.viewport.point_drag {
                        *grab = Some(q);
                    }
                    if let Ok(result) = app.session.execute("edit.checkpoint", &json!({}))
                        && let Some(checkpoint) = result["checkpoint"].as_u64()
                    {
                        app.viewport.text_drag = Some((checkpoint, link));
                    }
                }
            }
        }
        if let Some((id, depth, grab)) = app.viewport.point_drag.clone()
            && (resp.dragged_by(egui::PointerButton::Primary) || resp.drag_stopped())
            && let Some(mut q) = hover.and_then(|p| on_plane(app, p))
        {
            let from = grab.unwrap_or(q);
            let with = dragged_with(app, &id);
            // Snap the dragged point (or the line's ends) to other geometry, like drawing.
            let snap = {
                let st = crate::open_meshing::shown_world(app);
                app.session.active_sketch.and_then(|s| st.sketch(s)).and_then(|ss| {
                    let sk = &ss.sketch;
                    let (mut own, mut exclude) = crate::drag_snap::moving_points(sk, &id);
                    for (o, x) in with.iter().map(|w| crate::drag_snap::moving_points(sk, w)) {
                        own.extend(o);
                        exclude.extend(x);
                    }
                    own.sort_unstable();
                    own.dedup();
                    if own.iter().any(|&p| sk.point_locked(p)) {
                        return crate::drag_snap::find(sk, &[], &[], 0.0);
                    }
                    let a = proj.to_screen(ss.plane.to_world(q))?;
                    let b = proj.to_screen(ss.plane.to_world(q + Vec2::X))?;
                    let mm_px = 1.0 / (a.distance(b) as f64).max(1e-9);
                    let d = q - from;
                    let moving: Vec<(usize, Vec2)> =
                        own.iter().filter_map(|&p| if grab.is_some() { sk.point(p).map(|x| (p, x + d)) } else { Some((p, q)) }).collect();
                    crate::drag_snap::find(sk, &moving, &exclude, mm_px)
                })
            };
            if let Some(s) = snap {
                q += s.offset;
            }
            // Each frame re-solves the sketch only (`live`); the model follows when the drag ends.
            let params = json!({"entity": id, "with": with, "from": [from.x, from.y], "to": [q.x, q.y], "live": true});
            let at = match app.session.execute("sketch.drag", &params) {
                Ok(r) => {
                    app.session.undo.truncate(depth + 1);
                    // Budget-limited progress continues even when the mouse stops. A stalled
                    // search asks for no more frames; release gets one bounded final attempt.
                    if r.get("retry").and_then(serde_json::Value::as_bool) == Some(true) && !resp.drag_stopped() {
                        ui.ctx().request_repaint();
                    }
                    r.get("at").and_then(solidcraft_engine::params::vec2).unwrap_or(q)
                }
                Err(_) => from,
            };
            // The grab stays on the geometry: what it could not follow this frame it catches up
            // with on the next one instead of lagging the cursor for the rest of the drag.
            if grab.is_some() {
                app.viewport.point_drag = Some((id, depth, Some(at)));
            }
        }
        if resp.drag_stopped() {
            crate::sketch_text_handles::drag_end(app);
            if let Some((_, depth, _)) = app.viewport.point_drag.clone() {
                // Letting go on a point, midpoint or curve adds the matching constraint, then the
                // whole model follows the drag, all in the drag's undo step.
                let _ = app.session.execute("sketch.drag_end", &json!({ "constraint": crate::drag_snap::release() }));
                app.session.undo.truncate(depth + 1);
            }
            crate::drag_snap::clear();
            app.viewport.point_drag = None;
            crate::sketch_text_move::end_drag(app);
            crate::dim_view::drag_end();
            crate::sketch3d::drag_end();
        }
    }
    // A sketch drag whose release never came (focus lost, a tool or dialog took over) ends here,
    // so the model does not stay behind its sketch until the next command. Not while CAD input
    // is held back (an issue report's capture): a release then belongs to the report, and the
    // drag continues afterwards.
    if app.viewport.point_drag.is_some() && !crate::issue_report::cad_input_blocked() && !ui.input(|i| i.pointer.primary_down()) {
        app.viewport.point_drag = None;
        crate::drag_snap::clear();
        let _ = app.session.execute("sketch.drag_end", &json!({}));
        crate::sketch_text_move::end_drag(app);
    }
    if crate::sketch_text_handles::dragging(app) && !crate::issue_report::cad_input_blocked() && !ui.input(|i| i.pointer.primary_down()) {
        crate::sketch_text_handles::drag_end(app);
    }
    // Dragging a part moves its free chain (outside sketches), or reaches within its joints
    // when the chain is anchored. Shift, Ctrl/Cmd or Alt held as the drag starts
    // draws a selection box instead, as a drag from empty space does.
    if app.tool.is_none() && app.dialog.is_none() && primary_nav.is_none() && mode.is_none() && app.session.active_sketch.is_none() && !held {
        if resp.drag_started_by(egui::PointerButton::Primary)
            // Physical Ctrl on macOS is not the primary click-selection modifier, but must
            // still keep a modified drag from moving a part.
            && !(mods.shift || mods.command || mods.ctrl || mods.alt)
            && let Some(a) = ui.input(|i| i.pointer.press_origin())
            && rect.contains(a)
            && !crate::shell::covered(app, a)
            && !axes.contains(a)
            && !crate::occurrence_drag::start(app, &proj, a)
            && !crate::joint_drag::start(app, &proj, a)
        {
            crate::drag_extrude::press(app, &proj, a);
        }
        if resp.dragged_by(egui::PointerButton::Primary)
            && let Some(p) = hover
        {
            if crate::joint_drag::active(app) {
                crate::joint_drag::to(app, &proj, p);
            } else if crate::occurrence_drag::active(app) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                crate::occurrence_drag::to(app, &proj, p);
            }
        }
    }
    if crate::joint_drag::active(app) && (resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down())) {
        crate::joint_drag::end(app);
    }
    // A drag whose release never came (focus lost, a tool or dialog took over) ends where it is.
    if crate::occurrence_drag::active(app) && (resp.drag_stopped() || !ui.input(|i| i.pointer.primary_down())) {
        crate::occurrence_drag::end(app);
    }
    // Pulling a face or profile extrudes it; the release applies.
    if resp.dragged_by(egui::PointerButton::Primary)
        && let Some(p) = hover
    {
        crate::drag_extrude::to(app, &proj, p);
    }
    if resp.drag_stopped() || !primary_down {
        crate::drag_extrude::end(app);
    }
    // Box selection: a primary drag on the model when no navigation mode is on.
    if app.tool.is_none()
        && !held
        && primary_nav.is_none()
        && mode.is_none()
        && app.viewport.point_drag.is_none()
        && !crate::joint_drag::active(app)
        && !crate::occurrence_drag::active(app)
        && !crate::drag_extrude::started(app)
        && !crate::dim_view::dragging()
        && !crate::sketch3d::dragging()
        && !crate::sketch_text_handles::dragging(app)
    {
        let origin = ui.input(|i| i.pointer.press_origin());
        if resp.dragged_by(egui::PointerButton::Primary)
            && let (Some(a), Some(b)) = (origin, hover)
            && rect.contains(a)
            && !crate::shell::covered(app, a)
            && !axes.contains(a)
        {
            app.viewport.boxsel = Some(BoxSel::from_drag(a, b));
        }
        // A shell card disables its underlying widgets. The release still ends a
        // rubber band even when egui no longer sends drag_stopped to the viewport.
        if !primary_down && !inside {
            app.viewport.boxsel = None;
        }
        if resp.drag_stopped()
            && let Some(bx) = app.viewport.boxsel.take()
        {
            box_select(app, &proj, bx, add);
        }
    } else {
        app.viewport.boxsel = None;
    }
    crate::preview::paint_sketches(app, &painter, &proj);
    crate::ref_images::show(app, ui, &painter, &proj);
    crate::dialogs_assembly::origins(app, &painter, &proj);
    crate::dialogs_assembly::joint_glyphs(app, &painter, &proj);
    overlays(app, &painter, &proj);
    crate::dim_view::show(app, ui, &painter, &proj);
    crate::sketch_text_handles::show(app, ui, &painter, &proj, hover);
    crate::sketch3d::show(app, &painter, &proj);
    crate::inference::show(app, &painter, &proj);
    crate::drag_snap::show(app, &painter, &proj);
    crate::insert_handles::show(app, ui, &proj);
    crate::sketch_transform_ui::refresh(app);
    crate::sketch_transform_ui::show(app, ui, &proj);
    hover_highlight(app, &painter, &proj);
    crate::drag_extrude::tick(app);
    crate::drag_extrude::show(app, &painter);
    points_2d(app, &painter, &proj);
    if let Some((plane, sketch)) = crate::sketch_attachment_ui::lost_guide(app)
        && let Some((low, high)) = sketch.bounds()
        && let Some(at) = proj.to_screen(plane.to_world(solidcraft_engine::geom::Vec2::new((low.x + high.x) * 0.5, high.y)))
    {
        painter.text(
            at - vec2(0.0, 12.0),
            Align2::CENTER_BOTTOM,
            "Saved sketch plane — attachment lost",
            FontId::proportional(t.font_body),
            Tokens::view().warning,
        );
    }
    if let Some(bx) = app.viewport.boxsel {
        paint_box_selection(&painter, bx, app.session.active_sketch.is_some());
    }
    if let Some(tl) = app.tool.as_ref() {
        crate::tools::preview(app, tl, &painter, &proj);
    }
    crate::canvas::show(app, ui, &painter, &proj);
    crate::sketch_tools::show(app, ui, &painter, &proj);
    crate::sketch_dims::show(app, ui, &proj);
    crate::select_other::show(app, ui.ctx());
    axis_view(app, ui, rect);
    crate::navigation_bar_view::show(app, ui, rect);
    // The notices and the agent chip first, so the 3D mouse picture can keep clear of them.
    let toasts = transient_messages(app, ui, rect);
    if app.preferences.space_mouse_in_view
        && app.space_mouse.calibrating.is_none()
        && let Some(device) = &app.space_mouse.device
    {
        use crate::prefs::SpaceMouseGhost;
        let calibration = app.preferences.space_mouse_calibrations.get(device);
        let position = app.preferences.space_mouse_ghost_position;
        let navigation = app.viewport.layout.as_ref().map(|layout| layout.navigation);
        let obstacles = navigation.into_iter().chain(toasts).collect::<Vec<_>>();
        // The Viewport positions sit in the content area (right of the browser), Left Window in the
        // corner of the whole window; both stay above the floating History card, like the legend.
        let area = if position == SpaceMouseGhost::LeftWindow { rect } else { crate::shell::content_rect(app, rect) };
        let area = crate::shell::bottom_overlay_rect(app, area);
        let picture = crate::space_mouse_model::corner_rect(area, position, &obstacles);
        crate::space_mouse_model::corner(ui, picture, calibration, app.space_mouse.raw.into());
    }
    // Last, so it sits over the overlays; only where a click would reach the model.
    let badge_at = hover.filter(|p| {
        inside
            && !navigating
            && resp.hovered()
            && ui.ctx().layer_id_at(*p).is_none_or(|layer| layer == ui.layer_id())
            && !crate::shell::covered(app, *p)
    });
    crate::tool_cursor::show(app, &painter, badge_at);
}

fn paint_box_selection(painter: &egui::Painter, bx: BoxSel, sketch: bool) {
    let t = Tokens::get();
    let col = if sketch {
        t.sketch_accent
    } else if bx.crossing {
        t.box_crossing
    } else {
        t.box_window
    };
    painter.rect_filled(bx.rect, 0.0, col.gamma_multiply(0.12));
    if bx.crossing {
        let c = [bx.rect.left_top(), bx.rect.right_top(), bx.rect.right_bottom(), bx.rect.left_bottom()];
        for i in 0..4 {
            painter.add(Shape::dashed_line(&[c[i], c[(i + 1) % 4]], Stroke::new(1.2, col), 6.0, 4.0));
        }
    } else {
        painter.rect_stroke(bx.rect, 0.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
    }
}

/// An agent's command computing off the window thread (#1989): which one, in the agent cursor's
/// colour, and a button that stops waiting for it. Centred on the navigation bar, just above it,
/// and moved up past anything it would cover (`taken`: the toasts and other controls).
fn agent_chip(app: &mut SolveApp, ui: &egui::Ui, view: Rect, taken: &[Rect]) -> Option<Rect> {
    let t = Tokens::get();
    let painter = ui.painter().clone();
    if let Some((since, label)) = app.agent_computing() {
        let seconds = ((crate::now_ms() - since) / 1000.0).floor().max(0.0);
        let computing = crate::i18n::format("dialog.computing", &[("seconds", &format!("{seconds:.0}"))]);
        let galley = painter.layout_no_wrap(format!("{label} · {computing}"), FontId::proportional(t.font_label), t.text);
        let stop = painter.layout_no_wrap(crate::i18n::tr("ui.agent_chat_stop").to_string(), FontId::proportional(t.font_label), t.text);
        let size = vec2(galley.size().x + stop.size().x + 54.0, galley.size().y.max(stop.size().y) + 8.0);
        let nav = app.viewport.layout.as_ref().map(|l| l.navigation).filter(|n| n.is_positive() && n.is_finite());
        let (x, bottom) = nav.map_or((view.center().x, view.bottom() - t.space_8), |n| (n.center().x, n.top() - t.space_8));
        let x = x.clamp(view.left() + size.x * 0.5, (view.right() - size.x * 0.5).max(view.left() + size.x * 0.5));
        let mut r = Rect::from_min_size(pos2(x - size.x * 0.5, bottom - size.y), size);
        // Each step clears one rectangle, so this ends after at most `taken.len()` moves.
        for _ in 0..=taken.len() {
            match taken.iter().filter(|o| o.expand(t.space_4).intersects(r)).map(|o| o.top()).reduce(f32::min) {
                Some(top) => r = r.translate(vec2(0.0, top - t.space_8 - r.bottom())),
                None => break,
            }
        }
        let pos = r.min;
        crate::scenario::publish_rect("agent:chip", r);
        painter.rect_filled(r, t.radius_sm, t.overlay);
        painter.rect_stroke(r, t.radius_sm, Stroke::new(1.0, crate::icons::pointer::agent()), egui::StrokeKind::Inside);
        egui::Spinner::new()
            .color(crate::icons::pointer::agent())
            .size(t.space_12)
            .paint_at(ui, Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(12.0, 12.0)));
        painter.galley(pos + vec2(26.0, 4.0), galley.clone(), t.text);
        let button = Rect::from_min_size(pos2(pos.x + galley.size().x + 34.0, r.top() + 2.0), vec2(stop.size().x + 12.0, r.height() - 4.0));
        let clicked = ui.interact(button, egui::Id::new("sc_agent_stop"), Sense::click());
        painter.rect_filled(button, t.radius_sm.saturating_sub(1), if clicked.hovered() { t.hover } else { t.overlay });
        painter.galley(button.min + vec2(6.0, 2.0), stop, t.text);
        crate::scenario::publish_handle("agent:stop", button.center());
        if clicked.clicked() {
            app.stop_agent_command();
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        return Some(r);
    }
    None
}

/// How long a status message stays in the view (milliseconds).
const STATUS_TOAST_MS: f64 = 6000.0;

/// A single visual pattern for ongoing work, the existing six-second status and the 3D mouse
/// calibration offer, stacked from the bottom of the free view. Returns the toasts' rects.
fn transient_messages(app: &mut SolveApp, ui: &egui::Ui, viewport: Rect) -> Vec<Rect> {
    // Nothing to show or time: an idle window skips the layout. The offer is checked too, as its clock
    // runs from the frames `due` sees.
    if app.status.is_none()
        && app.computing_since().is_none()
        && app.agent_computing().is_none()
        && crate::space_mouse_offer::offerable(app).is_none()
    {
        return Vec::new();
    }
    let t = Tokens::get();
    let mut shown = Vec::new();
    let mut obstacles = crate::shell::covered_rects(app);
    if let Some(layout) = &app.viewport.layout {
        obstacles.extend([layout.navigation, layout.axis_view]);
    }
    let view = crate::shell::content_rect(app, viewport);
    let mut free = crate::viewport_layout::free_rect(view, &obstacles).shrink(t.space_8);
    // The 3D mouse calibration offer: text and actions from its module, drawn by the shared toast.
    if let Some(offer) = crate::space_mouse_offer::due(app, ui.ctx()) {
        let labels: Vec<String> = offer.actions.iter().map(|action| crate::i18n::tr(action.label()).to_string()).collect();
        let labels: Vec<(&str, ToastKind)> = labels.iter().map(String::as_str).zip(offer.actions.iter().map(|action| action.kind())).collect();
        if let Some((r, clicked)) = paint_toast_with(ui, free, crate::i18n::tr("ui.3d_mouse_offer").to_string(), ToastLead::Dot(false), &labels) {
            crate::scenario::publish_rect("toast:space_mouse_offer", r);
            obstacles.push(r);
            free.max.y = r.top() - t.space_8;
            shown.push(r);
            crate::space_mouse_offer::keep(app, ui.ctx(), r);
            if let Some(action) = clicked.and_then(|i| offer.actions.get(i)) {
                crate::space_mouse_offer::act(app, ui.ctx(), &offer, *action);
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
    }
    if let Some((msg, at, error)) = app.status.clone() {
        let shown_ms = crate::now_ms() - at;
        if shown_ms < STATUS_TOAST_MS {
            if let Some(r) = paint_toast(ui, free, msg, error, false) {
                obstacles.push(r);
                crate::scenario::publish_rect("toast", r);
                free.max.y = r.top() - t.space_8;
                shown.push(r);
                // Open the run's messages from the tokenised status toast.
                crate::scenario::publish_handle("status:chip", r.center());
                let hint = if app.status_undo { crate::i18n::tr_label("Undo") } else { crate::i18n::tr("messages.chip_hint") };
                if ui.interact(r, egui::Id::new("sc_status_chip"), Sense::click()).on_hover_text(hint).clicked() {
                    if app.status_undo {
                        app.status = None;
                        let _ = app.run("edit.undo", json!({}));
                    } else {
                        app.messages.open = true;
                    }
                }
            }
            // One frame when it is due to go, not a frame every half second while it shows.
            let left_ms = STATUS_TOAST_MS - shown_ms;
            let left_ms = if (0.0..=STATUS_TOAST_MS).contains(&left_ms) { left_ms } else { STATUS_TOAST_MS };
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left_ms / 1000.0 + 0.01));
        } else {
            app.status = None;
        }
    }
    if let Some(since) = app.computing_since() {
        if let Some(r) = paint_toast(ui, free, computing_text(app, since), false, true) {
            obstacles.push(r);
            crate::scenario::publish_rect("toast:computing", r);
            if app.status.is_none() {
                crate::scenario::publish_rect("toast", r);
            }
            shown.push(r);
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    // The navigation bar is the chip's anchor, so it doesn't count as something to move past.
    let nav = app.viewport.layout.as_ref().map(|l| l.navigation);
    obstacles.retain(|o| Some(*o) != nav);
    shown.extend(agent_chip(app, ui, view, &obstacles));
    shown
}

/// What the computing toast says: the feature the evaluation has reached, as opening a design
/// shows it, or, until it knows, how long it has computed since `since` (ms).
pub(crate) fn computing_text(app: &SolveApp, since: f64) -> String {
    match app.computing_feature() {
        Some((at, total)) => crate::i18n::format("file_load.rebuilding", &[("done", &at.to_string()), ("total", &total.to_string())]),
        None => {
            let seconds = ((crate::now_ms() - since) / 1000.0).floor().max(0.0);
            crate::i18n::format("dialog.computing", &[("seconds", &format!("{seconds:.0}"))])
        }
    }
}

/// What a toast's button is for, which sets its look and where it goes: the main action first
/// (the accent button), then an alternative, then the quiet dismissals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Primary,
    Secondary,
    Dismiss,
}

/// Where each button of a toast goes, relative to the row's top left: `buttons` are (width, kind) in
/// the order given; the primary and the alternative form one group and the dismissals another.
/// Groups go side by side while they fit in `row_width` and wrap as a whole, so a button is not
/// left alone on a row while its group-mate fits beside it; a group wider than the row wraps
/// button by button.
fn toast_action_spots(buttons: &[(f32, ToastKind)], row_width: f32, gap: f32, height: f32) -> Vec<Rect> {
    let mut spots = vec![Rect::NOTHING; buttons.len()];
    let groups: [Vec<usize>; 2] = [
        (0..buttons.len()).filter(|&i| buttons[i].1 != ToastKind::Dismiss).collect(),
        (0..buttons.len()).filter(|&i| buttons[i].1 == ToastKind::Dismiss).collect(),
    ];
    let (mut x, mut y) = (0.0_f32, 0.0_f32);
    for group in groups.iter().filter(|g| !g.is_empty()) {
        let whole: f32 = group.iter().map(|&i| buttons[i].0).sum::<f32>() + gap * (group.len() - 1) as f32;
        if x > 0.0 && x + whole > row_width {
            (x, y) = (0.0, y + height + gap);
        }
        for &i in group {
            let width = buttons[i].0;
            // Only a group wider than a whole row wraps inside itself.
            if x > 0.0 && x + width > row_width.max(width) && whole > row_width {
                (x, y) = (0.0, y + height + gap);
            }
            spots[i] = Rect::from_min_size(pos2(x, y), vec2(width, height));
            x += width + gap;
        }
        x += gap;
    }
    spots
}

/// What leads a toast's text: a dot (red for an error) or a spinner.
#[derive(Clone, Copy)]
enum ToastLead {
    Dot(bool),
    Spinner,
}

fn paint_toast(ui: &egui::Ui, free: Rect, text: String, error: bool, computing: bool) -> Option<Rect> {
    let lead = if computing { ToastLead::Spinner } else { ToastLead::Dot(error) };
    paint_toast_with(ui, free, text, lead, &[]).map(|(rect, _)| rect)
}

/// The one toast painter: its text in a card at the bottom centre of `free`, led by a dot or a
/// spinner, and, below the text, a wrapping row of buttons (`actions`, kit buttons). Returns the
/// card's rect and which button was clicked, if any. With no actions the card is exactly the status
/// and computing toast it always was. The buttons are not live while an issue report holds CAD
/// input; none is laid outside the card, whose width is bounded by `free`.
fn paint_toast_with(ui: &egui::Ui, free: Rect, text: String, lead: ToastLead, actions: &[(&str, ToastKind)]) -> Option<(Rect, Option<usize>)> {
    if !free.is_finite() || !free.is_positive() || free.width() <= 40.0 {
        return None;
    }
    let t = Tokens::get();
    let painter = ui.painter().with_clip_rect(free);
    let galley = painter.layout(text, crate::fonts::medium(t.font_label), t.text, (free.width() - 40.0).min(440.0));
    // The buttons: each as wide as its text and a margin, in rows no wider than the text may be.
    let gap = t.space_8;
    let button_height = crate::ui_kit::row_height();
    let row_width = (free.width() - 40.0).clamp(0.0, 440.0);
    let sizes: Vec<(f32, ToastKind)> = actions
        .iter()
        .map(|(label, kind)| {
            let padding = if *kind == ToastKind::Primary { t.space_16 } else { t.space_12 };
            let width = painter.layout_no_wrap((*label).to_string(), crate::fonts::medium(t.font_label), t.text).size().x + 2.0 * padding;
            (width.min(row_width), *kind)
        })
        .collect();
    let spots = toast_action_spots(&sizes, row_width, gap, button_height);
    let widest = spots.iter().map(|spot| spot.right()).fold(0.0, f32::max);
    let y = spots.iter().map(|spot| spot.top()).fold(0.0, f32::max);
    let actions_height = if spots.is_empty() { 0.0 } else { y + button_height + gap };
    let content = vec2(galley.size().x.max(widest), galley.size().y + actions_height);
    let size = content + vec2(40.0, 16.0);
    if !size.is_finite() || size.x > free.width() || size.y > free.height() {
        return None;
    }
    let r = Rect::from_center_size(pos2(free.center().x, free.bottom() - size.y * 0.5), size);
    if !r.is_finite() {
        return None;
    }
    let frame = Container::new(ui.ctx(), Surface::Toast).frame(ui.ctx(), t.panel, crate::frame::Side::Left);
    Container::paint_frame(ui.painter(), frame, r.shrink(frame.stroke.width));
    let dot = pos2(r.left() + 14.0, r.center().y.min(r.top() + 8.0 + galley.size().y / 2.0));
    match lead {
        ToastLead::Spinner => egui::Spinner::new().size(12.0).color(t.text).paint_at(ui, Rect::from_center_size(dot, vec2(12.0, 12.0))),
        ToastLead::Dot(error) => drop(painter.circle_filled(dot, 3.0, if error { t.error } else { t.text })),
    }
    let text_top = r.min + vec2(28.0, 8.0);
    painter.galley(text_top, galley.clone(), t.text);
    let mut clicked = None;
    let origin = text_top + vec2(0.0, galley.size().y + gap);
    for (index, ((label, kind), spot)) in actions.iter().zip(&spots).enumerate() {
        let at = spot.translate(origin.to_vec2());
        // A UI of its own on the viewport's layer, so the toast needs no `&mut Ui`.
        let mut button_ui = egui::Ui::new(
            ui.ctx().clone(),
            egui::Id::new(("sc_toast_action", index)),
            egui::UiBuilder::new().layer_id(ui.layer_id()).max_rect(at).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        button_ui.set_clip_rect(button_ui.clip_rect().intersect(r));
        let response = button_ui
            .add_enabled_ui(!crate::issue_report::cad_input_blocked(), |ui| match kind {
                ToastKind::Primary => crate::ui_kit::primary_button(ui, label),
                ToastKind::Secondary => crate::ui_kit::button(ui, *label, crate::ui_kit::Button::Normal),
                ToastKind::Dismiss => crate::ui_kit::button(ui, *label, crate::ui_kit::Button::Small),
            })
            .inner;
        if response.clicked() {
            clicked = Some(index);
        }
    }
    Some((r, clicked))
}

/// A click without a command: select the candidate (replacing the selection, or adding /
/// toggling with modifiers); a click on nothing clears.
///
/// `toggle`: whether already-selected items may be removed by this click (Ctrl/Cmd held).
/// When `add` is true but `toggle` is false (Shift add-only preference), picked items
/// are only added.
pub(crate) fn select(app: &mut SolveApp, sel: Option<Sel>, add: bool, toggle: bool) {
    if !add {
        // A joint picked in the browser (highlighted even when hidden) lets go.
        app.tree.picked_joints.clear();
    }
    match sel {
        Some(x) => {
            let items = picked_selection(&app.session, &x);
            // Toggle-removal: all picked items are already selected and toggle is allowed.
            if add && toggle && items.iter().all(|x| app.session.selection.iter().any(|y| crate::selection::same_item(x, y))) {
                let rest: Vec<Sel> =
                    app.session.selection.iter().filter(|y| !items.iter().any(|x| crate::selection::same_item(x, y))).cloned().collect();
                let _ = app.run("select.set", json!({ "items": rest }));
            } else {
                let _ = app.run("select.set", json!({"items": items, "add": add}));
                crate::browser::reveal_pick(app);
            }
        }
        None if !add => {
            let _ = app.run("select.clear", json!({}));
        }
        None => {}
    }
}

/// The rest of the selected sketch geometry when the point or curve `id` is part of it (#2621):
/// a drag that starts on a selected entity moves the whole selection with it.
fn dragged_with(app: &SolveApp, id: &str) -> Vec<String> {
    fn geometry(s: &Sel) -> Option<&str> {
        match s {
            Sel::SketchPoint { id } | Sel::SketchCurve { id, .. } => Some(id.as_str()),
            _ => None,
        }
    }
    let selected = || app.session.selection.iter().filter_map(geometry);
    if !selected().any(|s| s == id) {
        return Vec::new();
    }
    let rest: Vec<String> = selected().filter(|s| *s != id).map(str::to_owned).collect();
    // A curve's own points, selected with it by a window, are not a group: a circle grabbed on
    // its edge still takes its radius from the drag.
    let st = app.session.world_state();
    let own: Vec<&str> = app
        .session
        .active_sketch
        .and_then(|s| st.sketch(s))
        .and_then(|ss| {
            let sk = &ss.sketch;
            let c = sk.curves.get(sk.curve_index(id)?)?;
            Some(c.kind.point_ids().into_iter().filter_map(|p| sk.points.get(p).map(|p| p.id.as_str())).collect())
        })
        .unwrap_or_default();
    if rest.iter().all(|r| own.contains(&r.as_str())) {
        return Vec::new();
    }
    rest
}

/// What a click on `sel` picks: its pieces ([`grouped_selection`]), and for a letter of a sketch
/// text the whole text, unless Shift is held (Shift picks the one letter).
pub(crate) fn picked_selection(s: &solidcraft_engine::Session, sel: &Sel) -> Vec<Sel> {
    if let Sel::Profile { sketch, index } = sel
        && !crate::sketch_text::one_letter()
        && let Some(letters) = crate::sketch_text::text_profiles(s, *sketch, *index)
    {
        return letters.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
    }
    grouped_selection(s, sel)
}

/// Expand a visible rim or surface into the kernel pieces a command needs. The original
/// face pick keeps its point (used by tools such as Hole and construction geometry).
pub(crate) fn grouped_selection(s: &solidcraft_engine::Session, sel: &Sel) -> Vec<Sel> {
    let st = s.world_state();
    match sel {
        Sel::Edge { body, index, .. } => {
            if let Some(b) = st.body(body) {
                let m = b.mesh();
                return m
                    .circular_rim(*index)
                    .into_iter()
                    .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                    .collect();
            }
        }
        Sel::Face { body, index, .. } => {
            if let Some(b) = st.body(body) {
                let m = b.mesh();
                let mut patch = m.surface_patch(*index);
                patch.sort_by_key(|i| usize::from(i != index));
                return patch
                    .into_iter()
                    .filter_map(|i| {
                        if i == *index {
                            return Some(sel.clone());
                        }
                        m.triangles.iter().zip(&m.tri_face).find_map(|(t, f)| {
                            if *f as usize != i {
                                return None;
                            }
                            let [a, b, c] = m.tri(t)?;
                            Some(Sel::Face { body: body.clone(), index: i, point: (a + b + c) / 3.0 })
                        })
                    })
                    .collect();
            }
        }
        _ => {}
    }
    vec![sel.clone()]
}

/// Measure keeps one entry per visible item, rather than using its two slots for kernel halves.
pub(crate) fn measurement_selection(s: &solidcraft_engine::Session, sel: &Sel) -> Sel {
    let pieces = grouped_selection(s, sel);
    let mut representative = sel.clone();
    match &mut representative {
        Sel::Edge { index, .. } => {
            if let Some(Sel::Edge { index: i, .. }) = pieces.first() {
                *index = *i;
            }
        }
        Sel::Face { index, .. } => {
            if let Some(i) = pieces
                .iter()
                .filter_map(|p| match p {
                    Sel::Face { index, .. } => Some(*index),
                    _ => None,
                })
                .min()
            {
                *index = i;
            }
        }
        _ => {}
    }
    representative
}

/// What a box selects: sketch entities while sketching, otherwise what the open dialog's
/// active input takes (edges, faces or bodies), otherwise bodies.
fn box_select(app: &mut SolveApp, proj: &Proj, bx: BoxSel, add: bool) {
    use crate::selection::{BODIES, EDGES, FACES, PROFILES};
    // While OK computes, the dialog's inputs hold.
    if app.applying.is_some() {
        return;
    }
    let st = crate::open_meshing::shown_world(app);
    let mut out: Vec<Sel> = Vec::new();
    let to2 = |pts: &[Vec3]| -> Vec<Pos2> { pts.iter().filter_map(|p| proj.to_screen(*p)).collect() };
    let sketch_dialog = app.dialog.as_ref().is_some_and(|d| matches!(d.kind, crate::dialogs::Kind::SketchModify(_)));
    let accepts = app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept);
    if (app.dialog.is_none() || sketch_dialog)
        && let Some(ss) = app.session.active_sketch.and_then(|sid| st.sketch(sid))
    {
        // What the selection filter lets through (window: wholly inside; crossing: touching).
        let off = |k: &str| app.ui.pick_off.iter().any(|x| x == k);
        for (i, c) in ss.sketch.curves.iter().enumerate().filter(|_| !off("sketch") && !off("sketch_curves")) {
            if accepts
                .is_some_and(|a| a & crate::selection::CURVES == 0 && (a & crate::selection::AXES == 0 || !matches!(c.kind, CurveKind::Line { .. })))
            {
                continue;
            }
            if !solidcraft_engine::view::sketch_curve_visible(&ss.sketch, i) {
                continue;
            }
            let sampled = if bx.crossing {
                crate::sketch_display::curve_sample(app, ss.feature, i, &ss.plane, proj)
            } else {
                std::sync::Arc::new(crate::sketch_display::curve_unclipped(&ss.sketch, i, &ss.plane, proj, app.viewport.pixels_per_point()))
            };
            let parts: Vec<Vec<Pos2>> = sampled.parts.iter().map(|part| to2(part)).collect();
            let takes = if bx.crossing {
                parts.iter().any(|part| bx.takes(part))
            } else {
                sampled.quality == solidcraft_engine::render::SamplingQuality::Complete
                    && !parts.is_empty()
                    && parts.iter().all(|part| bx.takes(part))
            };
            if takes {
                out.push(Sel::SketchCurve { id: c.id.clone(), sketch: None });
            }
        }
        let visible = solidcraft_engine::view::SketchPointVisibility::new(&ss.sketch);
        for (i, p) in ss.sketch.points.iter().enumerate().filter(|_| !off("sketch") && !off("sketch_points")) {
            if accepts.is_none_or(|a| a & crate::selection::SKETCH_POINTS != 0)
                && i != 0
                && visible.takes(p)
                && bx.takes(&to2(&[ss.plane.to_world(p.pos)]))
            {
                out.push(Sel::SketchPoint { id: p.id.clone() });
            }
        }
        if !sketch_dialog && !off("dimensions") {
            for (id, r) in crate::dim_view::text_rects() {
                if bx.takes(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]) {
                    out.push(Sel::SketchConstraint { id });
                }
            }
        }
        if !sketch_dialog && !off("constraints") {
            for (id, c) in crate::sketch_tools::glyph_positions() {
                if bx.takes(&[c]) {
                    out.push(Sel::SketchConstraint { id });
                }
            }
        }
    } else {
        let accept = app.dialog.as_ref().and_then(|d| d.active_input()).map(|i| i.accept).unwrap_or(BODIES);
        // Bulk body selection follows the active component, just like its unfaded bodies.
        // Keep other dialogs and Move's reference inputs free to pick across components.
        let scoped_bodies = accept == BODIES && app.dialog.as_ref().is_none_or(|d| matches!(d.kind, crate::dialogs::Kind::Move { .. }));
        let active_bodies =
            app.session.active_occurrence_path.as_ref().filter(|_| scoped_bodies).and_then(|p| app.session.occurrence_bodies(p, true).ok());
        for b in &st.bodies {
            if !app.session.body_visible(&b.name) || (scoped_bodies && body_faded(app, b, &active_bodies)) {
                continue;
            }
            let m = b.mesh();
            if accept & EDGES != 0 {
                for (ei, e) in m.edges.iter().enumerate() {
                    let rim = m.circular_rim(ei);
                    let takes = if bx.crossing { bx.takes(&to2(e)) } else { rim.iter().all(|&i| m.edges.get(i).is_some_and(|p| bx.takes(&to2(p)))) };
                    if edge_pickable(app, &m, ei) && takes {
                        out.push(Sel::Edge { body: b.name.clone(), index: ei, point: polyline_mid(e) });
                    }
                }
            } else if accept & FACES != 0 {
                for f in 0..b.body.face_count() {
                    let mut pts: Vec<Vec3> = Vec::new();
                    for e in m.face_edges(u32::try_from(f).unwrap_or(u32::MAX)) {
                        pts.extend(m.edges.get(e).into_iter().flatten().copied());
                    }
                    let point = m
                        .triangles
                        .iter()
                        .zip(&m.tri_face)
                        .find(|(_, tf)| **tf as usize == f)
                        .and_then(|(t, _)| m.tri(t))
                        .map(|[a, bb, c]| (a + bb + c) / 3.0);
                    let takes = bx.takes(&to2(&pts))
                        && (bx.crossing
                            || m.surface_patch(f).iter().all(|&face| {
                                m.face_edges(u32::try_from(face).unwrap_or(u32::MAX))
                                    .iter()
                                    .all(|&e| m.edges.get(e).is_some_and(|p| bx.takes(&to2(p))))
                            }));
                    if let Some(point) = point
                        && takes
                    {
                        out.push(Sel::Face { body: b.name.clone(), index: f, point });
                    }
                }
            } else if accept & BODIES != 0 {
                let crossing_hit = m.edges.iter().any(|e| bx.takes(&to2(e)));
                let all_in = m.edges.iter().all(|e| bx.takes(&to2(e)));
                if (bx.crossing && crossing_hit) || (!bx.crossing && all_in && !m.edges.is_empty()) {
                    out.push(Sel::Body { name: b.name.clone() });
                }
            } else if accept & PROFILES != 0 {
                for sid in visible_sketches(app) {
                    let Some(ss) = st.sketch(sid) else { continue };
                    for (pi, p) in ss.profiles.iter().enumerate() {
                        let parts = crate::sketch_display::boundary_parts(&p.region.outer, &ss.plane, proj, app.viewport.pixels_per_point());
                        let takes = if bx.crossing {
                            parts.iter().any(|part| bx.takes(&to2(part)))
                        } else {
                            crate::sketch_display::profile_region(app, sid, pi, &ss.plane, proj).as_ref().as_ref().is_some_and(|display| {
                                let points: Vec<_> = display.outer.segs.iter().map(|segment| ss.plane.to_world(segment.start())).collect();
                                bx.takes(&to2(&points))
                            })
                        };
                        if takes {
                            out.push(Sel::Profile { sketch: sid, index: pi });
                        }
                    }
                }
                break;
            }
        }
    }
    let mut grouped = Vec::new();
    for item in out {
        for piece in grouped_selection(&app.session, &item) {
            if !grouped.iter().any(|x| crate::selection::same_item(x, &piece)) {
                grouped.push(piece);
            }
        }
    }
    let out = grouped;
    if let Some(mut d) = app.dialog.take() {
        d.take_box(&app.session, out, add);
        app.dialog = Some(d);
        return;
    }
    let items = serde_json::to_value(&out).unwrap_or_default();
    let _ = app.run("select.set", json!({"items": items, "add": add}));
}

/// Whether the origin point is drawn (the Origin shown and its point not hidden).
fn origin_point_shown(app: &SolveApp) -> bool {
    app.origin_visible() && !app.session.visibility.hidden_origin.iter().any(|h| h == "O")
}

/// Screen-space dots: the origin point, and hovered or selected body vertices.
fn points_2d(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    if origin_point_shown(app)
        && let Some(o) = proj.to_screen(Vec3::ZERO)
    {
        painter.circle(o, 4.5, t.origin_point, Stroke::new(1.0, Color32::from_gray(110)));
    }
    for x in app.highlighted() {
        if let Sel::Vertex { point, .. } = x
            && let Some(p) = proj.to_screen(point)
        {
            painter.circle(p, 4.0, t.sel_vertex, Stroke::new(1.5, t.sel_edge));
        }
    }
    if let Some(Hit::Vertex { point, .. }) = &app.viewport.hover
        && let Some(p) = proj.to_screen(*point)
    {
        painter.circle(p, 4.5, t.hover_edge_halo, Stroke::new(1.5, t.hover_edge));
    }
}

/// A grid line's colour, as faint as the fade makes it.
fn grid_colour(c: [u8; 4], alpha: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (f64::from(c[3]) * f64::from(alpha)).round().clamp(0.0, 255.0) as u8]
}

/// Where the ground grid is centred: where the view's axis (`back`, through the camera's target
/// `tf`) meets the plane through the origin with normal `n`, so the middle of the view has grid
/// (#3087). At a grazing slant that point runs far off, and the ground in front of the camera
/// would have none: it stays within `ext` (the grid's half-size) of the point under the target.
fn grid_centre(tf: Vec3, back: Vec3, n: Vec3, ext: f64) -> Vec3 {
    let under = tf - n * tf.dot(n);
    let slant = back.dot(n);
    if slant.abs() <= 1e-3 {
        return under;
    }
    let along = tf - back * (tf.dot(n) / slant) - under;
    let len = along.len();
    if len > ext && len > 0.0 { under + along * (ext / len) } else { under + along }
}

/// Hover feedback for the active sketch's points and curves (drawn on top of everything).
fn hover_highlight(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let col = Tokens::get().hover_profile_edge;
    let st = crate::open_meshing::shown_world(app);
    match &app.viewport.hover {
        // Trim shows the piece it would take away instead (`sketch_tools::preview`, #3052).
        Some(Hit::SketchCurve { sketch, id, .. }) if crate::sketch_tools::trim_hover_piece(app, *sketch, id).is_some() => {}
        Some(Hit::SketchCurve { sketch, id, .. }) => {
            if let Some(ss) = st.sketch(*sketch)
                && let Some(ci) = ss.sketch.curve_index(id)
            {
                for part in &crate::sketch_display::curve_sample(app, *sketch, ci, &ss.plane, proj).parts {
                    let mode = ss.sketch.curves.get(ci).map_or(0, |c| if c.centerline { 2 } else { u8::from(c.construction) });
                    crate::tools::paint_sketch_path(part, mode, painter, proj, Stroke::new(3.0, col));
                }
            }
        }
        Some(Hit::SketchMidpoint { sketch, at, .. }) => {
            if let Some(ss) = st.sketch(*sketch)
                && let Some(p) = proj.to_screen(ss.plane.to_world(*at))
            {
                midpoint_marker(painter, p, col);
            }
        }
        Some(Hit::SketchPoint { sketch, at, .. }) => {
            if let Some(ss) = st.sketch(*sketch)
                && let Some(p) = proj.to_screen(ss.plane.to_world(*at))
            {
                painter.circle_stroke(p, 6.0, Stroke::new(2.0, col));
            }
        }
        _ => {}
    }
    // The centre of the circle or arc hovered last: a cross in a ring, where a click takes it.
    if let Some((sketch, id)) = &app.viewport.woken_centre
        && !matches!(&app.viewport.hover, Some(Hit::SketchPoint { id: h, .. }) if h == id)
        && let Some(ss) = st.sketch(*sketch)
        && let Some(at) = ss.sketch.resolve_point(id).and_then(|i| ss.sketch.point(i))
        && let Some(p) = proj.to_screen(ss.plane.to_world(at))
    {
        let stroke = Stroke::new(1.5, col);
        painter.circle_stroke(p, 5.0, stroke);
        painter.line_segment([p - vec2(4.0, 0.0), p + vec2(4.0, 0.0)], stroke);
        painter.line_segment([p - vec2(0.0, 4.0), p + vec2(0.0, 4.0)], stroke);
        crate::scenario::publish_handle("woken_centre", p);
    }
}

/// How near the pointer must come to the woken centre of a circle or arc (screen points),
/// against 8 for other sketch points.
const WOKEN_CENTRE_PX: f32 = 16.0;

/// Hovering a circle's or an arc's edge in the sketch being edited wakes its centre: drawn, and
/// picked from further away, so a line or a dimension is easily taken to it, and it stays awake
/// while the pointer crosses the circle's inside to it. Hovering another curve or point (but
/// that centre) puts it back to sleep, as does leaving the sketch.
fn wake_centre(app: &mut SolveApp, hits: &[Hit]) {
    let Some(active) = app.session.active_sketch else {
        app.viewport.woken_centre = None;
        return;
    };
    let first = hits.iter().find(|h| matches!(h, Hit::SketchPoint { sketch, .. } | Hit::SketchCurve { sketch, .. } if *sketch == active));
    let st = crate::open_meshing::shown_world(app);
    match first {
        Some(Hit::SketchCurve { id, .. }) => {
            let centre = st.sketch(active).and_then(|ss| {
                let sk = &ss.sketch;
                let c = match sk.curves.get(sk.curve_index(id)?)?.kind {
                    CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => c,
                    _ => return None,
                };
                Some(sk.points.get(c)?.id.clone())
            });
            app.viewport.woken_centre = centre.map(|id| (active, id));
        }
        Some(Hit::SketchPoint { id, .. }) if app.viewport.woken_centre.as_ref().is_some_and(|(_, w)| w == id) => {}
        Some(_) => app.viewport.woken_centre = None,
        None => {
            if app.viewport.woken_centre.as_ref().is_some_and(|(k, _)| *k != active) {
                app.viewport.woken_centre = None;
            }
        }
    }
}

/// A midpoint stays attached to its edge; hover and selection share the same marker.
fn midpoint_marker(painter: &egui::Painter, at: Pos2, color: Color32) {
    let triangle = vec![at + vec2(0.0, -6.0), at + vec2(6.0, 5.0), at + vec2(-6.0, 5.0)];
    painter.add(Shape::closed_line(triangle, Stroke::new(2.0, color)));
    painter.text(at + vec2(10.0, -8.0), Align2::LEFT_BOTTOM, "MID", FontId::proportional(Tokens::get().font_button), color);
}

/// The standalone points of the finished sketches shown (placed with Point, not a curve's
/// ends or centres), in world space: they can still be picked (a hole's centre), so they are
/// drawn too (#3267). Not the sketch's origin, projected points, or a sketch's points while it
/// hides them.
pub(crate) fn standalone_points(app: &SolveApp) -> Vec<Vec3> {
    let st = app.session.world_state();
    let mut out = Vec::new();
    for sid in visible_sketches(app) {
        if app.session.active_sketch == Some(sid) {
            continue;
        }
        let Some(ss) = st.sketch(sid) else { continue };
        let sk = &ss.sketch;
        if sk.view.hide_points {
            continue;
        }
        let used: std::collections::HashSet<usize> = sk.curves.iter().flat_map(|c| c.kind.point_ids()).collect();
        let visible = solidcraft_engine::view::SketchPointVisibility::new(sk);
        for (i, p) in sk.points.iter().enumerate().skip(1) {
            if !used.contains(&i) && p.link.is_none() && visible.takes(p) {
                out.push(ss.plane.to_world(p.pos));
            }
        }
    }
    out
}

/// Sketch points and the status of the active sketch (constraint glyphs: `sketch_tools`).
fn overlays(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let t = Tokens::get();
    let st = crate::open_meshing::shown_world(app);
    for item in &app.session.selection {
        if let Sel::ExternalSketchPoint { sketch, id } = item
            && let Some(ss) = st.sketch(*sketch)
            && let Some(point) = ss.sketch.resolve_point(id).and_then(|i| ss.sketch.point(i))
            && let Some(pos) = proj.to_screen(ss.plane.to_world(point))
        {
            painter.circle_stroke(pos, 6.0, Stroke::new(2.0, t.sel_edge));
        }
    }
    for p in standalone_points(app) {
        if let Some(sp) = proj.to_screen(p) {
            painter.rect_filled(Rect::from_center_size(sp, vec2(4.0, 4.0)), 0.0, t.sketch);
        }
    }
    let Some(sid) = app.session.active_sketch else { return };
    let Some(ss) = st.sketch(sid) else { return };
    let sk = &ss.sketch;
    let locked = sk.locked_points();
    let virtual_corners: Vec<usize> = sk.virtual_corners().into_iter().map(|(i, _)| i).collect();
    let visible = solidcraft_engine::view::SketchPointVisibility::new(sk);
    for (i, p) in sk.points.iter().enumerate() {
        if !visible.takes(p) {
            continue;
        }
        let hidden = (sk.view.hide_points && i != 0) || (sk.view.hide_projected && p.link.is_some());
        let Some(sp) = proj.to_screen(ss.plane.to_world(p.pos)).filter(|_| !hidden) else { continue };
        let det = ss.report.point_determined.get(i).copied().unwrap_or(false);
        let c = if p.link.is_none() && locked.get(i).copied().unwrap_or(false) {
            t.sketch_locked
        } else if det {
            t.sketch_point
        } else {
            t.sketch_point_free
        };
        if i == 0 {
            painter.circle(sp, 3.5, Color32::from_rgb(240, 200, 60), Stroke::new(1.0, t.body_edge));
        } else if virtual_corners.binary_search(&i).is_ok() {
            // A virtual sharp corner: an open square, it is not on any drawn curve.
            painter.rect_stroke(Rect::from_center_size(sp, vec2(6.0, 6.0)), 0.0, Stroke::new(1.2, c), egui::StrokeKind::Middle);
        } else {
            painter.rect_filled(Rect::from_center_size(sp, vec2(5.0, 5.0)), 0.0, c);
        }
    }
    // The markers and the status line are drawn straight onto the view: its side's colours (#4146).
    let v = Tokens::view();
    for selected in &app.session.selection {
        if let Sel::SketchMidpoint { id } = selected
            && let Some(line) = sk.curve_index(id)
            && let Some(at) = crate::selection::line_midpoint(sk, line)
            && let Some(screen) = proj.to_screen(ss.plane.to_world(at))
        {
            midpoint_marker(painter, screen, v.sel_edge);
        }
    }
    loose_end_markers(sk, &ss.ends, &ss.plane, painter, proj, &v);
    // Sketch status.
    if let Some(r) = app.viewport.layout.as_ref().map(|l| l.content) {
        let msg = if !ss.report.ok() {
            format!("{} — constraints conflict", ss.name)
        } else if ss.report.dof == 0 {
            format!("{} — fully constrained", ss.name)
        } else {
            format!("{} — {} degrees of freedom", ss.name, ss.report.dof)
        };
        let c = if ss.report.ok() { v.sketch_accent } else { v.error };
        painter.text(pos2(r.left() + 12.0, r.top() + 12.0), Align2::LEFT_TOP, msg, FontId::proportional(t.font_button), c);
    }
}

/// Where a profile can fail: a ring round each open end (it touches no other curve) and a
/// diamond round each untied one (it touches another curve only by position), told apart by
/// shape. Both are warnings, not errors: an open sketch (a sweep path, an axis) is valid.
fn loose_end_markers(
    sk: &solidcraft_engine::sketch::Sketch,
    ends: &solidcraft_engine::sketch::EndReport,
    plane: &solidcraft_engine::geom::Plane,
    painter: &egui::Painter,
    proj: &Proj,
    t: &Tokens,
) {
    let screen = |i: usize| sk.point(i).and_then(|p| proj.to_screen(plane.to_world(p)));
    for sp in ends.open.iter().filter_map(|&i| screen(i)) {
        painter.circle_stroke(sp, 7.0, Stroke::new(1.8, t.warning));
    }
    for sp in ends.untied.iter().filter_map(|u| screen(u.point)) {
        let r = 7.5;
        let diamond = vec![pos2(sp.x, sp.y - r), pos2(sp.x + r, sp.y), pos2(sp.x, sp.y + r), pos2(sp.x - r, sp.y)];
        painter.add(egui::Shape::closed_line(diamond, Stroke::new(1.8, t.warning)));
    }
}

/// The colours of a headless picture of the model (`ui.render`, the agent's view image) in the
/// dark or the light theme and a colour vision mode, from the tokens the window draws with, so the
/// picture matches the window. The light theme's tokens (Off) are the engine's own colours, so a light
/// render is unchanged.
pub(crate) fn scene_palette(dark: bool, mode: crate::theme::ColorVision) -> solidcraft_engine::view::Palette {
    let t = Tokens::themed(dark, mode);
    let rgb = |c: Color32| Rgb(c.r(), c.g(), c.b());
    solidcraft_engine::view::Palette {
        background: (rgb(t.viewport_top), rgb(t.viewport_bottom)),
        grid: rgb(t.grid),
        grid_major: rgb(t.grid_major),
        edge: rgb(t.body_edge),
        body: rgb(t.body),
        body_selected: rgb(t.body_selected),
        sketch: rgb(t.sketch),
        sketch_fixed: rgb(t.sketch_fixed),
        sketch_done: rgb(t.sketch_fixed),
        sketch_locked: rgb(t.sketch_locked),
        sketch_projected: rgb(t.sketch_projected),
        sketch_lost: rgb(t.sketch_lost),
        sketch_construction: rgb(t.sketch_construction),
        sketch_centerline: rgb(t.sketch_centerline),
        axes: t.axes().map(rgb),
        cap: rgb(t.section_cap),
        analysis: t.analysis,
    }
}

pub(crate) fn refresh_layout(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some(rect) = app.viewport.rect {
        let docks = crate::shell::covered_rects(app);
        let mut content = crate::shell::content_rect(app, rect);
        if crate::render_ui::focused(app) {
            // Focus releases dock reservations while keeping controls below the stage bar.
            content = rect;
            for card in &app.shell.cards {
                content.min.y = (card.bottom() + Tokens::get().space_8).clamp(content.top(), content.bottom());
            }
        }
        // The full-height Assistant reserves control space, while rendering and picking
        // keep the original viewport projection. Ordinary feature dialogs do not move controls.
        if app.ui.layout == crate::shell::LayoutMode::Floating
            && crate::agent_chat::shown(app)
            && app.agent_chat.open
            && !app.preferences.agent_chat_popped
            && !app.preferences.agent_chat_minimized
            && let Some(chat) = app.agent_chat.panel_rect.filter(|r| r.is_finite() && r.is_positive())
        {
            if app.preferences.agent_chat_side == "left" {
                content.min.x = (chat.right() + crate::shell::SHELL_GAP).clamp(content.left(), content.right());
            } else {
                content.max.x = (chat.left() - crate::shell::SHELL_GAP).clamp(content.left(), content.right());
            }
        }
        // OK and Cancel in the view's corner (#6709) take the top right; the axis view goes below.
        let corner = crate::view_confirm::size(app);
        let style = app.preferences.navigator_style;
        app.viewport.layout = Some(if crate::render_workspace::active(app) {
            let navigation = if app.ui.show_navigation_bar { crate::navigation_bar_view::measured(app, ctx, content).rect } else { Rect::NOTHING };
            if app.preferences.render_inspector_side == "left" {
                crate::viewport_layout::Layout::product_left(rect, content, &docks, ctx.pixels_per_point(), style, corner, navigation)
            } else {
                crate::viewport_layout::Layout::product(rect, content, &docks, ctx.pixels_per_point(), style, corner, navigation)
            }
        } else {
            let content = if app.ui.layout == crate::shell::LayoutMode::Docked { rect } else { content };
            let navigation = if app.ui.show_navigation_bar { crate::navigation_bar_view::measured(app, ctx, content).rect } else { Rect::NOTHING };
            crate::viewport_layout::Layout::with_content(rect, content, &docks, ctx.pixels_per_point(), style, corner, navigation)
        });
    }
}

/// Orientation controls share the layout reservation used by model fitting.
fn axis_center(app: &SolveApp, rect: Rect) -> Pos2 {
    let c = axis_rect(app, rect).center();
    // The compass sits a little higher in its reserved box (less margin above, the same on the
    // right); the box itself is unchanged.
    if app.preferences.navigator_style == crate::prefs::NavigatorStyle::Axes { c - vec2(0.0, AXIS_CENTER_LIFT) } else { c }
}

/// How far the signed-axis compass is lifted inside its reserved box.
pub(crate) const AXIS_CENTER_LIFT: f32 = 4.0;

pub(crate) fn axis_rect(app: &SolveApp, rect: Rect) -> Rect {
    app.viewport.layout.as_ref().filter(|l| l.viewport == rect).map(|l| l.axis_view).unwrap_or_else(|| {
        let size = axis_view_size(app.preferences.navigator_style);
        Rect::from_min_size(pos2(rect.right() - size, rect.top()), vec2(size, size))
    })
}

const AXIS_LENGTH: f64 = 38.0;
/// The artwork sits inside the pointer reach so the compass keeps breathing room in its circle.
const AXIS_ART_SCALE: f64 = 0.84;
use crate::icons::navigation::AXIS_RADIUS;

/// Six signed world axes, sorted back to front so overlapping tips have one owner.
fn axis_tips(cam: &Camera, c: Pos2) -> Vec<(&'static str, Vec3, Pos2, f64)> {
    let (r, u, b) = cam.basis();
    let mut tips: Vec<_> = [("x", Vec3::X), ("-x", -Vec3::X), ("y", Vec3::Y), ("-y", -Vec3::Y), ("z", Vec3::Z), ("-z", -Vec3::Z)]
        .into_iter()
        .map(|(name, d)| {
            (name, d, c + vec2((d.dot(r) * AXIS_LENGTH * AXIS_ART_SCALE) as f32, -(d.dot(u) * AXIS_LENGTH * AXIS_ART_SCALE) as f32), d.dot(b))
        })
        .collect();
    tips.sort_by(|a, b| a.3.total_cmp(&b.3));
    tips
}

fn axis_hit(tips: &[(&str, Vec3, Pos2, f64)], p: Pos2) -> Option<usize> {
    tips.iter().enumerate().rev().find(|(_, (_, _, at, _))| at.distance(p) <= AXIS_RADIUS).map(|(i, _)| i)
}

/// Dragging anywhere on the axis gizmo orbits about the view centre. A stationary press
/// on a tip chooses its view. Floating windows own their input over the gizmo.
fn axis_view_orbit(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) -> bool {
    let (pressed, down, decided, origin, pos, shift) = ui.input(|i| {
        let p = &i.pointer;
        (p.primary_pressed(), p.primary_down(), p.is_decidedly_dragging(), p.press_origin(), p.interact_pos(), i.modifiers.shift)
    });
    if pressed && ui.is_enabled() && ui.rect_contains_pointer(rect) && !pos.is_some_and(|p| crate::shell::covered(app, p)) {
        app.viewport.axis_drag = origin.filter(|p| match app.preferences.navigator_style {
            crate::prefs::NavigatorStyle::Axes => axis_rect(app, rect).contains(*p),
            crate::prefs::NavigatorStyle::Cube => {
                navigation_cube::drag_origin(&app.cam, axis_center(app, rect), *p, axis_rect(app, rect).width() / navigation_cube::DESIGN_SIZE)
            }
        });
        if app.viewport.axis_drag.is_some() {
            app.viewport.orbit_free = None;
        }
    }
    if !down && app.viewport.axis_drag.take().is_some() {
        app.viewport.orbit_free = None;
    }
    let Some(last) = app.viewport.axis_drag.filter(|_| decided) else { return false };
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    if let Some(p) = pos {
        app.cancel_view_animation();
        orbit_drag(app, rect, f64::from(p.x - last.x), f64::from(p.y - last.y), None, shift, false);
        app.viewport.axis_drag = Some(p);
        ui.ctx().request_repaint();
    }
    true
}

pub(crate) fn axis_view(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    match app.preferences.navigator_style {
        crate::prefs::NavigatorStyle::Axes => signed_axis_view(app, ui, rect),
        crate::prefs::NavigatorStyle::Cube => navigation_cube::show(app, ui, rect),
    }
}

fn signed_axis_view(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    if !axis_rect(app, rect).is_positive() || !axis_rect(app, rect).is_finite() {
        app.viewport.axis_drag = None;
        crate::scenario::publish_count("axis_view_highlights", 0.0);
        return;
    }
    crate::scenario::publish_rect("axis_view:outer", axis_rect(app, rect));
    let t = Tokens::get();
    let c = axis_center(app, rect);
    let orbiting = axis_view_orbit(app, ui, rect);
    let tips = axis_tips(&app.cam, c);
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.is_enabled() && ui.rect_contains_pointer(rect) && !crate::shell::covered(app, *p));
    let hovered = if orbiting { None } else { hover.and_then(|p| axis_hit(&tips, p)) };
    let painter = ui.painter_at(rect);
    // Anywhere in the compass circle drags the view; the circle turns fully opaque under the pointer.
    let in_circle = hover.is_some_and(|p| p.distance(c) <= 49.5);
    let fill = if in_circle || orbiting { 1.0 } else { 0.35 };
    painter.circle(c, 49.5, t.overlay.gamma_multiply(fill), Stroke::new(1.0, t.border.gamma_multiply(0.75)));
    for (i, (name, d, at, depth)) in tips.iter().enumerate() {
        if i == 3 {
            // The origin dot sits behind the three front axes and over the three back ones.
            painter.circle(c, 3.6, t.text.gamma_multiply(0.9), Stroke::new(1.4, t.panel));
        }
        crate::icons::navigation::axis_tip(&painter, c, name, d, *at, *depth, hovered == Some(i), t);
        crate::scenario::publish_handle(&format!("axis_view:{name}"), *at);
    }
    crate::scenario::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    if let Some(i) = hovered
        && let Some((name, d, _, _)) = tips.get(i)
    {
        crate::scenario::publish_handle(&format!("axis_view_hover:{name}"), hover.unwrap_or(c));
        ui.interact(Rect::from_center_size(hover.unwrap_or(c), vec2(2.0, 2.0)), ui.id().with("axis_view_tip"), Sense::hover()).on_hover_text(
            format!(
                "View from {}. Click again to reverse. Drag to orbit{}.",
                name.to_uppercase(),
                if app.preferences.free_orbit { "" } else { "; hold Shift to snap" }
            ),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked()) {
            let frame = app.cam.to_frame(*d);
            let dir = if app.cam.back().dot(*d) > 0.9999 { -*d } else { *d };
            let mut to = app.cam.looking_from(dir);
            if frame.z.abs() > 0.5 {
                to.yaw = nearest_quarter_turn(app.cam.yaw);
            }
            app.animate_to(to);
        }
    }
    let home = Rect::from_center_size(pos2(c.x - 37.0, c.y - 37.0), vec2(18.0, 18.0));
    crate::scenario::publish_target("axis_view:home", home);
    let hr = ui.interact(home, ui.id().with("axis_view_home"), Sense::click());
    // The home button sits in its own small round chip.
    painter.circle(home.center(), 12.0, t.overlay.gamma_multiply(if hr.hovered() { 0.9 } else { 0.5 }), Stroke::new(1.0, t.border));
    icons::paint_icon(&painter, home, "home", f32::from(t.icon.a()) / 255.0);
    // Cursors: a grabbing hand while orbiting, an open hand over the circle, a pointer on the home
    // chip (the axis ends keep their pointing hand).
    if hr.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if orbiting {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if in_circle && hovered.is_none() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if hr.on_hover_text(crate::i18n::tr_label("Home view")).clicked() {
        app.animate_view("home");
    }
}

/// At a halfway tie, prefer the square heading nearer the standard plan view.
pub(crate) fn nearest_quarter_turn(yaw: f64) -> f64 {
    if !yaw.is_finite() {
        return 0.0;
    }
    let q = std::f64::consts::FRAC_PI_2;
    let lo = (yaw / q).floor() * q;
    let hi = lo + q;
    let (dlo, dhi) = (yaw - lo, hi - yaw);
    if (dlo - dhi).abs() < 1e-6 {
        let off_front = |a: f64| ((a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
        if off_front(lo) <= off_front(hi) { lo } else { hi }
    } else if dlo < dhi {
        lo
    } else {
        hi
    }
}

/// Turn the view to face the selected planar face or plane, else the active sketch; asked again
/// while it already faces it, from the other side (#1886).
pub fn look_at_selection(app: &mut SolveApp) {
    if solidcraft_engine::find_command("view.normal_to").is_some_and(|command| command.info(&app.session).disabled_reason.is_none()) {
        app.start("view.normal_to");
    } else {
        crate::dialogs::look_at_sketch_again(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::*;

    #[test]
    fn ground_shadow_does_not_enter_see_through_geometry() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = solidcraft_engine::Session::default();
        session.execute("solid.box", &json!({"length": 30, "width": 30, "height": 10}))?;
        let app = SolveApp::new(session, crate::Services::default());
        let mut scene = GpuScene::default();
        ground_shadow(&mut scene, &app, &app.session.world_state());
        assert!(scene.trans.is_empty(), "the ground shadow must not trigger order-independent transparency");
        assert_eq!(scene.shadow.len(), 48 * 5 * 3 * crate::gpu::TRI_SIZE, "all shadow rings are retained");
        let shadow_alpha = |v: &[u8]| {
            let i = u32::from_le_bytes(v.get(24..28)?.try_into().ok()?);
            scene.palette.get(crate::gpu::PaletteIndex(i)).map(|c| c[3])
        };
        assert!(scene.shadow.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().any(|v| shadow_alpha(v).is_some_and(|a| a > 0)));
        Ok(())
    }

    #[test]
    fn straight_3d_wire_is_offered_as_an_axis_but_spline_is_refused() {
        for (command, points, straight) in [
            ("sketch.line3d", json!([[10, 20, 30], [30, 20, 30]]), true),
            ("sketch.spline3d", json!([[10, 20, 30], [20, 25, 40], [30, 20, 30]]), false),
            ("sketch.spline3d", json!([[10, 20, 30], [20, 20, 30], [30, 20, 30]]), false),
        ] {
            let mut s = solidcraft_engine::Session::default();
            s.execute("sketch.create", &json!({"plane":"XZ", "offset":7})).unwrap();
            s.execute("sketch.options", &json!({"sketch_3d":true})).unwrap();
            let id = s.execute(command, &json!({"points":points})).unwrap()["curves"][0].as_str().unwrap().to_string();
            s.execute("sketch.finish", &json!({})).unwrap();
            let mut app = SolveApp::new(s, crate::Services::default());
            app.fit_view();
            let st = app.session.world_state();
            let wire = &st.sketches[0].sketch.wires[0];
            let at = (wire.pts[0] + wire.pts[1]) * 0.5;
            let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(800., 600.)));
            let hits = pick_all(&app, &proj, proj.to_screen(at).unwrap());
            let hit = hits.iter().find(|h| matches!(h, Hit::SketchCurve { id: picked, .. } if picked == &id)).unwrap();
            let axis = crate::selection::SelInput::new("Axis", crate::selection::AXES, false);
            assert_eq!(axis.accepts(hit, None, |_, _| false).is_some(), straight, "{hit:?}");
            let selected = Sel::SketchCurve { id, sketch: None };
            assert_eq!(crate::dialogs::axis_of(&app, &selected).is_some(), straight);
        }
    }

    #[test]
    fn plane_at_angle_uses_3d_wire_endpoints_in_model_space() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &json!({"plane":"XZ", "offset":7})).unwrap();
        s.execute("sketch.options", &json!({"sketch_3d":true})).unwrap();
        let id = s.execute("sketch.line3d", &json!({"points":[[10,20,30],[30,20,30]]})).unwrap()["curves"][0].as_str().unwrap().to_string();
        s.execute("sketch.finish", &json!({})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let selected = Sel::SketchCurve { id, sketch: None };
        let (origin, dir) = crate::dialogs::axis_of(&app, &selected).unwrap();
        assert!(origin.dist(Vec3::new(10., 20., 30.)) < 1e-9);
        assert!(dir.dist(Vec3::X) < 1e-9);
        let mut dialog = crate::dialogs::Dialog::for_command(&app, "construct.plane.at_angle").unwrap();
        dialog.inputs[0].items = vec![Sel::Plane { name: "XY".into() }];
        dialog.inputs[1].items = vec![selected];
        if let crate::dialogs::Kind::AnglePlane { angle } = &mut dialog.kind {
            *angle = "90 deg".into();
        }
        for (command, params) in crate::dialogs::apply_commands(&app, &dialog).unwrap() {
            app.run(&command, params).unwrap();
        }
        let planes = solidcraft_engine::view::construction_planes(&app.session);
        let plane = &planes.last().unwrap().2;
        assert!(plane.normal().dist(-Vec3::Y) < 1e-9);
        assert!(plane.origin.dist(Vec3::new(0., 50., 10.)) < 1e-9, "{plane:?}");
    }

    #[test]
    fn point_pick_tie_at_origin_prefers_the_drawn_point_after_deleting_coincident() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let drawn = s.execute("sketch.point", &json!({"point": [0, 0], "id": "drawn"})).unwrap();
        let constraint = drawn["constraints"][0].as_str().unwrap();
        s.execute("sketch.delete", &json!({"entities": [constraint]})).unwrap();
        let sid = s.active_sketch.unwrap();
        assert!(s.doc.sketch(sid).unwrap().constraints.is_empty());
        let mut app = SolveApp::new(s, crate::Services::default());
        let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)));
        let at = proj.to_screen(Vec3::ZERO).unwrap();
        assert!(app.preferences.prefer_free_sketch_points);
        assert_eq!(sketch_point_at(&app, &proj, at).unwrap().1.as_deref(), Some("origin"), "drawing keeps its origin snap");
        let hits = pick(&app, &proj, at);
        assert!(matches!(hits.first(), Some(Hit::SketchPoint { id, .. }) if id == "drawn"), "{hits:?}");
        let all = pick_all(&app, &proj, at);
        let points: Vec<_> = all
            .iter()
            .filter_map(|h| match h {
                Hit::SketchPoint { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(points, ["drawn", "origin"], "Select Other keeps both points");
        assert!(matches!(pick_cached(&mut app, &proj, at).first(), Some(Hit::SketchPoint { id, .. }) if id == "drawn"));
        let saved = crate::prefs::set(&mut app, &json!({"prefer_free_sketch_points": false})).unwrap();
        assert_eq!(saved["prefer_free_sketch_points"], false);
        assert!(matches!(pick_cached(&mut app, &proj, at).first(), Some(Hit::SketchPoint { id, .. }) if id == "origin"));
        assert!(matches!(pick_all(&app, &proj, at).first(), Some(Hit::SketchPoint { id, .. }) if id == "origin"));
        let Some(Hit::SketchPoint { id, .. }) = hits.first() else { panic!("no point picked") };
        app.run("sketch.drag", json!({"entity": id, "from": [0, 0], "to": [10, 5]})).unwrap();
        let sk = app.session.doc.sketch(sid).unwrap();
        assert!(sk.point(sk.resolve_point("drawn").unwrap()).unwrap().dist(Vec2::new(10.0, 5.0)) < 1e-6);
        assert_eq!(sk.point(0), Some(Vec2::ZERO));
    }

    /// Every tangent seam Continuity Analysis reports keeps its colour while the pointer rests on
    /// a face (#1247): a 60 x 40 x 20 box with its four top edges rounded (6 mm) has eight, four
    /// against the sides and four against the top face. The tool lit the face under the pointer,
    /// though its click takes only edges, and the face's outline covered the colours of its
    /// edges. Rendered offscreen with bodies built on the CPU and evaluated on the GPU (skipped
    /// without a GPU adapter); with `SOLIDCRAFT_CONTINUITY_TOP_SHOT` set, the pictures are
    /// written there (`-cpu.png`, `-gpu.png`).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn continuity_colours_stay_on_the_edges_of_the_face_under_the_pointer() {
        use solidcraft_engine::kernel::SeamVerdict;
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 60, "width": 40, "height": 20, "body_name": "B"})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[30, 0, 20], [30, 40, 20], [0, 20, 20], [60, 20, 20]], "radius": 6})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        // The toolbar button: every seam shaded, the tool left picking edges.
        app.start("inspect.continuity");
        let tangent = |app: &SolveApp| app.session.continuity.iter().filter(|seam| seam.verdict == SeamVerdict::CurvatureJump).count();
        assert_eq!(tangent(&app), 8);
        const SIZE: u32 = 512; // rows of 256-byte multiples (the copy alignment)
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        // The pointer rests in the middle of the top face, away from its edges: the tool takes
        // only edges, so nothing there lights up over the analysis.
        let proj = projection(&app, rect);
        let middle = proj.to_screen(Vec3::new(30.0, 20.0, 20.0)).unwrap();
        let hits = pick_cached(&mut app, &proj, middle);
        assert!(matches!(hits.first(), Some(Hit::Face { .. })), "{hits:?}");
        app.viewport.hover = tool_hover(&app, &hits);
        assert_eq!(app.viewport.hover, None);
        let t = Tokens::get();
        let g1 = t.continuity[1];
        // Each tangent seam lies in one flat face (the top, or a side under its round): that
        // face's outward normal, and the seam shows where it faces the eye (the body is convex).
        let flat_normal = |e: &[Vec3]| {
            let on = |f: fn(&Vec3) -> f64, at: f64| e.iter().all(|p| (f(p) - at).abs() < 1e-6);
            [
                (on(|p| p.z, 20.0), Vec3::Z),
                (on(|p| p.y, 0.0), -Vec3::Y),
                (on(|p| p.y, 40.0), Vec3::Y),
                (on(|p| p.x, 0.0), -Vec3::X),
                (on(|p| p.x, 60.0), Vec3::X),
            ]
            .into_iter()
            .find_map(|(lies, n)| lies.then_some(n))
        };
        for tess in [false, true] {
            app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            let Some(img) = crate::gpu::render_offscreen(
                build_scene(&app).merged(),
                Some(build_highlight(&app)),
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                tess.then_some(solidcraft_gpu::SHADER),
                [0.0; 8],
                crate::gpu::analysis_colors(&t.analysis),
            ) else {
                eprintln!("skipping: no GPU adapter");
                return;
            };
            if let Ok(path) = std::env::var("SOLIDCRAFT_CONTINUITY_TOP_SHOT")
                && let Some(png) = image::RgbaImage::from_raw(SIZE, SIZE, img.clone())
            {
                png.save(path.replace(".png", if tess { "-gpu.png" } else { "-cpu.png" })).unwrap();
            }
            let lit = |x: i64, y: i64| {
                (0..SIZE as i64).contains(&x)
                    && (0..SIZE as i64).contains(&y)
                    && img[((y * SIZE as i64 + x) * 4) as usize..][..3].iter().zip([g1.r(), g1.g(), g1.b()]).all(|(a, b)| a.abs_diff(b) <= 12)
            };
            let mut seen = 0;
            for seam in app.session.continuity.iter().filter(|seam| seam.verdict == SeamVerdict::CurvatureJump) {
                let (a, b) = (seam.points[0], seam.points[seam.points.len() - 1]);
                let normal = flat_normal(&seam.points).unwrap_or_else(|| panic!("a tangent seam off the flat faces: {a:?}..{b:?}"));
                if normal.dot(proj.cam.eye() - a.lerp(b, 0.5)) <= 0.0 {
                    continue;
                }
                seen += 1;
                // In its colour a quarter, half and three quarters along.
                for f in [0.25, 0.5, 0.75] {
                    let p = a.lerp(b, f);
                    let q = proj.to_screen(p).unwrap();
                    let (x, y) = (q.x.round() as i64, q.y.round() as i64);
                    let drawn = (-2..=2).any(|dy| (-2..=2).any(|dx| lit(x + dx, y + dy)));
                    assert!(drawn, "tess {tess}: tangent seam {a:?}..{b:?} is not drawn in its colour at {p:?} ({x}, {y})");
                }
            }
            assert_eq!(seen, 6, "the four top seams and the two sides facing the eye");
        }
        // On an edge, the edge lights up: what a click measures.
        let proj = projection(&app, rect);
        let on_edge = proj.to_screen(Vec3::new(30.0, 6.0, 20.0)).unwrap();
        let hits = pick_cached(&mut app, &proj, on_edge);
        assert!(matches!(tool_hover(&app, &hits), Some(Hit::Edge { .. })), "{hits:?}");
    }

    /// Continuity Analysis draws each seam in its verdict's `continuity` token (#770): a box
    /// with one edge rounded shows the round's two tangent seams (G1) and the creases (G0), and
    /// nothing in the smooth or gap colours.
    #[test]
    fn continuity_seams_take_their_colours_from_the_theme() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[20, 20, 10]], "radius": 4})).unwrap();
        s.execute("inspect.continuity", &json!({})).unwrap();
        let app = SolveApp::new(s, crate::Services::default());
        let sc = build_scene(&app);
        // An overlay line is two points, a colour and a width: 32 bytes.
        let colours: Vec<[u8; 4]> = sc
            .overlay
            .as_chunks::<32>()
            .0
            .iter()
            .filter_map(|r| {
                let i = u32::from_le_bytes(r.get(24..28)?.try_into().ok()?);
                sc.palette.get(crate::gpu::PaletteIndex(i))
            })
            .collect();
        let t = Tokens::get();
        let count = |c: Color32| colours.iter().filter(|x| **x == c4(c)).count();
        let [smooth, jump, sharp, gap] = t.continuity.map(count);
        assert!(jump > 0 && sharp > 0, "tangent seams {jump}, creases {sharp} segments");
        assert_eq!((smooth, gap), (0, 0));
    }

    /// Bodies drawn from their display plans, evaluated on the GPU, look like the same bodies
    /// built into triangles on the CPU: the sample design rendered both ways through the
    /// viewport's real pipelines, offscreen, on this machine's GPU (skipped without one).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bodies_evaluated_on_the_gpu_look_like_bodies_built_on_the_cpu() {
        let mut s = solidcraft_engine::Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let mut render = |tess: bool| {
            app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
            let scene = build_scene(&app).merged();
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            let (planned, shader) = (scene.planned.len(), tess.then_some(solidcraft_gpu::SHADER));
            (
                planned,
                scene.tris.len(),
                crate::gpu::render_offscreen(
                    scene,
                    None,
                    proj.vp.to_f32(),
                    proj.cam.back().to_f32(),
                    eye,
                    SIZE,
                    shader,
                    [0.0; 8],
                    crate::gpu::analysis_colors(&Tokens::get().analysis),
                ),
            )
        };
        let (planned, gpu_tris, gpu) = render(true);
        let (none, cpu_tris, cpu) = render(false);
        let (Some(gpu), Some(cpu)) = (gpu, cpu) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        assert!(planned > 0 && none == 0, "{planned} bodies planned");
        assert!(gpu_tris < cpu_tris, "the planned bodies' triangles are not built on the CPU ({gpu_tris} vs {cpu_tris} bytes)");
        let px = |img: &[u8], i: usize| [img[4 * i], img[4 * i + 1], img[4 * i + 2]];
        let n = (SIZE * SIZE) as usize;
        let drawn = (0..n).filter(|&i| px(&cpu, i) != [255, 255, 255]).count();
        assert!(drawn > n / 20, "the model covers the view: {drawn} pixels");
        let differ = (0..n).filter(|&i| px(&gpu, i).iter().zip(px(&cpu, i)).any(|(a, b)| a.abs_diff(b) > 24)).count();
        assert!(differ * 100 < drawn, "{differ} of {drawn} model pixels differ");
    }

    /// A device that cannot build the tessellation pipeline (D3D12's FXC rejected the shader
    /// at startup and the app panicked) still shows the model: the failure is caught, logged,
    /// and bodies are built on the CPU and drawn. A shader the device rejects stands in for FXC.
    /// Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bodies_are_built_on_the_cpu_when_the_tessellation_pipeline_fails() {
        const REJECTED: &str = "
            @group(0) @binding(1) var<storage, read_write> surfaces: array<u32>;
            @compute @workgroup_size(64)
            fn main(@builtin(global_invocation_id) gid: vec3<u32>) { surfaces[gid.x] = 1u; }
        ";
        let Some(tess) = crate::gpu::offscreen_tessellates(REJECTED) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        assert!(!tess, "the rejected shader built a pipeline");
        let mut s = solidcraft_engine::Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let scene = build_scene(&app).merged();
        assert!(scene.planned.is_empty(), "no body waits for the GPU evaluation");
        assert!(!scene.tris.is_empty(), "the bodies are meshed on the CPU");
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let Some(img) = crate::gpu::render_offscreen(
            scene,
            None,
            proj.vp.to_f32(),
            proj.cam.back().to_f32(),
            eye,
            SIZE,
            Some(REJECTED),
            [0.0; 8],
            crate::gpu::analysis_colors(&Tokens::get().analysis),
        ) else {
            return;
        };
        let n = (SIZE * SIZE) as usize;
        let drawn = img.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        assert!(drawn > n / 20, "the model is drawn: {drawn} pixels");
    }

    /// Supersampling (#1288): the sample design drawn at 2× and filtered down shows the same
    /// model (the same pixels covered, give or take its outline) with smoothed edges: pixels part
    /// model, part background, which a single-sampled frame does not have. Skipped without a GPU
    /// adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn supersampling_draws_the_same_model_with_smooth_edges() {
        let mut s = solidcraft_engine::Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render =
            |ssaa: f32| crate::gpu::render_offscreen_ssaa(build_scene(&app).merged(), proj.vp.to_f32(), proj.cam.back().to_f32(), eye, SIZE, ssaa);
        let (Some(plain), Some(super2)) = (render(1.0), render(2.0)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let covered = |img: &[u8]| img.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        let (a, b) = (covered(&plain), covered(&super2));
        assert!(a > 2000, "the model is drawn: {a} pixels");
        assert!((a as f64 - b as f64).abs() < a as f64 * 0.1, "same model: {a} vs {b} pixels covered");
        // Edge pixels against the white background: neither white nor any colour the plain frame
        // has, so blended. A count well above the plain frame's own.
        let palette: std::collections::HashSet<[u8; 3]> = plain.chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        let new_colours = super2.chunks(4).filter(|p| !palette.contains(&[p[0], p[1], p[2]])).count();
        assert!(new_colours > 100, "the supersampled frame blends its edges: {new_colours} new colours");
    }

    /// The supersampled frame stays within the device's largest texture (#869) and a memory
    /// budget, and is not drawn at all when that leaves too little to gain.
    #[test]
    fn supersampling_keeps_within_the_device_texture_limit() {
        use crate::gpu::supersampled_size;
        assert_eq!(supersampled_size([1920.0, 1080.0], 2.0, 16384, 1), Some([3840, 2160]));
        assert_eq!(supersampled_size([1920.0, 1080.0], 1.5, 16384, 1), Some([2880, 1620]));
        // A 4096 texture limit: a 2560 px wide view gets 1.6×.
        let [w, h] = supersampled_size([2560.0, 1440.0], 2.0, 4096, 1).unwrap();
        assert!((4095..=4096).contains(&w) && (2303..=2304).contains(&h), "{w} x {h}");
        // Under 1.25× is not worth it: drawn as usual.
        assert_eq!(supersampled_size([3840.0, 2160.0], 2.0, 4096, 1), None);
        assert_eq!(supersampled_size([1920.0, 1080.0], 1.0, 16384, 1), None);
        assert_eq!(supersampled_size([0.0, 1080.0], 2.0, 16384, 1), None);
        // Under 1: drawn smaller and filtered up (the product preview past its budget, #1464).
        assert_eq!(supersampled_size([3840.0, 2160.0], 0.5, 4096, 4), Some([1920, 1080]));
        // 4× MSAA at 1440p: the memory budget caps it below 2×.
        let [w, h] = supersampled_size([2560.0, 1440.0], 2.0, 16384, 4).unwrap();
        assert!(w < 5120 && w as f32 >= 2560.0 * 1.25 && u64::from(w) * u64::from(h) * 32 <= 384 << 20, "{w} x {h}");
    }

    /// Ambient occlusion beside a block on a plate (#4167, #3407), seen from above with a radius
    /// large on screen (a big design seen close up): the plate darkens toward the block and is
    /// clear beyond the radius, with no copies of the block's outline in the darkening. The same
    /// sample offsets at every pixel gave each sample its own shifted copy, so beside the block's
    /// straight side every row was the same staircase. Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn ambient_occlusion_draws_no_copies_of_a_part() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 300, "width": 300, "height": 4, "corner": [-150, -150, -4], "body_name": "plate"})).unwrap();
        s.execute("solid.box", &json!({"length": 30, "width": 160, "height": 16, "corner": [0, -80, 0], "operation": "new", "body_name": "block"}))
            .unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let (yaw, pitch) = solidcraft_engine::render::StandardView::Top.angles();
        app.cam = Camera { target: Vec3::new(-30.0, 0.0, 0.0), yaw, pitch, roll: 0.0, distance: 200.0, fov: 0.0, ..Default::default() };
        app.ui.perspective = false;
        app.ui.show_grid = false;
        app.set_origin_shown(false);
        app.ui.ground_shadow = false;
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        const SIZE: u32 = 512;
        const RADIUS: f32 = 45.0;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], 0.0];
        let render =
            |radius: f32| crate::gpu::render_offscreen_ao(build_scene(&app).merged(), proj.vp.to_f32(), proj.cam.back().to_f32(), eye, SIZE, radius);
        let (Some(off), Some(on)) = (render(0.0), render(RADIUS)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let (w, r) = (f64::from(SIZE), scene_radius(&app));
        let at = |p: Vec3| proj.cam.to_screen(p, w, w, r).unwrap();
        // The block's left side on screen, the radius in pixels, and rows beside the middle of
        // the side, far enough from its ends that every sample's copy spans them all.
        let edge = at(Vec3::new(0.0, 0.0, 16.0)).0;
        let reach = (at(Vec3::new(f64::from(RADIUS), 0.0, 0.0)).0 - at(Vec3::ZERO).0).abs();
        let (a, b) = (at(Vec3::new(-10.0, 10.0, 0.0)).1.round() as usize, at(Vec3::new(-10.0, -10.0, 0.0)).1.round() as usize);
        let (y0, y1) = (a.min(b), a.max(b));
        assert!(reach > 100.0 && y1 > y0 + 20, "{reach} px, rows {y0}..{y1}");
        let lum = |img: &[u8], x: usize, y: usize| img[(y * SIZE as usize + x) * 4..][..3].iter().map(|&c| f64::from(c)).sum::<f64>();
        // The plate's visibility in each column: its brightness with AO over its brightness
        // without, averaged over the rows.
        let column = |x: usize| (y0..y1).map(|y| lum(&on, x, y) / lum(&off, x, y).max(1.0)).sum::<f64>() / (y1 - y0) as f64;
        // From 30 px beyond the radius up to 4 px from the block (its outline).
        let from = (edge - reach - 30.0) as usize;
        let profile: Vec<f64> = (from..=(edge - 4.0) as usize).map(column).collect();
        let (clear, shaded) = (&profile[..20], &profile[30..]);
        let step = shaded.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0, f64::max);
        eprintln!(
            "plate {} without AO; edge {edge:.1} px, radius {reach:.1} px, rows {y0}..{y1}; clear {:.4}..{:.4}; next to the block {:.4}; largest step {step:.4}",
            lum(&off, from, y0) / 3.0,
            clear.iter().copied().fold(f64::INFINITY, f64::min),
            clear.iter().copied().fold(0.0, f64::max),
            profile[profile.len() - 1],
        );
        assert!(profile[profile.len() - 1] < 0.98, "the plate darkens beside the block: {:.4}", profile[profile.len() - 1]);
        assert!(clear.iter().all(|v| (v - 1.0).abs() < 0.004), "the plate is clear beyond the radius: {clear:?}");
        assert!(step < 0.005, "no copies of the block's outline: a step of {step:.4} between neighbouring columns");
    }

    /// Faded and see-through bodies are drawn from their plans where the renderer blends them
    /// order-independently, and look as they did built on the CPU: a see-through box beside a
    /// part faded outside the active component, drawn both ways. Without order-independent
    /// transparency they stay on the CPU (the sorted fallback needs their triangles). Skipped
    /// without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn faded_and_see_through_bodies_are_drawn_from_their_plans_with_oit() {
        let mut s = solidcraft_engine::Session::default();
        let part = s.execute("component.create", &json!({"name": "Part"})).unwrap()["component"].as_u64().unwrap();
        s.execute("solid.box", &json!({"length": 30, "width": 30, "height": 30, "corner": [0, 0, 0], "operation": "new", "body_name": "Glass"}))
            .unwrap();
        s.execute("appearance.assign", &json!({"bodies": ["Glass"], "color": "#3366cc", "opacity": 0.5})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "corner": [40, 0, 0], "operation": "new", "body_name": "Inside"}))
            .unwrap();
        s.execute("component.activate", &json!({"component": 0})).unwrap();
        s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20, "corner": [0, 40, 0], "operation": "new", "body_name": "Faded"}))
            .unwrap();
        s.execute("component.activate", &json!({"component": part})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.ui.ground_shadow = false;
        app.fit_view();
        const SIZE: u32 = 192;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let target =
            |tess: bool, oit: bool| Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit });
        let classes = |scene: &GpuScene| {
            let mut c: Vec<_> = scene.planned.iter().map(|b| format!("{:?}", b.class)).collect();
            c.sort();
            c
        };
        app.viewport.gpu = target(true, false);
        assert_eq!(classes(&build_scene(&app)), ["Opaque"], "without OIT only the opaque body is drawn from its plan");
        app.viewport.gpu = target(true, true);
        let planned = build_scene(&app);
        assert_eq!(classes(&planned), ["Faded", "Glass", "Opaque"], "with OIT every body is");
        let built: usize = planned.blocks.iter().map(|b| b.bytes.vertices.len()).sum();
        assert_eq!(built, 0, "and none is built on the CPU");
        // Hovered, a faded body drawn from its plan lights through the plan's triangles.
        let faded = app.session.world_state().bodies.iter().find(|b| b.name.starts_with("Faded")).map(|b| b.name.clone()).unwrap();
        app.viewport.hover_bodies = vec![faded];
        assert_eq!(build_highlight(&app).hover_planned.len(), 1, "the faded body's plan is lit");
        app.viewport.hover_bodies.clear();
        app.viewport.gpu = target(false, false);
        let cpu = build_scene(&app);
        let render = |scene: GpuScene, tess: bool| {
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            crate::gpu::render_offscreen(
                scene,
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                tess.then_some(solidcraft_gpu::SHADER),
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        let (Some(from_plans), Some(from_cpu)) = (render(planned, true), render(cpu, false)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let differ = from_plans.chunks(4).zip(from_cpu.chunks(4)).filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 6)).count();
        let shown = from_cpu.chunks(4).filter(|p| p[..3] != from_cpu[..3]).count();
        assert!(shown > 2000, "the bodies are in view ({shown} px)");
        assert!(differ * 100 < shown, "{differ} of {shown} px differ between the plan and CPU pictures");
    }

    /// A hovered face lights solidly over a body drawn from its display plan on the GPU: the
    /// highlight uses the plan's triangles, not the reference mesh's other triangulation of the
    /// same face, which fought the model at equal depth and shimmered in wedges. Every face of a
    /// box with a pocket and a round boss, hovered, changes the same pixels as on the CPU path (where model and
    /// highlight share the reference mesh). Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_hovered_face_lights_whole_over_a_body_drawn_from_its_plan() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 40, "height": 40, "corner": [0, 0, 0], "operation": "new"})).unwrap();
        s.execute("solid.box", &json!({"length": 16, "width": 16, "height": 20, "corner": [12, 12, 20], "operation": "cut"})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [33, 33, 40], "diameter": 10, "height": 10, "operation": "join"})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let body = app.session.world_state().bodies[0].clone();
        let faces = body.body.face_count();
        let mut render = |tess: bool, hover: Option<usize>| {
            app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
            app.viewport.hover = hover.map(|index| Hit::Face { body: body.name.clone(), index, point: Vec3::ZERO });
            let (scene, highlight) = (build_scene(&app), build_highlight(&app).merged());
            assert_eq!(!scene.planned.is_empty(), tess, "the box is drawn from its plan exactly on the GPU path");
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            let shader = tess.then_some(solidcraft_gpu::SHADER);
            crate::gpu::render_offscreen(
                scene,
                Some(highlight),
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                shader,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        let lit = |plain: &[u8], hovered: &[u8]| -> Vec<bool> { plain.chunks(4).zip(hovered.chunks(4)).map(|(a, b)| a != b).collect() };
        let (Some(gpu_plain), Some(cpu_plain)) = (render(true, None), render(false, None)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let mut checked = 0;
        for f in 0..faces {
            let (Some(gpu), Some(cpu)) = (render(true, Some(f)), render(false, Some(f))) else { return };
            let (on_gpu, on_cpu) = (lit(&gpu_plain, &gpu), lit(&cpu_plain, &cpu));
            let area = on_cpu.iter().filter(|x| **x).count();
            if area < 400 {
                continue; // facing away or hidden
            }
            // Inside the face, two pixels in from its outline: a curved face's outline follows
            // its own facets, which the plan and the reference mesh cut differently.
            let n = SIZE as i32;
            let inside = |i: usize| {
                let (x, y) = (i as i32 % n, i as i32 / n);
                (-2..=2).all(|dy| (-2..=2).all(|dx| on_cpu.get(((y + dy).clamp(0, n - 1) * n + (x + dx).clamp(0, n - 1)) as usize) == Some(&true)))
            };
            let missed = (0..on_cpu.len()).filter(|&i| inside(i) && !on_gpu[i]).count();
            let lights = missed * 200 < area;
            // A failure keeps its pictures: this failed once in a loaded full-suite run on Metal
            // and has not been reproduced since (#1383). The mask is red where the face does not
            // light on the GPU, grey over the rest of the face as the CPU lights it.
            let kept = if lights {
                String::new()
            } else {
                let mask: Vec<u8> = (0..on_cpu.len())
                    .flat_map(|i| match (inside(i) && !on_gpu[i], on_cpu[i]) {
                        (true, _) => [255, 0, 0, 255],
                        (false, true) => [128, 128, 128, 255],
                        (false, false) => [255, 255, 255, 255],
                    })
                    .collect();
                let changed = on_gpu.iter().filter(|x| **x).count();
                let pictures =
                    [("gpu_plain", &gpu_plain), ("gpu_hovered", &gpu), ("cpu_plain", &cpu_plain), ("cpu_hovered", &cpu), ("missed", &mask)];
                format!("; {changed} pixels change on the GPU; {}", keep_failed_pictures(&format!("hover-face-{f}"), &pictures, SIZE))
            };
            assert!(lights, "face {f}: {missed} of its {area} hovered pixels do not light (the highlight fights the model){kept}");
            checked += 1;
        }
        assert!(checked >= 3, "{checked} faces seen lit");
    }

    /// Saves a failing pixel test's pictures (straight RGBA, `size` × `size`) as PNGs in a folder
    /// of their own under the temporary folder, and says where.
    #[cfg(not(target_arch = "wasm32"))]
    fn keep_failed_pictures(name: &str, pictures: &[(&str, &Vec<u8>)], size: u32) -> String {
        let dir = std::env::temp_dir().join(format!("solidcraft-{name}-{}", std::process::id()));
        let saved = std::fs::create_dir_all(&dir).is_ok()
            && pictures.iter().all(|(file, rgba)| {
                image::RgbaImage::from_raw(size, size, rgba.to_vec()).is_some_and(|img| img.save(dir.join(format!("{file}.png"))).is_ok())
            });
        if saved { format!("pictures in {}", dir.display()) } else { format!("the pictures could not be saved in {}", dir.display()) }
    }

    /// The highlight scene as the viewport hands it over keeps its feedback (a hovered face, a
    /// body copied on the CPU) in a block of its own, kept while the highlight is unchanged. That
    /// block is drawn: the picture is the same as with the feedback merged into the scene's own
    /// triangles. Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_highlight_scenes_own_block_is_drawn() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20, "corner": [0, 0, 0], "operation": "new"})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        const SIZE: u32 = 128;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let body = app.session.world_state().bodies[0].clone();
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let model = build_scene(&app);
        let render = |highlight: GpuScene| {
            crate::gpu::render_offscreen(
                model.clone(),
                Some(highlight),
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        let Some(plain) = render(GpuScene::default()) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let mut lit = 0;
        for index in 0..body.body.face_count() {
            app.viewport.hover = Some(Hit::Face { body: body.name.clone(), index, point: Vec3::ZERO });
            let highlight = build_highlight(&app);
            assert!(highlight.blocks.iter().any(|b| !b.bytes.vertices.is_empty()), "the face's feedback is in the highlight's block");
            let (Some(as_handed), Some(merged)) = (render(highlight.clone()), render(highlight.merged())) else { return };
            assert!(as_handed == merged, "face {index}: the highlight's block is not drawn as its merged triangles are");
            lit += usize::from(as_handed != plain);
        }
        assert!(lit >= 3, "{lit} faces light up");
    }

    /// An origin plane lying on a body's face is tinted evenly over it (#1739: a box built at the
    /// origin, seen from below in orthographic; the XY plane's square z-fought with the bottom
    /// face in stripes). Faces are pushed back by their slope (`FACE_BIAS`), the planes are not.
    /// Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_origin_plane_on_a_face_is_tinted_evenly() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 10, "corner": [0, 0, 0]})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.cam = Camera {
            target: Vec3::new(16.904, 10.024, 3.661),
            yaw: -0.32,
            pitch: -0.9652,
            roll: 0.0,
            distance: 98.557,
            fov: 0.0,
            ..Default::default()
        };
        app.ui.perspective = false;
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let mut render = |origin: bool| {
            app.set_origin_shown(origin);
            app.viewport.gpu =
                Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
            let (scene, highlight) = (build_scene(&app), build_highlight(&app).merged());
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let img = crate::gpu::render_offscreen(
                scene,
                Some(highlight),
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                [e[0], e[1], e[2], 0.0],
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            );
            (img, proj)
        };
        let (Some(with), proj) = render(true) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let (Some(without), _) = render(false) else { return };
        app.set_origin_shown(true);
        let xy = origin_planes(&app).into_iter().find(|(n, _, _)| *n == "XY").map(|(_, _, q)| q).unwrap();
        let (w, h, r) = (f64::from(SIZE), f64::from(SIZE), scene_radius(&app));
        let (mut samples, mut plain) = (0, Vec::new());
        // Points inside the square (away from its outline), where it lies on the bottom face.
        for i in 1..12 {
            for j in 1..12 {
                let (u, v) = (f64::from(i) / 12.0, f64::from(j) / 12.0);
                let p = xy[0] + (xy[1] - xy[0]) * u + (xy[3] - xy[0]) * v;
                if !(p.x > 0.5 && p.x < 39.5 && p.y > 0.5 && p.y < 29.5) {
                    continue;
                }
                let Some((x, y, _)) = proj.cam.to_screen(p, w, h, r) else { continue };
                let at = (y as usize * SIZE as usize + x as usize) * 4;
                samples += 1;
                if with.get(at..at + 4) == without.get(at..at + 4) {
                    plain.push((x as u32, y as u32));
                }
            }
        }
        assert!(samples > 40, "{samples} points of the square on the face");
        assert!(
            plain.is_empty(),
            "{} of {samples} points of the XY plane's square do not show over the face: {:?}",
            plain.len(),
            &plain[..plain.len().min(6)]
        );
    }

    #[test]
    fn construction_drawing_and_hover_overlays_preserve_visible_gaps() {
        fn length(shape: &Shape) -> f32 {
            match shape {
                Shape::LineSegment { points, .. } => points[0].distance(points[1]),
                Shape::Path(path) => path.points.windows(2).map(|w| w[0].distance(w[1])).sum(),
                Shape::Vec(shapes) => shapes.iter().map(length).sum(),
                _ => 0.0,
            }
        }
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.settle();
        let proj = display_proj(&h.app);
        let a = Vec2::ZERO;
        let b = Vec2::new(20.0, 8.0);
        let whole = proj.to_screen(Vec3::new(b.x, b.y, 0.0)).unwrap().distance(proj.to_screen(Vec3::ZERO).unwrap());
        let mut draw = crate::tools::Tool::for_command("sketch.line").unwrap();
        draw.pts.push((a, None));
        draw.hover = Some((b, None));
        for mode in [0, 1, 2] {
            crate::sketch_palette::set_mode(&mut h.app, mode);
            let ctx = crate::fonts::test_context();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui.ctx().layer_painter(egui::LayerId::background());
                crate::tools::preview(&h.app, &draw, &painter, &proj);
            });
            output.textures_delta.clear();
            let preview: f32 = output.shapes.iter().map(|s| length(&s.shape)).sum();
            let r = h.app.run("sketch.line", json!({"points":[[0,0],[20,8]]})).unwrap();
            let id = r["curves"][0].as_str().unwrap().to_string();
            h.app.viewport.hover = Some(Hit::SketchCurve { sketch: h.app.session.active_sketch.unwrap(), id, straight: true });
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                hover_highlight(&h.app, &ui.ctx().layer_painter(egui::LayerId::background()), &proj);
            });
            output.textures_delta.clear();
            let hover: f32 = output.shapes.iter().map(|s| length(&s.shape)).sum();
            assert!((preview - hover).abs() < 0.01, "mode {mode}: preview {preview}, hover {hover}");
            if mode == 0 {
                assert!((preview - whole).abs() < 0.01);
            } else {
                assert!(preview > whole * 0.25 && preview < whole * 0.85, "mode {mode} covers {preview} of {whole}");
            }
            h.app.run("edit.undo", json!({})).unwrap();
        }
        crate::sketch_palette::set_mode(&mut h.app, 0);
    }

    /// Headless pictures of the view (`ui.render`, the agent's view image) are drawn in the
    /// window's theme (#974) and colour vision mode (#778). The light theme's tokens are the engine's own colours, so a
    /// light picture in Off is unchanged; otherwise each part takes the token the window draws it
    /// with. Sketch states follow what the window does with them (`sketch_color`) and the axes the
    /// mode's axis colours, so the two can't drift apart.
    #[test]
    fn pictures_are_drawn_in_the_windows_theme() {
        use crate::theme::ColorVision;
        use solidcraft_engine::view::Palette;
        assert_eq!(scene_palette(false, ColorVision::Off), Palette::DEFAULT, "the light theme is the engine's colours");
        let rgb = |c: Color32| Rgb(c.r(), c.g(), c.b());
        let (p, t) = (scene_palette(true, ColorVision::Off), Tokens::dark());
        assert_eq!(p.background, (rgb(t.viewport_top), rgb(t.viewport_bottom)));
        assert_eq!(
            [p.grid, p.grid_major, p.edge, p.body, p.body_selected, p.cap],
            [t.grid, t.grid_major, t.body_edge, t.body, t.body_selected, t.section_cap].map(rgb)
        );
        assert_ne!(p.background, Palette::DEFAULT.background);
        for (dark, mode) in [false, true].into_iter().flat_map(|d| ColorVision::ALL.map(|m| (d, m))) {
            let (p, t) = (scene_palette(dark, mode), Tokens::themed(dark, mode));
            assert_eq!(p.axes, t.axes().map(rgb), "dark {dark}, {mode:?}: the axes");
            assert_eq!(p.analysis, t.analysis, "dark {dark}, {mode:?}: the surface analysis");
            for (tag, cpu) in [
                (colors::SKETCH, p.sketch),
                (colors::SKETCH_FIXED, p.sketch_fixed),
                (colors::SKETCH_DONE, p.sketch_done),
                (colors::SKETCH_LOCKED, p.sketch_locked),
                (colors::SKETCH_PROJECTED, p.sketch_projected),
                (colors::SKETCH_LOST, p.sketch_lost),
                (colors::SKETCH_CONSTRUCTION, p.sketch_construction),
                (colors::SKETCH_CENTERLINE, p.sketch_centerline),
            ] {
                let [r, g, b, _] = sketch_color(&t, tag);
                assert_eq!(cpu, Rgb(r, g, b), "dark {dark}, {mode:?}: the window draws {tag:?} in another colour");
            }
        }
    }

    /// The view's picture follows a theme change: its background is the window's.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_view_picture_follows_the_theme() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.cam = solidcraft_engine::view::home_camera(&h.app.session);
        for dark in [false, true, false] {
            h.app.ui.dark = dark;
            let img = crate::control::view_rgba(&h.app, 160, 120).unwrap();
            // The top row's most common colour: the background's top.
            let mut counts = std::collections::HashMap::new();
            for px in img.as_chunks::<4>().0.iter().take(160) {
                *counts.entry([px[0], px[1], px[2]]).or_insert(0) += 1;
            }
            let px = counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c).unwrap_or_default();
            let want = if dark { Tokens::dark() } else { Tokens::light() }.viewport_top;
            let near = px.iter().zip([want.r(), want.g(), want.b()]).all(|(a, b)| a.abs_diff(b) <= 2);
            assert!(near, "dark {dark}: top row {px:?}, the window's background {want:?}");
        }
    }

    /// Changing only the 3D view's dark or light (#4146) rebuilds the view's scene and highlights
    /// in its colours, as a change of the window's theme does.
    #[test]
    fn changing_only_the_views_dark_or_light_rebuilds_its_scene() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.call("ui.prefs", json!({"theme": "dark"}));
        h.frames(2);
        let before = (scene_key(&h.app), highlight_geometry_key(&h.app));
        h.call("ui.prefs", json!({"viewport_theme": "light"}));
        h.frames(2);
        assert!(h.app.ui.dark, "the window stays dark");
        assert_ne!(scene_key(&h.app), before.0, "the scene is rebuilt");
        assert_ne!(highlight_geometry_key(&h.app), before.1, "and so are the highlights");
    }

    /// A section's cut outlines are drawn onto the view, so a light view in a dark window draws
    /// them in the light side's text colour (#4146): the window's would be pale on the light view.
    #[test]
    fn a_sections_outlines_take_the_views_text_colour() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).unwrap();
        h.app.run("inspect.section", json!({"plane": "XZ", "at": 15})).unwrap();
        h.call("ui.prefs", json!({"theme": "dark", "viewport_theme": "light"}));
        h.frames(2);
        let scene = build_scene(&h.app);
        let colours: Vec<[u8; 4]> = scene
            .lines
            .as_chunks::<32>()
            .0
            .iter()
            .filter_map(|l| {
                let i = u32::from_le_bytes(l.get(24..28)?.try_into().ok()?);
                scene.palette.get(crate::gpu::PaletteIndex(i))
            })
            .collect();
        let (view, window) = (Tokens::light().text.gamma_multiply(0.9), Tokens::dark().text.gamma_multiply(0.9));
        assert!(colours.contains(&c4(view)), "the outlines in the view's text colour");
        assert!(!colours.contains(&c4(window)), "none in the window's");
    }

    /// The view's picture is cut by the section the viewport shows: an open Section Analysis
    /// dialog's preview, a sketch's Slice.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_view_picture_uses_section_preview_and_sketch_slice() {
        fn pixels(h: &mut crate::scenario::Harness, cam: Camera) -> Vec<[u8; 4]> {
            h.app.cam = cam;
            let img = crate::control::view_rgba(&h.app, 320, 240).unwrap();
            img.as_chunks::<4>().0.to_vec()
        }
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
        let cam = solidcraft_engine::view::home_camera(&h.app.session);
        let whole = pixels(&mut h, cam);
        let before = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        h.app.session.selection = vec![Sel::Plane { name: "XZ".into() }];
        h.app.start("inspect.section");
        let Some(crate::dialogs::Kind::Section { offset, .. }) = h.app.dialog.as_mut().map(|d| &mut d.kind) else { panic!("section dialog") };
        *offset = "15 mm".into();
        assert!(h.app.session.section.is_none());
        let preview = pixels(&mut h, cam);
        assert!(whole.iter().zip(&preview).filter(|(a, b)| a != b).count() > 100, "the picture ignored the uncommitted section preview");
        assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        h.app.run("inspect.section", json!({"plane":"XZ","offset":15})).unwrap();
        h.app.dialog = None;
        assert_eq!(preview, pixels(&mut h, cam), "preview and committed section must draw identically");

        h.app.session.section = None;
        h.app.run("sketch.create", json!({"plane":{"origin":[0,0,10],"x_dir":[1,0,0],"y_dir":[0,1,0]}})).unwrap();
        let whole = pixels(&mut h, cam);
        h.app.run("sketch.options", json!({"slice":true})).unwrap();
        assert!(h.app.session.section.is_none());
        let sliced = pixels(&mut h, cam);
        assert!(whole.iter().zip(&sliced).filter(|(a, b)| a != b).count() > 100, "the picture ignored the active Sketch Slice");
        h.app.run("sketch.options", json!({"slice":false})).unwrap();
        assert_eq!(whole, pixels(&mut h, cam), "disabling Slice restores the whole-body pixels");
    }

    #[test]
    fn timeline_modifiers_highlight_on_hover_and_click() {
        for command in ["solid.fillet", "solid.chamfer"] {
            let mut h = crate::scenario::Harness::new();
            h.app.set_origin_shown(false);
            let base = h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap()["feature"].as_u64().unwrap();
            let id = h.app.run(command, json!({"edges":[[0,0,10]],"radius":2,"distance":2,"name":"Blend"})).unwrap()["feature"].as_u64().unwrap();
            h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"corner":[40,0,0]})).unwrap();
            h.frames(2);
            let geometry = feature_geometry(&h.app, id);
            assert_eq!(geometry.len(), 1, "{command}: {geometry:?}");
            let Sel::Face { body, index, .. } = &geometry[0] else { panic!("a modifier must highlight its face") };
            let st = h.app.session.world_state();
            let m = st.body(body).unwrap().mesh();
            let triangles = m.tri_face.iter().filter(|f| **f as usize == *index).count();
            assert!(triangles > 0 && triangles < m.triangles.len());
            let p = crate::scenario::handle_at("timeline:Blend").unwrap();
            h.step(&json!({"move":[p.x,p.y]})).unwrap();
            assert_eq!(h.app.viewport.hover_feature, Some(id));
            assert_eq!(build_highlight(&h.app).merged().tris.len(), triangles * 3 * crate::gpu::TRI_SIZE);
            assert!(!build_highlight(&h.app).merged().lines.is_empty());
            h.step(&json!({"click":[p.x,p.y]})).unwrap();
            assert_eq!(h.app.session.selection, vec![Sel::Feature { id }]);
            // Moving away removes hover but leaves the selected blend highlighted.
            h.step(&json!({"move":[5,5]})).unwrap();
            assert_eq!(h.app.viewport.hover_feature, None);
            assert_eq!(build_highlight(&h.app).merged().tris.len(), triangles * 3 * crate::gpu::TRI_SIZE);
            let bounds = selection_bounds(&h.app).unwrap();
            assert!(bounds.max.x <= 2.01 && bounds.max.y <= 2.01 && bounds.max.z > 19.99, "{bounds:?}");
            // Creator features retain whole-body feedback, while hidden bodies stay hidden.
            assert_eq!(feature_geometry(&h.app, base), vec![Sel::Body { name: body.clone() }]);
            h.app.session.visibility.hidden_bodies.push(body.clone());
            assert!(feature_geometry(&h.app, id).is_empty());
            assert!(selection_bounds(&h.app).is_none());
            assert!(build_highlight(&h.app).merged().tris.is_empty());
        }
    }

    /// A sketch picked in the timeline or the Browser lights all its curves in the selection
    /// colour, also while it is hidden (#7156); nothing is lit once it is no longer picked.
    #[test]
    fn a_picked_sketch_lights_its_curves_shown_or_hidden() {
        let mut h = crate::scenario::Harness::new();
        h.app.set_origin_shown(false);
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY","name":"Outline"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[30,0],"p1":[40,10]})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[35,5],"radius":3,"construction":true})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let id = h.app.session.doc.find_feature("Outline").unwrap().id;
        h.frames(2);
        let sel_edge = c4(Tokens::get().sel_edge);
        // The selection-coloured strokes of the highlight, drawn over the model (a line holds its
        // colour's palette index).
        let lit = |app: &SolveApp| {
            let scene = build_highlight(app).merged();
            scene
                .overlay
                .as_chunks::<{ crate::gpu::LINE_SIZE }>()
                .0
                .iter()
                .filter(|line| {
                    let index = line.get(24..28).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
                    scene.palette.get(crate::gpu::PaletteIndex(index)) == Some(sel_edge)
                })
                .count()
        };
        assert_eq!(lit(&h.app), 0, "nothing is picked");
        let p = crate::scenario::handle_at("timeline:Outline").unwrap();
        h.step(&json!({"click":[p.x,p.y]})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::Feature { id }]);
        let shown = lit(&h.app);
        // Four sides, the circle's many segments and its dashes.
        assert!(shown > 20, "the picked sketch's curves are lit: {shown}");
        h.app.session.visibility.hidden_sketches.push(id);
        assert!(!visible_sketches(&h.app).contains(&id));
        assert_eq!(lit(&h.app), shown, "a hidden sketch is lit too");
        h.app.session.selection = vec![Sel::Feature { id: h.app.session.doc.features[0].id }];
        assert_eq!(lit(&h.app), 0, "another feature picked");
        h.app.session.selection.clear();
        assert_eq!(lit(&h.app), 0, "nothing is picked");
    }

    #[test]
    fn history_marker_shows_only_its_sketch_without_changing_visibility() -> Result<(), Box<dyn std::error::Error>> {
        for explicitly_hidden in [false, true] {
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.ui.show_grid = false;
            let mut sketches = Vec::new();
            for (name, x) in [("First", 0), ("Second", 30)] {
                app.run("sketch.create", json!({"plane":"XY","name":name}))?;
                app.run("sketch.rectangle.two_point", json!({"p0":[x,0],"p1":[x+10,10]}))?;
                app.run("sketch.finish", json!({}))?;
                let id = app.session.doc.find_feature(name).ok_or("missing sketch")?.id;
                sketches.push(id);
                app.run("solid.extrude", json!({"sketch":name,"distance":5,"body_name":name}))?;
            }
            if explicitly_hidden {
                app.session.visibility.hidden_sketches.clone_from(&sketches);
            }
            let stored = app.session.visibility.clone();
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(640., 480.));
            for (position, expected) in [(0, None), (1, sketches.first().copied()), (2, None), (3, sketches.get(1).copied()), (4, None)] {
                app.run("timeline.roll_to", json!({"position":position}))?;
                assert_eq!(visible_sketches(&app), expected.into_iter().collect::<Vec<_>>(), "position {position}, hidden {explicitly_hidden}");
                let scene = build_sketch_scene(&app, &projection(&app, rect)).merged();
                assert_eq!(!scene.lines.is_empty(), expected.is_some(), "sketch lines at position {position}");
                assert_eq!(app.session.visibility, stored, "stepping must preserve stored visibility");
            }
        }
        Ok(())
    }

    #[test]
    fn timeline_modifier_highlight_follows_edits_and_rollback() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        let id = app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2})).unwrap()["feature"].as_u64().unwrap();
        // A later chamfer changes the same body without consuming the fillet.
        app.run("solid.chamfer", json!({"edges":[[20,20,10]],"distance":1})).unwrap();
        app.run("solid.move", json!({"bodies":["Body1"],"translate":[30,0,0]})).unwrap();
        app.run("body.rename", json!({"body":"Body1","name":"Moved"})).unwrap();
        app.session.selection = vec![Sel::Feature { id }];
        let bounds = selection_bounds(&app).unwrap();
        assert!(bounds.min.x >= 29.99 && bounds.max.x <= 32.01, "{bounds:?}");
        assert!(feature_geometry(&app, id).iter().all(|x| matches!(x, Sel::Face { body, .. } if body == "Moved")));
        app.run("timeline.roll_to", json!({"position":1})).unwrap();
        assert!(feature_geometry(&app, id).is_empty());
        app.run("timeline.roll_to", json!({})).unwrap();
        assert!(!feature_geometry(&app, id).is_empty());
        app.run("timeline.suppress", json!({"feature":id})).unwrap();
        assert!(feature_geometry(&app, id).is_empty());
    }

    #[test]
    fn timeline_modifier_highlight_follows_component_instances() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("component.create", json!({"name":"Part","grounded":false})).unwrap();
        app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        let id = app.run("solid.fillet", json!({"edges":[[0,0,10]],"radius":2})).unwrap()["feature"].as_u64().unwrap();
        app.run("occurrence.move", json!({"occurrence":"Part:1","translate":[30,0,0]})).unwrap();
        app.run("occurrence.copy", json!({"component":"Part","translate":[0,40,0]})).unwrap();
        let geometry = feature_geometry(&app, id);
        assert_eq!(geometry.len(), 2, "both occurrences retain the fillet's face: {geometry:?}");
        app.session.selection = vec![Sel::Feature { id }];
        let bounds = selection_bounds(&app).unwrap();
        assert!(bounds.min.x >= 29.99 && bounds.max.x <= 32.01 && bounds.max.y >= 41.99, "{bounds:?}");
    }

    /// The parts the instancing tests place: a box (flat triangles, its vertices shared by
    /// position and each fragment lit with its triangle's normal) and a cylinder (smooth, its
    /// vertices shared by position and normal).
    const PARTS: [(&str, &str); 2] = [("solid.box", r#"{"length":20,"width":10,"height":5}"#), ("solid.cylinder", r#"{"radius":6,"height":12}"#)];

    /// A design with a part (`command` with `params`, see [`PARTS`]) placed three times (one
    /// placement turned), drawn on a device that builds bodies on the CPU.
    fn three_placed_parts((command, params): (&str, &str)) -> SolveApp {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.ground_shadow = false;
        app.set_origin_shown(false);
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        app.run("component.create", json!({"name":"Part"})).unwrap();
        app.run(command, serde_json::from_str(params).unwrap()).unwrap();
        app.run("occurrence.copy", json!({"component":"Part","translate":[40,0,0]})).unwrap();
        let turned = app.run("occurrence.copy", json!({"component":"Part","translate":[0,40,0]})).unwrap()["occurrence"].clone();
        app.run("occurrence.move", json!({"occurrence":turned,"axis":[1,1,0],"angle":"30 deg"})).unwrap();
        // With the part active, its other placements are drawn faded.
        app.run("component.activate", json!({"component":"root"})).unwrap();
        app
    }

    /// A part placed by several occurrences is drawn from one copy of its triangles, once per
    /// placement: one instanced mesh with a placement for each occurrence, whose triangles land
    /// where each occurrence's own moved mesh has them, and no moved copy is made to draw them.
    #[test]
    fn a_part_placed_many_times_is_drawn_from_one_copy_of_its_triangles() {
        for (part, flat) in PARTS.into_iter().zip([true, false]) {
            placed_three_times_is_drawn_from_one_copy(part, flat);
        }
    }

    fn placed_three_times_is_drawn_from_one_copy(part: (&str, &str), flat: bool) {
        let app = three_placed_parts(part);
        let st = app.session.world_state();
        assert_eq!(st.bodies.len(), 3);
        eprintln!("DBG before {}", st.bodies.iter().filter(|b| b.cached_mesh().is_some()).count());
        let scene = build_scene(&app);
        assert_eq!(scene.instanced.len(), 1, "one shared mesh");
        assert_eq!(scene.instanced[0].count(), 3, "a placement per occurrence");
        let shared = &scene.instanced[0].tris;
        assert_eq!(shared.flat, flat, "{part:?}");
        let corners = shared.indices.len();
        let vertices = shared.vertices.len() / crate::gpu::SHARED_SIZE;
        assert!(vertices * 2 < corners, "{part:?}: {vertices} vertices shared by {corners} corners");
        assert!(scene.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no body's triangles built where it stands");
        let moved = st.bodies.iter().filter(|b| b.cached_mesh().is_some()).count();
        assert!(moved <= 1, "{moved} bodies made a moved mesh to be drawn");
        let merged = scene.merged();
        // Each placement's triangles where its own mesh has them, with its normals.
        let vertex = |b: &[u8]| -> [f32; 6] { std::array::from_fn(|i| f32::from_le_bytes([b[4 * i], b[4 * i + 1], b[4 * i + 2], b[4 * i + 3]])) };
        let mut drawn: Vec<[f32; 6]> = merged.tris.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().map(|v| vertex(v)).collect();
        let mut expected: Vec<[f32; 6]> = Vec::new();
        for b in &st.bodies {
            let m = b.mesh();
            for i in m.triangles.iter().flatten().map(|i| *i as usize) {
                let (p, n) = (m.positions[i].to_f32(), m.normals[i].to_f32());
                expected.push([p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
        }
        let order = |a: &[f32; 6], b: &[f32; 6]| a.iter().map(|x| (x * 1e3).round() as i64).cmp(b.iter().map(|x| (x * 1e3).round() as i64));
        drawn.sort_by(order);
        expected.sort_by(order);
        assert_eq!(drawn.len(), expected.len());
        for (d, e) in drawn.iter().zip(&expected) {
            assert!(d.iter().zip(e).all(|(x, y)| (x - y).abs() < 1e-3), "{d:?} vs {e:?}");
        }
        // Each placement's edges are drawn where it stands.
        let one = st.bodies[0].mesh();
        let segments: usize = one
            .edges
            .iter()
            .enumerate()
            .filter(|(i, _)| one.edge_pickable(*i) && one.shaded_edge_visible(*i))
            .map(|(_, e)| e.len().saturating_sub(1))
            .sum();
        assert_eq!(merged.lines.len(), 3 * segments * crate::gpu::LINE_SIZE);
    }

    #[test]
    fn imported_mesh_edges_draw_in_each_style_without_brep_picks() {
        let h = crate::scenario::Harness::new();
        let path = h.dir.join("box.3mf");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":20,"width":20,"height":10})).unwrap();
        app.run("file.save_mesh", json!({"path":path})).unwrap();
        app.run("file.new", json!({})).unwrap();
        let imported = app.run("file.insert_mesh", json!({"path":path})).unwrap();
        app.run("component.activate", json!({"component":"root"})).unwrap();
        app.run("occurrence.copy", json!({"component":imported["component"],"translate":[40,0,0]})).unwrap();
        app.ui.show_grid = false;
        app.set_origin_shown(false);
        app.ui.ground_shadow = false;
        app.fit_view();
        for style in 0..4 {
            app.ui.visual_style = style;
            let scene = build_scene(&app).merged();
            let lines = if style == 1 { 0 } else { 24 * crate::gpu::LINE_SIZE };
            assert_eq!(scene.lines.len(), lines, "style {style}: twelve edges per placed box");
            assert_eq!(scene.hidden.len(), if style == 3 { lines } else { 0 });
            assert_eq!(scene.tris.is_empty(), style == 2, "wireframe draws only the edges");
        }
        let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(800., 600.)));
        for body in &app.session.world_state().bodies {
            let mesh = body.mesh();
            assert_eq!(mesh.edges.len(), 12, "the placed mesh keeps its outlines");
            for (i, edge) in mesh.edges.iter().enumerate() {
                assert!(!mesh.edge_pickable(i), "placements keep display-only edge semantics");
                for point in [edge[0], (edge[0] + edge[1]) * 0.5] {
                    let hits = pick_all(&app, &proj, proj.to_screen(point).unwrap());
                    assert!(!hits.iter().any(|h| matches!(h, Hit::Edge { .. } | Hit::Vertex { .. })), "{hits:?}");
                }
            }
        }
    }

    #[test]
    fn mesh_outline_hover_cache_follows_measure_and_preserves_modeling_picks() {
        let h = crate::scenario::Harness::new();
        let path = h.dir.join("box.stl");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":20,"width":30,"height":10})).unwrap();
        app.run("file.save_mesh", json!({"path":path})).unwrap();
        app.run("file.new", json!({})).unwrap();
        app.run("file.insert_mesh", json!({"path":path})).unwrap();
        app.set_origin_shown(false);
        app.fit_view();
        let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(800., 600.)));
        let pos = proj.to_screen(Vec3::new(20., 0., 5.)).unwrap();
        let measure = crate::dialogs::Dialog::for_command(&app, "inspect.measure").unwrap();
        let mut fillet = crate::dialogs::Dialog::for_command(&app, "solid.fillet").unwrap();
        // Identical input masks must not make the two dialogs share cached geometry hits.
        fillet.inputs[0].accept = measure.inputs[0].accept;
        app.dialog = Some(fillet.clone());
        assert!(!pick_cached(&mut app, &proj, pos).iter().any(|h| matches!(h, Hit::Edge { .. })));
        app.dialog = Some(measure);
        assert!(matches!(pick_cached(&mut app, &proj, pos).first(), Some(Hit::Edge { .. })));
        let hits = pick_all(&app, &proj, pos);
        assert!(hits.iter().any(|h| matches!(h, Hit::Edge { .. })), "Select Other also offers the measured outline");
        app.dialog = Some(fillet);
        assert!(!pick_cached(&mut app, &proj, pos).iter().any(|h| matches!(h, Hit::Edge { .. })));
        app.dialog = None;
        let hits = pick_cached(&mut app, &proj, pos);
        assert!(!hits.iter().any(|h| matches!(h, Hit::Edge { .. } | Hit::Vertex { .. })));
        assert!(app.session.world_state().bodies[0].mesh().display_only_edges);
    }

    /// Manual GPU benchmark: 25 placements of the mesh `SOLIDCRAFT_BENCH_MESH` (an STL, OBJ or
    /// 3MF) drawn offscreen at 3600 × 2190 px (divided by `SOLIDCRAFT_BENCH_DIV`) with 4× MSAA
    /// while the camera turns, each frame waited for: the placements' own triangles (`merged`, as
    /// before instancing) and as instances. Prints the median frame time of each.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "manual: SOLIDCRAFT_BENCH_MESH=part.stl cargo test --release -p solidcraft-ui-egui --lib -- --ignored instanced_scene_gpu_benchmark --nocapture"]
    fn instanced_scene_gpu_benchmark() {
        use egui_wgpu::wgpu;
        let Some(path) = std::env::var_os("SOLIDCRAFT_BENCH_MESH") else { return };
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.ground_shadow = false;
        app.set_origin_shown(false);
        let component = app.run("file.insert_mesh", json!({"path": path.to_string_lossy()})).unwrap()["component"].clone();
        app.run("component.activate", json!({"component":"root"})).unwrap();
        let size = app.session.world_state().bodies[0].mesh_bounds().size();
        let step = size.x.max(size.y) * 1.3;
        for k in 1..25 {
            app.run("occurrence.copy", json!({"component":component,"translate":[f64::from(k % 5) * step, f64::from(k / 5) * step, 0.0]})).unwrap();
        }
        // `SOLIDCRAFT_BENCH_OPACITY=0.5`: every placement see-through.
        if let Some(opacity) = std::env::var("SOLIDCRAFT_BENCH_OPACITY").ok().and_then(|v| v.parse::<f64>().ok()) {
            app.run("appearance.assign", json!({"components": [component.to_string()], "color": "#b4b8bc", "opacity": opacity})).unwrap();
        }
        let Some((_, device, queue)) = crate::gpu::offscreen_device() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let scale: u32 = std::env::var("SOLIDCRAFT_BENCH_DIV").ok().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
        let (w, h, samples) = (3600u32 / scale, 2190u32 / scale, 4u32);
        let depth = solidcraft_gpu::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        app.viewport.gpu = Some(GpuTarget { format, depth: Some(depth), samples, tess: false, oit: false });
        app.fit_view();
        let texture = |format, samples| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("bench"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let (color, resolve, depth_view) = (texture(format, samples), texture(format, 1), texture(depth, samples));
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32));
        let t0 = std::time::Instant::now();
        let scene = build_scene(&app);
        let t1 = std::time::Instant::now();
        drop(build_scene(&app));
        eprintln!("scene built in {:.0} ms, again in {:.0} ms", (t1 - t0).as_secs_f64() * 1e3, t1.elapsed().as_secs_f64() * 1e3);
        let proj = projection(&app, rect);
        let st = app.session.world_state();
        let seen = st.bodies.iter().filter(|b| proj.to_screen(b.mesh_bounds().center()).is_some_and(|p| rect.contains(p))).count();
        eprintln!("{w} x {h} px: {seen} of {} placements in view", st.bodies.len());
        // `SOLIDCRAFT_BENCH_CLOSE=1`: close up on the middle placement, most of the others out of view.
        if std::env::var("SOLIDCRAFT_BENCH_CLOSE").is_ok_and(|v| v == "1") {
            let middle = st.bodies.iter().map(|b| b.mesh_bounds()).nth(12).unwrap_or_default();
            app.cam.target = middle.center();
            app.cam.distance = middle.diagonal() * 1.5;
            let proj = projection(&app, rect);
            let seen = st.bodies.iter().filter(|b| proj.to_screen(b.mesh_bounds().center()).is_some_and(|p| rect.contains(p))).count();
            eprintln!("close up: {seen} of {} placements' centres in view", st.bodies.len());
        }
        let yaw = app.cam.yaw;
        // The levels of detail, made off the window thread meanwhile.
        let t = std::time::Instant::now();
        while app.viewport.mesh_lods.busy() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let lod = build_scene(&app);
        eprintln!(
            "levels of detail made in {:.1} s: {:?}",
            t.elapsed().as_secs_f64(),
            lod.instanced.iter().map(|m| m.levels.iter().map(|(t, e)| (t.indices.len() / 3, *e)).collect::<Vec<_>>()).collect::<Vec<_>>()
        );
        // `SOLIDCRAFT_BENCH_PNG=dir`: the view drawn from each placement's own triangles, in full
        // as instances, and from the levels (`SOLIDCRAFT_BENCH_PNG_ONLY=1`: then stop).
        if let Some(dir) = std::env::var_os("SOLIDCRAFT_BENCH_PNG") {
            const SIDE: u32 = 1024;
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIDE as f32, SIDE as f32));
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            for (name, scene) in [("own", scene.clone().merged()), ("full", scene.clone()), ("lod", lod.clone())] {
                if let Some(rgba) = crate::gpu::render_offscreen(
                    scene,
                    None,
                    proj.vp.to_f32(),
                    proj.cam.back().to_f32(),
                    eye,
                    SIDE,
                    None,
                    [0.0; 8],
                    crate::gpu::analysis_colors(&Tokens::get().analysis),
                ) && let Some(png) = solidcraft_engine::render::png(SIDE as usize, SIDE as usize, rgba)
                {
                    let _ = std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png);
                }
            }
        }
        if std::env::var_os("SOLIDCRAFT_BENCH_PNG_ONLY").is_some() {
            return;
        }
        for (label, scene) in [("own triangles", scene.clone().merged()), ("instanced", scene), ("instanced, levels of detail", lod)] {
            let mut renderer = crate::gpu::Renderer::new(&device, GpuTarget { format, depth: Some(depth), samples, tess: false, oit: false }, None);
            let slot: SceneSlot = std::sync::Arc::new(std::sync::Mutex::new(Some(scene)));
            let none = || std::sync::Arc::new(std::sync::Mutex::new(None));
            let (mut times, mut prepared) = (Vec::new(), Vec::new());
            for k in 0..40 {
                app.cam.yaw = yaw + f64::from(k) * 0.05;
                let proj = projection(&app, rect);
                let e = proj.cam.eye().to_f32();
                let frame = Frame {
                    product: false,
                    key: 1,
                    slot: slot.clone(),
                    sketch_key: 0,
                    sketch_slot: none(),
                    hl_key: 0,
                    hl_slot: none(),
                    pv_key: 0,
                    pv_slot: none(),
                    pv_hide: false,
                    view_proj: proj.vp.to_f32(),
                    back: proj.cam.back().to_f32(),
                    size_px: [w as f32, h as f32],
                    clip: [0.0; 4],
                    cap: [0.0; 4],
                    images: Vec::new(),
                    analysis: [0.0; 8],
                    analysis_colors: crate::gpu::analysis_colors(&Tokens::get().analysis),
                    access: None,
                    outline: true,
                    eye: [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }],
                    ao_radius: 0.0,
                    ssaa: 1.0,
                    hover_tint: [0.0; 3],
                    selection_tint: [0.0; 3],
                };
                let t0 = std::time::Instant::now();
                let mut encoder = device.create_command_encoder(&Default::default());
                renderer.prepare(&device, &queue, &mut encoder, &frame);
                if k >= 5 {
                    prepared.push(t0.elapsed().as_secs_f64() * 1e3);
                }
                {
                    let mut pass = encoder
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("bench"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &color,
                                depth_slice: None,
                                resolve_target: Some(&resolve),
                                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Discard },
                            })],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &depth_view,
                                depth_ops: Some(wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(solidcraft_gpu::view::far_depth(solidcraft_gpu::view::reversed_depth(Some(depth)))),
                                    store: wgpu::StoreOp::Discard,
                                }),
                                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                            }),
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        })
                        .forget_lifetime();
                    renderer.paint(&frame, &mut pass, [0.0, 0.0, w as f32, h as f32]);
                }
                queue.submit([encoder.finish()]);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                if k >= 5 {
                    times.push(t0.elapsed().as_secs_f64() * 1e3);
                }
            }
            times.sort_by(f64::total_cmp);
            prepared.sort_by(f64::total_cmp);
            eprintln!(
                "{label}: median {:.1} ms a frame (min {:.1}, max {:.1}), of which {:.1} ms preparing on the CPU",
                times[times.len() / 2],
                times[0],
                times[times.len() - 1],
                prepared[prepared.len() / 2]
            );
        }
    }

    /// A large mesh's shared vertices made ahead (by the background mesher after a file is
    /// opened or inserted) are the ones its first scene draws: that frame welds nothing.
    #[test]
    fn a_large_meshs_shared_vertices_are_made_ahead_of_its_first_scene() {
        use solidcraft_engine::doc::{Document, FeatureKind, MeshData};
        // A flat grid of 160 × 160 squares: 51 200 triangles, each corner its own vertex.
        let n = 160u32;
        let corners: Vec<[u32; 2]> =
            (0..n).flat_map(|j| (0..n).flat_map(move |i| [[i, j], [i + 1, j], [i + 1, j + 1], [i, j], [i + 1, j + 1], [i, j + 1]])).collect();
        let mut doc = Document::default();
        let meshes = vec![MeshData {
            name: "Grid".into(),
            positions: corners.iter().flat_map(|[x, y]| [f64::from(*x), f64::from(*y), 0.0]).collect(),
            triangles: (0..u32::try_from(corners.len()).unwrap()).collect(),
            color: None,
        }];
        doc.add_feature(FeatureKind::MeshImport { file: "grid.stl".into(), meshes }, Some("Grid")).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::new(doc), crate::Services::default());
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        let (mesh, _) = app.session.world_state().bodies[0].mesh_in_place();
        assert!(mesh.triangles.len() >= INSTANCE_ALONE);
        warm_shared_tris(&mesh);
        let warmed = WARMED.lock().unwrap().iter().find(|(m, _)| std::sync::Arc::ptr_eq(m, &mesh)).map(|(_, t)| t.clone()).unwrap();
        assert_eq!(warmed.vertices.len() / crate::gpu::SHARED_SIZE, 161 * 161, "the grid's corners shared");
        let scene = build_scene(&app);
        assert_eq!(scene.instanced.len(), 1, "a large mesh alone is drawn from shared vertices");
        assert!(std::sync::Arc::ptr_eq(&scene.instanced[0].tris, &warmed), "the ones made ahead");
        assert!(!WARMED.lock().unwrap().iter().any(|(m, _)| std::sync::Arc::ptr_eq(m, &mesh)), "taken");
    }

    /// A design with one imported mesh of about 100 000 triangles (a sphere of radius 50, each
    /// corner its own point, as an STL is read), drawn on a device that builds bodies on the CPU.
    fn large_scan() -> SolveApp {
        use solidcraft_engine::doc::{Document, FeatureKind, MeshData};
        let n = 160u32;
        let at = |i: u32, j: u32| {
            let (t, p) = (std::f64::consts::PI * f64::from(i) / f64::from(n), std::f64::consts::TAU * f64::from(j) / f64::from(2 * n));
            let (st, sp, cp) =
                (if i == 0 || i == n { 0.0 } else { t.sin() }, if j == 2 * n { 0.0 } else { p.sin() }, if j == 2 * n { 1.0 } else { p.cos() });
            [50.0 * st * cp, 50.0 * st * sp, 50.0 * t.cos()]
        };
        let corners: Vec<[f64; 3]> = (0..n)
            .flat_map(|i| (0..2 * n).flat_map(move |j| [at(i, j), at(i + 1, j + 1), at(i, j + 1), at(i, j), at(i + 1, j), at(i + 1, j + 1)]))
            .collect();
        let mut doc = Document::default();
        let meshes = vec![MeshData {
            name: "Scan".into(),
            positions: corners.iter().flatten().copied().collect(),
            triangles: (0..u32::try_from(corners.len()).unwrap()).collect(),
            color: None,
        }];
        doc.add_feature(FeatureKind::MeshImport { file: "scan.stl".into(), meshes }, Some("Scan")).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::new(doc), crate::Services::default());
        app.ui.ground_shadow = false;
        app.set_origin_shown(false);
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        app
    }

    /// A large mesh is drawn in full until its levels of detail are made off the window thread;
    /// then the scene is built again with them, and seen small, a coarse level draws it as the
    /// full triangles do (within a fraction of a pixel). Skipped without a GPU adapter (the
    /// levels are checked anyway).
    #[test]
    fn a_large_mesh_seen_small_is_drawn_from_its_levels_of_detail() {
        let mut app = large_scan();
        let (mesh, _) = app.session.world_state().bodies[0].mesh_in_place();
        assert!(mesh.triangles.len() >= INSTANCE_ALONE);
        let key = scene_key(&app);
        let first = build_scene(&app);
        assert_eq!(first.instanced.len(), 1);
        assert!(first.instanced[0].levels.is_empty(), "drawn in full meanwhile");
        let began = std::time::Instant::now();
        while app.viewport.mesh_lods.busy() {
            assert!(began.elapsed() < std::time::Duration::from_secs(120), "the levels are made");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_ne!(scene_key(&app), key, "the scene is built again when they are");
        let scene = build_scene(&app);
        let levels = &scene.instanced[0].levels;
        assert!(levels.len() >= 2, "{} levels", levels.len());
        // Small on screen: the sphere about 30 px across.
        app.fit_view();
        app.cam.distance *= 4.0;
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |scene: GpuScene| {
            crate::gpu::render_offscreen(
                scene,
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        // The level the placement is drawn from there.
        let rows = [[1.0f32, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let scale = crate::gpu::placement_scale(&rows, &scene.instanced[0].tris.bounds, &proj.vp.to_f32(), [SIZE as f32; 2]);
        let errors: Vec<f32> = std::iter::once(0.0).chain(levels.iter().map(|(_, e)| *e)).collect();
        assert!(crate::gpu::level_for(&errors, scale) >= 1, "a coarse level: scale {scale} px/mm, errors {errors:?}");
        let (Some(lod), Some(full)) = (render(scene.clone()), render(scene.merged())) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let drawn = full.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        assert!(drawn > 300, "the sphere is drawn: {drawn} pixels");
        let differ = lod.chunks(4).zip(full.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
        assert!(differ * 20 < drawn, "{differ} of {drawn} pixels differ");
    }

    /// See-through and faded placements of a part are drawn as instances too, with their alpha:
    /// glass with an appearance's opacity, faded context with another component active. None
    /// of their triangles go to the glass sorted triangle by triangle, and they look as their
    /// own triangles did. GPU pictures skipped without an adapter.
    #[test]
    fn see_through_and_faded_placements_are_drawn_as_instances() {
        use crate::gpu::InstanceLook;
        let mut app = three_placed_parts(PARTS[0]);
        app.run("appearance.assign", json!({"components": ["Part"], "color": "#3366cc", "opacity": 0.4})).unwrap();
        let glass = build_scene(&app);
        assert_eq!(glass.instanced.len(), 1);
        assert_eq!((glass.instanced[0].look, glass.instanced[0].count()), (InstanceLook::Glass, 3));
        let alpha = |p: &[u8]| {
            let i = u32::from_le_bytes(p.get(48..52)?.try_into().ok()?);
            glass.palette.get(crate::gpu::PaletteIndex(i)).map(|c| c[3])
        };
        assert!(glass.instanced[0].instances.as_chunks::<{ crate::gpu::INSTANCE_SIZE }>().0.iter().all(|p| alpha(p) == Some(102)), "alpha 0.4");
        assert!(glass.glass.is_empty(), "no triangle sorted one by one");
        assert!(!glass.clone().merged().glass.is_empty());
        // One placement active (as right after the part is made): the others faded.
        let mut context = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        context.viewport.gpu = app.viewport.gpu;
        context.ui.ground_shadow = false;
        context.set_origin_shown(false);
        context.run("component.create", json!({"name":"Part"})).unwrap();
        context.run(PARTS[0].0, serde_json::from_str(PARTS[0].1).unwrap()).unwrap();
        context.run("occurrence.copy", json!({"component":"Part","translate":[40,0,0]})).unwrap();
        context.run("occurrence.copy", json!({"component":"Part","translate":[0,40,0]})).unwrap();
        let faded = build_scene(&context);
        let looks: Vec<_> = faded.instanced.iter().map(|m| (m.look, m.count())).collect();
        assert!(looks.contains(&(InstanceLook::Faded, 2)), "the other placements faded: {looks:?}");
        assert!(faded.trans.is_empty());
        for (name, scene) in [("glass", glass), ("faded", faded)] {
            app.fit_view();
            const SIZE: u32 = 256;
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
            let proj = projection(&app, rect);
            let e = proj.cam.eye().to_f32();
            let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
            let render = |scene: GpuScene| {
                crate::gpu::render_offscreen(
                    scene,
                    None,
                    proj.vp.to_f32(),
                    proj.cam.back().to_f32(),
                    eye,
                    SIZE,
                    None,
                    [0.0; 8],
                    crate::gpu::analysis_colors(&Tokens::get().analysis),
                )
            };
            let (Some(instanced), Some(own)) = (render(scene.clone()), render(scene.merged())) else {
                eprintln!("skipping: no GPU adapter");
                return;
            };
            let drawn = own.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
            assert!(drawn > 1000, "{name}: drawn {drawn}");
            let differ = instanced.chunks(4).zip(own.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
            // A pixel here and there along the boxes' edges, where moving the corners on the GPU
            // rounds them differently.
            assert!(differ * 50 < drawn, "{name}: {differ} of {drawn} pixels differ");
        }
    }

    /// Each placement of a part is picked where it stands, the turned one too, from the part's
    /// shared mesh: the face under the cursor names that placement's body, at the point the
    /// cursor is over, and no placement makes a moved copy of the mesh to be picked.
    #[test]
    fn each_placement_is_picked_where_it_stands_from_the_shared_mesh() {
        let mut app = three_placed_parts(PARTS[0]);
        app.fit_view();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let proj = projection(&app, rect);
        let st = app.session.world_state();
        // The top face's centre of each placement, from its moved box corners.
        let tops: Vec<(String, Vec3)> = st
            .bodies
            .iter()
            .map(|b| {
                let (mesh, m) = b.mesh_in_place();
                let local = mesh.bounds();
                (
                    b.name.clone(),
                    solidcraft_engine::doc::apply_point(
                        &m,
                        Vec3::new((local.min.x + local.max.x) / 2.0, (local.min.y + local.max.y) / 2.0, local.max.z),
                    ),
                )
            })
            .collect();
        for (name, top) in &tops {
            let at = proj.to_screen(*top).unwrap();
            let hits = pick(&app, &proj, at);
            let face = hits.iter().find_map(|h| match h {
                Hit::Face { body, point, .. } => Some((body.clone(), *point)),
                _ => None,
            });
            let (body, point) = face.unwrap_or_else(|| panic!("no face of {name} under {at:?}: {hits:?}"));
            assert_eq!(&body, name);
            assert!(point.dist(*top) < 0.05, "{point:?} vs {top:?}");
        }
        assert!(
            st.bodies.iter().all(|b| b.cached_mesh().is_none() || b.mesh_in_place().1 == solidcraft_engine::doc::IDENTITY),
            "a moved copy was made to pick"
        );
    }

    /// A part drawn as instances looks the same as its placements' own triangles drawn one by
    /// one. Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn instances_draw_like_their_placements_own_triangles() {
        for part in PARTS {
            if !placed_three_times_draws_like_its_own_triangles(part) {
                return;
            }
        }
    }

    /// `false` without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    fn placed_three_times_draws_like_its_own_triangles(part: (&str, &str)) -> bool {
        let mut app = three_placed_parts(part);
        app.fit_view();
        const SIZE: u32 = 256;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |scene: GpuScene| {
            crate::gpu::render_offscreen(
                scene,
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        let scene = build_scene(&app);
        assert!(!scene.instanced.is_empty());
        let (Some(instanced), Some(one_by_one)) = (render(scene.clone()), render(scene.clone().merged())) else {
            eprintln!("skipping: no GPU adapter");
            return false;
        };
        let drawn = instanced.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        assert!(drawn > (SIZE * SIZE) as usize / 50, "the parts are drawn: {drawn} pixels");
        let differ = instanced.chunks(4).zip(one_by_one.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
        assert!(differ * 200 < drawn, "{part:?}: {differ} of {drawn} pixels differ");
        // Close up on the turned placement, the others out of view: what is in view is all drawn.
        let turned = app.session.world_state().bodies[2].mesh_bounds();
        app.cam.target = turned.center();
        app.cam.distance = turned.diagonal() * 0.75;
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |scene: GpuScene| {
            crate::gpu::render_offscreen(
                scene,
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        let (Some(instanced), Some(one_by_one)) = (render(scene.clone()), render(scene.merged())) else { return false };
        let drawn = instanced.chunks(4).filter(|p| p[..3] != [255, 255, 255]).count();
        assert!(drawn > (SIZE * SIZE) as usize / 4, "{part:?}: close up, the part fills the view: {drawn} pixels");
        let differ = instanced.chunks(4).zip(one_by_one.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 24)).count();
        assert!(differ * 200 < drawn, "{part:?} close up: {differ} of {drawn} pixels differ");
        true
    }

    #[test]
    fn ray_segment_closest_takes_the_point_under_the_ray() {
        let (o, d) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0));
        let x = |a: Vec3, b: Vec3| ray_segment_closest(o, d, a, b);
        assert!(x(Vec3::new(-5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, -4.0)).dist(Vec3::new(0.0, 0.0, -2.0)) < 1e-9);
        // Beyond an end: the end.
        assert!(x(Vec3::new(1.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0)).dist(Vec3::new(1.0, 0.0, 0.0)) < 1e-9);
        // Parallel, degenerate and non-finite input: the start, no panic.
        assert_eq!(x(Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 5.0)), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(x(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)), Vec3::new(2.0, 0.0, 0.0));
        let nan = Vec3::new(f64::NAN, 0.0, 0.0);
        let _ = x(nan, Vec3::ZERO);
    }

    /// Hovering anywhere along a visible edge between two faces picks that edge, not a face
    /// (the edge used to pick only where its start or middle was as near as the face).
    #[test]
    fn edge_between_two_faces_picks_along_its_whole_length() {
        for style in [0u8, 2] {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.run("solid.box", json!({"length":200,"width":120,"height":160})).unwrap();
            app.set_origin_shown(false);
            app.ui.visual_style = style;
            app.cam.yaw = 0.7;
            app.cam.pitch = 0.5;
            app.cam.target = Vec3::new(100.0, 60.0, 80.0);
            app.cam.distance = 500.0;
            hover_frame(&mut app, &ctx, vec![]);
            let proj = projection(&app, app.viewport.rect.unwrap());
            let st = app.session.world_state();
            let m = st.body("Body1").unwrap().mesh();
            let mut checked = 0;
            for (ei, e) in m.edges.iter().enumerate() {
                let (Some(a), Some(b)) = (e.first().copied(), e.last().copied()) else { continue };
                for k in 1..10 {
                    let p = a.lerp(b, k as f64 / 10.0);
                    let Some(sp) = proj.to_screen(p) else { continue };
                    let (o, d) = proj.ray(sp);
                    // Only edges in front at that point (shaded); every edge in wireframe.
                    let depth = (p - o).dot(d);
                    if style != 2 && m.raycast(o, d).is_some_and(|(t, _)| t < depth - 1e-3) {
                        continue;
                    }
                    let hits = pick(&app, &proj, sp);
                    let first = candidate(&app, &hits).map(|x| x.1);
                    assert!(matches!(first, Some(Sel::Edge { index, .. }) if index == ei), "style {style}: edge {ei} at {k}/10 picked {first:?}");
                    checked += 1;
                }
            }
            assert!(checked > 50, "style {style}: only {checked} samples");
        }
    }

    fn hover_frame(app: &mut SolveApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let selected = app.highlighted();
        let mut output = ctx.run_ui(
            egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() },
            |ui| {
                show(app, ui);
            },
        );
        // This headless frame checks interactions without applying textures to a renderer.
        output.textures_delta.clear();
        // Click selection is applied after rendering; hover itself must already be current.
        if app.viewport.gpu.is_some() && selected == app.highlighted() {
            assert_eq!(app.viewport.hl_key, highlight_key(app), "GPU highlight must use this frame's hover");
        }
    }

    fn hover_app(gpu: bool) -> SolveApp {
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        s.execute("sketch.point", &json!({"point": [0, 0]})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        if gpu {
            // The callback is queued by egui; no device is needed to inspect scene updates.
            app.viewport.gpu =
                Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        }
        app
    }

    fn space(pressed: bool) -> egui::Event {
        egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::default() }
    }

    /// Holding Space is a momentary pan: the drag moves the view rather than the geometry under the
    /// pointer, in every preset. The point is over the sketch's own point, so without Space this
    /// drag would drag it (and the undo stack would grow).
    #[test]
    fn holding_space_pans_instead_of_dragging_geometry() {
        for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
            let ctx = crate::fonts::test_context();
            let mut app = hover_app(false);
            app.preferences.nav = preset.into();
            app.set_origin_shown(false);
            hover_frame(&mut app, &ctx, vec![]);
            let point = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
            let before = app.cam;
            let undo = app.session.undo.len();
            let press = |pressed: bool, at: Pos2| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            hover_frame(&mut app, &ctx, vec![space(true)]);
            hover_frame(&mut app, &ctx, vec![space(true), egui::Event::PointerMoved(point), press(true, point)]);
            hover_frame(&mut app, &ctx, vec![space(true), egui::Event::PointerMoved(point + vec2(40.0, 25.0))]);
            assert_ne!(app.cam, before, "{preset}: holding Space must pan the view");
            assert_eq!(app.session.undo.len(), undo, "{preset}: holding Space must not edit the design");
            assert!(app.viewport.point_drag.is_none(), "{preset}: holding Space must not drag geometry");
            hover_frame(&mut app, &ctx, vec![press(false, point + vec2(40.0, 25.0)), space(false)]);
        }
    }

    #[test]
    fn ctrl_left_drag_from_empty_space_orbits_in_every_preset() {
        for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
            for action in ["orbit", "tool", "point", "click"] {
                let ctx = crate::fonts::test_context();
                let mut app = hover_app(false);
                app.preferences.nav = preset.into();
                app.set_origin_shown(false);
                hover_frame(&mut app, &ctx, vec![]);
                let point = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
                let (start, end) = match action {
                    "point" => (point, point + vec2(30.0, 20.0)),
                    "click" => (point, point),
                    _ => (point - vec2(60.0, 60.0), point - vec2(20.0, 30.0)),
                };
                if action == "tool" {
                    app.start("sketch.line");
                }
                let before = app.cam;
                let undo = app.session.undo.len();
                let modifiers = egui::Modifiers { ctrl: true, command: !cfg!(target_os = "macos"), ..Default::default() };
                let frame = |app: &mut SolveApp, mut events: Vec<egui::Event>| {
                    events.insert(0, egui::Event::ModifiersChanged(modifiers));
                    hover_frame(app, &ctx, events);
                };
                frame(
                    &mut app,
                    vec![
                        egui::Event::PointerMoved(start),
                        egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers },
                    ],
                );
                frame(&mut app, vec![egui::Event::PointerMoved(end)]);
                frame(&mut app, vec![]);
                match action {
                    "orbit" | "tool" => {
                        assert_ne!(app.cam, before, "{preset}: Ctrl+left drag on empty space orbits ({action})");
                        assert!(app.viewport.boxsel.is_none(), "{preset}: no box selection while orbiting");
                    }
                    "point" => {
                        assert_eq!(app.cam, before, "{preset}: Ctrl+drag on sketch geometry is not an orbit");
                        assert!(app.viewport.point_drag.is_some(), "{preset}: Ctrl+drag still drags the point");
                    }
                    _ => assert_eq!(app.cam, before, "{preset}: a Ctrl+click does not orbit"),
                }
                frame(&mut app, vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers }]);
                frame(&mut app, vec![]);
                match action {
                    "click" => assert!(!app.session.selection.is_empty(), "{preset}: Ctrl+click still selects"),
                    "tool" => assert_eq!(app.session.undo.len(), undo, "{preset}: the orbit drag adds no geometry"),
                    "orbit" => assert!(app.session.selection.is_empty(), "{preset}: the end of the orbit selects nothing"),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn openscad_left_drag_preserves_editing_and_selection() {
        for action in ["point", "box", "tool"] {
            let ctx = crate::fonts::test_context();
            let mut app = hover_app(false);
            app.preferences.nav = crate::prefs::OPENSCAD_NAV.into();
            app.set_origin_shown(false);
            hover_frame(&mut app, &ctx, vec![]);
            let point = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
            let (start, end) =
                if action == "point" { (point, point + vec2(30.0, 20.0)) } else { (point - vec2(40.0, 40.0), point + vec2(40.0, 40.0)) };
            if action == "tool" {
                app.start("sketch.line");
                assert!(app.tool.is_some());
            }
            let before = app.cam;
            let undo = app.session.undo.len();
            let modifiers = egui::Modifiers { shift: action == "box", ..Default::default() };
            let frame = |app: &mut SolveApp, mut events: Vec<egui::Event>| {
                events.insert(0, egui::Event::ModifiersChanged(modifiers));
                let mut out = ctx.run_ui(
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() },
                    |ui| show(app, ui),
                );
                out.textures_delta.clear();
            };
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(start),
                    egui::Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers },
                ],
            );
            frame(&mut app, vec![egui::Event::PointerMoved(end)]);
            frame(&mut app, vec![]);
            assert_eq!(app.cam, before, "{action} drag must not orbit");
            if action == "point" {
                assert!(app.viewport.point_drag.is_some());
                assert_eq!(app.session.undo.len(), undo + 1);
            } else if action == "box" {
                assert!(app.viewport.boxsel.is_some());
            } else {
                assert!(app.tool.is_some());
                assert!(app.viewport.primary_nav.is_none());
            }
            frame(&mut app, vec![egui::Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers }]);
            if action == "box" {
                assert!(!app.session.selection.is_empty());
            }
        }
    }

    #[test]
    fn openscad_empty_space_drags_navigate_without_editing_or_box_selecting() {
        for (button, shift) in [
            (egui::PointerButton::Primary, false),
            (egui::PointerButton::Secondary, false),
            (egui::PointerButton::Middle, false),
            (egui::PointerButton::Secondary, true),
            (egui::PointerButton::Middle, true),
        ] {
            let ctx = crate::fonts::test_context();
            let mut app = hover_app(false);
            app.preferences.nav = crate::prefs::OPENSCAD_NAV.into();
            hover_frame(&mut app, &ctx, vec![]);
            let point = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
            let p = if button == egui::PointerButton::Primary { point + vec2(80.0, 80.0) } else { point };
            let before = app.cam;
            let undo = app.session.undo.len();
            let modifiers = egui::Modifiers { shift, ..Default::default() };
            let frame = |app: &mut SolveApp, mut events: Vec<egui::Event>| {
                events.insert(0, egui::Event::ModifiersChanged(modifiers));
                ctx.run_ui(
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))), events, ..Default::default() },
                    |ui| show(app, ui),
                )
                .textures_delta
                .clear();
            };
            frame(&mut app, vec![egui::Event::PointerMoved(p), egui::Event::PointerButton { pos: p, button, pressed: true, modifiers }]);
            let end = p + vec2(30.0, 20.0);
            frame(&mut app, vec![egui::Event::PointerMoved(end)]);
            frame(&mut app, vec![]);
            assert!(app.viewport.hover.is_none());
            assert!(app.viewport.boxsel.is_none() && app.viewport.point_drag.is_none());
            frame(&mut app, vec![egui::Event::PointerButton { pos: end, button, pressed: false, modifiers }]);
            assert_eq!(app.session.undo.len(), undo);
            assert!(app.session.selection.is_empty());
            assert!(app.viewport.context_menu.is_none());
            if button == egui::PointerButton::Primary {
                assert_ne!(app.cam.yaw, before.yaw);
                assert_eq!(app.cam.target, before.target);
                assert_eq!(app.cam.distance, before.distance);
            } else if shift {
                assert_ne!(app.cam.distance, before.distance);
                assert_eq!(app.cam.target, before.target);
                assert_eq!(app.cam.yaw, before.yaw);
            } else {
                assert_ne!(app.cam.target, before.target);
                assert_eq!(app.cam.distance, before.distance);
                assert_eq!(app.cam.yaw, before.yaw);
            }
        }
    }

    #[test]
    fn touchpad_modifier_motion_navigates_without_buttons_or_document_changes() {
        for alt in [false, true] {
            let ctx = crate::fonts::test_context();
            let mut app = hover_app(false);
            app.preferences.nav = "touchpad".into();
            let p = pos2(350.0, 300.0);
            hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
            let before = app.cam;
            let doc = app.session.doc.to_json();
            let undo = app.session.undo.len();
            let log = app.session.log.len();
            let mods = egui::Modifiers { shift: !alt, alt, ..Default::default() };
            hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(mods)]);
            hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(30.0, 20.0))]);
            assert!(!ctx.input(|i| i.pointer.any_down()));
            if alt {
                assert!((app.cam.yaw - before.yaw - 0.24).abs() < 1e-6);
                assert_eq!(app.cam.target, before.target);
            } else {
                assert_ne!(app.cam.target, before.target);
                assert_eq!(app.cam.yaw, before.yaw);
            }
            assert_eq!(app.cam.distance, before.distance);
            let after = app.cam;
            for _ in 0..3 {
                hover_frame(&mut app, &ctx, vec![]);
                assert_eq!(app.cam, after, "no drift between pointer events");
                assert!(app.viewport.hover.is_none());
            }
            hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE), egui::Event::PointerMoved(p)]);
            assert_eq!(app.cam, after, "release ends modifier-only navigation");
            assert_eq!(app.session.doc.to_json(), doc);
            assert_eq!(app.session.undo.len(), undo);
            assert_eq!(app.session.log.len(), log, "continuous motion must not flood the command log");
            assert!(app.session.selection.is_empty());
            assert!(app.viewport.boxsel.is_none() && app.viewport.point_drag.is_none());
        }
    }

    #[test]
    fn touchpad_motion_uses_the_effective_perspective_projection() {
        let mut app = hover_app(false);
        app.ui.perspective = true;
        app.cam = Camera { yaw: 0.0, pitch: 0.0, distance: 100.0, ..Default::default() };
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        let pivot = Vec3::new(15.0, -50.0, 0.0);
        let before = projection(&app, rect).to_screen(pivot).unwrap();
        camera_motion(&mut app, rect, json!({"motion":"orbit","delta":[20,10],"pivot":pivot}));
        let after = projection(&app, rect).to_screen(pivot).unwrap();
        assert!(before.distance(after) < 1e-3, "rendered pivot drifted from {before:?} to {after:?}");
        assert_eq!(app.cam.fov, 0.0, "the projection toggle remains UI state, not a stored-camera mutation");
    }

    #[test]
    fn touchpad_motion_does_not_move_the_camera_under_a_canvas_triad() {
        let ctx = crate::fonts::test_context();
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length":20,"width":20,"height":20})).unwrap();
        s.selection = vec![Sel::Body { name: "Body1".into() }];
        let mut app = SolveApp::new(s, crate::Services::default());
        app.preferences.nav = "touchpad".into();
        app.start("solid.move");
        app.dialog.as_mut().unwrap().focus = false;
        let p = pos2(100.0, 100.0);
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        let triad = crate::dialogs_move::triad(&app, app.dialog.as_ref().unwrap()).unwrap();
        let handle = projection(&app, app.viewport.rect.unwrap()).to_screen(triad.center).unwrap();
        let mods = egui::Modifiers { alt: true, ..Default::default() };
        hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(mods)]);
        assert!(!ctx.egui_wants_keyboard_input());
        let before = app.cam;
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(handle)]);
        assert_eq!(app.cam, before, "a triad handle owns its input before the camera changes");
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
        assert_eq!(app.cam, before, "leaving the handle rearms without a jump");
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(5.0, 0.0))]);
        assert_ne!(app.cam.yaw, before.yaw, "the exposed viewport remains navigable with Move open");
    }

    #[test]
    fn touchpad_motion_preserves_input_ownership_and_rearms_without_jumping() {
        let ctx = crate::fonts::test_context();
        let mut app = hover_app(false);
        app.preferences.nav = "touchpad".into();
        app.preferences.orbit_around = crate::prefs::OrbitAround::Cursor;
        let p = pos2(350.0, 300.0);
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
        hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(alt)]);
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(10.0, 0.0))]);
        let pivot = app.viewport.orbit_pivot;
        assert!(pivot.is_some());
        hover_frame(&mut app, &ctx, vec![]);
        assert_eq!(app.viewport.orbit_pivot, pivot, "pauses keep the same orbit pivot");
        let before = app.cam;
        let bar = app.viewport.layout.as_ref().unwrap().navigation.center();
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(bar)]);
        assert_eq!(app.cam, before, "the navigation bar owns its input");
        assert!(app.viewport.orbit_pivot.is_none());
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
        assert_eq!(app.cam, before, "reentry must not apply the distance from an overlay");
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(5.0, 0.0))]);
        assert_ne!(app.cam.yaw, before.yaw);
        let before = app.cam;
        for mods in [
            egui::Modifiers::NONE,
            egui::Modifiers { shift: true, alt: true, ..Default::default() },
            egui::Modifiers { shift: true, ctrl: true, ..Default::default() },
            egui::Modifiers { alt: true, command: true, mac_cmd: true, ..Default::default() },
        ] {
            hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(mods), egui::Event::PointerMoved(p)]);
            assert_eq!(app.cam, before, "unbound combinations must not navigate");
        }
        let shift = egui::Modifiers { shift: true, ..Default::default() };
        hover_frame(&mut app, &ctx, vec![egui::Event::ModifiersChanged(shift), egui::Event::PointerMoved(p)]);
        hover_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: shift }],
        );
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(30.0, 20.0))]);
        assert_eq!(app.cam, before, "Shift+left drag must still box-select, not pan");
        assert!(app.viewport.boxsel.is_some());
        hover_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton { pos: p + vec2(30.0, 20.0), button: egui::PointerButton::Primary, pressed: false, modifiers: shift }],
        );
        assert_eq!(app.cam, before, "the release frame still belongs to selection");
    }

    #[test]
    fn touchpad_focus_and_preset_changes_discard_old_motion() {
        let ctx = crate::fonts::test_context();
        let mut app = hover_app(false);
        app.preferences.nav = "touchpad".into();
        let alt = egui::Modifiers { alt: true, ..Default::default() };
        let frame = |app: &mut SolveApp, focused: bool, p: Pos2| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                    focused,
                    events: vec![egui::Event::ModifiersChanged(alt), egui::Event::PointerMoved(p)],
                    ..Default::default()
                },
                |ui| show(app, ui),
            );
            out.textures_delta.clear();
        };
        frame(&mut app, true, pos2(300.0, 300.0));
        frame(&mut app, true, pos2(310.0, 300.0));
        let before = app.cam;
        frame(&mut app, false, pos2(500.0, 400.0));
        frame(&mut app, true, pos2(350.0, 300.0));
        assert_eq!(app.cam, before, "focus loss and regain must not jump");
        frame(&mut app, true, pos2(360.0, 300.0));
        assert!((app.cam.yaw - before.yaw - 0.08).abs() < 1e-6);
        app.preferences.nav = "fusion".into();
        let before = app.cam;
        frame(&mut app, true, pos2(450.0, 300.0));
        assert_eq!(app.cam, before);
        app.preferences.nav = "touchpad".into();
        frame(&mut app, true, pos2(500.0, 300.0));
        assert_eq!(app.cam, before, "changing presets must rearm before moving");
        app.viewport.spin = true;
        frame(&mut app, true, pos2(510.0, 300.0));
        assert!(!app.viewport.spin, "pointer-only navigation stops continuous orbit");
        assert!((app.cam.yaw - before.yaw - 0.08).abs() < 1e-6);
    }

    #[test]
    fn touchpad_vertical_trackpad_scroll_zooms_without_overwriting_saved_pan() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "touchpad".into();
        h.app.preferences.smooth_wheel_zoom = false;
        h.app.preferences.zoom_to_cursor = false;
        h.call("ui.move", json!({"x":800,"y":500}));
        let before = h.app.cam;
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
                phase,
            });
            h.frame();
        }
        assert!((h.app.cam.distance - before.distance * (-0.18_f64).exp()).abs() < 1e-6);
        assert_eq!(h.app.cam.target, before.target);
        assert!(h.app.preferences.trackpad_pan);
        let after = h.app.cam;
        h.frames(60);
        assert_eq!(h.app.cam, after);
        h.app.preferences.nav = "fusion".into();
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
                phase,
            });
            h.frame();
        }
        assert_eq!(h.app.cam.distance, after.distance);
        assert_ne!(h.app.cam.target, after.target);
    }

    #[test]
    fn hover_stays_off_between_navigation_events_and_returns_on_release() {
        for gpu in [false, true] {
            for button in [egui::PointerButton::Middle, egui::PointerButton::Secondary, egui::PointerButton::Primary] {
                let ctx = crate::fonts::test_context();
                let mut app = hover_app(gpu);
                if button == egui::PointerButton::Primary {
                    app.viewport.nav = Some(NavMode::Orbit);
                }
                hover_frame(&mut app, &ctx, vec![]);
                let p = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
                hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
                assert!(app.viewport.hover.is_some());
                hover_frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: p, button, pressed: true, modifiers: Default::default() }]);
                assert!(app.viewport.hover.is_none(), "hover on navigation press");
                for offset in [1.0, 2.0, 3.0] {
                    hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p + vec2(offset, 0.0))]);
                    assert!(app.viewport.hover.is_none(), "hover while moving");
                    for _ in 0..3 {
                        hover_frame(&mut app, &ctx, vec![]);
                        assert!(app.viewport.hover.is_none(), "hover between movement events");
                    }
                }
                hover_frame(&mut app, &ctx, vec![egui::Event::PointerButton { pos: p, button, pressed: false, modifiers: Default::default() }]);
                assert!(app.viewport.hover.is_some(), "hover must return on release");
            }
        }
    }

    fn device_face_app(gpu: bool) -> SolveApp {
        let mut app = hover_app(gpu);
        app.session = solidcraft_engine::Session::default();
        app.session.execute("solid.box", &json!({"length":40,"width":40,"height":40,"corner":[-20,-20,-20]})).unwrap();
        app.set_origin_shown(false);
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 110.0;
        app.cam.yaw = 0.5;
        app.cam.pitch = 0.5;
        app
    }

    fn device_hover_frame(app: &mut SolveApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        use crate::space_mouse::{DeviceMotion, Motion};
        let before = app.cam;
        app.viewport.device_motion = Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.0, 0.0, 0.1] }, seconds: 0.016 });
        hover_frame(app, ctx, events);
        assert_ne!(app.cam, before, "the device must keep moving the camera");
        assert!(app.viewport.device_motion.is_none());
    }

    #[test]
    fn midpoint_picking_cache_follows_tool_lifecycle() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.line", json!({"points":[[10,10],[30,10]],"infer":false})).unwrap();
        h.call("ui.view", json!({"view":"top"}));
        h.frames(3);
        let proj = projection(&h.app, h.app.viewport.rect.unwrap());
        let at = proj.to_screen(Vec3::new(20.0, 10.0, 0.0)).unwrap();
        let is_mid = |hits: Vec<Hit>| matches!(hits.first(), Some(Hit::SketchMidpoint { id, .. }) if id == "l1");
        assert!(is_mid(pick_cached(&mut h.app, &proj, at)));
        h.app.tool = crate::tools::Tool::for_command("sketch.line");
        assert!(!is_mid(pick_cached(&mut h.app, &proj, at)), "starting a tool invalidates idle hover");
        h.app.tool = None;
        assert!(is_mid(pick_cached(&mut h.app, &proj, at)), "ending a tool restores MID at the same pixel");
        assert!(!is_mid(pick_for_tool(&h.app, &proj, at)), "a temporarily taken tool still picks its operands");
    }

    #[test]
    fn device_navigation_keeps_face_hover_current_for_a_stationary_or_moving_pointer() {
        for gpu in [false, true] {
            let ctx = crate::fonts::test_context();
            let mut app = device_face_app(gpu);
            hover_frame(&mut app, &ctx, vec![]);
            let p = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
            hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
            for offset in [0.0, 15.0, -15.0, 0.0] {
                let at = p + vec2(offset, 0.0);
                let mut key = app.viewport.last_pick.as_ref().unwrap().0;
                for events in [vec![egui::Event::PointerMoved(at)], vec![], vec![]] {
                    device_hover_frame(&mut app, &ctx, events);
                    let proj = projection(&app, app.viewport.rect.unwrap());
                    let (expected, _) = candidate(&app, &pick(&app, &proj, at)).unwrap();
                    assert!(matches!(expected, Hit::Face { .. }));
                    assert_eq!(app.viewport.hover.as_ref(), Some(&expected), "hover must use the current camera and pointer");
                    let next = app.viewport.last_pick.as_ref().unwrap().0;
                    assert_ne!(next, key, "a moving camera must invalidate cached picking");
                    key = next;
                }
            }
            hover_frame(&mut app, &ctx, vec![]);
            assert!(matches!(app.viewport.hover, Some(Hit::Face { .. })));
        }
    }

    #[test]
    fn device_navigation_allows_face_clicks_and_preserves_modifier_selection() {
        for gpu in [false, true] {
            for (modifiers, add) in [(egui::Modifiers::NONE, false), (egui::Modifiers::SHIFT, true), (egui::Modifiers::COMMAND, true)] {
                let ctx = crate::fonts::test_context();
                let mut app = device_face_app(gpu);
                let existing = Sel::Body { name: "Body1".into() };
                app.session.selection = vec![existing.clone()];
                hover_frame(&mut app, &ctx, vec![]);
                let p = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
                device_hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p - vec2(15.0, 0.0))]);
                assert_eq!(app.session.selection, vec![existing.clone()], "device motion alone must retain selection");
                let at = p + vec2(15.0, 0.0);
                device_hover_frame(
                    &mut app,
                    &ctx,
                    vec![
                        egui::Event::ModifiersChanged(modifiers),
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers },
                    ],
                );
                device_hover_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers }],
                );
                let proj = projection(&app, app.viewport.rect.unwrap());
                let (_, selected) = candidate(&app, &pick(&app, &proj, at)).unwrap();
                assert!(matches!(selected, Sel::Face { .. }));
                let expected = if add { vec![existing, selected] } else { vec![selected] };
                assert_eq!(app.session.selection, expected, "the click must select the face under the current camera");
            }
        }
    }

    /// Two-handed use: while a 3D mouse turns the view, the pointer still preselects what is
    /// under it in the view as it is now, and a click picks it (it neither does nothing nor
    /// clears the selection).
    #[test]
    fn device_navigation_keeps_hover_and_click_picking() {
        use crate::space_mouse::{DeviceMotion, Motion};
        for gpu in [false, true] {
            let ctx = crate::fonts::test_context();
            let mut s = solidcraft_engine::Session::default();
            s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
            let mut app = SolveApp::new(s, crate::Services::default());
            app.set_origin_shown(false);
            if gpu {
                app.viewport.gpu =
                    Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
            }
            app.cam.target = Vec3::new(20.0, 15.0, 10.0);
            hover_frame(&mut app, &ctx, vec![]);
            // The middle of the top face.
            let p = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::new(20.0, 15.0, 20.0)).unwrap();
            hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
            assert!(matches!(app.viewport.hover, Some(Hit::Face { .. })), "{:?}", app.viewport.hover);
            let turn = || Some(DeviceMotion { motion: Motion { translation: [0.0; 3], rotation: [0.0, 0.0, 0.1] }, seconds: 0.016 });
            let mut pick = app.viewport.last_pick.as_ref().unwrap().0;
            for _ in 0..3 {
                let before = app.cam;
                app.viewport.device_motion = turn();
                hover_frame(&mut app, &ctx, vec![]);
                assert_ne!(app.cam, before, "the device turns the view");
                assert!(matches!(app.viewport.hover, Some(Hit::Face { .. })), "hover stays on while the device moves the view");
                let now = app.viewport.last_pick.as_ref().unwrap().0;
                assert_ne!(now, pick, "the pick follows the turned view");
                pick = now;
            }
            // A click while the device keeps turning the view selects the face under the pointer.
            let button =
                |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            app.viewport.device_motion = turn();
            hover_frame(&mut app, &ctx, vec![button(true)]);
            app.viewport.device_motion = turn();
            hover_frame(&mut app, &ctx, vec![button(false)]);
            assert!(matches!(app.session.selection.as_slice(), [Sel::Face { .. }]), "gpu {gpu}: {:?}", app.session.selection);
        }
    }

    #[test]
    fn hover_stays_off_during_spin_and_animated_views() {
        let ctx = crate::fonts::test_context();
        let mut app = hover_app(true);
        hover_frame(&mut app, &ctx, vec![]);
        let p = projection(&app, app.viewport.rect.unwrap()).to_screen(Vec3::ZERO).unwrap();
        hover_frame(&mut app, &ctx, vec![egui::Event::PointerMoved(p)]);
        assert!(app.viewport.hover.is_some());
        app.viewport.spin = true;
        for _ in 0..3 {
            hover_frame(&mut app, &ctx, vec![]);
            assert!(app.viewport.hover.is_none());
        }
        app.viewport.spin = false;
        hover_frame(&mut app, &ctx, vec![]);
        assert!(app.viewport.hover.is_some());
        app.cam_anim = Some(solidcraft_engine::render::CameraAnim::new(app.cam, app.cam, 0.0));
        for _ in 0..3 {
            hover_frame(&mut app, &ctx, vec![]);
            assert!(app.viewport.hover.is_none());
        }
        app.cam_anim = None;
        hover_frame(&mut app, &ctx, vec![]);
        assert!(app.viewport.hover.is_some());
    }

    /// After a close-up refines a body (#1320), the model draws it from the refined plan (on the
    /// GPU) or its mesh (on the CPU), its edges come from that mesh too, and a hovered face lights
    /// exactly triangles the model draws, never another triangulation (#1064).
    #[test]
    fn a_refined_body_highlights_on_its_drawn_triangles() {
        use solidcraft_engine::doc::display_quality::current;
        let tris_of = |bytes: &[u8]| -> Vec<[u32; 9]> {
            bytes
                .chunks(3 * crate::gpu::TRI_SIZE)
                .map(|t| {
                    let mut v: Vec<[u32; 3]> = t
                        .chunks(crate::gpu::TRI_SIZE)
                        .map(|c| std::array::from_fn(|i| u32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap())))
                        .collect();
                    v.sort_unstable();
                    std::array::from_fn(|i| v[i / 3][i % 3])
                })
                .collect()
        };
        let mesh_tris = |m: &solidcraft_engine::geom::Mesh| -> std::collections::HashSet<[u32; 9]> {
            let mut sc = GpuScene::default();
            for f in 0..m.tri_face.iter().map(|f| *f as usize + 1).max().unwrap_or(0) {
                face_tris(&mut sc, m, f, [0; 4], true);
            }
            tris_of(&sc.tris).into_iter().collect()
        };
        for tess in [true, false] {
            let mut app = cylinder_app();
            app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess, oit: false });
            let body = app.session.world_state().bodies[0].clone();
            let before = build_scene(&app).merged();
            let key = scene_key(&app);
            assert!(app.viewport.lod.lock().unwrap().force(&body, current(), 2));
            assert_ne!(scene_key(&app), key, "a refinement rebuilds the scene");
            let (plan, mesh) = refined(&app, &body).unwrap();
            assert!(mesh.triangles.len() > body.plan_mesh().unwrap().triangles.len() * 2);
            let scene = build_scene(&app).merged();
            if tess {
                assert!(std::sync::Arc::ptr_eq(&scene.planned[0].plan, &plan), "faces drawn from the refined plan");
            } else {
                assert_eq!(scene.tris.len(), mesh.triangles.len() * 3 * crate::gpu::TRI_SIZE, "faces built from the refined mesh");
                assert!(scene.tris.len() > before.tris.len());
            }
            // Edges (lines) from the refined mesh: finer polylines.
            assert!(scene.lines.len() > before.lines.len(), "{} vs {}", scene.lines.len(), before.lines.len());
            let drawn = mesh_tris(&mesh);
            let mut lit_faces = 0;
            for index in 0..body.body.face_count() {
                app.viewport.hover = Some(Hit::Face { body: body.name.clone(), index, point: Vec3::ZERO });
                let lit = tris_of(&build_highlight(&app).merged().tris);
                assert!(!lit.is_empty(), "face {index} lights");
                assert!(lit.iter().all(|t| drawn.contains(t)), "face {index}: every lit triangle is a drawn one");
                lit_faces += 1;
            }
            assert!(lit_faces >= 3, "{lit_faces} faces");
        }
    }

    fn cylinder_app() -> SolveApp {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 20})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.set_origin_shown(false);
        app
    }

    /// Moving the pointer across a face keeps its highlight (a scanned mesh is one face of
    /// millions of triangles): only another face, edge or plane builds it again.
    #[test]
    fn a_face_highlight_is_kept_while_the_pointer_moves_across_the_face() {
        let mut app = cylinder_app();
        let hover = |index: usize, x: f64| Hit::Face { body: "Body1".into(), index, point: Vec3::new(x, 0.0, 5.0) };
        app.viewport.hover = Some(hover(0, 10.0));
        let key = highlight_key(&app);
        let before = build_highlight(&app).merged();
        assert!(!before.tris.is_empty(), "the face is lit");
        app.viewport.hover = Some(hover(0, 9.0));
        assert_eq!(highlight_key(&app), key, "the same face, another point");
        assert_eq!(build_highlight(&app).merged().tris, before.tris);
        app.viewport.hover = Some(hover(1, 9.0));
        assert_ne!(highlight_key(&app), key, "another face");
        app.viewport.hover = Some(Hit::Edge { body: "Body1".into(), index: 0, mid: Vec3::ZERO });
        let edge = highlight_key(&app);
        app.viewport.hover = Some(Hit::Edge { body: "Body1".into(), index: 0, mid: Vec3::X });
        assert_ne!(highlight_key(&app), edge, "an edge's middle picks which part of it is lit");
        app.viewport.hover = Some(Hit::Plane { name: "XY".into(), point: Vec3::ZERO });
        let plane = highlight_key(&app);
        app.viewport.hover = Some(Hit::Plane { name: "XY".into(), point: Vec3::X });
        assert_eq!(highlight_key(&app), plane, "the same plane, another point");
    }

    #[test]
    fn body_highlight_buffers_are_reused_when_the_camera_orbits_or_pans() {
        let mut app = cylinder_app();
        app.set_origin_shown(true);
        app.viewport.hover_bodies = vec!["Body1".into()];
        let key = highlight_key(&app);
        let before = build_highlight(&app).merged();
        assert!(!before.tris.is_empty());
        app.cam.orbit(25.0, 10.0);
        app.cam.roll_by(0.3);
        app.cam.pan(20.0, -15.0, 600.0);
        assert_eq!(highlight_key(&app), key, "world-space feedback must stay resident during navigation");
        let after = build_highlight(&app).merged();
        assert_eq!(before.tris, after.tris);
        assert_eq!(before.lines, after.lines);
        app.cam.zoom_at(1.2, None);
        assert_ne!(highlight_key(&app), key, "reference geometry follows the view's scale");

        let mut app = hover_app(true);
        let sketch = app.session.active_sketch.unwrap();
        app.run("sketch.circle.center", json!({"center":[0,0],"radius":5})).unwrap();
        app.session.selection = vec![Sel::Profile { sketch, index: 0 }];
        let key = highlight_key(&app);
        let before = build_highlight(&app).merged();
        assert!(!before.trans.is_empty());
        app.cam.orbit(25.0, 10.0);
        assert_ne!(highlight_key(&app), key, "profile feedback still follows the projected curves and lift toward the eye");
        assert_ne!(before.trans, build_highlight(&app).merged().trans);
    }

    #[test]
    fn world_highlight_block_survives_zoom_and_refreshes_when_bodies_are_hidden() {
        let mut app = cylinder_app();
        app.set_origin_shown(true);
        app.viewport.hover_bodies = vec!["Body1".into()];
        let before = build_highlight(&app);
        let bytes = before.blocks[0].bytes.clone();
        assert!(!bytes.vertices.is_empty());
        app.cam.zoom_at(4.0, None);
        let after = build_highlight(&app);
        assert!(std::sync::Arc::ptr_eq(&bytes, &after.blocks[0].bytes), "zoom must not regenerate or upload body feedback");
        assert_ne!(before.guide_trans, after.guide_trans, "reference planes still resize with the view");
        app.session.visibility.hidden_bodies.push("Body1".into());
        let hidden = build_highlight(&app);
        assert!(!std::sync::Arc::ptr_eq(&bytes, &hidden.blocks[0].bytes));
        assert!(hidden.blocks[0].bytes.vertices.is_empty(), "hidden bodies must lose their feedback");
        app.session.visibility.hidden_bodies.clear();
        let shown = build_highlight(&app);
        assert_eq!(shown.blocks[0].bytes.vertices, bytes.vertices);
    }

    #[test]
    fn whole_body_highlight_preserves_each_triangle_and_selected_face_once() {
        let mut app = cylinder_app();
        let body = app.session.world_state().bodies[0].clone();
        let expected = shown_mesh(&app, &body, &None).triangles.len() * 3 * crate::gpu::TRI_SIZE;
        app.viewport.hover_bodies = vec![body.name.clone()];
        for selected in
            [vec![], vec![Sel::Face { body: body.name.clone(), index: 0, point: Vec3::ZERO }], vec![Sel::Body { name: body.name.clone() }]]
        {
            app.session.selection = selected;
            assert_eq!(build_highlight(&app).merged().tris.len(), expected, "whole-body feedback must neither omit nor duplicate selected faces");
        }
    }

    /// A whole highlighted body is lit from the model's own triangles, not by copying every one
    /// of them onto the CPU (a whole assembly lit that way took most of a second a frame): from
    /// its display plan where the model draws one, else from its vertices in the model scene's
    /// blocks. See-through bodies keep the CPU feedback, which colours them.
    #[test]
    fn whole_body_highlights_reuse_the_models_own_triangles() {
        let mut app = cylinder_app();
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: true, oit: false });
        let body = app.session.world_state().bodies[0].clone();
        let plan = body.display_plan().expect("the test body is drawn from a plan");
        app.viewport.hover_bodies = vec![body.name.clone()];
        let hovered = build_highlight(&app);
        assert_eq!(hovered.hover_planned.len(), 1, "the whole hovered body is lit from its plan");
        assert!(std::sync::Arc::ptr_eq(&hovered.hover_planned[0], &plan));
        assert_eq!(hovered.tris.len(), 0);
        assert!(hovered.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no copy of the body's triangles");
        app.viewport.hover_bodies.clear();
        app.session.selection = vec![Sel::Body { name: body.name.clone() }];
        let selected = build_highlight(&app);
        assert_eq!(selected.selected_planned.len(), 1, "a selected whole body is lit from its plan too");
        assert_eq!(selected.tris.len(), 0);
        assert!(selected.blocks.iter().all(|b| b.bytes.vertices.is_empty()));
        // A body the model builds on the CPU (here: no GPU plans) is lit from its own vertices
        // in the model scene's blocks.
        app.session.selection.clear();
        app.viewport.hover_bodies.clear();
        app.viewport.gpu = None;
        build_scene(&app);
        app.viewport.hover_bodies = vec![body.name.clone()];
        let built = build_highlight(&app);
        assert!(built.hover_planned.is_empty());
        assert_eq!(built.hover_blocks.len(), 1, "a CPU-built body is lit from the model's block");
        assert_eq!(built.tris.len(), 0);
        assert!(built.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no copy of the body's triangles");
        // Two occurrences of one component are drawn as placements of its shared triangles: the
        // highlight draws those placements again, in its colour.
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 20, "body_name": "Sleeve"})).unwrap();
        let made = s.execute("component.from_bodies", &json!({"bodies": ["Sleeve"]})).unwrap();
        let part = made["components"][0]["component"].as_u64().unwrap();
        s.execute("occurrence.copy", &json!({"component": part, "translate": [40, 0, 0]})).unwrap();
        let mut assembly = SolveApp::new(s, crate::Services::default());
        assembly.set_origin_shown(false);
        let names: Vec<String> = assembly.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        assert_eq!(names.len(), 2, "two placements of the part");
        build_scene(&assembly);
        assembly.viewport.hover_bodies = names;
        let instanced = build_highlight(&assembly);
        assert_eq!(instanced.hover_instanced.len(), 1, "the part's one shared mesh");
        assert_eq!(instanced.hover_instanced[0].count(), 2, "both of its placements");
        assert_eq!(instanced.tris.len(), 0);
        assert!(instanced.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no copy of the placements");
        // A see-through body is not in the block's opaque runs: its own vertices in the block are
        // drawn again, recoloured in the highlight's palette, and still no triangle is copied.
        app.viewport.hover_bodies.clear();
        app.run("appearance.assign", json!({"bodies": [body.name.clone()], "color": [200, 200, 200], "opacity": 0.5})).unwrap();
        build_scene(&app);
        app.viewport.hover_bodies = vec![body.name.clone()];
        let see_through = build_highlight(&app);
        assert!(see_through.hover_planned.is_empty(), "a see-through body is not drawn from a plan");
        assert_eq!(see_through.recoloured.len(), 1, "it is lit from the model's own vertices");
        assert!(!see_through.recoloured[0].vertices.is_empty());
        assert!(see_through.blocks.iter().all(|b| b.bytes.vertices.is_empty()), "no copy of its triangles");
    }

    /// A whole faded or see-through body lit from the model's own vertices, recoloured in the
    /// highlight's palette, looks exactly as the copy of its triangles it replaces did: hovered,
    /// and selected. Skipped without a GPU adapter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn faded_and_see_through_bodies_light_as_their_copies_did() {
        const SIZE: u32 = 128;
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 30, "width": 20, "height": 10, "corner": [0, 0, 0], "body_name": "Plate"})).unwrap();
        s.execute("solid.cylinder", &json!({"base": [45, 10, 0], "diameter": 12, "height": 25, "body_name": "Post"})).unwrap();
        s.execute("component.from_bodies", &json!({"bodies": ["Post"]})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32));
        let proj = projection(&app, rect);
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |model: GpuScene, highlight: GpuScene| {
            crate::gpu::render_offscreen(
                model,
                Some(highlight),
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&Tokens::get().analysis),
            )
        };
        for case in ["see-through", "faded"] {
            match case {
                "see-through" => drop(app.run("appearance.assign", json!({"bodies": ["Plate"], "color": "#88aacc", "opacity": 0.4})).unwrap()),
                _ => {
                    app.run("appearance.assign", json!({"bodies": ["Plate"]})).unwrap();
                    app.run("component.activate", json!({"component": "Post"})).unwrap();
                }
            }
            for selected in [false, true] {
                app.viewport.hover_bodies = if selected { Vec::new() } else { vec!["Plate".into()] };
                app.session.selection = if selected { vec![Sel::Body { name: "Plate".into() }] } else { Vec::new() };
                let model = build_scene(&app);
                let highlight = build_highlight(&app);
                assert_eq!(highlight.recoloured.len(), 1, "{case}: the plate is lit from the model's vertices");
                let highlight_handed = highlight.clone();
                // The copy it replaces: every triangle, lit, in the colour the copy was drawn in.
                let t = Tokens::get();
                let colour = if selected {
                    c4(t.body_selected)
                } else {
                    let c = body_color(&app.session.doc, &app_body(&app, "Plate")).unwrap_or(t.body);
                    let k = t.hover_face_lift;
                    [c.r().saturating_add(k), c.g().saturating_add(k), c.b().saturating_add(k), 255]
                };
                // The same highlight (its edges and widgets), the plate copied instead of recoloured.
                let mut copy = highlight.clone().merged();
                copy.recoloured.clear();
                let mesh = drawn_mesh(&app, &app_body(&app, "Plate"));
                mesh_tris(&mut copy, &mesh, colour, true, |_| true);
                let (Some(lit), Some(copied)) = (render(model.clone(), highlight_handed), render(model, copy)) else {
                    eprintln!("skipping: no GPU adapter");
                    return;
                };
                assert!(lit == copied, "{case}, selected {selected}: the recoloured body does not look as its copy did");
            }
            app.session.selection.clear();
        }
    }

    fn app_body(app: &SolveApp, name: &str) -> solidcraft_engine::doc::ModelBody {
        app.session.world_state().body(name).cloned().unwrap()
    }

    #[test]
    fn smooth_wall_display_preserves_wireframe_and_edge_selection() {
        let mut s = solidcraft_engine::Session::default();
        let sketch = s.execute("sketch.create", &json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        s.execute("sketch.ellipse", &json!({"center":[0,0],"major":[25,0],"minor_radius":5})).unwrap();
        let inner =
            s.execute("sketch.offset", &json!({"curves":["e1"],"distance":-1,"chain":false})).unwrap()["curves"][0].as_str().unwrap().to_string();
        let outer =
            s.execute("sketch.offset", &json!({"curves":[inner],"distance":-0.5,"chain":false})).unwrap()["curves"][0].as_str().unwrap().to_string();
        s.execute(
            "solid.extrude",
            &json!({"sketch":sketch,"distance":3,"profiles":{"type":"whole_boundaries","profiles":[{"outer":[outer],"holes":[[inner]]}]}}),
        )
        .unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.ui.ground_shadow = false;
        let st = app.session.world_state();
        let b = st.bodies.first().unwrap();
        let mesh = b.mesh();
        assert_eq!(mesh.edges.len(), 1536);
        assert_eq!(mesh.smooth_edges.iter().filter(|f| **f).count(), 512);
        let smooth = mesh.smooth_edges.iter().position(|f| *f).unwrap();
        let expected = mesh.edges.iter().enumerate().filter(|(i, _)| mesh.shaded_edge_visible(*i)).map(|(_, e)| e.len() - 1).sum::<usize>();
        let shaded = build_scene(&app).merged();
        assert_eq!(shaded.lines.len() / crate::gpu::LINE_SIZE, expected);
        assert!(!shaded.tris.is_empty());
        app.ui.visual_style = 2;
        let wire = build_scene(&app).merged();
        let expected_wire =
            mesh.edges.iter().enumerate().filter(|(i, _)| !mesh.seams.get(*i).copied().unwrap_or(false)).map(|(_, e)| e.len() - 1).sum::<usize>();
        assert_eq!(wire.lines.len() / crate::gpu::LINE_SIZE, expected_wire);
        assert!(expected_wire > expected);
        assert!(wire.tris.is_empty());
        app.ui.visual_style = 0;
        app.session.selection = vec![Sel::Edge { body: b.name.clone(), index: smooth, point: polyline_mid(&mesh.edges[smooth]) }];
        assert!(!build_highlight(&app).merged().lines.is_empty());
        let mut without_display_mask = (*mesh).clone();
        without_display_mask.smooth_edges.clear();
        assert_eq!(mesh.surface_patch(mesh.edge_faces[smooth][0] as usize), without_display_mask.surface_patch(mesh.edge_faces[smooth][0] as usize));
        let cam = solidcraft_engine::view::home_camera(&app.session);
        let cpu = solidcraft_engine::view::scene(&app.session, &cam);
        let actual_edges = cpu.lines.iter().filter(|line| line.color == solidcraft_engine::view::colors::EDGE).count();
        assert_eq!(actual_edges, mesh.edges.iter().enumerate().filter(|(i, _)| mesh.shaded_edge_visible(*i)).count());
        if let Ok(directory) = solidcraft_engine::geom::env::var("SMOOTH_SCREENSHOTS").ok_or(()) {
            std::fs::create_dir_all(&directory).unwrap();
            for (name, old_edges, wireframe) in [("after", false, false), ("before", true, false), ("wireframe", true, true)] {
                let mut scene = solidcraft_engine::view::scene(&app.session, &cam);
                scene.lines.retain(|line| line.color == solidcraft_engine::view::colors::EDGE);
                if old_edges {
                    scene.lines.clear();
                    for (i, edge) in mesh.edges.iter().enumerate() {
                        if mesh.edge_pickable(i) {
                            scene.lines.push(solidcraft_engine::render::SceneLine {
                                points: edge.clone(),
                                color: solidcraft_engine::view::colors::EDGE,
                                alpha: 1.0,
                                width: 1.2,
                                on_top: false,
                            });
                        }
                    }
                }
                if wireframe {
                    scene.meshes.clear();
                }
                assert!(crate::gpu::test_picture_renderer(), "SMOOTH_SCREENSHOTS needs a GPU");
                let png = solidcraft_engine::capture::png(&scene, &cam, 1200, 800).unwrap();
                std::fs::write(std::path::Path::new(&directory).join(format!("smooth-ring-{name}.png")), png.bytes).unwrap();
            }
        }
    }

    #[test]
    fn certified_small_crease_overrides_old_seam_fit_for_picking_and_patches() {
        let mut s = solidcraft_engine::Session::default();
        let sketch = s.execute("sketch.create", &json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        let step = std::f64::consts::TAU / 8.;
        let k = 4. / 3. * (step / 4.).tan() * 10.;
        for i in 0..8 {
            let a = i as f64 * step;
            let b = (i + 1) as f64 * step;
            let p0 = [10. * a.cos(), 10. * a.sin()];
            let p3 = if i == 7 { [10., 0.] } else { [10. * b.cos(), 10. * b.sin()] };
            let mut p1 = [p0[0] - k * a.sin(), p0[1] + k * a.cos()];
            let mut p2 = [p3[0] + k * b.sin(), p3[1] - k * b.cos()];
            if i == 0 {
                p1[0] += k * 1e-6;
                p2[0] += k * 1e-6 * b.cos();
                p2[1] += k * 1e-6 * b.sin();
            }
            s.execute("sketch.spline.control_point", &json!({"points":[p0,p1,p2,p3]})).unwrap();
        }
        s.execute("solid.extrude", &json!({"sketch":sketch,"distance":3})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let st = app.session.world_state();
        let body = st.bodies.first().unwrap();
        let mesh = body.mesh();
        let crease = mesh
            .edges
            .iter()
            .position(|line| {
                line.first().zip(line.last()).is_some_and(|(a, b)| {
                    (a.x - 10.).abs() < 1e-8 && a.y.abs() < 1e-8 && (b.x - 10.).abs() < 1e-8 && b.y.abs() < 1e-8 && (a.z - b.z).abs() > 2.9
                })
            })
            .unwrap();
        assert!(mesh.seams[crease], "the fixture must exercise the coarse seam false positive");
        assert!(mesh.sharp_edges[crease]);
        assert!(mesh.shaded_edge_visible(crease));
        assert!(mesh.edge_pickable(crease));
        assert!(mesh.edge_faces[crease].iter().any(|f| mesh.surface_patch(*f as usize).len() == 1));
        let mid = polyline_mid(&mesh.edges[crease]);
        app.cam = app.cam.looking_from(Vec3::new(30., 0., 1.5));
        app.cam.target = Vec3::new(0., 0., 1.5);
        for perspective in [false, true] {
            app.ui.perspective = perspective;
            let proj = projection(&app, Rect::from_min_size(pos2(0., 0.), vec2(800., 600.)));
            let hits = pick(&app, &proj, proj.to_screen(mid).unwrap());
            assert!(hits.iter().any(|hit| matches!(hit,Hit::Edge{index,..} if *index==crease)), "{hits:?}");
        }
    }

    #[test]
    fn circular_items_measure_as_whole_rims_and_surfaces() {
        let mut app = cylinder_app();
        let st = app.session.world_state();
        let b = &st.bodies[0];
        let m = b.mesh();
        let edges: Vec<Sel> = m
            .edges
            .iter()
            .enumerate()
            .filter(|(_, p)| p.len() > 3 && p.iter().all(|q| (q.z - 20.0).abs() < 1e-6))
            .map(|(index, p)| Sel::Edge { body: b.name.clone(), index, point: polyline_mid(p) })
            .collect();
        let result = app.session.measure_items(&edges);
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert!((result["items"][0]["length_mm"].as_f64().unwrap() - 20.0 * std::f64::consts::PI).abs() < 0.2, "{result}");
        app.session.selection = edges;
        let d = crate::dialogs::Dialog::for_command(&app, "inspect.measure").unwrap();
        assert_eq!(d.items().len(), 1);

        let faces = m.edge_faces.iter().zip(&m.seams).find(|(_, seam)| **seam).unwrap().0;
        let wall = Sel::Face { body: b.name.clone(), index: faces[0] as usize, point: Vec3::new(10.0, 0.0, 10.0) };
        let wall_pieces = grouped_selection(&app.session, &wall);
        let result = app.session.measure_items(&wall_pieces);
        assert_eq!(result["items"].as_array().unwrap().len(), 1);
        assert!((result["items"][0]["area_mm2"].as_f64().unwrap() - 400.0 * std::f64::consts::PI).abs() < 10.0, "{result}");
        let mut d = crate::dialogs::Dialog::for_command(&app, "inspect.measure").unwrap();
        d.pick(&app.session, wall.clone());
        assert_eq!(d.items().len(), 2, "a rim and a wall are two measurement targets");
        d.pick(&app.session, Sel::Face { body: b.name.clone(), index: faces[1] as usize, point: Vec3::new(-10.0, 0.0, 10.0) });
        assert_eq!(d.items().len(), 1, "the other half toggles the same wall");
    }

    #[test]
    fn colour_analyses_show_a_legend() {
        // #973: Draft, Accessibility and Curvature Map say what their colours mean; Zebra and no
        // analysis draw no legend.
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20})).unwrap();
        let legend = |h: &mut crate::scenario::Harness| {
            h.frames(2);
            crate::scenario::handle_at("analysis:legend").is_some()
        };
        assert!(!legend(&mut h));
        for (cmd, shown) in [("inspect.draft", true), ("inspect.accessibility", true), ("inspect.curvature_map", true), ("inspect.zebra", false)] {
            h.app.session.analysis = None;
            h.app.run(cmd, json!({})).unwrap();
            assert!(h.app.session.analysis.is_some(), "{cmd}");
            assert_eq!(legend(&mut h), shown, "{cmd}");
        }
        h.app.session.analysis = None;
        assert!(!legend(&mut h));
    }

    #[test]
    fn a_section_leaves_the_origin_planes_and_axes_whole() {
        // #1229: the origin planes and axes are reference geometry; the section plane cuts the
        // model, not them, so they go to the lists the GPU draws without the section clip.
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length":20,"width":20,"height":20,"corner":[-10,-10,-10]})).unwrap();
        h.app.run("inspect.section", json!({"plane": "XZ", "offset": 0})).unwrap();
        h.app.set_origin_shown(true);
        h.frames(2);
        assert!(section_plane(&h.app).is_some());
        let sc = build_highlight(&h.app).merged();
        let planes = origin_planes(&h.app).len();
        assert!(planes > 0);
        assert_eq!(sc.guide_trans.len(), planes * 6 * crate::gpu::TRI_SIZE);
        assert!(sc.trans.is_empty(), "no origin plane is drawn where the section clips");
        // Each origin axis: a solid part and dashes beyond, plus each plane's four edges.
        assert!(sc.guide_lines.len() >= (planes * 4 + 3) * crate::gpu::LINE_SIZE);
        assert!(sc.xray.is_empty(), "depth-tested unless the preference is on");
        // Shown through bodies (Preferences), they are drawn over everything, also unclipped.
        h.app.preferences.reference_geometry_through_bodies = true;
        let sc = build_highlight(&h.app).merged();
        assert_eq!(sc.xray.len(), planes * 6 * crate::gpu::TRI_SIZE);
        assert!(sc.trans.is_empty() && sc.guide_trans.is_empty() && sc.guide_lines.is_empty());
        assert!(sc.overlay.len() >= (planes * 4 + 3) * crate::gpu::LINE_SIZE);
    }

    #[test]
    fn circular_rim_hover_click_and_dialog_pick_include_both_halves() {
        let mut app = cylinder_app();
        let st = app.session.world_state();
        let b = &st.bodies[0];
        let m = b.mesh();
        let top = m.edges.iter().position(|e| e.len() > 3 && e.iter().all(|p| (p.z - 20.0).abs() < 1e-6)).unwrap();
        let rim = m.circular_rim(top);
        assert_eq!(rim.len(), 2);
        let line_count: usize = rim.iter().map(|&i| m.edges[i].len() - 1).sum();
        for &i in &rim {
            let mid = polyline_mid(&m.edges[i]);
            app.viewport.hover = Some(Hit::Edge { body: b.name.clone(), index: i, mid });
            assert_eq!(build_highlight(&app).merged().lines.len(), 2 * line_count * crate::gpu::LINE_SIZE);
            let item = hit_sel(app.viewport.hover.as_ref().unwrap()).unwrap();
            select(&mut app, Some(item.clone()), false, false);
            assert_eq!(app.session.selection.len(), 2);
            // Selecting both pieces must not draw each highlight twice.
            assert_eq!(build_highlight(&app).merged().lines.len(), 2 * line_count * crate::gpu::LINE_SIZE);
            // Ctrl/Shift clicking either half deselects the entire rim.
            select(&mut app, Some(item.clone()), true, true);
            assert!(app.session.selection.is_empty());
            let mut d = crate::dialogs::Dialog::for_command(&app, "solid.chamfer").unwrap();
            if let crate::dialogs::Kind::Fillet { chain, .. } = &mut d.kind {
                *chain = false;
            }
            d.pick(&app.session, item.clone());
            assert_eq!(d.items().len(), 2);
            d.pick(&app.session, item);
            assert!(d.items().is_empty());
        }
        // Screen-space picking works from either side of the rim as the model turns.
        for &i in &rim {
            let mid = polyline_mid(&m.edges[i]);
            app.cam = app.cam.looking_from(Vec3::new(mid.x, mid.y, 20.0));
            app.cam.target = Vec3::new(0.0, 0.0, 10.0);
            for perspective in [false, true] {
                app.ui.perspective = perspective;
                let proj = projection(&app, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)));
                let hits = pick(&app, &proj, proj.to_screen(mid).unwrap());
                let (_, sel) = candidate(&app, &hits).unwrap();
                assert!(matches!(sel, Sel::Edge { .. }), "{hits:?}");
                assert_eq!(grouped_selection(&app.session, &sel).len(), 2);
            }
        }
    }

    #[test]
    fn curved_face_hover_selection_and_dialogs_cross_the_seam() {
        let mut app = cylinder_app();
        let st = app.session.world_state();
        let b = &st.bodies[0];
        let m = b.mesh();
        let faces = m.edge_faces.iter().zip(&m.seams).find(|(_, seam)| **seam).unwrap().0;
        assert_eq!(faces.len(), 2);
        let triangle_count = m.tri_face.iter().filter(|f| faces.contains(f)).count();
        for &f in faces {
            let index = f as usize;
            let t = m.triangles.iter().zip(&m.tri_face).find(|(_, tf)| **tf == f).unwrap().0;
            let [a, c, d] = m.tri(t).unwrap();
            let point = (a + c + d) / 3.0;
            app.viewport.hover = Some(Hit::Face { body: b.name.clone(), index, point });
            assert_eq!(build_highlight(&app).merged().tris.len(), triangle_count * 3 * crate::gpu::TRI_SIZE);
            let item = hit_sel(app.viewport.hover.as_ref().unwrap()).unwrap();
            select(&mut app, Some(item.clone()), false, false);
            assert_eq!(app.session.selection.len(), 2);
            assert_eq!(build_highlight(&app).merged().tris.len(), triangle_count * 3 * crate::gpu::TRI_SIZE);
            assert_eq!(app.session.selection.first(), Some(&item), "keep the original tool placement point first");
            let mut dialog = crate::dialogs::Dialog::for_command(&app, "solid.chamfer").unwrap();
            assert_eq!(dialog.items().len(), 2);
            dialog.pick(&app.session, item.clone());
            assert!(dialog.items().is_empty());
            dialog.pick(&app.session, item);
            assert_eq!(dialog.items().len(), 2);
            // A point elsewhere on the opposite half still toggles the whole surface off.
            let other = faces.iter().find(|&&x| x != f).unwrap();
            select(&mut app, Some(Sel::Face { body: b.name.clone(), index: *other as usize, point: m.positions[0] }), true, true);
            assert!(app.session.selection.is_empty());
        }
        // Planar caps and box corners stay separate.
        let cap = (0..b.body.face_count()).find(|&i| m.surface_patch(i).len() == 1).unwrap();
        assert_eq!(m.surface_patch(cap), vec![cap]);
        app.session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let st = app.session.world_state();
        let box_body = st.bodies.last().unwrap();
        for i in 0..box_body.body.face_count() {
            assert_eq!(box_body.mesh().surface_patch(i), vec![i]);
        }
    }

    #[test]
    fn immediate_wheel_zoom_applies_normalized_input_once() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.smooth_wheel_zoom = false;
        h.app.preferences.trackpad_pan = false;
        h.app.preferences.zoom_to_cursor = false;
        h.call("ui.move", json!({"x":800,"y":500}));
        let before = h.app.cam;
        let line = h.ctx.options(|o| o.input_options.line_scroll_speed);
        for (unit, amount, points) in
            [(egui::MouseWheelUnit::Point, 120.0, 120.0), (egui::MouseWheelUnit::Line, 1.0, line), (egui::MouseWheelUnit::Page, 0.1, 100.0)]
        {
            let height = h.app.cam.half_height();
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit,
                delta: vec2(0.0, amount),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            });
            h.frame();
            let expected = height * (-f64::from(points) * 0.0015).exp();
            assert!((h.app.cam.half_height() - expected).abs() < 1e-6);
            let after = h.app.cam;
            h.frames(30);
            assert_eq!(h.app.cam, after, "idle frames must not replay wheel input");
        }
        assert_eq!(h.app.cam.target, before.target);
        h.app.preferences.zoom_reverse = true;
        let height = h.app.cam.half_height();
        h.call("ui.scroll", json!({"x":800,"y":500,"delta":120}));
        assert!((h.app.cam.half_height() - height * 0.18_f64.exp()).abs() < 1e-6);
    }

    #[test]
    fn wheel_zoom_setting_changes_do_not_replay_smoothing() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.zoom_to_cursor = false;
        h.app.preferences.trackpad_pan = false;
        h.call("ui.move", json!({"x":800,"y":500}));
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
        assert!(h.ctx.input(|i| i.is_scrolling()));
        h.app.preferences.smooth_wheel_zoom = false;
        let stopped = h.app.cam;
        h.frame();
        assert_eq!(h.app.cam, stopped);
        h.app.preferences.smooth_wheel_zoom = true;
        h.frames(90);
        assert_eq!(h.app.cam, stopped, "re-enabling must not replay the old smoothing tail");
        assert!(!h.app.viewport.wheel_zoom_raw_until_idle);
        h.app.preferences.smooth_wheel_zoom = false;
        for _ in 0..3 {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            });
            h.frame();
        }
        assert!((h.app.cam.half_height() - stopped.half_height() * (-0.54_f64).exp()).abs() < 1e-6);
        let after = h.app.cam;
        h.frames(90);
        assert_eq!(h.app.cam, after);
    }

    #[test]
    fn trackpad_two_finger_scroll_pans_without_zooming() {
        let mut h = crate::scenario::Harness::new();
        assert!(h.app.preferences.trackpad_pan, "two-finger pan is the default");
        h.app.preferences.zoom_to_cursor = false;
        h.call("ui.move", json!({"x":800,"y":500}));
        let gesture =
            |phase| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(60.0, -40.0), modifiers: egui::Modifiers::NONE, phase };
        // A bracketed trackpad gesture pans (measured macOS Start…End bracketing).
        let before = h.app.cam;
        h.app.synthetic.push(gesture(egui::TouchPhase::Start));
        h.frame();
        h.app.synthetic.push(gesture(egui::TouchPhase::Move));
        h.frame();
        assert_eq!(h.app.cam.half_height(), before.half_height(), "trackpad scroll must not zoom");
        assert_ne!(h.app.cam.target, before.target, "trackpad scroll must pan");
        h.app.synthetic.push(gesture(egui::TouchPhase::End));
        h.frame();
        let after = h.app.cam;
        h.frames(30);
        assert_eq!(h.app.cam, after, "pan must not drift once input stops");
        // Bare moves are a mouse: vertical zooms…
        h.app.preferences.smooth_wheel_zoom = false;
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
        assert!((h.app.cam.half_height() - after.half_height() * (-0.18_f64).exp()).abs() < 1e-6);
        // …and horizontal pans without zooming (thumb wheel).
        let zoomed = h.app.cam;
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(40.0, 0.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
        assert_eq!(h.app.cam.half_height(), zoomed.half_height());
        assert_ne!(h.app.cam.target, zoomed.target);
        // Opting out restores the legacy wheel behaviour for all point input.
        h.app.preferences.trackpad_pan = false;
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
        assert!((h.app.cam.half_height() - zoomed.half_height() * (-0.18_f64).exp()).abs() < 1e-6);
        // Legacy mode also zooms bracketed input instead of panning it.
        let legacy = h.app.cam.half_height();
        let legacy_target = h.app.cam.target;
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 120.0),
                modifiers: egui::Modifiers::NONE,
                phase,
            });
            h.frame();
        }
        assert!((h.app.cam.half_height() - legacy * (-0.18_f64).exp()).abs() < 1e-6);
        assert_eq!(h.app.cam.target, legacy_target);
        // The thumb wheel pans sideways even with panning off: it is a mouse.
        let h0 = h.app.cam.half_height();
        h.app.synthetic.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(40.0, 0.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.frame();
        assert_eq!(h.app.cam.half_height(), h0);
        assert_ne!(h.app.cam.target, legacy_target);
    }

    #[test]
    fn fusion_shift_two_finger_scroll_orbits_without_pan_zoom_or_drift() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "fusion".into();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::ViewCentre;
        h.call("engine.execute", json!({"command":"solid.box","params":{"length":40,"width":30,"height":20}}));
        h.call("ui.camera", json!({"fit":true}));
        h.call("ui.move", json!({"x":800,"y":500}));
        h.step(&json!({"shot":"fusion-trackpad-before"})).unwrap();
        let before = h.app.cam;
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(60.0, -40.0),
                modifiers: egui::Modifiers::SHIFT,
                phase,
            });
            h.frame();
        }
        assert_ne!(h.app.cam.back(), before.back(), "Shift + two fingers must orbit");
        assert_eq!(h.app.cam.target, before.target, "orbit must not pan");
        assert_eq!(h.app.cam.half_height(), before.half_height(), "orbit must not zoom");
        let after = h.app.cam;
        h.frames(90);
        assert_eq!(h.app.cam, after, "orbit must not drift once input stops");
        h.step(&json!({"shot":"fusion-trackpad-orbit"})).unwrap();
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            h.app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(60.0, -40.0),
                modifiers: egui::Modifiers::NONE,
                phase,
            });
            h.frame();
        }
        assert_eq!(h.app.cam.back(), after.back(), "unmodified two fingers must not orbit");
        assert_ne!(h.app.cam.target, after.target, "unmodified two fingers must pan");
        h.step(&json!({"shot":"fusion-trackpad-pan"})).unwrap();
    }

    #[test]
    fn fusion_trackpad_uses_each_events_modifiers_and_can_switch_mid_gesture() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "fusion".into();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::ViewCentre;
        h.call("ui.move", json!({"x":800,"y":500}));
        let gesture = |phase, modifiers| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(20.0, -10.0), modifiers, phase };
        h.app.synthetic.push(gesture(egui::TouchPhase::Start, egui::Modifiers::NONE));
        h.frame();
        for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT, egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
            let before = h.app.cam;
            h.app.synthetic.push(gesture(egui::TouchPhase::Move, modifiers));
            h.frame();
            assert_eq!(h.app.cam.half_height(), before.half_height());
            if modifiers.shift {
                let mut expected = before;
                expected.orbit(20.0, -10.0);
                assert_eq!(h.app.cam, expected, "Shift must orbit continuously, without snapping");
            } else {
                assert_eq!(h.app.cam.back(), before.back());
                assert_ne!(h.app.cam.target, before.target);
            }
        }
        let before = h.app.cam;
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(h.ctx.input(|input| input.time) + 1.0 / 60.0),
            focused: true,
            events: vec![gesture(egui::TouchPhase::Move, egui::Modifiers::NONE), gesture(egui::TouchPhase::Move, egui::Modifiers::SHIFT)],
            ..Default::default()
        };
        let mut output = h.ctx.run_ui(raw, |ui| {
            h.app.logic(ui.ctx());
            h.app.ui(ui);
        });
        output.textures_delta.clear();
        assert_ne!(h.app.cam.target, before.target, "unmodified events in the same frame still pan");
        assert_ne!(h.app.cam.back(), before.back(), "Shift events in the same frame still orbit");
        h.app.synthetic.push(gesture(egui::TouchPhase::Cancel, egui::Modifiers::SHIFT));
        h.frame();
        assert!(!h.app.viewport.trackpad_gesture);
        assert!(h.app.viewport.orbit_pivot.is_none());
        assert!(h.app.viewport.orbit_from.is_none());
        let after = h.app.cam;
        h.frames(90);
        assert_eq!(h.app.cam, after);
    }

    /// #7327: macOS sends the inertia after the fingers lift as its own scroll bracket, right after
    /// the fingers' one, with the modifiers held at the time. Releasing Shift while an orbit coasts
    /// (or pressing it while a pan coasts) ends the coast instead of carrying it into the other mode.
    #[test]
    fn fusion_trackpad_inertia_stops_when_shift_changes() {
        use egui::{Modifiers, TouchPhase::*};
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "fusion".into();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::ViewCentre;
        h.call("ui.move", json!({"x":800,"y":500}));
        let gesture = |phase, modifiers| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(20.0, -10.0), modifiers, phase };
        let play = |h: &mut crate::scenario::Harness, phase, modifiers| {
            h.app.synthetic.push(gesture(phase, modifiers));
            h.frame();
        };
        for (held, other) in [(Modifiers::SHIFT, Modifiers::NONE), (Modifiers::NONE, Modifiers::SHIFT)] {
            // The fingers swipe and lift; the coast starts in the next frame and keeps their mode.
            for phase in [Start, Move, Move, End, Start, Move] {
                play(&mut h, phase, held);
            }
            let coasting = h.app.cam;
            play(&mut h, Move, held);
            assert_ne!(h.app.cam, coasting, "{held:?}: the coast keeps moving while the modifiers stay");
            // The modifiers change: the rest of the coast is dropped, even when they change back.
            let stopped = h.app.cam;
            for modifiers in [other, other, held, other] {
                play(&mut h, Move, modifiers);
                assert_eq!(h.app.cam, stopped, "{held:?}: the coast must end, not pan or orbit");
            }
            play(&mut h, End, other);
            h.frames(30);
            assert_eq!(h.app.cam, stopped);
        }
        // Fingers touching during a coast are a new swipe: it moves at once and still switches
        // between pan and orbit with Shift while the fingers stay down.
        for phase in [Start, Move, Move, End, Start, Move] {
            play(&mut h, phase, Modifiers::SHIFT);
        }
        play(&mut h, End, Modifiers::SHIFT);
        play(&mut h, Start, Modifiers::SHIFT);
        for modifiers in [Modifiers::SHIFT, Modifiers::NONE, Modifiers::SHIFT] {
            let before = h.app.cam;
            play(&mut h, Move, modifiers);
            if modifiers.shift {
                let mut expected = before;
                expected.orbit(20.0, -10.0);
                assert_eq!(h.app.cam, expected, "a new swipe orbits with Shift");
            } else {
                assert_eq!(h.app.cam.back(), before.back());
                assert_ne!(h.app.cam.target, before.target, "a new swipe pans without Shift");
            }
        }
        play(&mut h, End, Modifiers::NONE);
        // A swipe after a pause is a new swipe too, whatever the last one did.
        h.frames(30);
        play(&mut h, Start, Modifiers::NONE);
        let before = h.app.cam;
        play(&mut h, Move, Modifiers::NONE);
        assert_ne!(h.app.cam.target, before.target, "a later swipe pans");
        play(&mut h, End, Modifiers::NONE);
    }

    #[test]
    fn shift_trackpad_orbit_preserves_other_presets_and_legacy_scroll() {
        for preset in crate::prefs::NAV_PRESETS.iter().map(|preset| preset.id) {
            for trackpad_pan in [true, false] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.nav = preset.into();
                h.app.preferences.trackpad_pan = trackpad_pan;
                h.app.preferences.zoom_to_cursor = false;
                h.call("ui.move", json!({"x":800,"y":500}));
                let before = h.app.cam;
                for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
                    h.app.synthetic.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(20.0, -10.0),
                        modifiers: egui::Modifiers::SHIFT,
                        phase,
                    });
                    h.frame();
                }
                if preset == "fusion" && trackpad_pan {
                    assert_ne!(h.app.cam.back(), before.back());
                } else {
                    assert_eq!(h.app.cam.back(), before.back(), "{preset}, pan={trackpad_pan}: must not orbit");
                    if h.app.preferences.trackpad_pans() {
                        assert_ne!(h.app.cam.target, before.target, "{preset}: Shift still pans");
                        assert_eq!(h.app.cam.half_height(), before.half_height());
                    }
                }
            }
        }
    }

    #[test]
    fn fusion_shift_trackpad_respects_orbit_centre_and_rejects_invalid_motion() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.nav = "fusion".into();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::Origin;
        h.app.cam.target = Vec3::new(30.0, 20.0, 10.0);
        h.call("ui.move", json!({"x":800,"y":500}));
        let before = h.app.cam;
        let gesture = |phase, delta| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, modifiers: egui::Modifiers::SHIFT, phase };
        h.app.synthetic.push(gesture(egui::TouchPhase::Start, egui::Vec2::ZERO));
        h.frame();
        h.app.synthetic.push(gesture(egui::TouchPhase::Move, vec2(20.0, -10.0)));
        h.frame();
        let mut expected = before;
        expected.orbit_about(20.0, -10.0, Vec3::ZERO);
        assert_eq!(h.app.cam, expected);
        for delta in [vec2(f32::NAN, 1.0), vec2(1.0, f32::INFINITY)] {
            h.app.synthetic.push(gesture(egui::TouchPhase::Move, delta));
            h.frame();
            assert_eq!(h.app.cam, expected);
        }
        h.app.synthetic.push(gesture(egui::TouchPhase::End, egui::Vec2::ZERO));
        h.frame();
        assert!(h.app.viewport.orbit_from.is_none());
    }

    #[test]
    fn pinch_gesture_zooms_and_ignores_noise() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.smooth_wheel_zoom = false;
        h.app.preferences.zoom_to_cursor = false;
        h.call("ui.move", json!({"x":800,"y":500}));
        let height = h.app.cam.half_height();
        let target = h.app.cam.target;
        h.app.synthetic.push(egui::Event::Zoom(1.2));
        h.frame();
        let zoomed = h.app.cam.half_height();
        assert!(((zoomed - height / 1.2) / (height / 1.2)).abs() < 1e-6, "spread must zoom in");
        assert_eq!(h.app.cam.target, target, "centre zoom keeps the target");
        h.app.preferences.zoom_reverse = true;
        let height = h.app.cam.half_height();
        h.app.synthetic.push(egui::Event::Zoom(1.2));
        h.frame();
        let flipped = h.app.cam.half_height();
        assert!(((flipped - height * 1.2) / (height * 1.2)).abs() < 1e-6, "reverse flips the pinch direction");
        h.app.synthetic.push(egui::Event::Zoom(f32::NAN));
        h.app.synthetic.push(egui::Event::Zoom(0.0));
        h.app.synthetic.push(egui::Event::Zoom(1.0));
        let stopped = h.app.cam;
        h.frame();
        assert_eq!(h.app.cam, stopped, "noise must leave the camera still");
    }

    #[test]
    fn trackpad_gestures_over_dialogs_do_not_move_the_view() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(app.preferences.trackpad_pan);
        crate::help::run(&mut app, &ctx, "shortcuts");
        let mut time = 0.0;
        let pan = |phase, modifiers| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(40.0, -20.0), modifiers, phase };
        let mut frame = |app: &mut SolveApp, pointer: Option<Pos2>, events: Vec<egui::Event>| {
            time += 1.0 / 60.0;
            let mut input = events;
            if let Some(p) = pointer {
                input.insert(0, egui::Event::PointerMoved(p));
            }
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 800.0))),
                    time: Some(time),
                    events: input,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| show(app, ui));
                    crate::prefs::show(app, ui.ctx());
                },
            );
            output.textures_delta.clear();
        };
        for _ in 0..4 {
            frame(&mut app, None, Vec::new());
        }
        let window = ctx.memory(|m| m.area_rect(egui::Id::new("sc_prefs"))).unwrap();
        let over = window.center();
        let height = app.cam.half_height();
        let target = app.cam.target;
        let direction = app.cam.back();
        // A full gesture plus pinch over the dialog stays in the popup.
        for modifiers in [egui::Modifiers::NONE, egui::Modifiers::SHIFT] {
            for events in [
                vec![pan(egui::TouchPhase::Start, modifiers)],
                vec![pan(egui::TouchPhase::Move, modifiers), egui::Event::Zoom(1.2)],
                vec![pan(egui::TouchPhase::End, modifiers)],
            ] {
                frame(&mut app, Some(over), events);
                assert_eq!(app.cam.half_height(), height, "trackpad input over a dialog must stay in the popup");
                assert_eq!(app.cam.target, target);
                assert_eq!(app.cam.back(), direction, "Shift + trackpad over a dialog must not orbit");
            }
        }
        let exposed = pos2(40.0, 700.0);
        assert!(!window.contains(exposed));
        frame(&mut app, Some(exposed), Vec::new());
        frame(&mut app, Some(exposed), vec![pan(egui::TouchPhase::Start, egui::Modifiers::NONE)]);
        frame(&mut app, Some(exposed), vec![pan(egui::TouchPhase::Move, egui::Modifiers::NONE)]);
        assert_eq!(app.cam.half_height(), height);
        assert_ne!(app.cam.target, target, "the exposed viewport still pans");
        frame(&mut app, Some(exposed), vec![egui::Event::Zoom(1.2)]);
        assert_ne!(app.cam.half_height(), height, "the exposed viewport still pinch-zooms");
        frame(&mut app, Some(exposed), vec![pan(egui::TouchPhase::End, egui::Modifiers::NONE)]);
    }

    #[test]
    fn shortcuts_window_wheel_does_not_zoom_the_viewport() {
        for smooth in [true, false] {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.preferences.smooth_wheel_zoom = smooth;
            app.preferences.trackpad_pan = false;
            crate::help::run(&mut app, &ctx, "shortcuts");
            let mut time = 0.0;
            let mut frame = |app: &mut SolveApp, pointer: Option<Pos2>, wheel: f32| {
                time += 1.0 / 60.0;
                let mut events = Vec::new();
                if let Some(p) = pointer {
                    events.push(egui::Event::PointerMoved(p));
                }
                if wheel != 0.0 {
                    events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: vec2(0.0, wheel),
                        modifiers: egui::Modifiers::NONE,
                        phase: egui::TouchPhase::Move,
                    });
                }
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1100.0, 800.0))),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| show(app, ui));
                        crate::prefs::show(app, ui.ctx());
                    },
                );
                output.textures_delta.clear();
            };
            // Let egui establish the floating window's size and hit-test widgets.
            for _ in 0..4 {
                frame(&mut app, None, 0.0);
            }
            let window = ctx.memory(|m| m.area_rect(egui::Id::new("sc_prefs"))).unwrap();
            let list = window.center() + vec2(140.0, 0.0);
            let height = app.cam.half_height();
            let target = app.cam.target;
            frame(&mut app, Some(list), 0.0);
            for n in 0..12 {
                frame(&mut app, Some(list), if n == 0 { -120.0 } else { 0.0 });
                assert_eq!(app.cam.half_height(), height, "wheel input over shortcuts must stay in the popup");
                assert_eq!(app.cam.target, target);
            }
            // The window's non-scrolling sidebar must also shield the model.
            frame(&mut app, Some(window.left_center() + vec2(30.0, 0.0)), -120.0);
            assert_eq!(app.cam.half_height(), height);
            // An exposed part of the viewport still accepts wheel navigation with the popup open.
            let exposed = pos2(40.0, 700.0);
            assert!(!window.contains(exposed));
            frame(&mut app, Some(exposed), 0.0);
            frame(&mut app, Some(exposed), -120.0);
            assert_ne!(app.cam.half_height(), height);
            // Closing the popup returns its former area to the viewport.
            app.prefs_window.open = false;
            for _ in 0..12 {
                frame(&mut app, Some(list), 0.0);
            }
            let height = app.cam.half_height();
            frame(&mut app, Some(list), -120.0);
            assert_ne!(app.cam.half_height(), height);
        }
    }

    #[test]
    fn wheel_by_a_dialog_list_or_panel_edge_leaves_the_model_alone() {
        // #678: scrolling the Physical Material list, a drop-down that hangs over the model,
        // zoomed the model too. egui's hit test counts the model as under the pointer within a
        // few points of a window's, a list's or the viewport's own edge, so near those edges
        // the same wheel turn reached both.
        use crate::scenario::{Harness, handle_at};
        // One wheel turn at `p`, then frames until egui's smoothing is spent: did the camera move?
        let wheel = |h: &mut Harness, p: Pos2, delta: f32| {
            let before = h.app.cam;
            h.call("ui.scroll", json!({"x": p.x, "y": p.y, "delta": delta}));
            h.frames(20);
            h.app.cam != before
        };
        // A trackpad pinch, then a two-finger scroll (which pans the model): did the camera move?
        let gestures = |h: &mut Harness, p: Pos2| {
            let before = h.app.cam;
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            h.app.synthetic.push(egui::Event::Zoom(1.2));
            for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
                let unit = egui::MouseWheelUnit::Point;
                h.app.synthetic.push(egui::Event::MouseWheel { unit, delta: vec2(30.0, -20.0), modifiers: egui::Modifiers::NONE, phase });
            }
            h.settle();
            h.frames(20);
            h.app.cam != before
        };
        for (layout, smooth) in [
            (crate::shell::LayoutMode::Floating, true),
            (crate::shell::LayoutMode::Floating, false),
            (crate::shell::LayoutMode::Docked, true),
            (crate::shell::LayoutMode::Docked, false),
        ] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":layout}));
            h.app.preferences.smooth_wheel_zoom = smooth;
            h.app.preferences.zoom_to_cursor = false;
            h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}}));
            h.call("ui.start", json!({"command": "material.assign"}));
            let field = handle_at("dialog:Material").expect("the dialog's material field");
            let field = json!({"x": field.x, "y": field.y});
            h.call("ui.move", field.clone());
            h.call("ui.click", field.clone());
            let item = "dialog:Material:Steel";
            let first = handle_at(item).expect("the open material list");
            let list = h
                .ctx
                .memory(|m| {
                    let popups = m.areas().visible_layer_ids().into_iter().filter(|l| l.order == egui::Order::Foreground);
                    popups.filter_map(|l| m.area_rect(l.id)).find(|r| r.contains(first))
                })
                .expect("the list's popup");
            let dialog = h.app.viewport.dialog_rect.expect("the dialog");
            let vp = h.app.viewport.rect.expect("the viewport");
            // The dense header, hint and footer change which item first hangs beyond
            // the card. Keep the original reproduction over the exposed model.
            let (item, first) = crate::scenario::handles("dialog:Material:")
                .into_iter()
                .find(|(_, point)| !dialog.contains(*point) && list.shrink(2.0).contains(*point) && vp.contains(*point))
                .expect("a visible list item over the model");
            assert!(!dialog.contains(first) && vp.contains(first), "the list must hang over the model: {first:?} {dialog:?}");

            // The wheel scrolls the list and only the list, in its middle and by its edges.
            assert!(!wheel(&mut h, pos2(list.center().x, first.y), -60.0), "smooth {smooth}: the wheel over the list zoomed the model");
            let moved = handle_at(&item).expect("the list stays open");
            assert!(moved.y < first.y - 1.0, "the wheel must scroll the list: {first:?} -> {moved:?}");
            for p in [pos2(list.left() + 2.0, list.center().y), pos2(list.right() - 2.0, list.center().y), pos2(list.center().x, list.bottom() - 2.0)]
            {
                assert!(!wheel(&mut h, p, 60.0), "smooth {smooth}: the wheel at {p:?} by the list's edge ({list:?}) zoomed the model");
                assert!(!gestures(&mut h, p), "a pinch or two-finger scroll at {p:?} by the list's edge ({list:?}) moved the model");
                assert!(h.app.viewport.mouse.is_none(), "the model must not see the pointer at {p:?} over the list");
            }

            // With the list closed: by the dialog's edges, and over the browser and the timeline
            // next to the model.
            h.call("ui.move", field.clone());
            h.call("ui.click", field);
            assert!(handle_at(&item).is_none(), "the list must close");
            // Floating cards sit inside the full viewport. Probe their actual edges; the
            // content anchor also reserves gaps and space below the short browser card.
            // Docked panels end at the viewport boundary instead.
            let (browser_edge, mid, history_edge) = if layout == crate::shell::LayoutMode::Floating {
                let browser = h.app.shell.browser.expect("the floating browser card");
                let history = h.app.shell.history.expect("the floating history card");
                (browser.right(), browser.center().y, history.top())
            } else {
                (vp.left(), vp.center().y, vp.bottom())
            };
            let x = vp.center().x;
            for p in [
                pos2(dialog.left() + 2.0, dialog.center().y),
                pos2(dialog.center().x, dialog.bottom() - 2.0),
                pos2(browser_edge - 2.0, mid),
                pos2(x, history_edge + 2.0),
            ] {
                assert!(!wheel(&mut h, p, 60.0), "smooth {smooth}: the wheel at {p:?} off the model ({vp:?}, dialog {dialog:?}) zoomed it");
                assert!(h.app.viewport.mouse.is_none(), "the model must not see the pointer at {p:?}");
            }
            // The model itself still zooms right next to them.
            for p in [pos2(dialog.left() - 2.0, dialog.center().y), pos2(browser_edge + 2.0, mid), pos2(x, history_edge - 2.0)] {
                assert!(wheel(&mut h, p, 60.0), "smooth {smooth}: the wheel at {p:?} on the model ({vp:?}) must zoom it");
                assert_eq!(h.app.viewport.mouse, Some(p));
                assert!(gestures(&mut h, p), "a pinch and a two-finger scroll at {p:?} on the model must move it");
            }
        }
    }

    #[test]
    fn floating_dialog_blocks_viewport_hover_and_clicks() {
        let mut h = crate::scenario::Harness::new();
        h.call("engine.execute", json!({"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}}));
        h.call("ui.view", json!({"view": "fit"}));
        h.app.ui.pick_bodies = true;
        let p = h.app.viewport.rect.unwrap().center();
        h.call("ui.move", json!({"x": p.x, "y": p.y}));
        assert!(h.app.viewport.hover.is_some(), "the test point must hover over the model");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!(!h.app.session.selection.is_empty(), "the uncovered model must be selectable");
        h.app.session.selection.clear();

        h.call("ui.prefs", json!({"open": "Navigation"}));
        let window = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_prefs"))).unwrap();
        assert!(window.contains(p), "the dialog must cover the model at the test point");
        h.call("ui.move", json!({"x": p.x, "y": p.y}));
        assert!(h.app.viewport.hover.is_none(), "the dialog must block model highlights");
        assert!(h.app.viewport.mouse.is_none());
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!(h.app.session.selection.is_empty(), "clicks in the dialog must not select the model");

        h.call("ui.prefs", json!({"button": "cancel"}));
        h.call("ui.move", json!({"x": p.x, "y": p.y}));
        assert!(h.app.viewport.hover.is_some(), "closing the dialog must restore model highlights");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!(!h.app.session.selection.is_empty());
    }

    #[test]
    fn floating_dialog_blocks_axis_view_highlights_and_clicks() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.call("ui.resize", json!({"width": 800, "height": 700}));
            let p = crate::scenario::handle_at("axis_view:z").unwrap();
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(1.0));
            h.call("ui.prefs", json!({"open": "Navigation"}));
            let window = h.ctx.memory(|m| m.area_rect(egui::Id::new("sc_prefs"))).unwrap();
            assert!(window.contains(p), "the floating dialog must cover the axis target");
            let camera = serde_json::to_value(h.app.cam).unwrap();
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(0.0));
            h.call("ui.click", json!({"x": p.x, "y": p.y}));
            assert_eq!(serde_json::to_value(h.app.cam).unwrap(), camera, "dialog clicks must not turn the axis view");
            h.call("ui.drag", json!({"x0": p.x, "y0": p.y, "x1": p.x + 20.0, "y1": p.y, "steps": 8}));
            assert_eq!(serde_json::to_value(h.app.cam).unwrap(), camera, "dialog drags must not orbit the axis gizmo");
            assert!(h.app.viewport.axis_drag.is_none());
            h.call("ui.prefs", json!({"button": "cancel"}));
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(1.0));
            h.call("ui.click", json!({"x": p.x, "y": p.y}));
            assert_ne!(serde_json::to_value(h.app.cam).unwrap(), camera);
        }
    }

    #[test]
    fn view_axis_drag_owns_navigation_and_keeps_the_view_centre() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for nav in [None, Some(NavMode::Orbit), Some(NavMode::Pan)] {
                let mut h = crate::scenario::Harness::new();
                h.call("ui.set", json!({"layout":mode}));
                h.call("ui.view", json!({"view": "front"}));
                h.app.viewport.nav = nav;
                let before = h.app.cam;
                let p = crate::scenario::handle_at("axis_view:-y").unwrap();
                h.call("ui.drag", json!({"x0": p.x, "y0": p.y, "x1": p.x + 48.0, "y1": p.y, "steps": 8}));
                assert!((h.app.cam.yaw - 48.0 * 0.008).abs() < 1e-6, "{nav:?}: axis drag must orbit exactly once");
                assert_eq!(h.app.cam.target, before.target, "{nav:?}: axis drag must not pan");
                assert_eq!(h.app.cam.distance, before.distance);
                assert_eq!(h.app.cam.pitch, before.pitch);
                assert!(h.app.viewport.axis_drag.is_none(), "release clears the axis gesture");
                assert!(h.app.viewport.boxsel.is_none());
            }
        }
    }

    #[test]
    fn cosmetic_threads_use_depth_tested_lines_and_keep_selection_and_wireframe() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.cylinder", &json!({"radius":3,"height":10})).unwrap();
        s.execute("solid.thread", &json!({"face":[3,0,5],"designation":"M6","length":5})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let count = |sc: GpuScene| {
            sc.merged()
                .lines
                .as_chunks::<{ crate::gpu::LINE_SIZE }>()
                .0
                .iter()
                .filter(|line| line.get(28..32) == Some(0.85f32.to_le_bytes().as_slice()))
                .count()
        };
        assert_eq!(count(build_scene(&app)), 0, "markings are off by default");
        let off_key = scene_key(&app);
        app.preferences.cosmetic_threads = true;
        assert_ne!(scene_key(&app), off_key, "toggling invalidates the scene without a camera move");
        assert!(count(build_scene(&app)) > 20);
        assert!(crate::control::view_scene(&app, Default::default()).lines.iter().any(|l| l.width == 0.85));
        app.preferences.cosmetic_threads = false;
        assert_eq!(count(build_scene(&app)), 0);
        assert!(!crate::control::view_scene(&app, Default::default()).lines.iter().any(|l| l.width == 0.85));
        app.preferences.cosmetic_threads = true;
        app.session.selection = vec![Sel::Face { body: "Body1".into(), index: 0, point: Vec3::new(3., 0., 5.) }];
        let selection = app.session.selection.clone();
        assert!(count(build_scene(&app)) > 20);
        assert!(!build_highlight(&app).merged().tris.is_empty());
        assert_eq!(app.session.selection, selection);
        app.ui.visual_style = 2;
        assert_eq!(count(build_scene(&app)), 0);
        app.ui.visual_style = 0;
        app.session.visibility.hidden_bodies.push("Body1".into());
        assert_eq!(count(build_scene(&app)), 0);
        app.session.visibility.hidden_bodies.clear();
        app.session.doc_mut().appearances.bodies.insert("Body1".into(), solidcraft_engine::doc::appearance::Look::custom([150, 150, 150], 0.5));
        assert_eq!(count(build_scene(&app)), 0, "transparent bodies omit cosmetic strokes");
        app.session.doc_mut().appearances.bodies.clear();
        app.session.execute("component.create", &json!({"name":"Other active component"})).unwrap();
        assert_eq!(count(build_scene(&app)), 0, "inactive bodies are faded and unmarked");
    }

    #[test]
    fn cosmetic_threads_preference_controls_internal_and_external_markings_without_editing_the_design() {
        for internal in [false, true] {
            let mut s = solidcraft_engine::Session::default();
            let face = if internal {
                s.execute("solid.box", &json!({"length":20,"width":20,"height":10})).unwrap();
                s.execute("solid.hole", &json!({"position":[10,10,10],"direction":[0,0,-1],"diameter":5})).unwrap();
                json!([12.5, 10, 9])
            } else {
                s.execute("solid.cylinder", &json!({"radius":3,"height":10})).unwrap();
                json!([3, 0, 9])
            };
            s.execute("solid.thread", &json!({"face":face,"designation":"M6","length":4})).unwrap();
            let mut app = SolveApp::new(s, crate::Services::default());
            let design = app.session.doc.to_json();
            let camera = app.cam;
            let count = |app: &SolveApp| {
                build_scene(app)
                    .merged()
                    .lines
                    .as_chunks::<{ crate::gpu::LINE_SIZE }>()
                    .0
                    .iter()
                    .filter(|line| line.get(28..32) == Some(0.85f32.to_le_bytes().as_slice()))
                    .count()
            };
            let key = scene_key(&app);
            assert_eq!(count(&app), 0);
            crate::prefs::set(&mut app, &json!({"cosmetic_threads":true})).unwrap();
            assert_ne!(scene_key(&app), key);
            assert!(count(&app) > 20, "internal={internal}");
            assert!(crate::control::view_scene(&app, Default::default()).lines.iter().any(|l| l.width == 0.85));
            crate::prefs::set(&mut app, &json!({"cosmetic_threads":false})).unwrap();
            assert_eq!(scene_key(&app), key);
            assert_eq!(count(&app), 0);
            assert_eq!(app.session.doc.to_json(), design);
            assert_eq!(app.cam, camera);
        }
    }

    #[test]
    fn cosmetic_thread_scene_is_bounded_for_many_occurrences_and_cached_across_camera_moves() {
        let mut s = solidcraft_engine::Session::default();
        let c = s.execute("component.create", &json!({"name":"Threaded pegs"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius":3,"height":20})).unwrap();
        s.execute("solid.thread", &json!({"face":[3,0,10],"designation":"M6"})).unwrap();
        for i in 1..96 {
            s.execute("occurrence.copy", &json!({"component":c["component"],"translate":[i*8,0,0]})).unwrap();
        }
        s.execute("component.activate", &json!({"component":0})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let started = std::time::Instant::now();
        app.preferences.cosmetic_threads = true;
        let scene = build_scene(&app).merged();
        let strokes = scene
            .lines
            .as_chunks::<{ crate::gpu::LINE_SIZE }>()
            .0
            .iter()
            .filter(|line| line.get(28..32) == Some(0.85f32.to_le_bytes().as_slice()))
            .count();
        eprintln!("96 occurrence viewport scene: {strokes} cosmetic strokes, {:?}", started.elapsed());
        assert!(strokes > 1000 && strokes <= solidcraft_engine::cosmetic_threads::MAX_SCENE_SEGMENTS);
        let marked_occurrences: std::collections::HashSet<i32> = scene
            .lines
            .as_chunks::<{ crate::gpu::LINE_SIZE }>()
            .0
            .iter()
            .filter(|line| line.get(28..32) == Some(0.85f32.to_le_bytes().as_slice()))
            .filter_map(|line| line.get(0..4).and_then(|bytes| bytes.try_into().ok()).map(f32::from_le_bytes))
            .map(|x| (x / 8.0).round() as i32)
            .collect();
        assert_eq!(marked_occurrences.len(), 96, "schematic density shares strokes across all 96 pegs");
        let key = scene_key(&app);
        app.cam.distance *= 1.5;
        app.cam.yaw += 0.2;
        assert_eq!(scene_key(&app), key, "ordinary camera changes reuse the scene; LOD changes have their own generation");
    }

    #[test]
    fn section_outlines_follow_the_cut() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let st = s.model.state();
        let m = st.bodies[0].mesh();
        let (o, n) = (Vec3::new(0.0, 15.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let segs = solidcraft_engine::view::section_segments(&m, o, n);
        assert!(segs.len() >= 4, "{segs:?}");
        // On the plane (a hair to the kept side) and around the 40 x 20 cut.
        let mut len = 0.0;
        for (a, b) in &segs {
            assert!((a.y - 15.0).abs() < 0.01 && (b.y - 15.0).abs() < 0.01);
            len += a.dist(*b);
        }
        assert!((len - 120.0).abs() < 0.5, "outline length {len}");
        // A plane that misses the body cuts nothing.
        assert!(solidcraft_engine::view::section_segments(&m, Vec3::new(0.0, 99.0, 0.0), n).is_empty());
    }

    #[test]
    fn selection_shows_where_the_model_hides_it() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        assert!(build_highlight(&app).merged().hidden.is_empty());
        // The back face: its outline is drawn (dimmed) where the box hides it.
        app.session.selection = vec![Sel::Face { body: "Body1".into(), index: 3, point: Vec3::new(20.0, 30.0, 10.0) }];
        assert!(!build_highlight(&app).merged().hidden.is_empty());
    }

    #[test]
    fn axis_view_top_bottom_choose_nearest_square_heading() {
        for (from, to) in [
            (0.0, 0.0),
            (30.0, 0.0),
            (-80.0, -90.0),
            (100.0, 90.0),
            (170.0, 180.0),
            (400.0, 360.0),
            (-400.0, -360.0),
            (-45.0, 0.0),
            (45.0, 0.0),
            (135.0, 90.0),
            (-135.0, -90.0),
            (44.99, 0.0),
            (45.01, 90.0),
            (-44.99, 0.0),
            (-45.01, -90.0),
        ] {
            let got = nearest_quarter_turn(f64::to_radians(from)).to_degrees();
            assert!((got - to).abs() < 1e-9, "{from}° → {got}°, expected {to}°");
        }
        assert_eq!(nearest_quarter_turn(f64::NAN), 0.0);
        assert_eq!(nearest_quarter_turn(f64::INFINITY), 0.0);
    }

    #[test]
    fn shift_orbit_accumulates_and_returns_to_free_rotation() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.view", json!({"view":"front"}));
        let rect = h.app.viewport.rect.unwrap();
        let before = h.app.cam;
        for _ in 0..100 {
            orbit_drag(&mut h.app, rect, 2.0, 0.0, None, true, false);
        }
        assert!((h.app.cam.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(h.app.cam.target, before.target);
        assert_eq!(h.app.cam.distance, before.distance);
        orbit_drag(&mut h.app, rect, 0.0, 0.0, None, false, false);
        h.frames(2);
        assert!((h.app.cam.yaw - 1.6).abs() < 1e-6);
        assert!(h.app.viewport.orbit_free.is_none());
    }

    #[test]
    fn axis_snap_selects_all_directions_in_both_up_frames() {
        assert_eq!(snap_directions().count(), 26);
        for up in [solidcraft_engine::render::UpAxis::Z, solidcraft_engine::render::UpAxis::Y] {
            for d in snap_directions() {
                let mut cam = Camera { up, ..Camera::default() };
                cam = cam.looking_from(cam.from_frame(d + Vec3::new(0.05, 0.03, 0.02)));
                let to = snapped_axis_camera(cam);
                assert!(to.back().dist(cam.from_frame(d)) < 1e-6);
                assert_eq!(to.target, cam.target);
                assert_eq!(to.distance, cam.distance);
                assert_eq!(to.fov, cam.fov);
            }
        }
    }

    #[test]
    fn camera_resets_drop_the_unsnapped_orbit() {
        for reset in 0..3 {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            let rect = h.app.viewport.rect.unwrap();
            orbit_drag(&mut h.app, rect, 120.0, 40.0, None, true, false);
            assert!(h.app.viewport.orbit_free.is_some());
            match reset {
                0 => h.app.fit_view(),
                1 => h.app.set_up_axis(solidcraft_engine::render::UpAxis::Y),
                _ => crate::documents::new_design(&mut h.app),
            }
            assert!(h.app.viewport.orbit_free.is_none());
        }
    }

    #[test]
    fn axis_snap_is_applied_at_once() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.view", json!({"view":"front"}));
        let rect = h.app.viewport.rect.unwrap();
        orbit_drag(&mut h.app, rect, 120.0, 0.0, None, true, false);
        assert!((h.app.cam.yaw - std::f64::consts::FRAC_PI_4).abs() < 1e-6, "the snapped view shows in the same frame");
    }

    /// The origin pivot holds through a Shift drag that snaps to an axis: the snapped
    /// view turns without letting the origin slide on screen.
    #[test]
    fn snapping_keeps_the_origin_still() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::Origin;
        h.app.run("solid.box", json!({"length":40,"width":40,"height":40})).unwrap();
        h.app.fit_view();
        let rect = h.app.viewport.rect.unwrap();
        let at = rect.center() + vec2(50.0, -20.0);
        orbit_drag(&mut h.app, rect, 0.0, 0.0, Some(at), true, true);
        let before = projection(&h.app, rect).to_screen(Vec3::ZERO).unwrap();
        for _ in 0..30 {
            orbit_drag(&mut h.app, rect, 4.0, 2.0, Some(at), true, true);
            let after = projection(&h.app, rect).to_screen(Vec3::ZERO).unwrap();
            assert!(after.distance(before) < 2.0, "{before:?} -> {after:?}");
        }
    }

    /// An orbit drag that starts on the model turns about the point under the cursor, which
    /// stays on its pixel, in perspective as in parallel views. The perspective view used to
    /// correct with a pan at the target's depth, so the point slid away and the view swung
    /// about the camera instead.
    #[test]
    fn cursor_orbit_keeps_the_picked_point_on_its_pixel() {
        for perspective in [false, true] {
            for offset in [vec2(30.0, 20.0), vec2(-60.0, 10.0), vec2(120.0, -40.0)] {
                let mut h = crate::scenario::Harness::new();
                h.app.preferences.orbit_around = crate::prefs::OrbitAround::Cursor;
                h.app.ui.perspective = perspective;
                h.app.run("solid.box", json!({"length":80,"width":50,"height":20})).unwrap();
                h.app.fit_view();
                h.frames(2); // Fit resolves against the measured floating shell before the drag.
                let rect = h.app.viewport.rect.unwrap();
                let at = rect.center() + offset;
                let proj = projection(&h.app, rect);
                assert!(pick(&h.app, &proj, at).iter().any(|hit| matches!(hit, Hit::Face { .. })), "the drag starts on the box");
                let pivot = crate::prefs::pivot_under(&h.app, &proj, at);
                let before = h.app.cam;
                h.call("ui.drag", json!({"x0":at.x,"y0":at.y,"x1":at.x+150.0,"y1":at.y+60.0,"button":"middle","shift":true,"steps":20}));
                assert!((h.app.cam.yaw - before.yaw).abs() > 0.5, "the view turned");
                let after = projection(&h.app, rect).to_screen(pivot).unwrap();
                assert!(after.distance(at) < 0.5, "perspective={perspective}: {at:?} -> {after:?}");
            }
        }
    }

    /// An erratic orbit drag (big jumps, reversals, the pointer over the panels and back) keeps
    /// the point picked when it began on its pixel the whole way, and coming back to where it
    /// began brings back the view it began with (up to the rounding of the pointer's f32 steps).
    #[test]
    fn an_erratic_cursor_orbit_keeps_its_pivot_and_comes_back() {
        for perspective in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.orbit_around = crate::prefs::OrbitAround::Cursor;
            h.app.ui.perspective = perspective;
            h.app.run("solid.box", json!({"length":80,"width":50,"height":20})).unwrap();
            h.app.fit_view();
            h.frames(2);
            let rect = h.app.viewport.rect.unwrap();
            let at = rect.center() + vec2(30.0, 20.0);
            let pivot = crate::prefs::pivot_under(&h.app, &projection(&h.app, rect), at);
            let start = h.app.cam;
            let shift = egui::Modifiers { shift: true, ..Default::default() };
            let send = |h: &mut crate::scenario::Harness, e: egui::Event| {
                h.app.synthetic.push(e);
                h.frame();
            };
            send(&mut h, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
            send(&mut h, egui::Event::PointerMoved(at));
            send(&mut h, egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: true, modifiers: shift });
            let mut seed: u32 = 12345;
            let mut next = move || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from((seed >> 16) as u16) / 65535.0
            };
            for i in 0..300 {
                // Mostly small moves, now and then a big jump, some of them over the panels.
                let p = if i % 17 == 0 { pos2(next() * 1600.0, next() * 1000.0) } else { at + vec2((next() - 0.5) * 900.0, (next() - 0.5) * 200.0) };
                send(&mut h, egui::Event::PointerMoved(p));
                assert_eq!(h.app.viewport.orbit_pivot, Some(pivot), "move {i}: the pivot is picked once, when the drag begins");
                let s = projection(&h.app, rect).to_screen(pivot).unwrap();
                assert!(s.distance(at) < 1.0, "perspective={perspective}, move {i} to {p:?}: the pivot moved to {s:?}");
            }
            send(&mut h, egui::Event::PointerMoved(at));
            let s = projection(&h.app, rect).to_screen(pivot).unwrap();
            assert!(s.distance(at) < 1.0, "{s:?}");
            let back = h.app.cam;
            assert!(
                (back.yaw - start.yaw).rem_euclid(std::f64::consts::TAU).min((start.yaw - back.yaw).rem_euclid(std::f64::consts::TAU)) < 1e-6,
                "{start:?} -> {back:?}"
            );
            assert!((back.pitch - start.pitch).abs() < 1e-6, "{start:?} -> {back:?}");
            assert!((back.target - start.target).len() < 1e-4, "back where it began: {start:?} -> {back:?}");
            send(&mut h, egui::Event::PointerButton { pos: at, button: egui::PointerButton::Middle, pressed: false, modifiers: shift });
        }
    }

    #[test]
    fn shift_orbit_keeps_cursor_pivot_and_default_middle_pan() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.orbit_around = crate::prefs::OrbitAround::Cursor;
        h.app.run("solid.box", json!({"length":40,"width":40,"height":40})).unwrap();
        h.app.fit_view();
        let rect = h.app.viewport.rect.unwrap();
        let at = rect.center() + vec2(50.0, -20.0);
        orbit_drag(&mut h.app, rect, 0.0, 0.0, Some(at), false, true);
        let pivot = h.app.viewport.orbit_pivot.unwrap();
        let before = projection(&h.app, rect).to_screen(pivot).unwrap();
        for _ in 0..20 {
            orbit_drag(&mut h.app, rect, 4.0, 2.0, Some(at), true, true);
            let after = projection(&h.app, rect).to_screen(pivot).unwrap();
            assert!(after.distance(before) < 2.0);
        }
        h.app.viewport.orbit_free = None;
        h.app.viewport.orbit_pivot = None;
        h.call("ui.view", json!({"view":"front"}));
        let before = h.app.cam;
        h.call("ui.drag", json!({"x0":800,"y0":500,"x1":1020,"y1":500,"button":"middle","shift":true,"steps":40}));
        assert!((h.app.cam.yaw - 1.76).abs() < 1e-6, "before={before:?}, after={:?}", h.app.cam);
        h.call("ui.view", json!({"view":"front"}));
        h.call("ui.drag", json!({"x0":800,"y0":500,"x1":850,"y1":520,"button":"middle"}));
        assert_eq!(h.app.cam.yaw, before.yaw);
        assert_eq!(h.app.cam.pitch, before.pitch);
        assert_ne!(h.app.cam.target, before.target);
    }

    #[test]
    fn preset_shift_middle_orbit_is_free_but_shift_right_orbit_snaps() {
        for preset in ["fusion", "inventor"] {
            let mut h = crate::scenario::Harness::new();
            h.app.preferences.nav = preset.into();
            h.call("ui.view", json!({"view":"front"}));
            let before = h.app.cam;
            h.call("ui.drag", json!({"x0":800,"y0":500,"x1":850,"y1":520,"button":"middle","shift":true,"steps":20}));
            let mut expected = before;
            expected.orbit(50.0, 20.0);
            assert!((h.app.cam.yaw - expected.yaw).abs() < 1e-6, "{preset}: {:?}", h.app.cam);
            assert!((h.app.cam.pitch - expected.pitch).abs() < 1e-6, "{preset}: {:?}", h.app.cam);
            assert_eq!(h.app.cam.target, before.target);
            assert_eq!(h.app.cam.distance, before.distance);
            h.call("ui.view", json!({"view":"front"}));
            h.call("ui.drag", json!({"x0":800,"y0":500,"x1":1020,"y1":500,"button":"right","shift":true,"steps":40}));
            assert!((h.app.cam.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-6, "{preset}: {:?}", h.app.cam);
        }
    }

    #[test]
    fn navigation_cube_repeated_face_click_keeps_view_and_drag_snaps() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"cube"}));
        h.run(&json!([
            {"call":"ui.view","params":{"view":"home"}},
            {"click":{"handle":"axis_view:z"}},
            {"expect":{"camera":{"pitch_deg":90,"yaw_deg":0}}},
            {"click":{"handle":"axis_view:z"}},
            {"expect":{"camera":{"pitch_deg":90,"yaw_deg":0}}},
            {"call":"ui.view","params":{"view":"front"}}
        ]))
        .unwrap();
        let c = axis_center(&h.app, h.app.viewport.rect.unwrap());
        h.call("ui.drag", json!({"x0":c.x,"y0":c.y,"x1":c.x+220.0,"y1":c.y,"shift":true,"steps":40}));
        assert!((h.app.cam.yaw - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(h.app.viewport.axis_drag.is_none());
        assert!(h.app.viewport.orbit_free.is_none());
        assert!(h.app.viewport.boxsel.is_none());
    }

    #[test]
    fn construction_point_is_pickable_only_while_visible() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("construct.point.vertex", &json!({"point":{"point":[20,15,10]},"name":"Point1"})).unwrap();
        s.execute("construct.axis.two_points", &json!({"a":{"point":[-20,0,10]},"b":{"point":[-20,0,30]},"name":"Axis1"})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0)));
        let pos = proj.to_screen(Vec3::new(20.0, 15.0, 10.0)).unwrap();
        let is_point =
            |hit: &Hit| hit_sel(hit).is_some_and(|sel| serde_json::to_value(sel).unwrap() == json!({"type":"construction_point","name":"Point1"}));
        for prefer_free in [false, true] {
            app.preferences.prefer_free_sketch_points = prefer_free;
            assert!(pick(&app, &proj, pos).iter().any(is_point));
            assert!(pick_all(&app, &proj, pos).iter().any(is_point));
        }
        let axis_pos = proj.to_screen(Vec3::new(-20.0, 0.0, 20.0)).unwrap();
        let axis = Hit::Axis { name: "Axis1".into() };
        assert!(pick(&app, &proj, axis_pos).contains(&axis));
        assert_eq!(
            crate::selection::SelInput::new("Axis", crate::selection::AXES, false).accepts(&axis, None, |_, _| false),
            Some(Sel::Axis { name: "Axis1".into() })
        );
        app.session.visibility.hidden_origin.extend(["Point1".into(), "Axis1".into()]);
        assert!(!pick(&app, &proj, axis_pos).contains(&axis));
        assert!(!pick(&app, &proj, pos).iter().any(is_point));
        assert!(!pick_all(&app, &proj, pos).iter().any(is_point));
    }

    #[test]
    fn construction_axes_span_the_model_and_points_show() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 40, "width": 30, "height": 20})).unwrap();
        s.execute("construct.axis.two_points", &json!({"a": {"vertex": [0, 0, 20]}, "b": {"vertex": [40, 30, 20]}})).unwrap();
        s.execute("construct.point.vertex", &json!({"point": {"vertex": [40, 0, 20]}})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        let axes = construction_axis_segments(&app);
        assert_eq!(axes.len(), 1);
        let (name, a, b) = axes[0].clone();
        // The segment runs through both points and past them.
        let d = (b - a).normalized().unwrap();
        for p in [Vec3::new(0.0, 0.0, 20.0), Vec3::new(40.0, 30.0, 20.0)] {
            let t = (p - a).dot(d);
            assert!(t > 0.0 && t < a.dist(b) && (a + d * t).dist(p) < 1e-6, "{p:?} not on {a:?}..{b:?}");
        }
        assert_eq!(construction_points(&app).len(), 1);
        // It is an axis for dialogs (pattern directions, revolve axes), and hiding it hides it.
        let (o, dir) = crate::dialogs::axis_of(&app, &Sel::Axis { name: name.clone() }).unwrap();
        assert!(o.dist(Vec3::new(0.0, 0.0, 20.0)) < 1e-6 && dir.dist(d) < 1e-6);
        app.session.visibility.hidden_origin.push(name);
        assert!(construction_axis_segments(&app).is_empty());
    }

    /// Draft Analysis on the GPU takes its colours from the tokens: the sample design through the
    /// viewport's real pipeline, offscreen (skipped without a GPU). In Off the faces that pass are
    /// green; in Protan they are the red-green table's blue, and the negative faces are striped.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn draft_analysis_on_the_gpu_draws_the_modes_colours_and_stripes() {
        use crate::theme::ColorVision;
        let mut s = solidcraft_engine::Session::default();
        s.run_script(&solidcraft_engine::sample::script()).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.fit_view();
        app.run("inspect.draft", json!({"pull": [-1, -1, 0]})).unwrap();
        app.viewport.gpu = Some(GpuTarget { format: egui_wgpu::wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false });
        const SIZE: u32 = 256;
        let proj = projection(&app, Rect::from_min_size(Pos2::ZERO, vec2(SIZE as f32, SIZE as f32)));
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |c: &crate::theme::AnalysisColors| {
            crate::gpu::render_offscreen(
                build_scene(&app),
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                analysis_uniform(&app),
                crate::gpu::analysis_colors(c),
            )
        };
        let off = Tokens::themed(false, ColorVision::Off).analysis;
        let protan = Tokens::themed(false, ColorVision::Protan).analysis;
        let flat = crate::theme::AnalysisColors { hatch: false, ..protan };
        let (Some(a), Some(b), Some(c)) = (render(&off), render(&protan), render(&flat)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        // Pixels of a colour under any light: the same hue, so the same ratios between channels.
        let like = |img: &[u8], c: [f32; 3]| {
            img.as_chunks::<4>()
                .0
                .iter()
                .filter(|p| {
                    let m = c.iter().copied().fold(0.0, f32::max);
                    let k = f32::from(p[0].max(p[1]).max(p[2])) / 255.0 / m;
                    k > 0.3 && (0..3).all(|i| (f32::from(p[i]) / 255.0 - c[i] * k).abs() < 0.04)
                })
                .count()
        };
        let n = (SIZE * SIZE) as usize;
        assert!(like(&a, off.draft[0]) > n / 50 && like(&a, protan.draft[0]) == 0, "Off: green passing faces");
        assert!(like(&b, protan.draft[0]) > n / 50 && like(&b, off.draft[0]) == 0, "Protan: blue passing faces");
        // The stripes: with them on, some negative pixels are darker, and nothing else changes.
        let negative = |p: &[u8]| like(p, protan.draft[2]) == 1;
        let changed: Vec<usize> = (0..n).filter(|&i| b[4 * i..4 * i + 3] != c[4 * i..4 * i + 3]).collect();
        let neg = (0..n).filter(|&i| negative(&c[4 * i..4 * i + 4])).count();
        assert!(!changed.is_empty() && changed.len() * 2 < neg, "{} striped of {neg} negative pixels", changed.len());
        assert!(changed.iter().all(|&i| negative(&c[4 * i..4 * i + 4])), "only negative faces are striped");
        assert!(changed.iter().all(|&i| b[4 * i..4 * i + 3].iter().zip(&c[4 * i..4 * i + 3]).all(|(x, y)| x <= y)), "stripes are darker");
    }

    /// The view's picture draws in the colour vision mode: a fixed sketch line in the light theme
    /// takes Deutan's value, and Off's is gone.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_view_picture_draws_in_the_colour_vision_mode() {
        use crate::theme::ColorVision;
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut h = crate::scenario::Harness::new();
        h.app.ui.dark = false;
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        let made = h.app.run("sketch.line", json!({"points": [[-30, -20], [30, 20]]})).unwrap();
        h.app.run("sketch.constraint.fix", json!({"entity": made["created"]["curves"][0]})).unwrap();
        let mut cam = solidcraft_engine::view::home_camera(&h.app.session);
        cam.set_view(solidcraft_engine::render::StandardView::Top);
        h.app.cam = cam;
        let count = |h: &crate::scenario::Harness, c: Color32| {
            let img = crate::control::view_rgba(&h.app, 320, 240).unwrap();
            img.as_chunks::<4>().0.iter().filter(|p| p[..3].iter().zip([c.r(), c.g(), c.b()]).all(|(a, b)| a.abs_diff(b) <= 8)).count()
        };
        let (off, deutan) = (Tokens::light().sketch_locked, Tokens::themed(false, ColorVision::Deutan).sketch_locked);
        let (a, b) = (count(&h, off), count(&h, deutan));
        assert!(a > 20 && b == 0, "Off: {a} fixed-line pixels, {b} in Deutan's colour");
        h.app.preferences.color_vision = ColorVision::Deutan;
        let (a, b) = (count(&h, off), count(&h, deutan));
        assert!(a == 0 && b > 20, "Deutan: {a} fixed-line pixels in Off's colour, {b} in Deutan's");
    }

    /// A mode's table reaches the headless picture's palette: the sketch states and axes it
    /// changes, and nothing else (the light theme in Off is the engine's own colours).
    #[test]
    fn picture_palette_follows_the_colour_vision_mode() {
        use crate::theme::ColorVision;
        let rgb = |c: Color32| Rgb(c.r(), c.g(), c.b());
        let off = scene_palette(false, ColorVision::Off);
        let rg = scene_palette(false, ColorVision::Protan);
        let t = Tokens::themed(false, ColorVision::Protan);
        assert_eq!((rg.sketch_projected, rg.sketch_locked), (rgb(t.sketch_projected), rgb(t.sketch_locked)));
        assert_ne!(rg.sketch_projected, off.sketch_projected, "the red-green table changes projected lines in light");
        assert_eq!((rg.axes[0], rg.axes[1], rg.axes[2]), (rgb(t.axis_x), rgb(t.axis_y), off.axes[2]));
        assert_eq!((rg.sketch, rg.background, rg.grid, rg.body), (off.sketch, off.background, off.grid, off.body));
        let by = scene_palette(false, ColorVision::Tritan);
        assert_eq!(by.sketch_locked, rgb(Tokens::themed(false, ColorVision::Tritan).sketch_locked));
        assert_ne!(by.sketch_locked, off.sketch_locked);
        assert_eq!(by.sketch_projected, off.sketch_projected);
    }

    /// The mode is on from the next frame (theme, widgets, caches, `ui.render`), and Cancel puts
    /// it back.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn colour_vision_mode_reaches_the_frame_and_the_view_caches() {
        use crate::theme::ColorVision;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let keys = |app: &SolveApp| (scene_key(app), highlight_key(app));
        h.call("ui.prefs", json!({"open": "Interface"}));
        crate::prefs::reveal_preference(&mut h, "prefs:color_vision");
        assert!(h.widget("Colour vision").is_some() && h.widget("Off").is_some() && h.widget("Deutan").is_some());
        let before = keys(&h.app);
        h.step(&json!({"click":{"handle":"dialog:Mode:Deutan"}})).unwrap();
        h.frame();
        assert_eq!(crate::theme::vision(), ColorVision::Deutan);
        assert_eq!(Tokens::get(), Tokens::themed(crate::theme::is_dark(), ColorVision::Deutan));
        let after = keys(&h.app);
        assert!(before.0 != after.0 && before.1 != after.1, "both caches are keyed on the mode");
        h.call("ui.prefs", json!({"button": "cancel"}));
        h.frame();
        assert_eq!((h.app.preferences.color_vision, crate::theme::vision()), (ColorVision::Off, ColorVision::Off));
        assert_eq!(keys(&h.app), before);
        // `ui.render` shows the mode too: a fixed sketch line in Deutan's value (in the light
        // theme, where the red-green table changes it; the render follows the theme).
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        h.call("ui.prefs", json!({"theme": "light"}));
        h.frame();
        h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        let made = h.app.run("sketch.line", json!({"points": [[-30, -20], [30, 20]]})).unwrap();
        h.app.run("sketch.constraint.fix", json!({"entity": made["created"]["curves"][0]})).unwrap();
        h.call("ui.view", json!({"view": "top"}));
        let path = std::env::temp_dir().join(format!("sc_colour_vision_render_{}.png", std::process::id()));
        let near = |h: &mut crate::scenario::Harness, c: Color32| {
            let r = h.call("ui.render", json!({"path": path.to_string_lossy(), "width": 400, "height": 300}));
            assert_eq!(r["ok"], true, "{r}");
            let img = image::open(&path).unwrap().to_rgb8();
            img.pixels().filter(|p| p.0.iter().zip([c.r(), c.g(), c.b()]).all(|(a, b)| a.abs_diff(b) <= 8)).count()
        };
        let (off, deutan) = (Tokens::light().sketch_locked, Tokens::themed(false, ColorVision::Deutan).sketch_locked);
        let (a, b) = (near(&mut h, off), near(&mut h, deutan));
        assert!(a > 20 && b == 0, "Off: {a} fixed-line pixels, {b} in Deutan's colour");
        h.call("ui.prefs", json!({"color_vision": "deutan"}));
        h.frame();
        let (a, b) = (near(&mut h, off), near(&mut h, deutan));
        h.call("ui.prefs", json!({"color_vision": "off"}));
        h.frame();
        let _ = std::fs::remove_file(&path);
        assert!(a == 0 && b > 20, "Deutan: {a} fixed-line pixels in Off's colour, {b} in Deutan's");
    }
}

#[cfg(test)]
#[path = "view_home_tests.rs"]
mod view_home_tests;

#[cfg(test)]
mod grid_plane_tests {
    use super::*;
    use solidcraft_engine::render::{StandardView, UpAxis};

    fn plane(view: StandardView, up: UpAxis) -> (Vec3, Vec3) {
        let (yaw, pitch) = view.angles();
        let cam = Camera { yaw, pitch, up, ..Camera::default() };
        let (u, v) = grid_plane(cam.to_frame(cam.back()), GridPlane::default()).axes();
        (cam.from_frame(u), cam.from_frame(v))
    }

    /// The camera frame's direction toward the eye for a view at `yaw` and `pitch` degrees.
    fn back_at(yaw_deg: f64, pitch_deg: f64) -> Vec3 {
        let cam = Camera { yaw: yaw_deg.to_radians(), pitch: pitch_deg.to_radians(), ..Camera::default() };
        cam.to_frame(cam.back())
    }

    /// #1470: the grid lies on the ground unless the view sees the ground edge-on; then it lies on
    /// the upright origin plane facing the view, Z up or Y up alike.
    #[test]
    fn the_grid_faces_side_views_and_stays_on_the_ground_otherwise() {
        let normal = |(u, v): (Vec3, Vec3)| u.cross(v);
        for up in [UpAxis::Z, UpAxis::Y] {
            for view in [StandardView::Top, StandardView::Bottom, StandardView::Iso] {
                assert_eq!(normal(plane(view, up)).dot(up.dir()).abs(), 1.0, "{view:?} {up:?}: the ground");
            }
            for view in [StandardView::Front, StandardView::Back, StandardView::Right, StandardView::Left] {
                let (yaw, pitch) = view.angles();
                let back = Camera { yaw, pitch, up, ..Camera::default() }.back();
                let n = normal(plane(view, up));
                assert!(n.dot(up.dir()).abs() < 1e-9, "{view:?} {up:?}: an upright plane");
                assert!((n.dot(back).abs() - 1.0).abs() < 1e-9, "{view:?} {up:?}: facing the view");
            }
        }
    }

    /// #1993: the view has to turn clearly off the ground before the grid leaves it, and clearly
    /// away from a wall before it goes back, so a view wobbling at the threshold does not flicker.
    #[test]
    fn the_grid_plane_thresholds_have_hysteresis() {
        // 9°: edge-on enough to enter the wall.
        assert_eq!(grid_plane(back_at(0.0, 9.0), GridPlane::Ground), GridPlane::Xz);
        // 12°: past the entering limit, but not enough to leave a wall the grid is already on.
        assert_eq!(grid_plane(back_at(0.0, 12.0), GridPlane::Ground), GridPlane::Ground);
        assert_eq!(grid_plane(back_at(0.0, 12.0), GridPlane::Xz), GridPlane::Xz);
        // 16°: clearly off the ground, so even a wall goes back.
        assert_eq!(grid_plane(back_at(0.0, 16.0), GridPlane::Xz), GridPlane::Ground);
        // Below the ground the same, and a view straight down stays on the ground.
        assert_eq!(grid_plane(back_at(30.0, -12.0), GridPlane::Yz), GridPlane::Yz);
        assert_eq!(grid_plane(Vec3::new(0.0, 0.0, 1.0), GridPlane::Xz), GridPlane::Ground);
        assert_eq!(grid_plane(Vec3::new(0.0, 0.0, -1.0), GridPlane::Xz), GridPlane::Ground);
    }

    /// #1993: orbiting at a low elevation keeps the wall until a side view is near; the other
    /// wall takes over only there, never at the 45° crossing.
    #[test]
    fn a_low_orbit_switches_walls_only_near_a_side_view() {
        let low = |yaw_deg: f64| back_at(yaw_deg, 3.0);
        // Entering from the ground still picks the wall facing the view.
        assert_eq!(grid_plane(low(44.0), GridPlane::Ground), GridPlane::Xz);
        assert_eq!(grid_plane(low(50.0), GridPlane::Ground), GridPlane::Yz);
        // A wall is kept across the crossing and only changes near the side view it faces.
        assert_eq!(grid_plane(low(44.0), GridPlane::Xz), GridPlane::Xz);
        assert_eq!(grid_plane(low(50.0), GridPlane::Xz), GridPlane::Xz, "no swap at the 45° crossing");
        assert_eq!(grid_plane(low(70.0), GridPlane::Xz), GridPlane::Xz);
        assert_eq!(grid_plane(low(80.0), GridPlane::Xz), GridPlane::Yz, "the Left view's wall takes over near it");
        assert_eq!(grid_plane(low(10.0), GridPlane::Yz), GridPlane::Xz, "the Front view's wall takes over near it");
    }

    /// #1993: a hostile view direction (a design file, a control request) never panics and never
    /// leaves the grid off the ground's axes.
    #[test]
    fn a_hostile_view_direction_stays_on_the_ground() {
        for back in [Vec3::new(f64::NAN, 0.0, 0.0), Vec3::new(0.0, f64::NEG_INFINITY, 0.0), Vec3::new(0.0, 0.0, f64::INFINITY), Vec3::ZERO] {
            assert_eq!(grid_plane(back, GridPlane::Xz), GridPlane::Ground, "{back:?}");
        }
    }

    /// #1993: the viewport keeps the plane it drew, so the next frame's choice is hysteretic.
    #[test]
    fn the_viewport_keeps_the_grid_plane_between_frames() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.preferences.grid_follows_view = true;
        app.cam.yaw = 0.0;
        app.cam.pitch = 0.0;
        let _ = build_sketch_scene(&app, &projection(&app, rect));
        assert_eq!(*app.viewport.grid_plane.lock().unwrap(), GridPlane::Xz, "the Front view takes the wall");
        // Wobble past the entering limit: the wall stays (before #1993 it flipped to the ground).
        app.cam.pitch = 0.23;
        let _ = build_sketch_scene(&app, &projection(&app, rect));
        assert_eq!(*app.viewport.grid_plane.lock().unwrap(), GridPlane::Xz, "a wobble does not flicker");
        app.cam.pitch = 0.3;
        let _ = build_sketch_scene(&app, &projection(&app, rect));
        assert_eq!(*app.viewport.grid_plane.lock().unwrap(), GridPlane::Ground, "a clear turn off the ground returns");
    }

    /// The grid's centre is where the view's axis meets the ground, but at a grazing slant (a
    /// camera 10 mm above the ground, looking almost along it) no further than the grid's
    /// half-size from the point under the target, so the ground in front of the camera keeps
    /// its grid.
    #[test]
    fn the_grid_centre_stays_within_reach_of_the_point_under_the_target() {
        let n = Vec3::Z;
        // Looking down at 45 degrees at a target 10 above the ground: the axis meets it 10 ahead.
        let tf = Vec3::new(0.0, 0.0, 10.0);
        let back = Vec3::new(0.0, -1.0, 1.0).normalized().unwrap();
        let c = grid_centre(tf, back, n, 100.0);
        assert!((c - Vec3::new(0.0, 10.0, 0.0)).len() < 1e-9, "{c:?}");
        // Grazing: the axis meets the ground 1 m ahead, the grid is 120 wide.
        let back = Vec3::new(0.0, -1.0, 0.01).normalized().unwrap();
        let c = grid_centre(tf, back, n, 120.0);
        assert!(c.z.abs() < 1e-9 && (c - Vec3::new(0.0, 0.0, 0.0)).len() <= 120.0 + 1e-9, "{c:?}");
        assert!(c.y > 0.0, "still toward where the view looks: {c:?}");
        // Edge-on: under the target.
        let c = grid_centre(tf, Vec3::Y, n, 120.0);
        assert!(c.len() < 1e-9, "{c:?}");
    }

    /// #3087: zoomed in on a target below the ground (an orbit left it there), seen at a slant,
    /// the grid still covers the ground in the middle of the view: where the view's axis meets
    /// it, some fifty millimetres from the point under the target, round which it was drawn.
    #[test]
    fn the_grid_covers_the_middle_of_the_view_when_the_target_is_off_the_ground() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 720.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.show_grid = true;
        app.ui.perspective = false;
        // The camera of the report.
        app.cam.target = Vec3::new(-38.888, -34.247, -28.617);
        app.cam.yaw = -2.288;
        app.cam.pitch = 0.504;
        app.cam.distance = 25.563;
        let back = app.cam.back();
        let mid = app.cam.target - back * (app.cam.target.z / back.z);
        assert!(mid.z.abs() < 1e-9 && (mid - app.cam.target).len() > 40.0, "{mid:?}");
        let under = build_sketch_scene(&app, &projection(&app, rect)).under;
        let points: Vec<[f32; 3]> = under
            .as_chunks::<32>()
            .0
            .iter()
            .flat_map(|l| [&l[0..12], &l[12..24]])
            .map(|b| [0, 4, 8].map(|i| f32::from_le_bytes(b[i..i + 4].try_into().unwrap())))
            .collect();
        let (lo, hi) = points
            .iter()
            .fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
        let (x, y) = (mid.x as f32, mid.y as f32);
        assert!(x > lo[0] && x < hi[0] && y > lo[1] && y < hi[1], "the grid spans {lo:?}..{hi:?}, the middle of the view is at {mid:?}");
    }

    /// #4214: the grid used to stop at a fixed distance from the view, so its edge showed as a
    /// cut across the picture. It now fades out with the distance from the middle of the view:
    /// the lines near the middle are at full strength, the ones further out are dimmer, and the
    /// grid is gone before the distance where it used to stop.
    #[test]
    fn the_grid_fades_out_with_the_distance_from_the_middle_of_the_view() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.show_grid = true;
        app.ui.perspective = false;
        // A top view: the ground is the XY plane, and the grid is centred on the target.
        app.cam.pitch = std::f64::consts::FRAC_PI_2;
        app.cam.yaw = 0.0;
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 100.0;
        let scene = build_sketch_scene(&app, &projection(&app, rect));
        let under = &scene.under;
        // Each line of the grid: two points, a colour and a width.
        let segments: Vec<([f32; 3], [f32; 3], u8)> = under
            .as_chunks::<32>()
            .0
            .iter()
            .map(|l| {
                let point = |at: usize| [0, 4, 8].map(|i| f32::from_le_bytes(l[at + i..at + i + 4].try_into().unwrap()));
                let index = u32::from_le_bytes(l[24..28].try_into().unwrap());
                let alpha = scene.palette.get(crate::gpu::PaletteIndex(index)).map_or(0, |c| c[3]);
                (point(0), point(12), alpha)
            })
            .collect();
        assert!(!segments.is_empty(), "the grid is drawn");
        // How far each piece of the grid is from the middle of the view, and the alpha it is
        // drawn with.
        let mut pieces: Vec<(f64, u8)> = segments
            .iter()
            .map(|(a, b, alpha)| {
                let mid = [0, 1, 2].map(|i| f64::from((a[i] + b[i]) * 0.5));
                ((mid[0] * mid[0] + mid[1] * mid[1]).sqrt(), *alpha)
            })
            .collect();
        pieces.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        // The grid in the middle of the view is at full strength.
        assert_eq!(pieces.iter().filter(|(d, _)| *d < 1.0).map(|(_, a)| *a).max(), Some(255), "the middle of the grid is at full strength");
        // The further from the middle, the dimmer: the outer half of the grid is fainter than the
        // inner half.
        let (near, far) = pieces.split_at(pieces.len() / 2);
        let mean = |v: &[(f64, u8)]| v.iter().map(|(_, a)| f64::from(*a)).sum::<f64>() / v.len().max(1) as f64;
        assert!(mean(near) > mean(far) + 10.0, "the grid fades: {} near the middle, {} far from it", mean(near), mean(far));
        // The grid is gone before the distance where it used to stop, so its edge never shows.
        let (_, major) = grid_step(app.cam.half_height());
        let cut = (app.cam.half_height() * 3.0 / major).ceil() * major;
        let farthest = pieces.last().map(|(d, _)| *d).unwrap_or(0.0);
        assert!(farthest < cut, "the grid fades out at {farthest}, short of the cut at {cut}");
    }

    /// #4214: the picture and the window draw the same grid. On a sketch's plane the headless
    /// render's grid used to reach further than the viewport's (the picture stretched the grid
    /// with the slant, the window did not), so a picture saved from the command line and the
    /// window the user sees showed different grids.
    #[test]
    fn the_picture_and_the_window_draw_the_same_grid_on_a_sketch_plane() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        app.ui.perspective = false;
        app.cam.fov = 0.0;
        app.cam.yaw = 0.0;
        app.cam.pitch = 35.0_f64.to_radians();
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 100.0;
        // The grid's centre, as both draw it: the sketch plane's, round the view's target.
        let plane = solidcraft_engine::view::open_sketch_plane(&app.session).unwrap();
        let (_, major) = grid_step(app.cam.half_height());
        let (centre, _, _) = solidcraft_engine::view::sketch_grid_frame(&plane, app.cam.target, major);
        // How far the picture's grid reaches from that centre.
        let scene = solidcraft_engine::view::scene(&app.session, &app.cam);
        let picture = scene.grid.iter().flat_map(|l| l.points.iter()).map(|p| (*p - centre).len()).fold(0.0_f64, f64::max);
        // How far the window's grid reaches: the same points, as bytes.
        let under = build_sketch_scene(&app, &projection(&app, rect)).under;
        let window = under
            .as_chunks::<32>()
            .0
            .iter()
            .flat_map(|l| [&l[0..12], &l[12..24]])
            .map(|b| {
                let p = [0, 4, 8].map(|i| f64::from(f32::from_le_bytes(b[i..i + 4].try_into().unwrap())));
                (Vec3::new(p[0], p[1], p[2]) - centre).len()
            })
            .fold(0.0_f64, f64::max);
        assert!(picture > 0.0, "the picture draws a grid");
        assert!(
            (picture - window).abs() <= 1e-3 * picture.max(window),
            "the picture's grid reaches {picture} mm from the middle of the view, the window's {window} mm"
        );
    }

    /// #4214: the grid fades in the picture as well. The same grid drawn at full strength is
    /// much fainter there where the fade has thinned it, and the same in the middle of the view,
    /// where the fade leaves the grid alone. Where the picture's far plane cuts the ground, the
    /// faded grid is already gone: it fades out instead of ending in a cut.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_grid_fades_out_in_the_picture_instead_of_ending_in_a_cut() {
        const SIZE: u32 = 512;
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(SIZE as f32, SIZE as f32));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.show_grid = true;
        app.ui.perspective = true;
        // A low view of the ground: the far ground is in the picture, where the grid used to end
        // in a cut.
        app.cam.pitch = 0.25;
        app.cam.yaw = 0.0;
        app.cam.target = Vec3::ZERO;
        app.cam.distance = 100.0;
        let proj = projection(&app, rect);
        let t = Tokens::get();
        let e = proj.cam.eye().to_f32();
        let eye = [e[0], e[1], e[2], if proj.cam.fov > 0.0 { 1.0 } else { 0.0 }];
        let render = |scene: GpuScene| {
            crate::gpu::render_offscreen(
                scene,
                None,
                proj.vp.to_f32(),
                proj.cam.back().to_f32(),
                eye,
                SIZE,
                None,
                [0.0; 8],
                crate::gpu::analysis_colors(&t.analysis),
            )
        };
        let faded = build_sketch_scene(&app, &proj).merged();
        // The same grid at full strength: every piece of it opaque.
        let mut full = faded.clone();
        let palette = std::sync::Arc::make_mut(&mut full.palette);
        for piece in full.under.as_chunks::<32>().0 {
            let index = u32::from_le_bytes(piece[24..28].try_into().unwrap());
            let slot = crate::gpu::PaletteIndex(index);
            if let Some(mut colour) = palette.get(slot) {
                colour[3] = 255;
                palette.set(slot, colour);
            }
        }
        let (Some(faded), Some(full)) = (render(faded), render(full)) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        // How far the pixels of each row are from the white the picture was cleared to: the
        // grid's lines show. The ink of a row, from the top of the picture.
        let row_ink = |img: &[u8]| -> Vec<u64> {
            (0..SIZE)
                .map(|y| {
                    (0..SIZE)
                        .map(|x| {
                            let i = ((y * SIZE + x) * 4) as usize;
                            img[i..i + 3].iter().map(|c| u64::from(255 - c)).sum::<u64>()
                        })
                        .sum::<u64>()
                })
                .collect()
        };
        let (faded_rows, full_rows) = (row_ink(&faded), row_ink(&full));
        // The middle of the view: the grid is drawn there, and the fade leaves it alone.
        let middle = SIZE as usize / 2;
        assert!(faded_rows[middle] > 0, "the grid is drawn in the middle of the view");
        assert_eq!(faded_rows[middle], full_rows[middle], "the fade leaves the middle of the grid alone");
        // The first row where the grid at full strength shows: the picture's far plane cuts the
        // ground there, and the rows above it are the far ground the grid has to fade out in.
        let first = full_rows.iter().position(|ink| *ink > 0).expect("the grid is drawn");
        // Where the ground is cut, the faded grid is gone: it fades out instead of ending in a
        // cut (#4214).
        let band = first.saturating_sub(10)..=first;
        let faded_ink: u64 = band.clone().map(|y| faded_rows[y]).sum();
        let full_ink: u64 = band.map(|y| full_rows[y]).sum();
        assert!(full_ink > 0, "the grid shows up to the far plane");
        assert!(
            faded_ink * 4 < full_ink,
            "the grid still shows at full strength where the far plane cuts the ground: {faded_ink} against {full_ink}"
        );
    }

    /// An open sketch always has its grid, on its own plane, whatever the Grid switch and the
    /// follow-the-view preference say; outside a sketch the switch decides.
    #[test]
    fn an_open_sketch_always_shows_a_grid_on_its_plane() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.show_grid = false;
        let grid = |app: &SolveApp| build_sketch_scene(app, &projection(app, rect)).under;
        assert!(grid(&app).is_empty(), "no grid outside a sketch with the switch off");
        app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
        for follows in [false, true] {
            app.preferences.grid_follows_view = follows;
            let under = grid(&app);
            assert!(!under.is_empty(), "the open sketch shows a grid (follows the view: {follows})");
            // Each line is 32 bytes: two f32 points, a colour and a width; the XZ plane is Y = 0.
            let ys: Vec<f32> =
                under.as_chunks::<32>().0.iter().flat_map(|l| [&l[4..8], &l[16..20]]).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            assert!(ys.iter().all(|y| y.abs() < 1e-3), "the grid lies on the sketch plane");
        }
        app.ui.show_grid = true;
        assert!(!grid(&app).is_empty());
    }

    #[test]
    fn imported_scene_projection_toggle_keeps_front_components_in_clip_range() {
        use solidcraft_engine::doc::{Document, FeatureKind, MeshData};
        let mut doc = Document::default();
        let meshes = [-10_000.0, 10_000.0]
            .into_iter()
            .enumerate()
            .map(|(i, y)| MeshData {
                name: format!("Panel {i}"),
                positions: vec![-0.25, y, -0.25, 0.25, y, -0.25, 0.0, y, 0.25].into(),
                triangles: vec![0, 1, 2].into(),
                color: None,
            })
            .collect();
        doc.add_feature(FeatureKind::MeshImport { file: "synthetic.stl".into(), meshes }, Some("Panels")).unwrap();
        let mut app = SolveApp::new(solidcraft_engine::Session::new(doc), crate::Services::default());
        app.cam.yaw = 0.0;
        app.cam.pitch = 0.0;
        app.fit_view();
        app.cancel_view_animation();
        app.cam.distance = 10_002.0;
        assert_eq!(app.session.world_state().bodies.len(), 2);
        let camera = app.cam;
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        let original = projection(&app, rect).vp;
        for shadow in [false, true] {
            app.ui.ground_shadow = shadow;
            for perspective in [false, true, false] {
                app.ui.perspective = perspective;
                let proj = projection(&app, rect);
                for body in &app.session.world_state().bodies {
                    let (mesh, _) = body.mesh_in_place();
                    assert_eq!(mesh.positions.len(), 3);
                    for p in &mesh.positions {
                        let c = proj.vp.apply(*p);
                        assert!(c.iter().all(|v| v.is_finite()));
                        assert!(c[3] > 0.0 && (0.0..=c[3]).contains(&c[2]), "shadow={shadow}, perspective={perspective}, {p:?}: {c:?}");
                        assert!(c[0].abs() <= c[3] && c[1].abs() <= c[3]);
                    }
                }
                assert_eq!(app.cam, camera, "changing projection preserves the orbit camera");
                if !perspective {
                    assert_eq!(proj.vp, original);
                }
            }
        }
    }

    /// A sketch is drawn in a parallel view even when the preference asks for perspective; the
    /// preference comes back when the sketch is finished.
    #[test]
    fn an_open_sketch_is_drawn_orthographic() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.ui.perspective = true;
        assert!(projection(&app, rect).cam.fov > 0.0);
        app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
        assert!(!app.perspective_view());
        assert_eq!(projection(&app, rect).cam.fov, 0.0);
        app.finish_sketch();
        assert!(app.ui.perspective, "the preference is left alone");
        assert!(projection(&app, rect).cam.fov > 0.0);
    }

    /// #2682: with the auto mode on, a camera facing a standard axis view draws orthographic and
    /// an off-axis one draws perspective; with the mode off the manual preference rules again.
    #[test]
    fn auto_ortho_faces_follows_camera_alignment() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        assert!(!app.ui.auto_ortho_faces, "the option is off by default");
        app.ui.auto_ortho_faces = true;
        for view in [StandardView::Front, StandardView::Top, StandardView::Left] {
            app.cam.set_view(view);
            assert_eq!(projection(&app, rect).cam.fov, 0.0, "{view:?} is orthographic");
        }
        // Orbiting off the face returns to perspective immediately.
        app.cam.yaw += 0.3;
        app.cam.pitch += 0.2;
        assert!(projection(&app, rect).cam.fov > 0.0, "off-axis is perspective");
        // Snapping back to a face (a Shift-snapped orbit or Home-style view) restores it.
        app.cam.set_view(StandardView::Left);
        assert_eq!(projection(&app, rect).cam.fov, 0.0);
        // The preference round-trips through the saved prefs.
        let saved = app.prefs();
        let mut fresh = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        fresh.load_prefs(&saved);
        assert!(fresh.ui.auto_ortho_faces, "the option is kept between runs");
        // With the mode off, the manual preference rules again (default orthographic).
        app.ui.auto_ortho_faces = false;
        assert_eq!(projection(&app, rect).cam.fov, 0.0);
        app.ui.perspective = true;
        assert!(projection(&app, rect).cam.fov > 0.0, "manual perspective is back");
        // An open sketch still wins: always orthographic.
        app.run("sketch.create", json!({"plane": "XZ"})).unwrap();
        assert_eq!(projection(&app, rect).cam.fov, 0.0);
    }

    #[test]
    fn perspective_button_is_inert_while_editing_a_sketch() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            for perspective in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.call("ui.set", json!({"layout":mode,"perspective":perspective}));
                h.app.preferences.navigation_bar.slots = vec!["projection".into()];
                h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
                h.settle();
                crate::test_support::click(&mut h, "nav:slot:projection");
                assert_eq!(h.app.ui.perspective, perspective, "a sketch's disabled control must preserve the normal view preference");
                let rect = h.app.viewport.rect.unwrap();
                assert_eq!(projection(&h.app, rect).cam.fov, 0.0);

                h.app.finish_sketch();
                h.settle();
                assert_eq!(h.app.perspective_view(), perspective, "finishing restores the original projection");
                crate::test_support::click(&mut h, "nav:slot:projection");
                assert_eq!(h.app.perspective_view(), !perspective, "the toggle works again outside sketch editing");
            }
        }
    }

    /// Create Sketch shows the Origin through bodies; the preference opts into that outside the
    /// tool while remaining off by default.
    #[test]
    fn create_sketch_shows_the_origin_planes_through_the_model() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":40,"width":40,"height":40})).unwrap();
        app.set_origin_shown(false);
        assert!(origin_planes(&app).is_empty());
        assert!(build_highlight(&app).xray.is_empty());
        app.start("sketch.create");
        assert!(app.picking_sketch_plane());
        assert_eq!(origin_planes(&app).len(), 3, "every origin plane is offered");
        let sc = build_highlight(&app);
        assert_eq!(sc.xray.len(), 3 * 6 * (6 * 4 + 4), "each plane's two triangles are drawn through the model");
        assert!(sc.guide_trans.is_empty());
        app.dialog = None;
        assert!(origin_planes(&app).is_empty());
        app.set_origin_shown(true);
        let sc = build_highlight(&app);
        assert!(sc.xray.is_empty(), "through-body reference geometry is off by default");
        assert_eq!(sc.guide_trans.len(), 3 * 6 * crate::gpu::TRI_SIZE, "planes use the ordinary depth-tested path by default");
        app.preferences.reference_geometry_through_bodies = true;
        let sc = build_highlight(&app);
        assert_eq!(sc.xray.len(), 3 * 6 * crate::gpu::TRI_SIZE, "the preference draws reference planes through bodies");
        assert!(sc.guide_trans.is_empty());
    }

    /// A finished sketch is depth-tested unless the preference draws reference geometry through
    /// bodies; switching it redraws the sketch scene while nothing else changes.
    #[test]
    fn the_preference_draws_finished_sketches_through_bodies() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        app.run("sketch.rectangle.two_point", json!({"p0": [-10, -10], "p1": [10, 10]})).unwrap();
        app.run("sketch.finish", json!({})).unwrap();
        let proj = projection(&app, rect);
        let off = build_sketch_scene(&app, &proj);
        assert!(!off.lines.is_empty() && off.overlay.is_empty(), "off by default: the sketch is depth-tested");
        let key = sketch_scene_key(&app, &proj);
        app.preferences.reference_geometry_through_bodies = true;
        assert_ne!(sketch_scene_key(&app, &proj), key, "the cached sketch scene is rebuilt");
        let on = build_sketch_scene(&app, &proj);
        assert!(on.lines.is_empty() && on.overlay.len() == off.lines.len(), "the sketch is drawn over the bodies");
    }

    /// #2581: a dialog waiting for a plane (here Mirror's plane) shows the origin planes in the
    /// view as Create Sketch does, so the plane can be picked there; while it waits for its
    /// objects it doesn't.
    #[test]
    fn a_dialog_picking_a_plane_shows_the_origin_planes() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("solid.box", json!({"length":40,"width":40,"height":40})).unwrap();
        app.set_origin_shown(false);
        app.start("solid.mirror");
        let d = app.dialog.as_ref().expect("the Mirror dialog");
        assert_eq!(d.active, 0, "it starts on its objects");
        assert!(!app.picking_plane() && origin_planes(&app).is_empty(), "no planes while picking objects");
        app.dialog.as_mut().unwrap().active = 1;
        assert!(app.picking_plane());
        assert_eq!(origin_planes(&app).len(), 3, "every origin plane is offered for the mirror plane");
        assert!(!build_highlight(&app).xray.is_empty(), "drawn through the model");
        app.dialog = None;
        assert!(origin_planes(&app).is_empty());
    }

    /// Primary rolls a live preview back for reselect, but not while a manipulator is dragged
    /// (same gate [`show`] uses via [`preview_should_roll_back`]).
    #[test]
    fn preview_rollback_stays_off_while_manipulator_dragging() {
        let mut s = solidcraft_engine::Session::default();
        s.execute("sketch.create", &json!({"plane": "xy"})).unwrap();
        s.execute("sketch.line", &json!({"points": [[0, 0], [20, 0]]})).unwrap();
        s.execute("sketch.finish", &json!({})).unwrap();
        let mut app = SolveApp::new(s, crate::Services::default());
        app.dialog = crate::dialogs::Dialog::for_command(&app, "solid.extrude");
        assert!(app.dialog.as_ref().is_some_and(|d| d.previews()), "extrude opens a preview dialog");
        app.preview.active = true;
        let primary = crate::tool_modifiers::Held { primary: true, ..Default::default() };
        assert!(preview_should_roll_back(&app, primary, false), "Primary alone rolls the preview back when a preview dialog is open");
        assert!(!preview_should_roll_back(&app, primary, true), "Primary must not roll the preview back while a manipulator arrow is dragged");
    }
}

#[cfg(test)]
mod standalone_point_tests {
    use super::*;

    /// #3267: a finished sketch's standalone points stay drawn (they can be picked for a hole);
    /// its curves' ends are not added, the open sketch draws its own points, and a hidden
    /// sketch draws none.
    #[test]
    fn a_finished_sketch_shows_its_standalone_points() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.run("sketch.create", json!({"plane": "XY"})).unwrap();
        app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})).unwrap();
        app.run("sketch.point", json!({"point": [5, 4]})).unwrap();
        assert!(standalone_points(&app).is_empty(), "the open sketch draws its own points");
        app.run("sketch.finish", json!({})).unwrap();
        let points = standalone_points(&app);
        assert_eq!(points.len(), 1, "only the placed point, not the rectangle's corners: {points:?}");
        assert!(points[0].dist(Vec3::new(5.0, 4.0, 0.0)) < 1e-9, "{points:?}");
        app.session.visibility.hidden_folders.push("c0/sketches".into());
        assert!(standalone_points(&app).is_empty(), "sketches hidden");
    }
}
