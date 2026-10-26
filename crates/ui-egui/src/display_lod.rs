//! View-dependent display refinement (#1320): bodies seen up close are drawn from finer display
//! plans, so curved faces, their edges and silhouettes stay smooth as the view zooms in.
//!
//! Each frame the viewport works out every shown body's screen-space error: the chord tolerance
//! of the chosen display quality (Preferences ▸ Graphics) in physical pixels, where the body is
//! nearest the eye. [`refinement_for`] turns that into a refinement step (each halves the chord),
//! with hysteresis. A step not made yet is planned off the UI thread and swapped in when it is
//! ready; until then the body keeps the step it has. Nothing is chosen or swapped in while the
//! camera moves (each swap rebuilds and uploads the whole model scene), only once it has stood
//! still for a moment (`viewport::REFINE_STILL_MS`). Only bodies on screen and big enough get
//! refined, within a triangle budget (the bodies that need it most first) and a cache size.
//!
//! The model scene draws a refined body's faces from the refined plan (on the GPU, or its CPU
//! mesh), and its edges and highlights from that plan's mesh, so a highlight keeps lying on the
//! drawn triangles (#1064). Display only: the reference mesh the model reads, exports and the
//! B-rep never change.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

use solidcraft_engine::doc::ModelBody;
use solidcraft_engine::doc::display_quality::{DisplayQuality, MAX_REFINE, refinement_for};
use solidcraft_engine::geom::tess::TessPlan;
use solidcraft_engine::geom::{Aabb3, Mesh, Vec3};

/// Most triangles all refined bodies may draw together (on top of the rest of the model).
pub(crate) const TRIANGLE_BUDGET: usize = 6_000_000;
/// Most bytes the refined plans and meshes kept may take; the least recently drawn go first.
pub(crate) const CACHE_BYTES: usize = 768 << 20;
/// A body smaller than this on screen (its box's longer side, physical pixels) is not refined.
pub(crate) const MIN_PX: f64 = 32.0;
/// Refinements planned at once, off the UI thread.
#[cfg(not(target_arch = "wasm32"))]
const MAX_JOBS: usize = 2;

/// A refined plan and its CPU mesh (none: the body has no plan at that step).
pub(crate) type Refined = Option<(Arc<TessPlan>, Arc<Mesh>)>;

/// Whether the viewport refines at all: `SOLIDCRAFT_DISPLAY_REFINE=0` turns it off (to compare
/// pictures and frame times with and without it).
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SOLIDCRAFT_DISPLAY_REFINE").map_or(true, |v| v != "0"))
}

/// What keeps a body's identity ([`body_id`]) its own while the cache holds it.
pub(crate) type Holder = Arc<dyn std::any::Any + Send + Sync>;

/// A body's identity here: the address of its [`ModelBody::display_identity`], which is shared by
/// every copy of the same body across timeline snapshots and held by the cache so the address
/// stays its own. Taking it meshes nothing: a part placed many times is drawn without making its
/// moved copies.
pub(crate) fn body_id(b: &ModelBody) -> (usize, Holder) {
    let holder = b.display_identity();
    (Arc::as_ptr(&holder) as *const () as usize, holder)
}

fn source_id(b: &ModelBody) -> (usize, Holder) {
    let holder = b.display_source_identity();
    (Arc::as_ptr(&holder) as *const () as usize, holder)
}

/// How the camera maps millimetres to pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lens {
    pub eye: Vec3,
    /// Unit direction from the target toward the eye.
    pub back: Vec3,
    /// Orthographic: pixels per millimetre everywhere. Perspective: pixels per millimetre at a
    /// depth of 1 mm (divided by the depth).
    pub scale: f64,
    pub perspective: bool,
    /// Nearest depth considered (mm), so a body around the eye does not ask for infinity.
    pub near: f64,
}

impl Lens {
    /// The view of `cam` in a viewport `height_px` physical pixels high.
    pub fn new(cam: &solidcraft_engine::render::Camera, height_px: f64) -> Lens {
        let perspective = cam.fov > 0.0;
        let scale = if perspective { height_px / (2.0 * (cam.fov * 0.5).tan()) } else { height_px / (2.0 * cam.half_height()) };
        Lens { eye: cam.eye(), back: cam.back(), scale, perspective, near: (cam.distance * 1e-2).max(1e-3) }
    }

