//! The model's edges as the view sees them, worked out from the exact faces while the camera
//! stands still (shaded views with edges).
//!
//! The GPU draws faces pushed back from the eye a little, so a body's own edges draw whole on
//! faces seen at a slant, and draws curved faces from triangles that lie inside the true
//! surface. Both let an edge that lies behind a face show through it, by millimetres where the
//! face is seen at a grazing angle. Once the view and the model have stood still for
//! [`crate::viewport::REFINE_STILL_MS`], a job off the window thread casts rays from points
//! along every edge on the screen toward the viewer, against the exact trimmed faces of the
//! opaque bodies there ([`solidcraft_engine::geom::hlr::occlusion`]), and makes an edge layer:
//! the visible stretches of those edges (and in Shaded with hidden edges the hidden ones), which
//! the GPU draws instead of those bodies' lines in the model scene. Any change of the view or
//! the model drops it at once, so a moving view draws the model's own lines as before.
//!
//! Faded context bodies are left alone: their faces are a see-through ghost that hides
//! nothing, and their edges are drawn as before. See-through bodies' edges are worked out, but
//! their faces hide nothing. Mesh bodies and bodies placed with a scale are not worked out;
//! their faces still hide what the GPU's depth test hides.

use crate::SolveApp;
use crate::gpu::{EdgeLayer, PaletteIndex, ReplacedLines};
use solidcore_record::Vec3;
use solidcraft_engine::geom::hlr::{self, occlusion};
use solidcraft_engine::geom::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Samples along an edge are at most this many pixels apart: a hidden stretch shorter than this
/// between two seen samples can be missed.
const SAMPLE_PX: f64 = 3.0;
/// The most a drawn chord strays from its edge, in pixels (as [`crate::edge_detail`] draws).
const CHORD_PX: f64 = 0.25;
/// Boundary samples per curve piece when a face's trim is prepared: enough to tell inside from
/// outside away from the boundary; a hit nearer the boundary than the samples' chords hides
/// nothing, so coarser samples only leave more near the boundary seen.
const TRIM_SAMPLES: usize = 16;
/// The longest a body may take to prepare (ms): one that takes longer is left to the GPU's
/// depth test, and not tried again while it is in the design.
const PREPARE_BUDGET_MS: f64 = 2000.0;
/// The largest coordinate of a body's box worked out here (mm): a hundredth of what hidden lines
/// take, so placed bodies stay inside it.
const EXTENT_MM: f64 = 1e10;
/// Widths of the model's edge lines ([`crate::viewport`]'s `BodyDraw::build`).
const EDGE_WIDTH: f32 = 1.3;
const HIDDEN_WIDTH: f32 = 1.0;

/// Whether the viewport works out its edges: `SOLIDCRAFT_EXACT_EDGES=0` turns it off (to compare
/// pictures and timings with and without it). Never in the browser, which has no threads.
pub(crate) fn enabled() -> bool {
    if cfg!(target_arch = "wasm32") {
        return false;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SOLIDCRAFT_EXACT_EDGES").map_or(true, |v| v != "0"))
}

/// What the last finished job did.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct Stats {
    /// From the view standing still to the layer being ready (ms), and the job's own time.
    pub wait_ms: f64,
    pub job_ms: f64,
    pub prepare_ms: f64,
    pub snapshot_ms: f64,
    /// Bodies prepared in this job (the rest came from the cache), bodies whose edges were
    /// worked out, and faces that could hide them.
    pub prepared: usize,
    pub bodies: usize,
    pub edges: usize,
    pub rays: usize,
    /// Edges with a hidden stretch.
    pub hidden_edges: usize,
    /// Bodies left to the GPU (could not be prepared or are not rigidly placed).
    pub refused: usize,
    /// The longest one body took to prepare, and the sum over the bodies prepared (ms).
    pub slowest_prepare_ms: f64,
    pub prepare_sum_ms: f64,
    /// Bodies whose box is past [`EXTENT_MM`].
    pub too_large: usize,
    /// Bodies over [`PREPARE_BUDGET_MS`] in this job.
    pub over_budget: usize,
    /// Why the last job made no layer.
    pub error: Option<String>,
    /// Jobs started and finished since the design was opened, and whether one is running.
    pub started: usize,
    pub finished: usize,
    pub running: bool,
}

