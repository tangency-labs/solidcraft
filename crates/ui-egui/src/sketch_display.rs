//! Display-only sampling. CAD curves, profiles and constraints are never changed here.
use solidcraft_engine::geom::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_engine::doc::ModelState;
use solidcraft_engine::render::{PixelCurveView, SampledPath, SamplingBudget, SamplingQuality, sample_segment, sample_segment_unclipped};
use solidcraft_engine::sketch::Sketch;

use crate::SolveApp;
use crate::viewport::Proj;

pub fn projection_key(proj: &Proj, ppp: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for n in proj.vp.0.iter().flatten() {
        n.to_bits().hash(&mut h);
    }
    for n in [proj.rect.left(), proj.rect.top(), proj.rect.width(), proj.rect.height(), ppp] {
        n.to_bits().hash(&mut h);
    }
    h.finish()
}

pub fn pixel_view(proj: &Proj, ppp: f32) -> PixelCurveView {
    PixelCurveView { vp: proj.vp, width_px: f64::from(proj.rect.width() * ppp), height_px: f64::from(proj.rect.height() * ppp) }
}

/// The pieces a curve is drawn and picked from. An infinite line (#1359) reaches across the whole
/// view: through the point of its line nearest the view's centre, far past the view each way
/// (the sampler clips it to the view). Only the display changes: the line keeps its two points.
fn display_segs(sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj) -> Vec<solidcore_record::Seg2> {
    use solidcore_record::Seg2;
    let segs = sketch.exact_segs(index).unwrap_or_else(|| sketch.segs(index));
    if !sketch.curves.get(index).is_some_and(|c| c.infinite) {
        return segs;
    }
    let target = proj.cam.target - plane.origin;
    let centre = Vec2::new(target.dot(plane.x), target.dot(plane.y));
    let reach = 4.0 * (proj.cam.distance.abs() + proj.cam.half_height().abs()) + 1.0;
    segs.into_iter()
        .map(|s| match s {
            Seg2::Line { a, b } => match (b - a).normalized() {
                Some(dir) => {
                    let mid = a + dir * (centre - a).dot(dir);
                    let reach = reach + mid.dist(centre);
                    Seg2::Line { a: mid - dir * reach, b: mid + dir * reach }
                }
                None => s,
            },
            other => other,
        })
        .collect()
}

pub fn curve_with_budget(sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj, ppp: f32, budget: &mut SamplingBudget) -> SampledPath {
    let segments = display_segs(sketch, index, plane, proj);
    let mut local = SamplingBudget::new(budget.remaining_vertices.min(4096), budget.remaining_nodes.min(65_536));
    let initial = local.clone();
    let mut out = SampledPath { parts: Vec::new(), quality: SamplingQuality::Complete, vertex_count: 0 };
    for segment in segments {
        let sampled = sample_segment(&segment, plane, &pixel_view(proj, ppp), &mut local);
        if sampled.quality != SamplingQuality::Complete {
            out.quality = sampled.quality;
        }
        out.vertex_count += sampled.vertex_count;
        out.parts.extend(sampled.parts);
    }
    budget.remaining_vertices -= initial.remaining_vertices - local.remaining_vertices;
    budget.remaining_nodes -= initial.remaining_nodes - local.remaining_nodes;
    out
}

pub fn curve_unclipped(sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj, ppp: f32) -> SampledPath {
    let mut budget = SamplingBudget::new(4096, 65_536);
    let mut out = SampledPath { parts: Vec::new(), quality: SamplingQuality::Complete, vertex_count: 0 };
    for segment in display_segs(sketch, index, plane, proj) {
        let sampled = sample_segment_unclipped(&segment, plane, &pixel_view(proj, ppp), &mut budget);
        if sampled.quality != SamplingQuality::Complete {
            out.quality = sampled.quality;
        }
        out.vertex_count += sampled.vertex_count;
        out.parts.extend(sampled.parts);
    }
    out
}

#[cfg(test)]
pub fn curve_parts(sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj, ppp: f32) -> Vec<Vec<Vec3>> {
    curve_with_budget(sketch, index, plane, proj, ppp, &mut SamplingBudget::new(4096, 65_536)).parts
}

// One profile can contain thousands of source spans and holes. It shares the sketch's global
// budget, but must not inherit the much smaller per-curve cap or its entire fill disappears.
const REGION_VERTICES: usize = 65_536;
const REGION_NODES: usize = 262_144;
const FILL_EDGES: usize = 20_000;
const FILL_CROSSINGS: usize = 1_000_000;