    /// Whether any of a box lies in front of the eye.
    pub fn in_front(&self, b: &Aabb3) -> bool {
        let toward = -self.back;
        let far_corner = Vec3::new(
            if toward.x > 0.0 { b.max.x } else { b.min.x },
            if toward.y > 0.0 { b.max.y } else { b.min.y },
            if toward.z > 0.0 { b.max.z } else { b.min.z },
        );
        !self.perspective || (far_corner - self.eye).dot(toward) > 0.0
    }

    /// Pixels per millimetre where a box comes nearest the eye.
    pub fn px_per_mm(&self, b: &Aabb3) -> f64 {
        if !self.perspective {
            return self.scale;
        }
        // The box's nearest depth along the view: its support point toward the eye.
        let toward = -self.back;
        let near_corner = Vec3::new(
            if toward.x > 0.0 { b.min.x } else { b.max.x },
            if toward.y > 0.0 { b.min.y } else { b.max.y },
            if toward.z > 0.0 { b.min.z } else { b.max.z },
        );
        let depth = (near_corner - self.eye).dot(toward);
        self.scale / depth.max(self.near)
    }
}

/// The screen-space error (physical pixels) of drawing a body at `chord` and its box's
/// projected extent (`screen_px`, physical pixels; `None` when the box is not on screen).
pub(crate) fn error_px(chord: f64, bounds: &Aabb3, screen_px: Option<f64>, lens: &Lens) -> f64 {
    match screen_px {
        Some(px) if px >= MIN_PX => chord * lens.px_per_mm(bounds),
        _ => 0.0,
    }
}

/// One shown body as the refinement sees it this frame.
pub(crate) struct Candidate<'a> {
    pub body: &'a ModelBody,
    /// The chosen level's screen-space error ([`error_px`]).
    pub err_px: f64,
    /// Triangles at the chosen level (an estimate of the step-0 count).
    pub triangles: usize,
}

struct Entry {
    /// Keeps the body's id address its own while the entry lives.
    _holder: Holder,
    refined: Refined,
    bytes: usize,
    used: u64,
}

struct Shown {
    _holder: Holder,
    source: usize,
    quality: DisplayQuality,
    level: u8,
    /// World coordinates are needed only for individual face/edge feedback and CPU fallbacks.
    mesh: OnceLock<Arc<Mesh>>,
}

#[cfg(not(target_arch = "wasm32"))]
struct Job {
    key: (usize, DisplayQuality, u8),
    holder: Holder,
    rx: std::sync::mpsc::Receiver<Refined>,
}

/// What the refinement holds now (`ui.inspect` → `display_lod`).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct Stats {
    /// Bodies drawn finer than the chosen level.
    pub refined_bodies: usize,
    /// Triangles those bodies draw.
    pub refined_triangles: usize,
    /// Refined plans and meshes kept, and their bytes.
    pub cached: usize,
    pub cache_bytes: usize,
    /// Refinements being planned.
    pub planning: usize,
    /// Refinements planned so far, and the time they took (ms, off the UI thread).
    pub planned: usize,
    pub planning_ms: f64,
    /// Time the last choice of steps took on the UI thread (ms).
    pub choose_ms: f64,
}

/// The refinement state of a viewport.
#[derive(Default)]
pub(crate) struct Lod {
    /// The step each body is drawn at, by [`body_id`] (absent: 0, the chosen level).
    shown: HashMap<usize, Shown>,
    cache: HashMap<(usize, DisplayQuality, u8), Entry>,
    live_sources: std::collections::HashSet<usize>,
    #[cfg(not(target_arch = "wasm32"))]
    jobs: Vec<Job>,
    /// The web plans one refinement per frame: more are waiting.
    #[cfg(target_arch = "wasm32")]
    more: bool,
    /// Bumped whenever a body's drawn step changes (part of the scene's key).
    pub generation: u64,
    clock: u64,
    pub stats: Stats,
}