/// A prepared body, by the address of its record, with the body that holds that record.
type Cache = HashMap<usize, (solidcraft_engine::kernel::Body, Arc<Option<hlr::Prepared>>)>;

#[derive(Default)]
pub(crate) struct ExactEdges {
    rest: crate::viewport::CameraRest,
    /// The view (model scene and camera) the layer was made for, and the layer.
    layer: Option<(u64, Arc<EdgeLayer>)>,
    job: Option<Job>,
    /// The last view a job was started for, so a view whose job came back empty is not tried again.
    tried: u64,
    cache: Arc<Mutex<Cache>>,
    pub stats: Stats,
}

struct Job {
    view: u64,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<(Option<EdgeLayer>, Stats)>>>,
    started: f64,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// One shown body as a job sees it.
struct JobBody {
    body: solidcraft_engine::kernel::Body,
    place: solidcraft_engine::doc::Mat,
    /// Its faces hide edges.
    occludes: bool,
    /// Its edges to work out and draw: the lines they replace, its colours, and its edges
    /// (numbered as its record's) with their polylines where they stand, to leave out those off
    /// the screen.
    draw: Option<Draw>,
}

struct Draw {
    replaced: ReplacedLines,
    /// Where the body stands (`mesh` is the definition's).
    place: solidcraft_engine::doc::Mat,
    edge: PaletteIndex,
    hidden: PaletteIndex,
    mesh: Arc<solidcraft_engine::geom::Mesh>,
    edges: Vec<usize>,
}

/// The edge layer to draw this frame, if one is ready for the view; starts its job once the view
/// has stood still.
pub(crate) fn update(app: &mut SolveApp, ctx: &egui::Context, proj: &crate::viewport::Proj) -> Option<Arc<EdgeLayer>> {
    if !enabled() || app.viewport.gpu.is_none() || !matches!(app.ui.visual_style, 0 | 3) || crate::render_workspace::active(app) {
        let e = &mut app.viewport.exact_edges;
        e.layer = None;
        e.job = None;
        return None;
    }
    let ppp = app.viewport.pixels_per_point();
    let view = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        app.viewport.key.hash(&mut h);
        crate::sketch_display::projection_key(proj, ppp).hash(&mut h);
        app.ui.visual_style.hash(&mut h);
        h.finish()
    };
    let now = crate::now_ms();
    let e = &mut app.viewport.exact_edges;
    if let Some((v, layer)) = &e.layer {
        if *v == view {
            return Some(layer.clone());
        }
        e.layer = None;
    }
    let wait = e.rest.wait(view, now);
    if let Some(job) = &e.job {
        if job.view != view {
            e.job = None;
        } else {
            let done = job.result.lock().ok().and_then(|mut r| r.take());
            let Some((layer, mut stats)) = done else {
                e.stats.running = true;
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
                return None;
            };
            stats.wait_ms = now - job.started + crate::viewport::REFINE_STILL_MS;
            stats.started = e.stats.started;
            stats.finished = e.stats.finished + 1;
            e.stats = stats;
            e.job = None;
            let layer = Arc::new(layer?);
            e.layer = Some((view, layer.clone()));
            return Some(layer);
        }
    }
    if wait > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_millis(wait as u64 + 5));
        return None;
    }
    if e.tried == view {
        return None;
    }
    e.tried = view;
    e.stats.started += 1;
    let t0 = crate::now_ms();
    let (bodies, live) = snapshot(app, proj);
    let snapshot_ms = crate::now_ms() - t0;
    let e = &mut app.viewport.exact_edges;
    if let Ok(mut cache) = e.cache.lock() {
        cache.retain(|k, _| live.contains(k));
    }
    let job = Job { view, cancel: Arc::new(AtomicBool::new(false)), result: Arc::new(Mutex::new(None)), started: now };
    let input = Input {
        bodies,
        proj: *proj,
        height_px: f64::from(proj.rect.height()) * f64::from(ppp),
        hidden: app.ui.visual_style == 3,
        scene_key: app.viewport.key,
        snapshot_ms,
    };
    let (cancel, result, cache, repaint) = (job.cancel.clone(), job.result.clone(), e.cache.clone(), ctx.clone());
    e.job = Some(job);
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || {
        let done = run(input, &cache, &cancel);
        if !cancel.load(Ordering::Relaxed)
            && let Ok(mut r) = result.lock()
        {
            *r = Some(done);
            repaint.request_repaint();
        }
    });
    #[cfg(target_arch = "wasm32")]
    let _ = (input, cancel, result, cache, repaint);
    ctx.request_repaint_after(std::time::Duration::from_millis(30));
    None
}

