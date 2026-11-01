//! Display-only sampling. CAD curves, profiles and constraints are never changed here.
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use solidcraft_engine::doc::ModelState;
use solidcraft_engine::geom::{Loop2, Plane, Region2, Vec2, Vec3};
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
fn display_segs(sketch: &Sketch, index: usize, plane: &Plane, proj: &Proj) -> Vec<solidcraft_engine::geom::Seg2> {
    use solidcraft_engine::geom::Seg2;
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

/// A closed display region made from complete, unclipped adaptive boundaries. Near-plane
/// clipping or an exhausted budget suppresses the fill instead of inventing closing edges.
#[cfg(test)]
pub fn display_region(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Option<Region2> {
    let mut budget = SamplingBudget::new(4096, 65_536);
    display_region_with_budget(region, plane, proj, ppp, &mut budget)
}

fn display_region_with_budget(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32, shared: &mut SamplingBudget) -> Option<Region2> {
    let mut budget = SamplingBudget::new(shared.remaining_vertices.min(4096), shared.remaining_nodes.min(65_536));
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

#[cfg(test)]
pub fn region_triangles(region: &Region2, plane: &Plane, proj: &Proj, ppp: f32) -> Vec<[Vec2; 3]> {
    display_region(region, plane, proj, ppp).map_or_else(Vec::new, |display| fill_triangles(&display, &mut 65_536))
}

/// Bound the slab triangulator's allocation before calling it. Every slab emits one
/// triangle per edge crossing it; complex fills are omitted rather than cut into holes.
pub fn fill_triangles(display: &Region2, budget: &mut usize) -> Vec<[Vec2; 3]> {
    let mut spans = Vec::new();
    for loop_ in std::iter::once(&display.outer).chain(&display.holes) {
        for segment in &loop_.segs {
            let (a, b) = (segment.start(), segment.end());
            if (a.y - b.y).abs() > 1e-12 {
                spans.push((a.y.min(b.y), a.y.max(b.y)));
            }
        }
    }
    let mut heights: Vec<f64> = spans.iter().flat_map(|&(lo, hi)| [lo, hi]).collect();
    heights.sort_by(f64::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    // An edge crosses the slabs between its end heights. Counting every edge in every slab
    // overestimated a zoomed-in ring a hundredfold and dropped its hover fill (#164).
    let slab = |y: f64| heights.partition_point(|h| *h < y - 1e-12);
    let crossings = spans.iter().fold(0usize, |n, &(lo, hi)| n.saturating_add(slab(hi).saturating_sub(slab(lo))));
    if crossings > *budget {
        return Vec::new();
    }
    let triangles = display.triangulate(1.0);
    *budget = budget.saturating_sub(triangles.len());
    triangles
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

/// The most curves and profiles one sketch caches per model state and view.
const SKETCH_CURVES: usize = 4096;
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
        if budget.curves >= SKETCH_CURVES
            || self.curves.len() >= CACHED_CURVES
            || budget.sampling.remaining_vertices == 0
            || budget.sampling.remaining_nodes == 0
        {
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
        if budget.profiles >= SKETCH_PROFILES
            || self.profiles.len() >= CACHED_PROFILES
            || budget.sampling.remaining_vertices == 0
            || budget.sampling.remaining_nodes == 0
        {
            return Arc::new(None);
        }
        let region = Arc::new(
            state
                .sketch(sketch)
                .and_then(|ss| ss.profiles.get(index))
                .and_then(|profile| display_region_with_budget(&profile.region, plane, proj, ppp, &mut budget.sampling)),
        );
        budget.profiles += 1;
        self.profiles.insert(key, region.clone());
        region
    }
}

pub fn profile_region(app: &SolveApp, sketch: u64, index: usize, plane: &Plane, proj: &Proj) -> Arc<Option<Region2>> {
    let state = app.session.world_state();
    let ppp = app.viewport.pixels_per_point();
    match app.viewport.display.lock() {
        Ok(mut cache) => cache.profile(state, sketch, index, plane, proj, ppp),
        Err(_) => Arc::new(None),
    }
}

pub fn curve_sample(app: &SolveApp, sketch: u64, index: usize, plane: &Plane, proj: &Proj) -> Arc<SampledPath> {
    let state = app.session.world_state();
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