impl Lod {
    /// The refined plan and mesh `b` is drawn from, when it is drawn finer than the chosen level.
    pub fn refined(&self, b: &ModelBody, q: DisplayQuality) -> Refined {
        let (id, _) = body_id(b);
        let shown = self.shown.get(&id).filter(|s| s.quality == q)?;
        self.cache.get(&(shown.source, q, shown.level))?.refined.clone()
    }

    pub fn mesh(&self, b: &ModelBody, q: DisplayQuality) -> Option<Arc<Mesh>> {
        let shown = self.shown.get(&body_id(b).0).filter(|s| s.quality == q)?;
        let (_, mesh) = self.cache.get(&(shown.source, q, shown.level))?.refined.as_ref()?;
        Some(shown.mesh.get_or_init(|| b.place_display_mesh(mesh)).clone())
    }

    /// The step `b` is drawn at.
    #[cfg(test)]
    pub fn step(&self, b: &ModelBody) -> u8 {
        self.shown.get(&body_id(b).0).map_or(0, |s| s.level)
    }

    /// Choose each candidate's step, start planning the steps not made yet, and swap in those
    /// that are ready. `live` are the ids of every body in the model now (hidden ones too): the
    /// cache drops the others. Returns whether any body's drawn step changed.
    pub fn update(&mut self, q: DisplayQuality, candidates: Vec<Candidate<'_>>, live: &std::collections::HashSet<usize>) -> bool {
        self.clock += 1;
        self.collect();
        self.live_sources.clear();
        self.live_sources.extend(candidates.iter().map(|c| source_id(c.body).0));
        let mut changed = false;
        // The bodies that need it most first, within the triangle budget.
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|&a, &b| candidates[b].err_px.total_cmp(&candidates[a].err_px));
        let mut remaining = TRIANGLE_BUDGET;
        let mut wanted: Vec<(usize, u8)> = Vec::new();
        let mut requests: Vec<(usize, u8)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for i in order {
            let c = &candidates[i];
            let (id, _) = body_id(c.body);
            if !seen.insert(id) {
                continue;
            }
            let current = self.shown.get(&id).map_or(0, |s| s.level);
            let source = source_id(c.body).0;
            let mut k = refinement_for(c.err_px, current, MAX_REFINE);
            let estimate =
                |k: u8| self.cache.get(&(source, q, k)).and_then(|e| e.refined.as_ref()).map_or(c.triangles << k, |(_, m)| m.triangles.len());
            while k > 0 && estimate(k) > remaining {
                k -= 1;
            }
            if k > 0 {
                remaining -= estimate(k);
            }
            wanted.push((i, k));
        }
        for (i, k) in wanted {
            let (id, holder) = body_id(candidates[i].body);
            let source = source_id(candidates[i].body).0;
            let current = self.shown.get(&id).map_or(0, |s| s.level);
            if k == current && (k == 0 || (self.shown.get(&id).is_some_and(|s| s.quality == q) && self.cache.contains_key(&(source, q, k)))) {
                continue;
            }
            if k == 0 {
                self.shown.remove(&id);
                changed = true;
                continue;
            }
            match self.cache.get(&(source, q, k)) {
                Some(e) if e.refined.is_some() => {
                    self.shown.insert(id, Shown { _holder: holder, source, quality: q, level: k, mesh: OnceLock::new() });
                    changed = true;
                }
                // The planner refused this step: keep what is drawn.
                Some(_) => {}
                None => {
                    requests.push((i, k));
                }
            }
        }
        self.start(q, &candidates, requests);
        // Keep the steps drawn now from being evicted.
        for shown in self.shown.values() {
            if let Some(e) = self.cache.get_mut(&(shown.source, shown.quality, shown.level)) {
                e.used = self.clock;
            }
        }
        self.shown.retain(|id, _| live.contains(id));
        self.evict();
        self.refresh_stats(q);
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Whether refinements are still being planned (the viewport repaints to collect them).
    pub fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            !self.jobs.is_empty()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.more
        }
    }

    fn insert(&mut self, key: (usize, DisplayQuality, u8), holder: Holder, refined: Refined) {
        let bytes = refined.as_ref().map_or(0, |(plan, mesh)| mesh_bytes(mesh) + plan.vertex_count() * 48 + mesh.triangles.len() * 12);
        self.cache.insert(key, Entry { _holder: holder, refined, bytes, used: self.clock });
    }

    /// Collect finished refinements; whether any finished. They are swapped in by the next
    /// choice of steps ([`Lod::update`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn collect(&mut self) -> bool {
        let mut done = Vec::new();
        self.jobs.retain(|j| match j.rx.try_recv() {
            Ok(r) => {
                done.push((j.key, j.holder.clone(), r));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done.push((j.key, j.holder.clone(), None));
                false
            }
        });
        let any = !done.is_empty();
        for (key, holder, r) in done {
            self.insert(key, holder, r);
        }
        any
    }

    #[cfg(target_arch = "wasm32")]
    pub fn collect(&mut self) -> bool {
        self.more
    }

    /// Plan the requested steps: off the UI thread, a few at a time, the most needed first (the
    /// web has no threads: one per frame, right away).
    fn start(&mut self, q: DisplayQuality, candidates: &[Candidate<'_>], mut requests: Vec<(usize, u8)>) {
        requests.sort_by(|a, b| candidates[b.0].err_px.total_cmp(&candidates[a.0].err_px));
        #[cfg(not(target_arch = "wasm32"))]
        for (i, k) in requests {
            if self.jobs.len() >= MAX_JOBS {
                break;
            }
            let body = candidates[i].body;
            let (id, holder) = source_id(body);
            let key = (id, q, k);
            if self.jobs.iter().any(|j| j.key == key) {
                continue;
            }
            let body = body.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new().name("display-refine".into()).spawn(move || {
                let t0 = std::time::Instant::now();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body.refined_display_in_place(q, k))).ok().flatten();
                log::debug!("refined {} {k} steps in {:.1} ms", body.name, t0.elapsed().as_secs_f64() * 1e3);
                PLANNING_US.fetch_add(t0.elapsed().as_micros() as u64, std::sync::atomic::Ordering::Relaxed);
                let _ = tx.send(r);
            });
            match spawned {
                Ok(_) => self.jobs.push(Job { key, holder, rx }),
                Err(e) => {
                    log::warn!("display refinement: {e}");
                    break;
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.more = requests.len() > 1;
        }
        #[cfg(target_arch = "wasm32")]
        if let Some((i, k)) = requests.into_iter().next() {
            let body = candidates[i].body;
            let (id, holder) = source_id(body);
            let r = body.refined_display_in_place(q, k);
            self.insert((id, q, k), holder, r);
        }
    }

    /// Drop what belongs to bodies no longer in the model, then the least recently drawn steps
    /// until the cache fits its budget.
    fn evict(&mut self) {
        self.cache.retain(|(id, _, _), _| self.live_sources.contains(id));
        let mut total: usize = self.cache.values().map(|e| e.bytes).sum();
        if total <= CACHE_BYTES {
            return;
        }
        let mut by_age: Vec<((usize, DisplayQuality, u8), u64, usize)> = self.cache.iter().map(|(k, e)| (*k, e.used, e.bytes)).collect();
        by_age.sort_by_key(|(_, used, _)| *used);
        for (key, used, bytes) in by_age {
            if total <= CACHE_BYTES || used == self.clock {
                break;
            }
            self.cache.remove(&key);
            total -= bytes;
        }
    }

    fn refresh_stats(&mut self, q: DisplayQuality) {
        let drawn: Vec<&Entry> = self.shown.values().filter_map(|s| self.cache.get(&(s.source, q, s.level))).collect();
        self.stats.refined_bodies = drawn.len();
        self.stats.refined_triangles = drawn.iter().filter_map(|e| e.refined.as_ref()).map(|(_, m)| m.triangles.len()).sum();
        self.stats.cached = self.cache.len();
        self.stats.cache_bytes = self.cache.values().map(|e| e.bytes).sum();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.stats.planning = self.jobs.len();
        }
        self.stats.planning_ms = PLANNING_US.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e3;
        self.stats.planned = self.cache.len();
    }

    /// Plan a step right away and draw `b` at it (tests).
    #[cfg(test)]
    pub fn force(&mut self, b: &ModelBody, q: DisplayQuality, k: u8) -> bool {
        let (id, placed) = body_id(b);
        let (source, holder) = source_id(b);
        if !self.cache.contains_key(&(source, q, k)) {
            let r = b.refined_display_in_place(q, k);
            self.insert((source, q, k), holder, r);
        }
        let ok = self.cache.get(&(source, q, k)).is_some_and(|e| e.refined.is_some());
        if ok {
            self.shown.insert(id, Shown { _holder: placed, source, quality: q, level: k, mesh: OnceLock::new() });
            self.generation += 1;
        }
        ok
    }
}