/// The shown bodies a job needs (those on the screen, but for faded ones), and the record
/// addresses of every shown body (the prepared bodies worth keeping).
fn snapshot(app: &SolveApp, proj: &crate::viewport::Proj) -> (Vec<JobBody>, std::collections::HashSet<usize>) {
    let st = crate::open_meshing::shown_world(app);
    let replaced_bodies = app.preview.effective_replaced();
    let edges = app.viewport.body_edges.lock().map(|m| m.clone()).unwrap_or_default();
    let active = crate::viewport::active_body_names(app);
    let bounds = crate::viewport::body_bounds(app, &st);
    let lens = crate::display_lod::Lens::new(&proj.cam, f64::from(proj.rect.height()));
    let arena = app.viewport.palette.lock().unwrap_or_else(|e| e.into_inner());
    let looks = crate::viewport::see_through_bodies(app);
    let mut out = Vec::new();
    let mut live = std::collections::HashSet::new();
    for (b, (name, bx)) in st.bodies.iter().zip(bounds.iter()) {
        if b.name != *name || !app.body_visible(&b.name) || replaced_bodies.contains(&b.name) {
            continue;
        }
        let (body, place) = b.body_in_place();
        if body.is_mesh() {
            continue;
        }
        live.insert(std::ptr::from_ref(body.record()) as usize);
        let on_screen = match crate::viewport::screen_box(proj, bx, 0.0) {
            Some(r) => r.intersects(proj.rect),
            None => proj.cam.fov > 0.0 && lens.in_front(bx),
        };
        if !on_screen || crate::viewport::body_faded(app, b, &active) {
            continue;
        }
        let draw = edges.get(&b.name).zip(arena.slots(&b.name)).map(|(replaced, slots)| {
            let mesh = b.mesh_in_place().0;
            let edges = (0..mesh.edges.len()).filter(|&i| mesh.edge_visible(i) && mesh.shaded_edge_visible(i)).collect();
            Draw { replaced: replaced.clone(), place, edge: slots.edge, hidden: slots.hidden_edge, mesh, edges }
        });
        out.push(JobBody { body: body.clone(), place, occludes: !looks.contains(&b.name), draw });
    }
    (out, live)
}

struct Input {
    bodies: Vec<JobBody>,
    proj: crate::viewport::Proj,
    height_px: f64,
    hidden: bool,
    scene_key: u64,
    snapshot_ms: f64,
}