/// A closed display region made from complete, unclipped adaptive boundaries. Near-plane
/// clipping or an exhausted budget suppresses the fill instead of inventing closing edges.
#[cfg(test)]
pub fn display_region(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Option<Region2> {
    let mut budget = SamplingBudget::new(REGION_VERTICES, REGION_NODES);
    display_region_with_budget(region, plane, proj, ppp, &mut budget, false)
}

fn display_region_with_budget(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32, shared: &mut SamplingBudget, thin: bool) -> Option<Region2> {
    let mut budget = SamplingBudget::new(shared.remaining_vertices.min(REGION_VERTICES), shared.remaining_nodes.min(REGION_NODES));
    let initial = budget.clone();
    let result = (|| {
        let mut boundary = |loop_: &Loop2| -> Option<Loop2> {
            let mut points: Vec<Vec2> = Vec::new();
            for segment in &loop_.segs {
                let sampled = sample_segment_unclipped(segment, plane, &pixel_view(proj, ppp), &mut budget);
                if sampled.quality != SamplingQuality::Complete || sampled.parts.len() != 1 {
                    return None;
                }
                let part = sampled.parts.first()?;
                // Both exact endpoints must survive; a clipped boundary cannot close a region.
                if part.first()?.dist(plane.to_world(segment.start())) > 1e-8 || part.last()?.dist(plane.to_world(segment.end())) > 1e-8 {
                    return None;
                }
                for point in part {
                    let point = plane.to_local(*point);
                    if points.last().is_none_or(|last| last.dist(point) > 1e-12) {
                        points.push(point);
                    }
                }
            }
            if points.len() < 3 || points.first()?.dist(*points.last()?) > 1e-8 {
                return None;
            }
            points.pop();
            // A repaired drawing is thousands of sub-pixel spans per outline. Sub-pixel detail is
            // invisible in a fill but costs a triangle each, so drawn fills are thinned first
            // (display only: the sketch and its regions keep their exact geometry).
            if let Some(per_point) = points.first().filter(|_| thin).and_then(|at| points_per_unit(plane, proj, *at)) {
                let thinned = simplify_closed(&points, FILL_TOLERANCE_POINTS / per_point);
                if thinned.len() >= 3 {
                    points = thinned;
                }
            }
            Some(Loop2::polygon(&points))
        };
        let outer = boundary(&region.outer)?;
        let holes = region.holes.iter().map(&mut boundary).collect::<Option<Vec<_>>>()?;
        Some(Region2 { outer, holes })
    })();
    shared.remaining_vertices -= initial.remaining_vertices - budget.remaining_vertices;
    shared.remaining_nodes -= initial.remaining_nodes - budget.remaining_nodes;
    result
}

/// How far, in screen points, a drawn fill may sit from its exact boundary.
const FILL_TOLERANCE_POINTS: f64 = 0.35;

/// Screen points per sketch unit around `at`, or `None` where the view cannot say.
fn points_per_unit(plane: &Plane, proj: &Proj, at: Vec2) -> Option<f64> {
    let a = proj.to_screen(plane.to_world(at))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(at.x + 1.0, at.y)))?;
    let c = proj.to_screen(plane.to_world(Vec2::new(at.x, at.y + 1.0)))?;
    let scale = f64::from(a.distance(b)).max(f64::from(a.distance(c)));
    (scale.is_finite() && scale > 1e-9).then_some(scale)
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 { (((p.x - a.x) * dx + (p.y - a.y) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
    ((p.x - a.x - t * dx).powi(2) + (p.y - a.y - t * dy).powi(2)).sqrt()
}

/// Douglas-Peucker for a closed loop: drops points within `tolerance` of the chord they sit on.
/// Anchored at the first point and the one farthest from it, so the loop never collapses.
fn simplify_closed(points: &[Vec2], tolerance: f64) -> Vec<Vec2> {
    let n = points.len();
    if n < 8 || !tolerance.is_finite() || tolerance <= 0.0 {
        return points.to_vec();
    }
    let Some(first) = points.first() else { return Vec::new() };
    let Some(far) = (1..n).max_by(|a, b| first.dist(points[*a]).total_cmp(&first.dist(points[*b]))) else { return points.to_vec() };
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    let mut stack = vec![(0usize, far), (far, n)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (points[a], points[b % n]);
        let farthest = (a + 1..b).map(|i| (distance_to_segment(points[i], pa, pb), i)).max_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((distance, index)) = farthest
            && distance > tolerance
        {
            keep[index] = true;
            stack.push((a, index));
            stack.push((index, b));
        }
    }
    points.iter().zip(keep).filter_map(|(p, k)| k.then_some(*p)).collect()
}

#[cfg(test)]
pub fn region_triangles(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Vec<[Vec2; 3]> {
    display_region(region, plane, proj, ppp).map_or_else(Vec::new, |display| fill_triangles(&display, &mut 65_536))
}

/// Keep fills bounded without charging one output trapezoid for every unrelated vertex height.
/// A finely subdivided outline or many disjoint holes can share a long-lived pair of slab edges.
pub fn fill_triangles(display: &Region2, budget: &mut usize) -> Vec<[Vec2; 3]> {
    let mut edges = Vec::new();
    for loop_ in std::iter::once(&display.outer).chain(&display.holes) {
        for segment in &loop_.segs {
            let (a, b) = (segment.start(), segment.end());
            if !(a.is_finite() && b.is_finite()) {
                return Vec::new();
            }
            if (a.y - b.y).abs() > 1e-12 {
                if edges.len() >= FILL_EDGES {
                    return Vec::new();
                }
                edges.push(if a.y < b.y { [a, b] } else { [b, a] });
            }
        }
    }
    let mut heights: Vec<f64> = edges.iter().flat_map(|edge| [edge[0].y, edge[1].y]).collect();
    heights.sort_by(f64::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let slab = |y: f64| heights.partition_point(|h| *h < y - 1e-12);
    let crossings = edges.iter().fold(0usize, |n, edge| n.saturating_add(slab(edge[1].y).saturating_sub(slab(edge[0].y))));
    let triangles = if crossings <= *budget {
        display.triangulate(1.0)
    } else if crossings <= FILL_CROSSINGS {
        match coalesced_fill(edges, &heights, *budget) {
            Some(triangles) => triangles,
            None => return Vec::new(),
        }
    } else {
        return Vec::new();
    };
    if triangles.len() > *budget {
        return Vec::new();
    }
    *budget = budget.saturating_sub(triangles.len());
    triangles
}

fn edge_x(edge: &[Vec2; 2], y: f64) -> Option<f64> {
    let value = edge[0].x + (y - edge[0].y) / (edge[1].y - edge[0].y) * (edge[1].x - edge[0].x);
    value.is_finite().then_some(value)
}

/// Adjacent slabs with the same left and right source edges form one trapezoid exactly. An
/// edge pair ends at every changed boundary, so no extension crosses a hole or joins islands.
fn coalesced_fill(mut edges: Vec<[Vec2; 2]>, heights: &[f64], budget: usize) -> Option<Vec<[Vec2; 3]>> {
    edges.sort_by(|a, b| a[0].y.total_cmp(&b[0].y));
    let mut next_edge = 0;
    let mut active: Vec<usize> = Vec::new();
    let mut pending: HashMap<(usize, usize), (f64, f64)> = HashMap::new();
    let mut triangles = Vec::new();
    let mut visited = 0usize;
    for pair in heights.windows(2) {
        let [y0, y1] = pair else { continue };
        let middle = y0 * 0.5 + y1 * 0.5;
        while let Some(edge) = edges.get(next_edge)
            && edge[0].y <= middle
        {
            active.push(next_edge);
            next_edge += 1;
        }
        active.retain(|index| edges.get(*index).is_some_and(|edge| edge[1].y >= middle));
        visited = visited.saturating_add(active.len());
        if visited > FILL_CROSSINGS {
            return None;
        }
        let mut ordered = active.iter().map(|index| Some((*index, edge_x(edges.get(*index)?, middle)?))).collect::<Option<Vec<_>>>()?;
        ordered.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        if !ordered.len().is_multiple_of(2) {
            return None;
        }
        let mut current = HashMap::new();
        for pair in ordered.chunks(2) {
            let [left, right] = pair else { return None };
            let key = (left.0, right.0);
            let start = pending.remove(&key).map(|run| run.0).unwrap_or(*y0);
            current.insert(key, (start, *y1));
        }
        for (key, (start, end)) in pending.drain() {
            append_trapezoid(&edges, key, start, end, budget, &mut triangles)?;
        }
        pending = current;
    }
    for (key, (start, end)) in pending {
        append_trapezoid(&edges, key, start, end, budget, &mut triangles)?;
    }
    Some(triangles)
}

fn append_trapezoid(edges: &[[Vec2; 2]], key: (usize, usize), y0: f64, y1: f64, budget: usize, triangles: &mut Vec<[Vec2; 3]>) -> Option<()> {
    if triangles.len().checked_add(2)? > budget {
        return None;
    }
    let left = edges.get(key.0)?;
    let right = edges.get(key.1)?;
    let [a, b, c, d] =
        [Vec2::new(edge_x(left, y0)?, y0), Vec2::new(edge_x(right, y0)?, y0), Vec2::new(edge_x(right, y1)?, y1), Vec2::new(edge_x(left, y1)?, y1)];
    triangles.push([a, b, c]);
    triangles.push([a, c, d]);
    Some(())
}

pub fn boundary_parts(loop_: &Loop2, plane: &Plane, proj: &Proj, ppp: f32) -> Vec<Vec<Vec3>> {
    let mut budget = SamplingBudget::new(4096, 65_536);
    loop_.segs.iter().flat_map(|segment| sample_segment(segment, plane, &pixel_view(proj, ppp), &mut budget).parts).collect()
}

/// What one sketch may sample per model state and view: its curves and profiles cached, and the
/// vertices and nodes they take. Each sketch has its own (#6524): one shared by every sketch
/// let a large one drawn first (an imported drawing) use it all, so a later sketch's curves came
/// back empty, neither drawn nor pickable, while their points still showed.
struct SketchBudget {
    sampling: SamplingBudget,
    curves: usize,
    profiles: usize,
}

impl Default for SketchBudget {
    fn default() -> Self {
        Self { sampling: SamplingBudget::new(131_072, 1_048_576), curves: 0, profiles: 0 }
    }
}

impl SketchBudget {
    fn sampling_left(&self) -> bool {
        self.sampling.remaining_vertices > 0 && self.sampling.remaining_nodes > 0
    }

    fn room_for_curve(&self) -> bool {
        self.curves < SKETCH_CURVES && self.sampling_left()
    }

    fn room_for_profile(&self) -> bool {
        self.profiles < SKETCH_PROFILES && self.sampling_left()
    }
}

/// One sketch sampled past the cache (the open sketch at a dimension's typed value), within the
/// limits the cache gives one sketch: past them a curve or profile is not drawn, as there.
pub(crate) struct Uncached(SketchBudget);

#[cfg(test)]
thread_local! {
    /// A smaller sampling budget for [`Uncached`] (vertices), so a test can run out of it.
    pub(crate) static UNCACHED_VERTICES: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

impl Default for Uncached {
    fn default() -> Self {
        #[allow(unused_mut)]
        let mut budget = SketchBudget::default();
        #[cfg(test)]
        if let Some(vertices) = UNCACHED_VERTICES.with(std::cell::Cell::get) {
            budget.sampling = SamplingBudget::new(vertices, 1_048_576);
        }
        Self(budget)
    }
}

impl Uncached {
    pub(crate) fn curve(&mut self, sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj, ppp: f32) -> Vec<Vec<Vec3>> {
        if !self.0.room_for_curve() {
            return Vec::new();
        }
        self.0.curves += 1;
        curve_with_budget(sketch, index, plane, proj, ppp, &mut self.0.sampling).parts
    }

    pub(crate) fn profile(&mut self, region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Option<Region2> {
        if !self.0.room_for_profile() {
            return None;
        }
        self.0.profiles += 1;
        display_region_with_budget(region, plane, proj, ppp, &mut self.0.sampling, false)
    }

    /// Like [`Self::profile`], thinned to what a drawn fill can show, never what is picked.
    pub(crate) fn fill_profile(&mut self, region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Option<Region2> {
        if !self.0.room_for_profile() {
            return None;
        }
        self.0.profiles += 1;
        display_region_with_budget(region, plane, proj, ppp, &mut self.0.sampling, true)
    }
}

/// The most curves and profiles one sketch caches per model state and view.
const SKETCH_CURVES: usize = solidcraft_engine::sketch::MAX_POINTS;
const SKETCH_PROFILES: usize = 1024;
/// The most curves (and profiles) cached for every sketch together: a bound on memory however
/// many sketches are shown.
const CACHED_CURVES: usize = 65_536;
const CACHED_PROFILES: usize = 16_384;

#[derive(Default)]
pub struct DisplayCache {
    state: Option<Arc<ModelState>>,
    view: u64,
    curves: HashMap<(u64, usize, [u64; 9]), Arc<SampledPath>>,
    profiles: HashMap<(u64, usize, [u64; 9]), Arc<Option<Region2>>>,
    budgets: HashMap<u64, SketchBudget>,
}

impl DisplayCache {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.curves.len()
    }

    pub fn prepare(&mut self, state: Arc<ModelState>, proj: &Proj, ppp: f32) {
        self.reset(&state, proj, ppp);
    }

    fn reset(&mut self, state: &Arc<ModelState>, proj: &Proj, ppp: f32) {
        let view = projection_key(proj, ppp);
        if view != self.view || self.state.as_ref().is_none_or(|old| !Arc::ptr_eq(old, state)) {
            self.curves.clear();
            self.profiles.clear();
            self.budgets.clear();
            self.view = view;
            self.state = Some(Arc::clone(state));
        }
    }

    fn curve(&mut self, state: Arc<ModelState>, sketch: u64, index: usize, plane: &Plane, proj: &Proj, ppp: f32) -> Arc<SampledPath> {
        self.reset(&state, proj, ppp);
        let plane_key =
            [plane.origin.x, plane.origin.y, plane.origin.z, plane.x.x, plane.x.y, plane.x.z, plane.y.x, plane.y.y, plane.y.z].map(f64::to_bits);
        let key = (sketch, index, plane_key);
        if let Some(path) = self.curves.get(&key) {
            return path.clone();
        }
        let budget = self.budgets.entry(sketch).or_default();
        if !budget.room_for_curve() || self.curves.len() >= CACHED_CURVES {
            return Arc::new(SampledPath { parts: Vec::new(), quality: SamplingQuality::BudgetLimited, vertex_count: 0 });
        }
        let path = Arc::new(state.sketch(sketch).map_or_else(
            || SampledPath { parts: Vec::new(), quality: SamplingQuality::Invalid, vertex_count: 0 },
            |ss| curve_with_budget(&ss.sketch, index, plane, proj, ppp, &mut budget.sampling),
        ));
        budget.curves += 1;
        self.curves.insert(key, path.clone());
        path
    }

    fn profile(&mut self, state: Arc<ModelState>, sketch: u64, index: usize, plane: &Plane, proj: &Proj, ppp: f32) -> Arc<Option<Region2>> {
        self.reset(&state, proj, ppp);
        let plane_key =
            [plane.origin.x, plane.origin.y, plane.origin.z, plane.x.x, plane.x.y, plane.x.z, plane.y.x, plane.y.y, plane.y.z].map(f64::to_bits);
        let key = (sketch, index, plane_key);
        if let Some(region) = self.profiles.get(&key) {
            return region.clone();
        }
        let budget = self.budgets.entry(sketch).or_default();
        if !budget.room_for_profile() || self.profiles.len() >= CACHED_PROFILES {
            return Arc::new(None);
        }
        let region = Arc::new(
            state
                .sketch(sketch)
                .and_then(|ss| ss.profiles.get(index))
                .and_then(|profile| display_region_with_budget(&profile.region, plane, proj, ppp, &mut budget.sampling, false)),
        );
        budget.profiles += 1;
        self.profiles.insert(key, region.clone());
        region
    }
}

pub fn profile_region(app: &SolveApp, sketch: u64, index: usize, plane: &Plane, proj: &Proj) -> Arc<Option<Region2>> {
    let state = crate::open_meshing::session_world(&app.session);
    let ppp = app.viewport.pixels_per_point();
    match app.viewport.display.lock() {
        Ok(mut cache) => cache.profile(state, sketch, index, plane, proj, ppp),
        Err(_) => Arc::new(None),
    }
}

pub fn curve_sample(app: &SolveApp, sketch: u64, index: usize, plane: &Plane, proj: &Proj) -> Arc<SampledPath> {
    let state = crate::open_meshing::session_world(&app.session);
    let ppp = app.viewport.pixels_per_point();
    match app.viewport.display.lock() {
        Ok(mut cache) => cache.curve(state, sketch, index, plane, proj, ppp),
        Err(_) => Arc::new(state.sketch(sketch).map_or_else(
            || SampledPath { parts: Vec::new(), quality: SamplingQuality::Invalid, vertex_count: 0 },
            |ss| curve_with_budget(&ss.sketch, index, plane, proj, ppp, &mut SamplingBudget::new(4096, 65_536)),
        )),
    }
}

#[cfg(test)]
#[path = "infinite_line_display_tests.rs"]
mod infinite_line_display_tests;