/// Microseconds spent planning refinements (all threads).
static PLANNING_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn mesh_bytes(m: &Mesh) -> usize {
    m.positions.len() * 24
        + m.normals.len() * 24
        + m.triangles.len() * 12
        + m.tri_face.len() * 4
        + m.edges.iter().map(|e| e.len() * 24).sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::render::Camera;

    fn cylinder() -> ModelBody {
        let body = solidcraft_engine::kernel::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        ModelBody::new("Body1".into(), body, 1)
    }

    #[test]
    fn placements_join_one_refinement_and_choose_their_levels_independently() {
        use serde_json::json;
        let mut s = solidcraft_engine::Session::default();
        s.execute("component.create", &json!({"name":"Part"})).unwrap();
        s.execute("solid.cylinder", &json!({"radius":10,"height":20})).unwrap();
        s.execute("occurrence.copy", &json!({"component":"Part","translate":[40,0,0]})).unwrap();
        s.execute("component.activate", &json!({"component":"root"})).unwrap();
        let world = s.world_state();
        let (a, b) = (&world.bodies[0], &world.bodies[1]);
        assert_ne!(body_id(a).0, body_id(b).0);
        assert_eq!(source_id(a).0, source_id(b).0);
        let q = DisplayQuality::Normal;
        let live = [body_id(a).0, body_id(b).0].into();
        let base = a.mesh_in_place().0.triangles.len();
        let candidates = |second| vec![Candidate { body: a, err_px: 6., triangles: base }, Candidate { body: b, err_px: second, triangles: base }];
        let mut lod = Lod::default();
        lod.update(q, candidates(6.), &live);
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(lod.jobs.len(), 1, "placements join the same pending work");
        for _ in 0..2000 {
            lod.update(q, candidates(6.), &live);
            if lod.step(a) == 3 && lod.step(b) == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!((lod.step(a), lod.step(b)), (3, 3));
        assert_eq!(lod.cache.len(), 1);
        let ma = lod.refined(a, q).unwrap().1;
        let mb = lod.refined(b, q).unwrap().1;
        assert!(Arc::ptr_eq(&ma, &mb));
        let moved = lod.mesh(b, q).unwrap();
        let expected = solidcraft_engine::doc::apply_point(&b.display_transform(), mb.positions[0]);
        assert!(moved.positions[0].dist(expected) < 1e-9);
        assert!(Arc::ptr_eq(&moved, &lod.mesh(b, q).unwrap()));
        lod.update(q, candidates(0.), &live);
        assert_eq!((lod.step(a), lod.step(b)), (3, 0));
        assert!(Arc::ptr_eq(&ma, &lod.refined(a, q).unwrap().1));
    }

    /// Zooming in raises the error with the pixels per millimetre, perspective or not, and a
    /// box nearer the eye counts more.
    #[test]
    fn the_error_follows_the_zoom() {
        let b = Aabb3 { min: Vec3::new(-10.0, -10.0, 0.0), max: Vec3::new(10.0, 10.0, 20.0) };
        let q = DisplayQuality::Normal;
        for fov in [0.0, 0.6] {
            let mut cam = Camera { distance: 200.0, fov, ..Camera::default() };
            let far = error_px(q.chord(30.0), &b, Some(300.0), &Lens::new(&cam, 1000.0));
            cam.distance = 50.0;
            let near = error_px(q.chord(30.0), &b, Some(1200.0), &Lens::new(&cam, 1000.0));
            assert!(near > far * 3.0, "fov {fov}: {far} -> {near}");
            assert_eq!(error_px(q.chord(30.0), &b, None, &Lens::new(&cam, 1000.0)), 0.0, "off screen");
            assert_eq!(error_px(q.chord(30.0), &b, Some(MIN_PX / 2.0), &Lens::new(&cam, 1000.0)), 0.0, "too small");
        }
        // Orthographic: chord × pixels per mm.
        let cam = Camera { distance: 100.0, fov: 0.0, ..Camera::default() };
        let e = error_px(q.chord(30.0), &b, Some(500.0), &Lens::new(&cam, 800.0));
        assert!((e - q.chord(30.0) * 800.0 / (2.0 * cam.half_height())).abs() < 1e-12);
    }

    /// A close-up refines the body (planned off the thread, then swapped in), a step finer has
    /// more triangles and stays within its chord, and zooming back out returns to the chosen level.
    #[test]
    fn a_close_up_refines_and_zooming_out_returns() {
        let b = cylinder();
        let q = DisplayQuality::Normal;
        let live: std::collections::HashSet<usize> = [body_id(&b).0].into();
        let base = b.plan_mesh_at(q).unwrap().triangles.len();
        let mut lod = Lod::default();
        let cand = |err: f64| vec![Candidate { body: &b, err_px: err, triangles: base }];
        assert!(!lod.update(q, cand(0.5), &live));
        assert_eq!(lod.step(&b), 0);
        // 6 px of error: three steps.
        let mut swapped = false;
        for _ in 0..2000 {
            swapped |= lod.update(q, cand(6.0), &live);
            if lod.step(&b) == 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(swapped && lod.step(&b) == 3, "refined to step {}", lod.step(&b));
        let (_, mesh) = lod.refined(&b, q).unwrap();
        assert!(mesh.triangles.len() > base * 2, "{} vs {base}", mesh.triangles.len());
        let r = |v: Vec3| (v.x * v.x + v.y * v.y).sqrt();
        let sag = mesh
            .triangles
            .iter()
            .map(|t| t.map(|i| mesh.positions[i as usize]))
            .filter(|t| t.iter().all(|p| (r(*p) - 10.0).abs() < 1e-9))
            .map(|[a, b, c]| 10.0 - r((a + b + c) * (1.0 / 3.0)))
            .fold(0.0, f64::max);
        assert!(sag <= q.refined_chord(b.body.size(), 3) * 1.01, "sag {sag}");
        // Refining never makes (or replaces) the reference mesh.
        assert!(b.cached_mesh().is_none());
        // Zoomed out a little: kept; far out: the chosen level.
        assert!(!lod.update(q, cand(3.0), &live));
        assert_eq!(lod.step(&b), 3);
        assert!(lod.update(q, cand(0.1), &live));
        assert_eq!(lod.step(&b), 0);
        assert!(lod.refined(&b, q).is_none());
        // Back in: swapped from the cache at once.
        assert!(lod.update(q, cand(6.0), &live));
        assert_eq!(lod.step(&b), 3);
        // The body leaves the model: dropped.
        lod.update(q, Vec::new(), &Default::default());
        assert_eq!(lod.stats.cached, 0);
        assert_eq!(lod.step(&b), 0);
    }

    /// The budget refines the bodies that need it most and leaves the rest at the chosen level.
    #[test]
    fn the_budget_goes_to_the_largest_errors() {
        let (a, b) = (cylinder(), cylinder());
        let q = DisplayQuality::Normal;
        let live: std::collections::HashSet<usize> = [body_id(&a).0, body_id(&b).0].into();
        let mut lod = Lod::default();
        // Each would want four steps at 3M triangles a step-0 count: only the first fits.
        let cands = vec![
            Candidate { body: &a, err_px: 2.0, triangles: TRIANGLE_BUDGET / 3 },
            Candidate { body: &b, err_px: 12.0, triangles: TRIANGLE_BUDGET / 3 },
        ];
        // Distinct bodies (two ModelBody::new calls), so distinct ids.
        assert_ne!(body_id(&a).0, body_id(&b).0);
        lod.update(q, cands, &live);
        assert_eq!(lod.stats.planning, 1, "one refinement fits the budget");
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(lod.jobs[0].key.0, body_id(&b).0, "the larger error first");
    }
}