/// The job: prepare the bodies not prepared yet, then work out the edges on the screen, on as
/// many threads as the machine has to spare.
fn run(input: Input, cache: &Mutex<Cache>, cancel: &AtomicBool) -> (Option<EdgeLayer>, Stats) {
    let t0 = crate::now_ms();
    let mut stats = Stats { snapshot_ms: input.snapshot_ms, ..Stats::default() };
    let stop = || cancel.load(Ordering::Relaxed);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1).max(1));
    // 1. Prepared bodies, by record.
    let keys: Vec<usize> = input.bodies.iter().map(|b| std::ptr::from_ref(b.body.record()) as usize).collect();
    let mut have: HashMap<usize, Arc<Option<hlr::Prepared>>> = HashMap::new();
    let mut todo: Vec<(usize, &solidcraft_engine::kernel::Body)> = Vec::new();
    if let Ok(c) = cache.lock() {
        for (k, b) in keys.iter().zip(&input.bodies) {
            match c.get(k) {
                Some((_, p)) => {
                    have.insert(*k, p.clone());
                }
                None if !todo.iter().any(|(t, _)| t == k) => todo.push((*k, &b.body)),
                None => {}
            }
        }
    }
    stats.prepared = todo.len();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let made: Mutex<Vec<(usize, Arc<Option<hlr::Prepared>>, f64)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads.min(todo.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((k, body)) = todo.get(i) else { break };
                    if stop() {
                        break;
                    }
                    let t = crate::now_ms();
                    let options = hlr::Options { trim_samples: TRIM_SAMPLES, ..hlr::Options::default() };
                    let late = || stop() || crate::now_ms() - t > PREPARE_BUDGET_MS;
                    let p = hlr::prepare_cancellable(body.record(), &options, &late).ok();
                    if let Ok(mut m) = made.lock() {
                        m.push((*k, Arc::new(p), crate::now_ms() - t));
                    }
                }
            });
        }
    });
    // Bodies prepared before a cancel are kept; a body that failed then may have been cancelled.
    let cancelled = stop();
    let made: Vec<(usize, Arc<Option<hlr::Prepared>>)> = made
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, p, ms)| {
            if ms > PREPARE_BUDGET_MS {
                stats.over_budget += 1;
            }
            stats.slowest_prepare_ms = stats.slowest_prepare_ms.max(ms);
            stats.prepare_sum_ms += ms;
            (k, p)
        })
        .filter(|(_, p)| !cancelled || p.is_some())
        .collect();
    // `made` is in finishing order: match bodies by key.
    let bodies_by_key: HashMap<usize, &solidcraft_engine::kernel::Body> = todo.iter().map(|(k, b)| (*k, *b)).collect();
    if let Ok(mut c) = cache.lock() {
        for (k, p) in &made {
            if let Some(body) = bodies_by_key.get(k) {
                c.insert(*k, ((*body).clone(), p.clone()));
            }
        }
    }
    if cancelled {
        return (None, stats);
    }
    have.extend(made);
    stats.prepare_ms = crate::now_ms() - t0;
    // 2. The scene: one definition per prepared record, one placement per rigidly placed body.
    let mut defs: Vec<&hlr::Prepared> = Vec::new();
    let mut def_of: HashMap<usize, usize> = HashMap::new();
    let mut scene = Vec::new();
    let mut occludes = Vec::new();
    let mut drawn: Vec<(usize, &Draw)> = Vec::new();
    for (k, b) in keys.iter().zip(&input.bodies) {
        let Some(Some(def)) = have.get(k).map(|p| p.as_ref().as_ref()) else {
            stats.refused += 1;
            continue;
        };
        // A box past the coordinates hidden lines take (a surface bounded by little but its
        // record) is left to the GPU.
        let (lo, hi) = def.bounds();
        if ![lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].iter().all(|v| v.abs() <= EXTENT_MM) {
            stats.refused += 1;
            stats.too_large += 1;
            continue;
        }
        let m = b.place;
        let col = |c: usize| Vec3::new(m[c][0], m[c][1], m[c][2]);
        let placement = hlr::Placement { body: 0, rotation: [col(0), col(1), col(2)], translation: col(3) };
        if !placement.rigid() {
            stats.refused += 1;
            continue;
        }
        let at = *def_of.entry(*k).or_insert_with(|| {
            defs.push(def);
            defs.len() - 1
        });
        if let Some(d) = &b.draw {
            drawn.push((scene.len(), d));
        }
        scene.push(hlr::Placement { body: at, ..placement });
        occludes.push(b.occludes);
    }
    let occluders = match occlusion::Occluders::new(&defs, &scene, &occludes, &stop) {
        Ok(o) => o,
        Err(e) => {
            stats.error = Some(e.to_string());
            return (None, stats);
        }
    };
    // 3. Each drawn edge on the screen, from a shared list.
    let proj = input.proj;
    let lens = crate::display_lod::Lens::new(&proj.cam, input.height_px);
    let toward = -lens.back;
    let mm_per_px = move |p: Vec3| {
        if lens.perspective { (p - lens.eye).dot(toward).max(lens.near) / lens.scale } else { 1.0 / lens.scale }
    };
    let eye = if lens.perspective { occlusion::Eye::At(lens.eye) } else { occlusion::Eye::Along(toward) };
    let items: Vec<(usize, &Draw, usize)> = drawn
        .iter()
        .flat_map(|(pi, d)| d.edges.iter().filter(move |&&e| on_screen(&proj, d.mesh.edges.get(e), &d.place)).map(move |&e| (*pi, *d, e)))
        .collect();
    stats.bodies = drawn.len();
    stats.edges = items.len();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let parts: Mutex<Vec<(EdgeLayer, usize, usize)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut layer = EdgeLayer::default();
                let (mut rays, mut hidden_edges) = (0, 0);
                let f32s = |p: Vec3| [p.x as f32, p.y as f32, p.z as f32];
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(pi, d, edge)) = items.get(i) else { break };
                    if i.is_multiple_of(32) && stop() {
                        break;
                    }
                    let spacing = |p: Vec3| SAMPLE_PX * mm_per_px(p);
                    let tol = |p: Vec3| CHORD_PX * mm_per_px(p);
                    let pieces = match occluders.edge_pieces(pi, edge, eye, &spacing, &stop) {
                        Ok(p) => p,
                        // Not worked out: drawn whole, as before.
                        Err(_) => occlusion::EdgePieces { visible: occluders.edge_domain(pi, edge).into_iter().collect(), ..Default::default() },
                    };
                    rays += pieces.rays;
                    if !pieces.hidden.is_empty() {
                        hidden_edges += 1;
                    }
                    for r in &pieces.visible {
                        for w in occluders.edge_points(pi, edge, *r, &tol).windows(2) {
                            layer.line(f32s(w[0]), f32s(w[1]), d.edge, EDGE_WIDTH);
                        }
                    }
                    if input.hidden {
                        for r in &pieces.hidden {
                            for w in occluders.edge_points(pi, edge, *r, &tol).windows(2) {
                                layer.hidden_line(f32s(w[0]), f32s(w[1]), d.hidden, HIDDEN_WIDTH);
                            }
                        }
                    }
                }
                if let Ok(mut p) = parts.lock() {
                    p.push((layer, rays, hidden_edges));
                }
            });
        }
    });
    if stop() {
        return (None, stats);
    }
    let mut layer = EdgeLayer { scene_key: input.scene_key, ..EdgeLayer::default() };
    for (part, rays, hidden_edges) in parts.into_inner().unwrap_or_default() {
        layer.lines.extend_from_slice(&part.lines);
        layer.hidden.extend_from_slice(&part.hidden);
        stats.rays += rays;
        stats.hidden_edges += hidden_edges;
    }
    layer.replaced = drawn.iter().map(|(_, d)| d.replaced.clone()).collect();
    stats.job_ms = crate::now_ms() - t0;
    log::debug!("exact edges: {stats:?}");
    (Some(layer), stats)
}

/// Whether any of an edge's polyline (moved by `place`) can be on the screen: its box in clip
/// space meets the view, or a point is behind a perspective eye.
fn on_screen(proj: &crate::viewport::Proj, edge: Option<&Vec<Vec3>>, place: &solidcraft_engine::doc::Mat) -> bool {
    let Some(edge) = edge else { return false };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in edge {
        let p = solidcraft_engine::doc::apply_point(place, *p);
        let c = proj.vp.apply(p);
        if !(c[3] > 1e-9) {
            return true;
        }
        let (x, y) = (c[0] / c[3], c[1] / c[3]);
        lo = [lo[0].min(x), lo[1].min(y)];
        hi = [hi[0].max(x), hi[1].max(y)];
    }
    lo[0] <= 1.0 && hi[0] >= -1.0 && lo[1] <= 1.0 && hi[1] >= -1.0
}
