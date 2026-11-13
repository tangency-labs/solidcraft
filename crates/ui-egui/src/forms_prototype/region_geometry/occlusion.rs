//! Frozen display occlusion only. No triangle identity leaves this helper.
//! False means no hull certificate; it never means that a hull is visible.
use super::super::{context, controls, live_cad};
use crate::{SolveApp, viewport::Proj};
use solidcore_record::Vec3;
use solidcraft_engine::geom::{Mesh, MeshIndex, Vec3Geom as _, bspline::HPoint};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

const MAX_TRIANGLES: usize = 2_000_000;
const MAX_SOURCES: usize = 256;
const MAX_CONTROLS: usize = 1089;
const MAX_DISCRETE_RAYS: usize = 64;
const MAX_CANDIDATES: usize = 256;
const MAX_CELLS: usize = 4096;
const MAX_COVER_DEPTH: usize = 20;
const MAX_LATTICE_WORK: usize = 180_000;
const MAX_POLYGON: usize = 768;
const MAX_OWN_RAY_MEMO: usize = 4096;
type ScreenKeys = (Vec<f32>, Vec<f32>);

#[cfg(test)]
#[derive(Debug, Default)]
struct CertificateStats {
    hull_refused: usize,
    candidate_refused: usize,
    candidates: usize,
    geometry: usize,
    normal: usize,
    facing: usize,
    near_origin: usize,
    direction: usize,
    margin: usize,
    world_depth: usize,
    endpoint_depth: usize,
    endpoint_tight_proved: usize,
    projection: usize,
    area: usize,
    safe: usize,
    polygons: usize,
    union_boundaries: usize,
    coverage_refused: usize,
    coverage_proved: usize,
    discrete_refused: usize,
    discrete_rays: usize,
    discrete_missed: usize,
    discrete_depth: usize,
    discrete_proved: usize,
}
#[cfg(test)]
thread_local! {static CERTIFICATE_TRACE:std::cell::RefCell<Option<CertificateStats>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)]
fn certificate_trace(update: impl FnOnce(&mut CertificateStats)) {
    CERTIFICATE_TRACE.with(|trace| {
        if let Some(stats) = trace.borrow_mut().as_mut() {
            update(stats);
        }
    });
}

fn charge(budget: &mut usize, work: usize) -> Result<(), String> {
    *budget = budget.checked_sub(work).ok_or("Form occlusion exceeded its work budget")?;
    Ok(())
}
fn reserve<T>(items: &mut Vec<T>, count: usize) -> Result<(), String> {
    items.try_reserve(count).map_err(|_| "Form occlusion allocation budget exceeded".into())
}

/// Outward-rounded intervals keep an uncertain depth/edge from hiding geometry.
#[derive(Clone, Copy, Debug)]
struct I {
    lo: f64,
    hi: f64,
}
impl I {
    fn at(x: f64) -> Self {
        Self { lo: x, hi: x }
    }
    fn whole() -> Self {
        Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY }
    }
    fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }
    fn add(self, b: Self) -> Self {
        Self { lo: (self.lo + b.lo).next_down(), hi: (self.hi + b.hi).next_up() }
    }
    fn neg(self) -> Self {
        Self { lo: -self.hi, hi: -self.lo }
    }
    fn sub(self, b: Self) -> Self {
        self.add(b.neg())
    }
    fn mul(self, b: Self) -> Self {
        if !self.finite() || !b.finite() {
            return Self::whole();
        }
        let p = [self.lo * b.lo, self.lo * b.hi, self.hi * b.lo, self.hi * b.hi];
        if p.iter().any(|v| !v.is_finite()) {
            return Self::whole();
        }
        Self { lo: p.iter().copied().fold(f64::INFINITY, f64::min).next_down(), hi: p.iter().copied().fold(f64::NEG_INFINITY, f64::max).next_up() }
    }
    fn div(self, b: Self) -> Self {
        if !b.finite() || b.lo <= 0.0 && b.hi >= 0.0 {
            return Self::whole();
        }
        self.mul(Self { lo: (1.0 / b.hi).next_down(), hi: (1.0 / b.lo).next_up() })
    }
    fn square(self) -> Self {
        if !self.finite() {
            return Self::whole();
        }
        let minimum = if self.lo <= 0.0 && self.hi >= 0.0 { 0.0 } else { self.lo.abs().min(self.hi.abs()) };
        let maximum = self.abs();
        Self { lo: (minimum * minimum).next_down().max(0.0), hi: (maximum * maximum).next_up() }
    }
    fn sqrt(self) -> Self {
        if self.lo < 0.0 || !self.finite() { Self::whole() } else { Self { lo: self.lo.sqrt().next_down().max(0.0), hi: self.hi.sqrt().next_up() } }
    }
    fn abs(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    fn grow(self, e: f64) -> Self {
        Self { lo: (self.lo - e).next_down(), hi: (self.hi + e).next_up() }
    }
}
type V = [I; 3];
type P = [I; 2];
fn v(p: Vec3) -> V {
    [I::at(p.x), I::at(p.y), I::at(p.z)]
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].add(b[i]))
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].sub(b[i]))
}
fn scale(a: V, b: I) -> V {
    a.map(|x| x.mul(b))
}
fn dot(a: V, b: V) -> I {
    (0..3).fold(I::at(0.0), |s, i| s.add(a[i].mul(b[i])))
}
fn cross(a: V, b: V) -> V {
    [a[1].mul(b[2]).sub(a[2].mul(b[1])), a[2].mul(b[0]).sub(a[0].mul(b[2])), a[0].mul(b[1]).sub(a[1].mul(b[0]))]
}
fn norm(a: V) -> I {
    a.iter().fold(I::at(0.0), |s, x| s.add(x.square())).sqrt()
}
fn magnitude(a: V) -> f64 {
    a.iter().fold(1.0_f64, |m, x| m.max(x.abs()))
}
fn orient(a: P, b: P, p: P) -> I {
    b[0].sub(a[0]).mul(p[1].sub(a[1])).sub(b[1].sub(a[1]).mul(p[0].sub(a[0])))
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    lo: [f64; 2],
    hi: [f64; 2],
}
impl Bounds {
    fn empty() -> Self {
        Self { lo: [f64::INFINITY; 2], hi: [f64::NEG_INFINITY; 2] }
    }
    fn add(&mut self, p: P) {
        for (i, x) in p.into_iter().enumerate() {
            if let (Some(lo), Some(hi)) = (self.lo.get_mut(i), self.hi.get_mut(i)) {
                *lo = lo.min(x.lo);
                *hi = hi.max(x.hi);
            }
        }
    }
    fn union(&mut self, b: Self) {
        self.add([I { lo: b.lo[0], hi: b.hi[0] }, I { lo: b.lo[1], hi: b.hi[1] }]);
    }
    fn valid(self) -> bool {
        (0..2).all(|i| self.lo[i].is_finite() && self.hi[i].is_finite() && self.lo[i] <= self.hi[i])
    }
    fn overlaps(self, b: Self) -> bool {
        (0..2).all(|i| self.lo[i] <= b.hi[i] && b.lo[i] <= self.hi[i])
    }
    fn encloses(self, b: Self) -> bool {
        (0..2).all(|i| self.lo[i] <= b.lo[i] && b.hi[i] <= self.hi[i])
    }
    fn point(self) -> P {
        [I { lo: self.lo[0], hi: self.hi[0] }, I { lo: self.lo[1], hi: self.hi[1] }]
    }
    fn grow(self, e: f64) -> Self {
        Self { lo: self.lo.map(|x| (x - e).next_down()), hi: self.hi.map(|x| (x + e).next_up()) }
    }
    fn corners(self) -> [[f64; 2]; 4] {
        [[self.lo[0], self.lo[1]], [self.hi[0], self.lo[1]], [self.hi[0], self.hi[1]], [self.lo[0], self.hi[1]]]
    }
}

/// A projective chart of physical camera rays, not triangle/model ownership.
/// Its common affine pixel scale is immaterial to coverage. In orthographic
/// mode the two raw rows annihilate the exact stored ray direction algebraically.
struct Chart {
    eye: Vec3,
    target: Vec3,
    r: Vec3,
    u: Vec3,
    back: Vec3,
    d: Vec3,
    axes: [usize; 3],
    a: [[f64; 2]; 2],
    center: [f64; 2],
    perspective: bool,
}
impl Chart {
    fn new(proj: &Proj) -> Result<Self, String> {
        let (r, u, back) = proj.cam.basis();
        let (origin, d) = proj.ray(proj.rect.center());
        let width = f64::from(proj.rect.width());
        let height = f64::from(proj.rect.height());
        let hh = proj.cam.half_height();
        if !proj.rect.is_finite()
            || width < 1.0
            || height < 1.0
            || width > 1e7
            || height > 1e7
            || !hh.is_finite()
            || hh <= 0.0
            || !proj.cam.fov.is_finite()
            || !proj.cam.distance.is_finite()
            || proj.cam.distance <= 0.0
            || !proj.cam.target.is_finite()
            || !proj.cam.eye().is_finite()
            || !r.is_finite()
            || !u.is_finite()
            || !back.is_finite()
            || !origin.is_finite()
            || !d.is_finite()
            || proj.vp.0.iter().flatten().any(|x| !x.is_finite())
        {
            return Err("invalid Form occlusion camera".into());
        }
        let coordinates = [d.x, d.y, d.z];
        let k = (0..3).max_by(|&i, &j| coordinates[i].abs().total_cmp(&coordinates[j].abs())).ok_or("missing camera axis")?;
        if coordinates[k].abs() < 0.1 {
            return Err("invalid Form occlusion ray".into());
        }
        let axes = [(k + 1) % 3, (k + 2) % 3, k];
        let raw = |p: Vec3| {
            let q = [p.x, p.y, p.z];
            [q[axes[0]] * coordinates[k] - q[k] * coordinates[axes[0]], q[axes[1]] * coordinates[k] - q[k] * coordinates[axes[1]]]
        };
        let x = raw(r);
        let y = raw(u);
        let determinant = x[0] * y[1] - x[1] * y[0];
        if !determinant.is_finite() || determinant.abs() < 1e-6 {
            return Err("degenerate Form occlusion camera chart".into());
        }
        let sx = width / (2.0 * hh * (width / height));
        let sy = -height / (2.0 * hh);
        let a = if proj.cam.fov > 0.0 {
            [[sx * proj.cam.distance, 0.0], [0.0, sy * proj.cam.distance]]
        } else {
            [[sx * y[1] / determinant, -sx * y[0] / determinant], [-sy * x[1] / determinant, sy * x[0] / determinant]]
        };
        if a.iter().flatten().any(|x| !x.is_finite()) {
            return Err("Form occlusion chart overflowed".into());
        }
        Ok(Self {
            eye: proj.cam.eye(),
            target: proj.cam.target,
            r,
            u,
            back,
            d,
            axes,
            a,
            center: [f64::from(proj.rect.left()) + width * 0.5, f64::from(proj.rect.top()) + height * 0.5],
            perspective: proj.cam.fov > 0.0,
        })
    }
    fn orthographic_offset(&self, delta: V) -> P {
        let d = [self.d.x, self.d.y, self.d.z];
        let [i, j, k] = self.axes;
        let raw = [delta[i].mul(I::at(d[k])).sub(delta[k].mul(I::at(d[i]))), delta[j].mul(I::at(d[k])).sub(delta[k].mul(I::at(d[j])))];
        std::array::from_fn(|i| I::at(self.a[i][0]).mul(raw[0]).add(I::at(self.a[i][1]).mul(raw[1])))
    }

    fn project(&self, q: V) -> Option<P> {
        let p = if self.perspective {
            let delta = sub(q, v(self.eye));
            let depth = dot(v(self.back), delta).neg();
            if !depth.finite() || depth.lo <= 1e-9 {
                return None;
            }
            [dot(v(self.r), delta).div(depth), dot(v(self.u), delta).div(depth)]
        } else {
            let delta = sub(q, v(self.target));
            let d = [self.d.x, self.d.y, self.d.z];
            let [i, j, k] = self.axes;
            [delta[i].mul(I::at(d[k])).sub(delta[k].mul(I::at(d[i]))), delta[j].mul(I::at(d[k])).sub(delta[k].mul(I::at(d[j])))]
        };
        let result = std::array::from_fn(|i| I::at(self.center[i]).add(I::at(self.a[i][0]).mul(p[0])).add(I::at(self.a[i][1]).mul(p[1])));
        result.iter().all(|x| x.finite() && x.abs() < 1e12).then_some(result)
    }
}

struct Source {
    mesh: Arc<Mesh>,
    index: Arc<MeshIndex>,
    own: bool,
}
#[derive(Clone, Copy)]
struct Item {
    bounds: Bounds,
    source: u32,
    triangle: u32,
}
struct Node {
    bounds: Bounds,
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
#[derive(Default)]
struct Tree {
    items: Vec<Item>,
    order: Vec<usize>,
    nodes: Vec<Node>,
}
impl Tree {
    fn build(items: Vec<Item>, budget: &mut usize) -> Result<Self, String> {
        let mut order = Vec::new();
        reserve(&mut order, items.len())?;
        order.extend(0..items.len());
        charge(budget, items.len().saturating_mul(32))?;
        let mut scene = Bounds::empty();
        for item in &items {
            scene.union(item.bounds);
        }
        let mut codes = Vec::new();
        reserve(&mut codes, items.len())?;
        for item in &items {
            let quantize = |axis: usize| {
                let width = scene.hi[axis] - scene.lo[axis];
                let center = item.bounds.lo[axis] * 0.5 + item.bounds.hi[axis] * 0.5;
                if width > 0.0 { ((center - scene.lo[axis]) / width * 65535.0).clamp(0.0, 65535.0) as u32 } else { 0 }
            };
            codes.push(morton(quantize(0), quantize(1)));
        }
        // Quantization orders BVH leaves only. All pruning/coverage continues
        // to use outward projected bounds, never these Morton cells.
        let levels = usize::BITS - items.len().leading_zeros();
        charge(budget, items.len().saturating_mul(levels as usize).saturating_mul(8))?;
        order.sort_unstable_by(|&a, &b| codes.get(a).copied().unwrap_or(0).cmp(&codes.get(b).copied().unwrap_or(0)).then(a.cmp(&b)));
        let mut out = Self { items, order, nodes: Vec::new() };
        if !out.items.is_empty() {
            Self::node(&out.items, &mut out.order, 0, &mut out.nodes, budget)?;
        }
        Ok(out)
    }
    fn node(items: &[Item], order: &mut [usize], offset: usize, nodes: &mut Vec<Node>, budget: &mut usize) -> Result<usize, String> {
        charge(budget, order.len())?;
        let mut bounds = Bounds::empty();
        for &i in order.iter() {
            bounds.union(items.get(i).ok_or("missing projected occlusion triangle")?.bounds);
        }
        reserve(nodes, 1)?;
        let id = nodes.len();
        let end = offset.checked_add(order.len()).ok_or("Form occlusion index overflow")?;
        nodes.push(Node { bounds, range: offset..end, children: None });
        if order.len() > 8 {
            let mid = order.len() / 2;
            let (left, right) = order.split_at_mut(mid);
            let a = Self::node(items, left, offset, nodes, budget)?;
            let b = Self::node(items, right, offset + mid, nodes, budget)?;
            nodes.get_mut(id).ok_or("missing Form occlusion node")?.children = Some([a, b]);
        }
        Ok(id)
    }
    fn candidates(&self, bounds: Bounds, budget: &mut usize) -> Result<Option<Vec<usize>>, String> {
        if self.nodes.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let mut stack = vec![0usize];
        let mut found = Vec::new();
        while let Some(id) = stack.pop() {
            charge(budget, 1)?;
            let node = self.nodes.get(id).ok_or("missing Form occlusion node")?;
            if !node.bounds.overlaps(bounds) {
                continue;
            }
            if let Some(children) = node.children {
                stack.extend(children);
            } else {
                for position in node.range.clone() {
                    charge(budget, 1)?;
                    let id = *self.order.get(position).ok_or("missing Form occlusion item")?;
                    if self.items.get(id).is_some_and(|item| item.bounds.overlaps(bounds)) {
                        if found.len() == MAX_CANDIDATES {
                            return Ok(None);
                        }
                        found.push(id);
                    }
                }
            }
        }
        Ok(Some(found))
    }
}
fn morton(x: u32, y: u32) -> u32 {
    (0..16).fold(0u32, |code, bit| code | ((x >> bit) & 1) << (2 * bit) | ((y >> bit) & 1) << (2 * bit + 1))
}

/// A closest hit or a successful miss depends on this immutable own mesh and
/// projection, not on the target point's depth. Full screen bits are compared;
/// eviction changes work only and errors never enter the memo.
#[derive(Default)]
struct OwnRayMemo {
    values: BTreeMap<[u32; 2], Option<f64>>,
    order: VecDeque<[u32; 2]>,
    #[cfg(test)]
    hit_reuses: usize,
    #[cfg(test)]
    miss_reuses: usize,
    #[cfg(test)]
    raycasts: usize,
    #[cfg(test)]
    errors: usize,
    #[cfg(test)]
    evictions: usize,
}
#[cfg(test)]
struct MemoBypass<'a> {
    flag: &'a std::sync::atomic::AtomicBool,
    previous: bool,
}
#[cfg(test)]
impl Drop for MemoBypass<'_> {
    fn drop(&mut self) {
        self.flag.store(self.previous, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) struct Snapshot {
    proj: Proj,
    chart: Chart,
    sources: Vec<Source>,
    fit: f64,
    tree: Option<Tree>,
    own_rays: Mutex<OwnRayMemo>,
    #[cfg(test)]
    memo_bypass: std::sync::atomic::AtomicBool,
}
impl Snapshot {
    pub(super) fn capture(app: &SolveApp, proj: &Proj) -> Result<Self, String> {
        let chart = Chart::new(proj)?;
        let (mesh, index) = live_cad::occlusion_snapshot(app).ok_or("wait for the current CAD occlusion preview")?;
        if mesh.triangles.len() > MAX_TRIANGLES || app.forms_prototype.context.entries.len() > MAX_SOURCES {
            return Err("Form occlusion snapshot exceeds its geometry budget".into());
        }
        let mut sources = vec![Source { mesh, index, own: true }];
        let mut triangles = sources[0].mesh.triangles.len();
        for entry in context::retained_entries(app) {
            let Some(index) = &entry.index else { continue };
            triangles = triangles.saturating_add(entry.mesh.triangles.len());
            if triangles > MAX_TRIANGLES || sources.len() >= MAX_SOURCES {
                return Err("Form occlusion snapshot exceeds its geometry budget".into());
            }
            sources.push(Source { mesh: entry.mesh.clone(), index: index.clone(), own: false });
        }
        Ok(Self {
            proj: *proj,
            chart,
            sources,
            fit: controls::tolerance(app).unwrap_or(0.01),
            tree: None,
            own_rays: Default::default(),
            #[cfg(test)]
            memo_bypass: Default::default(),
        })
    }
    pub(super) fn prepare(&mut self, budget: &mut usize) -> Result<(), String> {
        if self.tree.is_some() {
            return Ok(());
        }
        let count =
            self.sources.iter().try_fold(0usize, |n, s| n.checked_add(s.mesh.triangles.len())).ok_or("Form occlusion triangle count overflow")?;
        if count > MAX_TRIANGLES {
            return Err("Form occlusion exceeds its triangle budget".into());
        }
        charge(budget, count.saturating_mul(64))?;
        let mut items = Vec::new();
        reserve(&mut items, count)?;
        for (source, s) in self.sources.iter().enumerate() {
            for triangle in 0..s.mesh.triangles.len() {
                let Some((_, world)) = world_triangle(&s.mesh, triangle) else { continue };
                let mut bounds = Bounds::empty();
                let mut usable = true;
                for p in world {
                    if let Some(p) = self.chart.project(v(p)) {
                        bounds.add(p);
                    } else {
                        usable = false;
                        break;
                    }
                }
                if usable && bounds.valid() {
                    items.push(Item {
                        bounds,
                        source: u32::try_from(source).map_err(|_| "Form occlusion source overflow")?,
                        triangle: u32::try_from(triangle).map_err(|_| "Form occlusion triangle overflow")?,
                    });
                }
            }
        }
        self.tree = Some(Tree::build(items, budget)?);
        Ok(())
    }
    fn own_hit(&self, screen: egui::Pos2, budget: &mut usize) -> Result<Option<f64>, String> {
        charge(budget, 1)?;
        if !screen.is_finite() {
            return Err("invalid own Form occlusion ray".into());
        }
        let (origin, direction) = self.proj.ray(screen);
        if !origin.is_finite() || !direction.is_finite() {
            return Err("invalid own Form occlusion ray".into());
        }
        let source = self.sources.first().ok_or("missing own Form occlusion source")?;
        #[cfg(test)]
        if self.memo_bypass.load(std::sync::atomic::Ordering::Relaxed) {
            return source
                .index
                .raycast_bounded(&source.mesh, origin, direction, budget)
                .map(|hit| hit.map(|(distance, _)| distance))
                .map_err(str::to_owned);
        }
        let key = [screen.x.to_bits(), screen.y.to_bits()];
        {
            let memo = self.own_rays.lock().map_err(|_| "own Form ray memo is unavailable")?;
            #[cfg(test)]
            let mut memo = memo;
            if let Some(hit) = memo.values.get(&key).copied() {
                #[cfg(test)]
                if hit.is_some() {
                    memo.hit_reuses += 1;
                } else {
                    memo.miss_reuses += 1;
                }
                return Ok(hit);
            }
            #[cfg(test)]
            {
                memo.raycasts += 1;
            }
        }
        let hit = match source.index.raycast_bounded(&source.mesh, origin, direction, budget) {
            Ok(hit) => hit.map(|(distance, _)| distance),
            Err(error) => {
                #[cfg(test)]
                if let Ok(mut memo) = self.own_rays.lock() {
                    memo.errors += 1;
                }
                return Err(error.to_owned());
            }
        };
        let mut memo = self.own_rays.lock().map_err(|_| "own Form ray memo is unavailable")?;
        if memo.values.len() >= MAX_OWN_RAY_MEMO {
            let oldest = memo.order.pop_front().ok_or("own Form ray memo lost its eviction order")?;
            memo.values.remove(&oldest);
            #[cfg(test)]
            {
                memo.evictions += 1;
            }
        }
        // A concurrent lookup may have inserted this same successful answer
        // while the ray ran. Replace it without duplicating its FIFO record.
        if memo.values.insert(key, hit).is_none() {
            memo.order.push_back(key);
        }
        Ok(hit)
    }
    /// Test accounting replays start from an empty cache. This changes cost,
    /// never geometry, and is absent from application builds.
    #[cfg(test)]
    pub(super) fn clear_own_ray_cache_for_test(&self) -> Result<(), String> {
        *self.own_rays.lock().map_err(|_| "own Form ray memo is unavailable")? = OwnRayMemo::default();
        Ok(())
    }
    /// Diagnostic witnesses must not warm or disturb the actual query's memo.
    #[cfg(test)]
    pub(super) fn without_own_ray_cache_for_test<T>(&self, run: impl FnOnce() -> T) -> T {
        let previous = self.memo_bypass.swap(true, std::sync::atomic::Ordering::Relaxed);
        let _reset = MemoBypass { flag: &self.memo_bypass, previous };
        run()
    }
    #[cfg(test)]
    pub(super) fn own_ray_cache_diagnostic_for_test(&self) -> String {
        match self.own_rays.lock() {
            Ok(memo) => format!(
                "entries={}, hit_reuses={}, miss_reuses={}, raycasts={}, errors={}, evictions={}",
                memo.values.len(),
                memo.hit_reuses,
                memo.miss_reuses,
                memo.raycasts,
                memo.errors,
                memo.evictions,
            ),
            Err(_) => "own ray memo unavailable".to_owned(),
        }
    }
    /// Same rays, biases and retained-context filter as surface::visible.
    pub(super) fn visible(&self, point: Vec3, budget: &mut usize) -> Result<bool, String> {
        if !point.is_finite() {
            return Ok(false);
        }
        let Some(p) = self.proj.to_screen(point).filter(|p| p.is_finite()) else { return Ok(false) };
        charge(budget, 1)?;
        let (o, d) = self.proj.ray(p);
        let depth = (point - o).dot(d);
        if !o.is_finite() || !d.is_finite() || !depth.is_finite() {
            return Ok(false);
        }
        let (near, _) = self.proj.ray(p + egui::vec2(2.0, 0.0));
        let tolerance = (near - o).len().max(3.0 * self.fit.max(0.05));
        if self.own_hit(p, budget)?.is_some_and(|t| t < depth - tolerance) {
            return Ok(false);
        }
        let (origin, direction, depth) = if self.proj.cam.fov > 0.0 {
            let delta = point - o;
            let Some(direction) = delta.normalized() else { return Ok(false) };
            (o, direction, delta.len())
        } else {
            (point - d * depth, d, depth)
        };
        for source in self.sources.iter().skip(1) {
            charge(budget, 1)?;
            if let Some((hit, _)) = source.index.raycast_bounded(&source.mesh, origin, direction, budget).map_err(str::to_owned)? {
                let scale = origin.x.abs().max(origin.y.abs()).max(origin.z.abs()).max(depth.abs()).max(hit.abs()).max(1.0);
                if hit < depth - 64.0 * f64::EPSILON * scale {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn world_triangle(mesh: &Mesh, triangle: usize) -> Option<([u32; 3], [Vec3; 3])> {
    let ids = *mesh.triangles.get(triangle)?;
    let points = [*mesh.positions.get(ids[0] as usize)?, *mesh.positions.get(ids[1] as usize)?, *mesh.positions.get(ids[2] as usize)?];
    points.iter().all(|p| p.is_finite()).then_some((ids, points))
}

struct Rays {
    origin: V,
    end: V,
    direction: V,
    depth: I,
    bounds: Bounds,
    bias: f64,
}

/// For each enclosed ray, depth is affine on the positive-weight control
/// hull. Its extrema therefore lie within the per-control depth intervals.
fn tight_ray_depth(points: &[V], world: V, rays: &Rays) -> I {
    let span = I::at(magnitude(world)).add(I::at(magnitude(rays.origin)));
    let direction = I::at(magnitude(rays.direction));
    let product = span.mul(direction);
    if !product.finite() || product.hi > f64::MAX / 64.0 {
        return I::whole();
    }
    // Allow for the three rounded coordinate subtractions, three products,
    // and two sums in visible(). With each exact product bounded by `product`,
    // 64 epsilons cover their accumulated error and all intermediate sums.
    // The absolute term also covers underflow, including its amplification
    // by a direction component after subtraction.
    let pad = I::at(64.0 * f64::EPSILON).mul(product).add(I::at(64.0 * f64::MIN_POSITIVE).mul(direction)).hi;
    let mut depth = I { lo: f64::INFINITY, hi: f64::NEG_INFINITY };
    for &point in points {
        let value = dot(sub(point, rays.origin), rays.direction);
        if !value.finite() {
            return I::whole();
        }
        depth.lo = depth.lo.min(value.lo);
        depth.hi = depth.hi.max(value.hi);
    }
    depth.grow(pad)
}
struct Hull {
    points: Vec<V>,
    world: V,
    bounds: Bounds,
    screen: Bounds,
    context_origin: V,
    context_depth: I,
    own: Option<Rays>,
    scale: f64,
}
impl Snapshot {
    fn hull(&self, controls: &[HPoint], budget: &mut usize) -> Result<Option<Hull>, String> {
        if controls.is_empty() || controls.len() > MAX_CONTROLS || controls.iter().any(|p| !p.is_finite() || p.w < f64::MIN_POSITIVE) {
            return Err("invalid positive-weight Form occlusion hull".into());
        }
        // Point-weight quotients need one division each, and screen bounds
        // use three clip rows. Debit the remaining interval hull work.
        charge(budget, controls.len().saturating_mul(112))?;
        let mut points = Vec::new();
        reserve(&mut points, controls.len())?;
        let mut world = [I { lo: f64::INFINITY, hi: f64::NEG_INFINITY }; 3];
        let mut weights = [f64::INFINITY, 0.0_f64];
        let mut world_scale = magnitude(v(self.chart.eye)).max(magnitude(v(self.chart.target))).max(self.proj.cam.distance);
        for p in controls {
            weights[0] = weights[0].min(p.w);
            weights[1] = weights[1].max(p.w);
            let q = [p.x, p.y, p.z].map(|coordinate| {
                let quotient = coordinate / p.w;
                I { lo: quotient.next_down(), hi: quotient.next_up() }
            });
            if q.iter().any(|q| !q.finite()) {
                return Err("Form occlusion hull overflowed".into());
            }
            world_scale = world_scale.max(magnitude(q));
            points.push(q);
        }
        // Ill-conditioned/subnormal restrictions remain undecided, rather than
        // relying on a relative rounding bound outside its supported range.
        if weights[1] / weights[0] > 1e12 || !world_scale.is_finite() || world_scale > 1e100 {
            return Ok(None);
        }
        let roundoff = 4096.0 * f64::EPSILON * world_scale.max(1.0);
        let mut bounds = Bounds::empty();
        let mut screen = Bounds::empty();
        for q in &mut points {
            for (i, x) in q.iter_mut().enumerate() {
                *x = x.grow(roundoff);
                let slot = world.get_mut(i).ok_or("missing hull coordinate")?;
                slot.lo = slot.lo.min(x.lo);
                slot.hi = slot.hi.max(x.hi);
            }
            let Some(p) = self.chart.project(*q) else { return Ok(None) };
            bounds.add(p);
            let Some(p) = self.root_project(*q) else { return Ok(None) };
            screen.add(p);
        }
        if !bounds.valid() || !screen.valid() {
            return Ok(None);
        }
        let chart_pad = 1e-7 + 8192.0 * f64::EPSILON * bounds.lo.iter().chain(bounds.hi.iter()).fold(1.0_f64, |n, x| n.max(x.abs()));
        bounds = bounds.grow(chart_pad);
        let screen_scale = screen.lo.iter().chain(screen.hi.iter()).fold(1.0_f64, |n, x| n.max(x.abs()))
            + f64::from(self.proj.rect.left().abs() + self.proj.rect.top().abs() + self.proj.rect.width() + self.proj.rect.height());
        let rounding = 8.0 * f64::from(f32::EPSILON) * screen_scale.max(1.0);
        // The pad encloses both f32 screen rounding in to_screen and the
        // f32 viewport-origin subtraction in Proj::ray.
        let ray_screen = screen.grow(rounding);
        let own = self.rays(world, ray_screen, rounding).and_then(|mut rays| {
            rays.bounds = if !self.chart.perspective
                && let Some(bounds) = self.orthographic_ray_bounds(ray_screen)
            {
                bounds.grow(chart_pad)
            } else {
                let q = if self.chart.perspective { add(v(self.chart.eye), rays.direction) } else { rays.origin };
                let mut bounds = Bounds::empty();
                bounds.add(self.chart.project(q)?);
                bounds.grow(chart_pad)
            };
            Some(rays)
        });
        let (context_origin, context_depth) = if self.chart.perspective {
            (v(self.chart.eye), norm(sub(world, v(self.chart.eye))))
        } else if let Some(rays) = &own {
            (sub(world, scale(v(self.chart.d), rays.depth)), rays.depth)
        } else {
            return Ok(None);
        };
        Ok(Some(Hull { points, world, bounds, screen, context_origin, context_depth, own, scale: world_scale }))
    }
    fn root_project(&self, q: V) -> Option<P> {
        let w = self.clip_coordinate(q, 3);
        if !w.finite() || w.lo <= 1e-9 {
            return None;
        }
        let x = self.clip_coordinate(q, 0).div(w);
        let y = self.clip_coordinate(q, 1).div(w);
        let p = [
            I::at(f64::from(self.proj.rect.left())).add(x.add(I::at(1.0)).mul(I::at(f64::from(self.proj.rect.width()) * 0.5))),
            I::at(f64::from(self.proj.rect.top())).add(I::at(1.0).sub(y).mul(I::at(f64::from(self.proj.rect.height()) * 0.5))),
        ];
        p.iter().all(|x| x.finite() && x.abs() < 1e12).then_some(p)
    }
    /// Intervals for Camera::ray's two screen-plane scalars. Multiply in the
    /// camera's order before combining with the basis vectors.
    fn ray_plane_coordinates(&self, screen: Bounds) -> [I; 2] {
        let width = f64::from(self.proj.rect.width());
        let height = f64::from(self.proj.rect.height());
        let hh = I::at(self.proj.cam.half_height());
        let x =
            I { lo: screen.lo[0], hi: screen.hi[0] }.sub(I::at(f64::from(self.proj.rect.left()))).div(I::at(width)).mul(I::at(2.0)).sub(I::at(1.0));
        let y =
            I::at(1.0).sub(I { lo: screen.lo[1], hi: screen.hi[1] }.sub(I::at(f64::from(self.proj.rect.top()))).div(I::at(height)).mul(I::at(2.0)));
        [x.mul(hh).mul(I::at(width / height)), y.mul(hh)]
    }

    /// In an orthographic camera the ray chart is affine. Compose it with
    /// the right, up and back axes before varying the two screen scalars.
    /// Projecting a world AABB first loses the cancellation between correlated
    /// origin coordinates, especially at an angled view.
    fn orthographic_ray_bounds(&self, screen: Bounds) -> Option<Bounds> {
        let [x, y] = self.ray_plane_coordinates(screen);
        let distance = I::at(self.proj.cam.distance * 4.0);
        let axes = [v(self.chart.r), v(self.chart.u), v(self.chart.back)];
        let scales = [x, y, distance];
        let terms = std::array::from_fn::<_, 3, _>(|i| scale(axes[i], scales[i]));
        let largest = terms.iter().fold(magnitude(v(self.chart.target)), |m, &term| m.max(magnitude(term)));
        if !largest.is_finite() || largest > 1e100 {
            return None;
        }
        // Each component of the actual origin has three rounded products
        // and three rounded additions. Bound their combined absolute error
        // using the largest of the four terms, including the target. The
        // scalar conversions above already enclose their own rounding.
        let rounding = I::at(128.0 * f64::EPSILON).mul(I::at(largest)).add(I::at(128.0 * f64::MIN_POSITIVE)).hi;
        let uncertainty = self.chart.orthographic_offset([I::at(0.0).grow(rounding); 3]);
        let projected = axes.map(|axis| self.chart.orthographic_offset(axis));
        let point = std::array::from_fn(|axis| {
            scales
                .into_iter()
                .enumerate()
                .fold(I::at(self.chart.center[axis]).add(uncertainty[axis]), |sum, (i, factor)| sum.add(projected[i][axis].mul(factor)))
        });
        let mut bounds = Bounds::empty();
        bounds.add(point);
        (bounds.valid() && point.iter().all(|coordinate| coordinate.abs() < 1e12)).then_some(bounds)
    }

    fn rays(&self, world: V, screen: Bounds, rounding: f64) -> Option<Rays> {
        let height = f64::from(self.proj.rect.height());
        let hh = self.proj.cam.half_height();
        let [x, y] = self.ray_plane_coordinates(screen);
        let lateral = add(scale(v(self.chart.r), x), scale(v(self.chart.u), y));
        let (origin, direction) = if self.chart.perspective {
            let raw = sub(lateral, scale(v(self.chart.back), I::at(self.proj.cam.distance)));
            let length = norm(raw);
            if !length.finite() || length.lo <= 1e-12 {
                return None;
            }
            (v(self.chart.eye), raw.map(|x| x.div(length)))
        } else {
            (add(add(v(self.chart.target), lateral), scale(v(self.chart.back), I::at(self.proj.cam.distance * 4.0))), v(self.chart.d))
        };
        let depth = dot(sub(world, origin), direction);
        let end = add(origin, scale(direction, depth));
        if !depth.finite() || depth.lo <= 0.0 || end.iter().any(|x| !x.finite()) {
            return None;
        }
        let pixel = if self.chart.perspective { 0.0 } else { (2.0 + rounding) * 2.0 * hh / height * norm(v(self.chart.r)).hi };
        Some(Rays { origin, end, direction, depth, bounds: Bounds::empty(), bias: pixel.max(3.0 * self.fit.max(0.05)) })
    }
    pub(super) fn occludes_hull(&self, controls: &[HPoint], budget: &mut usize) -> Result<bool, String> {
        self.occludes_with_depth(controls, true, budget)
    }
    pub(super) fn occludes_curve_hull(&self, controls: &[HPoint], budget: &mut usize) -> Result<bool, String> {
        // Curve restrictions generally subdivide more cheaply than paying
        // for additional endpoint bounds and larger triangle unions.
        self.occludes_with_depth(controls, false, budget)
    }
    fn occludes_with_depth(&self, controls: &[HPoint], tight_endpoint: bool, budget: &mut usize) -> Result<bool, String> {
        if self.tree.is_none() {
            return Err("prepare Form occlusion on the region worker before querying it".into());
        }
        let Some(hull) = self.hull(controls, budget)? else {
            #[cfg(test)]
            certificate_trace(|stats| stats.hull_refused += 1);
            return Ok(false);
        };
        if let Some((xs, ys)) = self.discrete_keys(&hull, budget)?
            && self.discrete_own_keys(&hull, &xs, &ys, budget)?
        {
            return Ok(true);
        }
        self.continuous_occlusion(&hull, tight_endpoint, budget)
    }
    fn continuous_occlusion(&self, hull: &Hull, tight_endpoint: bool, budget: &mut usize) -> Result<bool, String> {
        let tree = self.tree.as_ref().ok_or("prepare Form occlusion on the region worker before querying it")?;
        let mut query = hull.bounds;
        if let Some(own) = &hull.own {
            query.union(own.bounds);
        }
        let Some(candidates) = tree.candidates(query, budget)? else {
            #[cfg(test)]
            certificate_trace(|stats| stats.candidate_refused += 1);
            return Ok(false);
        };
        #[cfg(test)]
        certificate_trace(|stats| stats.candidates += candidates.len());
        let mut safe = Vec::new();
        reserve(&mut safe, candidates.len())?;
        let mut tight_depth = None;
        for candidate in candidates {
            charge(budget, 1)?;
            let item = *tree.items.get(candidate).ok_or("missing occlusion candidate")?;
            if let Some(triangle) = self.safe_triangle(item, hull, tight_endpoint.then_some(&mut tight_depth), budget)? {
                let bounds = if triangle.own { hull.own.as_ref().map(|rays| rays.bounds) } else { Some(hull.bounds) };
                // One depth-safe triangle can cover the whole ray box. The
                // BVH bounds only screen out candidates for the strict
                // interval half-plane proof.
                if let Some(bounds) = bounds.filter(|&bounds| item.bounds.encloses(bounds))
                    && convex_contains(&triangle.vertices, bounds, budget)?
                {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.coverage_proved += 1);
                    return Ok(true);
                }
                safe.push(triangle);
            }
        }
        if safe.is_empty() {
            return Ok(false);
        }
        // Keep the bias classes separate: a context polygon may certify the
        // exact point rays, whereas an own polygon must cover quantized rays.
        for own in [false, true] {
            let selected: Vec<_> = safe.iter().filter(|p| p.own == own).cloned().collect();
            if selected.is_empty() {
                continue;
            }
            let polygons = polygons(&selected, budget)?;
            #[cfg(test)]
            certificate_trace(|stats| {
                stats.polygons += polygons.len();
                stats.union_boundaries += polygons.iter().filter(|polygon| polygon.union_boundary).count();
            });
            let bounds = if own {
                let Some(rays) = &hull.own else { continue };
                rays.bounds
            } else {
                hull.bounds
            };
            let complete = (own && tight_endpoint && self.small_lattice_cover(hull, &polygons, budget)?) || covered(bounds, &polygons, budget)?;
            #[cfg(test)]
            certificate_trace(|stats| {
                if complete {
                    stats.coverage_proved += 1;
                } else {
                    stats.coverage_refused += 1;
                }
            });
            if complete {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A continuous box can straddle an unresolved seam between safe
    /// components even when every rounded own-surface ray is hidden. Split
    /// the finite key rectangle, keeping whole-cell continuous proofs where
    /// available and reusing the small exact ray check for remaining cells.
    fn small_lattice_cover(&self, hull: &Hull, polygons: &[Polygon], budget: &mut usize) -> Result<bool, String> {
        let max_keys = (MAX_DISCRETE_RAYS * MAX_CELLS) as u64;
        if self.chart.perspective || polygons.len() < 2 || projected_key_product(hull.screen) > max_keys as f64 {
            return Ok(false);
        }
        let allotment = (*budget).min(MAX_LATTICE_WORK);
        let mut remaining = allotment;
        let proof = (|| {
            charge(&mut remaining, 64 + hull.points.len().saturating_mul(64))?;
            let Some(ranges) = self.tight_screen_keys(hull) else { return Ok(false) };
            let [x, y] = ranges.map(screen_key_count);
            if x.checked_mul(y).is_none_or(|count| count > max_keys) {
                return Ok(false);
            }
            self.lattice_cover(hull, polygons, ranges, &mut remaining)
        })();
        charge(budget, allotment - remaining)?;
        // An exhausted optional proof supplies no coverage. Its actual work
        // is still debited, and the ordinary continuous path remains valid.
        Ok(matches!(proof, Ok(true)))
    }

    fn lattice_cover(&self, hull: &Hull, polygons: &[Polygon], ranges: [[f32; 2]; 2], budget: &mut usize) -> Result<bool, String> {
        let chart_pad = 1e-7 + 8192.0 * f64::EPSILON * hull.bounds.lo.iter().chain(hull.bounds.hi.iter()).fold(1.0_f64, |n, x| n.max(x.abs()));
        let mut cover = LatticeCover { snapshot: self, hull, polygons, chart_pad, cells: 0 };
        cover.cell(ranges, 0, None, budget)
    }

    /// Preserve the real f32 viewport-origin subtraction in Proj::ray.
    fn rounded_ray_screen_bounds(&self, keys: [[f32; 2]; 2]) -> Option<Bounds> {
        let origins = [self.proj.rect.left(), self.proj.rect.top()];
        let coords = std::array::from_fn(|axis| {
            let origin = origins[axis];
            let effective = keys[axis].map(|screen| f64::from(screen - origin));
            I::at(f64::from(origin)).add(I { lo: effective[0], hi: effective[1] })
        });
        let mut bounds = Bounds::empty();
        bounds.add(coords);
        bounds.valid().then_some(bounds)
    }
    /// The own-surface picker projects through f32 screen coordinates before
    /// constructing its ray. A sufficiently small complete world hull therefore
    /// has a finite, enumerable set of possible rays. Exhaustively proving every
    /// key hidden avoids filling microscopic display seams or guessing from
    /// sampled points. Context uses continuous point rays and is not used here.
    #[cfg(test)]
    fn discrete_own_occlusion(&self, hull: &Hull, budget: &mut usize) -> Result<bool, String> {
        let Some((xs, ys)) = self.discrete_keys(hull, budget)? else { return Ok(false) };
        self.discrete_own_keys(hull, &xs, &ys, budget)
    }
    fn discrete_keys(&self, hull: &Hull, budget: &mut usize) -> Result<Option<ScreenKeys>, String> {
        charge(budget, 64)?;
        if let Some(keys) = self.screen_keys(hull.world).and_then(screen_grid) {
            return Ok(Some(keys));
        }
        if small_projected_extent(hull.screen) {
            charge(budget, 64 + hull.points.len().saturating_mul(64))?;
            if let Some(keys) = self.tight_screen_keys(hull).and_then(screen_grid) {
                return Ok(Some(keys));
            }
        }
        #[cfg(test)]
        certificate_trace(|stats| stats.discrete_refused += 1);
        Ok(None)
    }
    fn discrete_own_keys(&self, hull: &Hull, xs: &[f32], ys: &[f32], budget: &mut usize) -> Result<bool, String> {
        for &x in xs {
            for &y in ys {
                charge(budget, 32)?;
                #[cfg(test)]
                certificate_trace(|stats| stats.discrete_rays += 1);
                let screen = egui::pos2(x, y);
                let (origin, direction) = self.proj.ray(screen);
                let (near, _) = self.proj.ray(screen + egui::vec2(2.0, 0.0));
                if !origin.is_finite() || !direction.is_finite() || !near.is_finite() {
                    return Ok(false);
                }
                // Reproduce visible()'s bias at this exact screen key. Only the
                // complete world's minimum ray depth is interval-bounded.
                let bias = (near - origin).len().max(3.0 * self.fit.max(0.05));
                let depth = dot(sub(hull.world, v(origin)), v(direction));
                let limit = depth.sub(I::at(bias));
                if !bias.is_finite() || !depth.finite() || !limit.finite() {
                    return Ok(false);
                }
                let Some(hit) = self.own_hit(screen, budget)? else {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.discrete_missed += 1);
                    return Ok(false);
                };
                if !hit.is_finite() || hit >= limit.lo {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.discrete_depth += 1);
                    return Ok(false);
                }
            }
        }
        #[cfg(test)]
        certificate_trace(|stats| stats.discrete_proved += 1);
        Ok(true)
    }
    /// Interval form of Proj::to_screen's actual operation order: divide in
    /// f64, cast the local pixel to f32, then add the f32 viewport origin. Each
    /// rounded operation is monotone, so endpoint images enclose every key.
    fn screen_keys(&self, world: V) -> Option<[[f32; 2]; 2]> {
        let w = self.clip_coordinate(world, 3);
        if !w.finite() || w.lo <= 1e-9 {
            return None;
        }
        self.ndc_keys([self.clip_coordinate(world, 0).div(w), self.clip_coordinate(world, 1).div(w)])
    }
    fn clip_coordinate(&self, world: V, row: usize) -> I {
        [world[0], world[1], world[2], I::at(1.0)]
            .into_iter()
            .enumerate()
            .fold(I::at(0.0), |sum, (column, component)| sum.add(component.mul(I::at(self.proj.vp.0[column][row]))))
    }
    /// The ratio of two affine clip coordinates with positive denominator
    /// lies in the convex hull of the ratios at the controls. The same bound
    /// holds after padding every control's clip interval by the uniform
    /// error in the actual rounded dot products of `Mat4::apply`.
    fn tight_screen_keys(&self, hull: &Hull) -> Option<[[f32; 2]; 2]> {
        let pad = [0, 1, 3].map(|row| {
            let bounds = [hull.world[0].abs(), hull.world[1].abs(), hull.world[2].abs(), 1.0];
            let scale = bounds
                .into_iter()
                .enumerate()
                .fold(I::at(0.0), |sum, (column, bound)| sum.add(I::at(bound).mul(I::at(self.proj.vp.0[column][row].abs()))));
            if scale.finite() && scale.hi <= f64::MAX / 64.0 {
                I::at(64.0 * f64::EPSILON).mul(scale).add(I::at(64.0 * f64::MIN_POSITIVE)).hi
            } else {
                f64::INFINITY
            }
        });
        let mut ratios = [I { lo: f64::INFINITY, hi: f64::NEG_INFINITY }; 2];
        for &point in &hull.points {
            let w = self.clip_coordinate(point, 3).grow(pad[2]);
            if !w.finite() || w.lo <= 1e-9 {
                return None;
            }
            for (axis, ratio) in ratios.iter_mut().enumerate() {
                let value = self.clip_coordinate(point, axis).grow(pad[axis]).div(w);
                if !value.finite() {
                    return None;
                }
                ratio.lo = ratio.lo.min(value.lo);
                ratio.hi = ratio.hi.max(value.hi);
            }
        }
        self.ndc_keys(ratios)
    }
    fn ndc_keys(&self, ndc: [I; 2]) -> Option<[[f32; 2]; 2]> {
        let local = [
            ndc[0].add(I::at(1.0)).mul(I::at(0.5)).mul(I::at(f64::from(self.proj.rect.width()))),
            I::at(1.0).sub(ndc[1]).mul(I::at(0.5)).mul(I::at(f64::from(self.proj.rect.height()))),
        ];
        if local.iter().any(|range| !range.finite()) {
            return None;
        }
        let origins = [self.proj.rect.left(), self.proj.rect.top()];
        let ranges: [[f32; 2]; 2] = std::array::from_fn(|axis| [origins[axis] + local[axis].lo as f32, origins[axis] + local[axis].hi as f32]);
        ranges.iter().all(|range| range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]).then_some(ranges)
    }
    #[cfg(test)]
    pub(super) fn certificate_diagnostic(&self, controls: &[HPoint]) -> String {
        let previous = CERTIFICATE_TRACE.with(|trace| trace.replace(Some(CertificateStats::default())));
        let mut budget = super::QUERY_WORK;
        let result = self.occludes_curve_hull(controls, &mut budget);
        let stats = CERTIFICATE_TRACE.with(|trace| trace.replace(previous)).unwrap_or_default();
        format!("root certificate={result:?}, used={}, stages={stats:?}", super::QUERY_WORK - budget)
    }
    fn safe_triangle(&self, item: Item, hull: &Hull, tight_depth: Option<&mut Option<I>>, budget: &mut usize) -> Result<Option<Triangle>, String> {
        let source = self.sources.get(item.source as usize).ok_or("missing occlusion source")?;
        let Some((_, points)) = world_triangle(&source.mesh, item.triangle as usize) else {
            #[cfg(test)]
            certificate_trace(|stats| stats.geometry += 1);
            return Ok(None);
        };
        charge(budget, 64)?;
        let a = v(points[0]);
        let mut n = cross(sub(v(points[1]), a), sub(v(points[2]), a));
        let length = norm(n);
        if !length.finite() || length.hi <= 0.0 {
            #[cfg(test)]
            certificate_trace(|stats| stats.normal += 1);
            return Ok(None);
        }
        let (origin, own_rays, bias, direction_scale) = if source.own {
            let Some(rays) = &hull.own else { return Ok(None) };
            (rays.origin, Some(rays), rays.bias, norm(rays.direction).hi)
        } else {
            if !hull.context_depth.finite() || hull.context_depth.lo <= 0.0 {
                return Ok(None);
            }
            let scale = magnitude(hull.context_origin).max(hull.context_depth.abs()).max(hull.scale).max(1.0);
            (hull.context_origin, None, 64.0 * f64::EPSILON * scale, 2.0)
        };
        let facing = dot(n, sub(origin, a));
        let origin_plane = if facing.hi < 0.0 {
            n = n.map(I::neg);
            facing.neg()
        } else if facing.lo > 0.0 {
            facing
        } else {
            #[cfg(test)]
            certificate_trace(|stats| stats.facing += 1);
            return Ok(None);
        };
        // Mesh ray hits at t <= 1e-9 are deliberately ignored by RayFrame.
        let near_margin = (2e-9 * direction_scale + 4096.0 * f64::EPSILON * hull.scale.max(magnitude(a))) * length.hi;
        if origin_plane.lo <= near_margin {
            #[cfg(test)]
            certificate_trace(|stats| stats.near_origin += 1);
            return Ok(None);
        }
        // The existing visibility bias is a ray-parameter gap, not a normal
        // distance. n dot (end-a)=-gap*(-n dot direction); using |n|*|direction|
        // would unnecessarily reject a genuinely hidden grazing restriction.
        // Perspective direction intervals cover every quantized ray in the
        // hull; only orthographic directions are constant. A finite strictly
        // positive denominator proves all rays cross the oriented plane.
        let normal_direction = own_rays.map(|rays| dot(n, rays.direction));
        let plane_speed = if let Some(direction) = normal_direction {
            let denominator = direction.neg();
            if !denominator.finite() || denominator.lo <= 0.0 {
                #[cfg(test)]
                certificate_trace(|stats| stats.direction += 1);
                return Ok(None);
            }
            denominator.hi
        } else {
            I::at(direction_scale).mul(I::at(length.hi)).hi
        };
        let roundoff = 4096.0 * f64::EPSILON * hull.scale.max(magnitude(a));
        let margin = I::at(bias).mul(I::at(plane_speed)).add(I::at(roundoff).mul(I::at(length.hi))).hi;
        if !margin.is_finite() {
            #[cfg(test)]
            certificate_trace(|stats| stats.margin += 1);
            return Ok(None);
        }
        // The interval AABB contains every inflated control point. If the
        // whole box clears the same plane margin, all controls clear it.
        // Test individual controls when the box loses their correlation.
        charge(budget, 8)?;
        let world = dot(n, sub(hull.world, a));
        if world.finite() && world.lo >= -margin {
            #[cfg(test)]
            certificate_trace(|stats| stats.world_depth += 1);
            return Ok(None);
        }
        if !world.finite() || world.hi >= -margin {
            for &point in &hull.points {
                charge(budget, 8)?;
                if dot(n, sub(point, a)).hi >= -margin {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.world_depth += 1);
                    return Ok(None);
                }
            }
        }
        if let Some((rays, direction)) = own_rays.zip(normal_direction) {
            let coarse = dot(n, sub(rays.end, a));
            if !coarse.finite() || coarse.hi >= -margin {
                let Some(tight_depth) = tight_depth else {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.endpoint_depth += 1);
                    return Ok(None);
                };
                charge(budget, 8)?;
                if tight_depth.is_none() {
                    charge(budget, hull.points.len().saturating_mul(8).saturating_add(32))?;
                    *tight_depth = Some(tight_ray_depth(&hull.points, hull.world, rays));
                }
                // Evaluate the endpoint in the normal direction directly.
                // A world-coordinate box would lose the common ray depth
                // by treating its three endpoint components independently.
                let endpoint = origin_plane.add(direction.mul((*tight_depth).unwrap_or_else(I::whole)));
                if !endpoint.finite() || endpoint.hi >= -margin {
                    #[cfg(test)]
                    certificate_trace(|stats| stats.endpoint_depth += 1);
                    return Ok(None);
                }
                #[cfg(test)]
                certificate_trace(|stats| stats.endpoint_tight_proved += 1);
            }
        }
        let uncertainty =
            4096.0 * f64::EPSILON * hull.scale.max(points.iter().map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs())).fold(1.0_f64, f64::max));
        let mut vertices = Vec::with_capacity(3);
        for p in points {
            let Some(screen) = self.chart.project(v(p).map(|x| x.grow(uncertainty))) else {
                #[cfg(test)]
                certificate_trace(|stats| stats.projection += 1);
                return Ok(None);
            };
            vertices.push(Vertex { id: vertex_key(item.source, p), point: screen, world: p });
        }
        let area = orient(
            vertices.first().ok_or("missing occlusion triangle corner")?.point,
            vertices.get(1).ok_or("missing occlusion triangle corner")?.point,
            vertices.get(2).ok_or("missing occlusion triangle corner")?.point,
        );
        if area.hi < 0.0 {
            vertices.swap(1, 2);
        } else if area.lo <= 0.0 {
            #[cfg(test)]
            certificate_trace(|stats| stats.area += 1);
            return Ok(None);
        }
        #[cfg(test)]
        certificate_trace(|stats| stats.safe += 1);
        Ok(Some(Triangle { vertices, own: source.own }))
    }
}

/// Enumerate representable f32 values, never pixel samples or a tolerance grid.
/// A refused range supplies no certificate. Signed zero denotes the same ray.
fn float_keys(range: [f32; 2]) -> Option<Vec<f32>> {
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return None;
    }
    let mut keys = Vec::with_capacity(MAX_DISCRETE_RAYS);
    let mut key = range[0];
    for _ in 0..MAX_DISCRETE_RAYS {
        keys.push(key);
        if key == range[1] {
            return Some(keys);
        }
        let next = key.next_up();
        if !next.is_finite() || next <= key || next > range[1] {
            return None;
        }
        key = next;
    }
    None
}

fn screen_grid(ranges: [[f32; 2]; 2]) -> Option<ScreenKeys> {
    let x = float_keys(ranges[0])?;
    let y = float_keys(ranges[1])?;
    (x.len().checked_mul(y.len())? <= MAX_DISCRETE_RAYS).then_some((x, y))
}

fn screen_key_count([lo, hi]: [f32; 2]) -> u64 {
    let rank = |x: f32| {
        let bits = x.to_bits();
        if bits & 0x8000_0000 == 0 { bits | 0x8000_0000 } else { !bits }
    };
    // The two signed zero encodings denote the same ray. Counting both
    // here only overestimates the size used for admission and splitting.
    u64::from(rank(hi).saturating_sub(rank(lo))) + 1
}

/// Admission heuristic only. A small projected control hull can be a much
/// narrower raster range than its enclosing world-coordinate box suggests.
fn small_projected_extent(screen: Bounds) -> bool {
    projected_key_product(screen) <= MAX_DISCRETE_RAYS as f64
}
fn projected_key_product(screen: Bounds) -> f64 {
    let steps = [0, 1].map(|axis| {
        let at = screen.lo[axis].abs().max(screen.hi[axis].abs()) as f32;
        let ulp = f64::from(at.next_up() - at);
        ((screen.hi[axis] - screen.lo[axis]) / ulp).max(1.0)
    });
    steps[0] * steps[1]
}

// CAD tessellation may repeat the same boundary vertex in adjacent native
// faces. Bit-identical points represent the same private display coverage node;
// no geometric-nearness welding or model identity is inferred from the mesh.
type VertexId = (u32, [u64; 3]);
fn vertex_key(source: u32, point: Vec3) -> VertexId {
    (source, [point.x, point.y, point.z].map(|value| if value == 0.0 { 0 } else { value.to_bits() }))
}
#[derive(Clone)]
struct Vertex {
    id: VertexId,
    point: P,
    world: Vec3,
}
#[derive(Clone)]
struct Triangle {
    vertices: Vec<Vertex>,
    own: bool,
}
struct Polygon {
    vertices: Vec<Vertex>,
    boundary_edges: Vec<[P; 2]>,
    bounds: Bounds,
    /// Only a verified complete boundary of positively oriented safe triangles.
    union_boundary: bool,
}
impl Polygon {
    fn boundary(&self) -> impl Iterator<Item = [P; 2]> + Clone {
        self.boundary_edges.iter().copied().chain(edges(&self.vertices).map(|(a, b)| [a.point, b.point]))
    }

    fn boundary_edge(&self, index: usize) -> Result<[P; 2], String> {
        if !self.boundary_edges.is_empty() {
            return self.boundary_edges.get(index).copied().ok_or_else(|| "missing winding edge".into());
        }
        let a = self.vertices.get(index).ok_or("missing winding edge")?;
        let b = self.vertices.get(index + 1).or_else(|| self.vertices.first()).ok_or("missing winding edge")?;
        Ok([a.point, b.point])
    }
}
fn edges(vertices: &[Vertex]) -> impl Iterator<Item = (&Vertex, &Vertex)> + Clone {
    vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len())
}
fn edge_key(a: VertexId, b: VertexId) -> (VertexId, VertexId) {
    if a < b { (a, b) } else { (b, a) }
}
fn axis_line(a: Vec3, b: Vec3, c: Vec3) -> bool {
    (a.x == b.x && b.x == c.x && a.y == b.y && b.y == c.y)
        || (a.x == b.x && b.x == c.x && a.z == b.z && b.z == c.z)
        || (a.y == b.y && b.y == c.y && a.z == b.z && b.z == c.z)
}
fn polygon(vertices: Vec<Vertex>) -> Option<Polygon> {
    if vertices.len() < 3 || vertices.len() > MAX_POLYGON {
        return None;
    }
    for i in 0..vertices.len() {
        let a = vertices.get(i)?;
        let b = vertices.get((i + 1) % vertices.len())?;
        let c = vertices.get((i + 2) % vertices.len())?;
        let turn = orient(a.point, b.point, c.point);
        let between = b.world.x >= a.world.x.min(c.world.x)
            && b.world.x <= a.world.x.max(c.world.x)
            && b.world.y >= a.world.y.min(c.world.y)
            && b.world.y <= a.world.y.max(c.world.y)
            && b.world.z >= a.world.z.min(c.world.z)
            && b.world.z <= a.world.z.max(c.world.z);
        if turn.lo <= 0.0 && !(axis_line(a.world, b.world, c.world) && between) {
            return None;
        }
    }
    // Local turns alone also accept a winding star. Every other vertex must
    // lie in each supporting half-plane before a boundary is called convex.
    for (a, b) in edges(&vertices) {
        for c in &vertices {
            if c.id == a.id || c.id == b.id {
                continue;
            }
            if orient(a.point, b.point, c.point).lo <= 0.0 && !axis_line(a.world, b.world, c.world) {
                return None;
            }
        }
    }
    let mut bounds = Bounds::empty();
    for vertex in &vertices {
        bounds.add(vertex.point);
    }
    bounds.valid().then_some(Polygon { vertices, boundary_edges: Vec::new(), bounds, union_boundary: false })
}
fn polygons(triangles: &[Triangle], budget: &mut usize) -> Result<Vec<Polygon>, String> {
    let mut uses = BTreeMap::<_, Vec<(usize, bool)>>::new();
    for (i, triangle) in triangles.iter().enumerate() {
        for (a, b) in edges(&triangle.vertices) {
            charge(budget, 1)?;
            uses.entry(edge_key(a.id, b.id)).or_default().push((i, a.id < b.id));
        }
    }
    // Projection can fold two depth-safe facets onto the same side of an
    // exact common edge. Their directed contributions add in that case.
    // Only opposite occurrences connect the components and cancel.
    let mut paired = BTreeMap::new();
    for (edge, uses) in uses {
        charge(budget, 1)?;
        if let [(a, dir), (b, rev)] = uses.as_slice()
            && dir != rev
        {
            paired.insert(edge, [*a, *b]);
        }
    }
    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    for seed in 0..triangles.len() {
        if !visited.insert(seed) {
            continue;
        }
        let mut component = vec![seed];
        let mut stack = vec![seed];
        while let Some(i) = stack.pop() {
            let triangle = triangles.get(i).ok_or("missing safe occlusion triangle")?;
            for (a, b) in edges(&triangle.vertices) {
                charge(budget, 1)?;
                if let Some(adjacent) = paired.get(&edge_key(a.id, b.id)) {
                    for &next in adjacent {
                        if visited.insert(next) {
                            component.push(next);
                            stack.push(next);
                        }
                    }
                }
            }
        }
        let mut boundary = BTreeMap::new();
        let mut valid = true;
        for &i in &component {
            let triangle = triangles.get(i).ok_or("missing safe occlusion triangle")?;
            for (a, b) in edges(&triangle.vertices) {
                charge(budget, 1)?;
                if !paired.contains_key(&edge_key(a.id, b.id)) && boundary.insert(a.id, (a.clone(), b.id)).is_some() {
                    valid = false;
                }
            }
        }
        let mut chain = Vec::new();
        if valid {
            if let Some(&start) = boundary.keys().next() {
                let mut current = start;
                for _ in 0..boundary.len() {
                    charge(budget, 1)?;
                    let Some((vertex, next)) = boundary.get(&current) else {
                        valid = false;
                        break;
                    };
                    chain.push(vertex.clone());
                    current = *next;
                    if current == start {
                        break;
                    }
                }
                valid &= current == start && chain.len() == boundary.len();
            } else {
                valid = false;
            }
        }
        if valid && (3..=MAX_POLYGON).contains(&chain.len()) {
            // Shared interior edges cancel only by the exact source/position
            // identity above, and all contributors have positive projected
            // orientation. Their complete boundary has winding equal to the
            // sum of triangle coverage indicators, including concave regions.
            charge(budget, chain.len())?;
            let mut bounds = Bounds::empty();
            for vertex in &chain {
                bounds.add(vertex.point);
            }
            if bounds.valid() {
                result.push(Polygon { vertices: chain, boundary_edges: Vec::new(), bounds, union_boundary: true });
                continue;
            }
        }
        // Several rings, or several branches at a vertex, still bound the
        // sum of the positively oriented triangle indicators. Retain every
        // uncancelled directed occurrence, including the reverse winding
        // around holes. No synthetic closing edge or convex hull is needed.
        if let Some(capacity) = component.len().checked_mul(3).filter(|&n| n <= MAX_POLYGON) {
            let mut boundary_edges = Vec::new();
            reserve(&mut boundary_edges, capacity)?;
            let mut bounds = Bounds::empty();
            for &i in &component {
                let triangle = triangles.get(i).ok_or("missing safe occlusion triangle")?;
                for (a, b) in edges(&triangle.vertices) {
                    charge(budget, 1)?;
                    if !paired.contains_key(&edge_key(a.id, b.id)) {
                        bounds.add(a.point);
                        bounds.add(b.point);
                        boundary_edges.push([a.point, b.point]);
                    }
                }
            }
            if boundary_edges.len() >= 3 && bounds.valid() {
                result.push(Polygon { vertices: Vec::new(), boundary_edges, bounds, union_boundary: true });
                continue;
            }
        }
        // Retain individual depth-safe facets if a bounded union is unavailable.
        for i in component {
            charge(budget, 144)?;
            if let Some(single) = triangles.get(i).and_then(|triangle| polygon(triangle.vertices.clone())) {
                result.push(single);
            }
        }
    }
    Ok(result)
}
fn contains(polygon: &Polygon, bounds: Bounds, budget: &mut usize) -> Result<bool, String> {
    if !polygon.bounds.overlaps(bounds) {
        return Ok(false);
    }
    if polygon.union_boundary {
        return union_contains(polygon, bounds, budget);
    }
    convex_contains(&polygon.vertices, bounds, budget)
}
fn convex_contains(vertices: &[Vertex], bounds: Bounds, budget: &mut usize) -> Result<bool, String> {
    charge(budget, vertices.len().saturating_mul(16))?;
    Ok(edges(vertices).all(|(a, b)| orient(a.point, b.point, bounds.point()).lo > 0.0))
}
/// Prove whole-box membership from the verified signed union boundary. The
/// box is connected, so strictly excluding every uncertain boundary segment
/// makes winding constant throughout it. A positive certified winding probe
/// then proves complete triangle-union coverage, not just sample coverage.
fn union_contains(polygon: &Polygon, bounds: Bounds, budget: &mut usize) -> Result<bool, String> {
    // A box spanning a vertex ordinate may still qualify by clearance.
    if let Some(winding) = boundary_winding(polygon, bounds.point(), budget)? {
        return Ok(winding > 0);
    }
    winding_after_clearance(polygon.boundary().map(Ok), 0, bounds, budget)
}

fn winding_after_clearance(
    uncertain: impl Iterator<Item = Result<[P; 2], String>> + Clone,
    constant: i32,
    bounds: Bounds,
    budget: &mut usize,
) -> Result<bool, String> {
    if bounds.lo == bounds.hi {
        return Ok(false);
    }
    for edge in uncertain.clone() {
        let [a, b] = edge?;
        if !winding_clearance(a, b, bounds, budget)? {
            return Ok(false);
        }
    }
    // A probe only supplies the winding after the complete boundary has
    // been proved disjoint from the connected query rectangle.
    for point in winding_probes(bounds) {
        let mut winding = constant;
        let mut complete = true;
        for edge in uncertain.clone() {
            let [a, b] = edge?;
            let Some(term) = winding_term(a, b, point.map(I::at), budget)? else {
                complete = false;
                break;
            };
            winding += term;
        }
        if complete {
            return Ok(winding > 0);
        }
    }
    Ok(false)
}

fn boundary_winding(polygon: &Polygon, probe: P, budget: &mut usize) -> Result<Option<i32>, String> {
    let mut winding = 0i32;
    for [a, b] in polygon.boundary() {
        let Some(term) = winding_term(a, b, probe, budget)? else { return Ok(None) };
        winding += term;
    }
    Ok(Some(winding))
}

/// A constant term certifies both its signed contribution and strict
/// separation of the segment from the entire probe rectangle.
fn winding_term(a: P, b: P, probe: P, budget: &mut usize) -> Result<Option<i32>, String> {
    charge(budget, 4)?;
    if a[0].hi.max(b[0].hi) < probe[0].lo {
        return Ok(Some(0));
    }
    let above = |p: P| p[1].lo > probe[1].hi;
    let below = |p: P| p[1].hi < probe[1].lo;
    let (a_above, a_below, b_above, b_below) = (above(a), below(a), above(b), below(b));
    if (a_above && b_above) || (a_below && b_below) {
        return Ok(Some(0));
    }
    let crossing = if a_below && b_above {
        1
    } else if b_below && a_above {
        -1
    } else {
        return Ok(None);
    };
    charge(budget, 12)?;
    let side = orient(a, b, probe);
    if !side.finite() || (side.lo <= 0.0 && side.hi >= 0.0) {
        return Ok(None);
    }
    Ok(Some(if (crossing > 0 && side.lo > 0.0) || (crossing < 0 && side.hi < 0.0) { crossing } else { 0 }))
}

#[derive(Default)]
struct WindingState {
    constant: i32,
    uncertain: Vec<usize>,
}

impl WindingState {
    fn refine(polygon: &Polygon, bounds: Bounds, parent: Option<&Self>, budget: &mut usize) -> Result<Self, String> {
        let mut state = Self { constant: parent.map_or(0, |p| p.constant), uncertain: Vec::new() };
        let mut observe = |index, a, b| {
            if let Some(term) = winding_term(a, b, bounds.point(), budget)? {
                state.constant += term;
            } else {
                reserve(&mut state.uncertain, 1)?;
                state.uncertain.push(index);
            }
            Ok::<_, String>(())
        };
        if let Some(parent) = parent {
            for &i in &parent.uncertain {
                let [a, b] = polygon.boundary_edge(i)?;
                observe(i, a, b)?;
            }
        } else {
            for (i, [a, b]) in polygon.boundary().enumerate() {
                observe(i, a, b)?;
            }
        }
        Ok(state)
    }

    fn contains(&self, polygon: &Polygon, bounds: Bounds, budget: &mut usize) -> Result<bool, String> {
        if self.uncertain.is_empty() {
            return Ok(self.constant > 0);
        }
        winding_after_clearance(self.uncertain.iter().map(|&i| polygon.boundary_edge(i)), self.constant, bounds, budget)
    }
}

fn winding_clearance(a: P, b: P, bounds: Bounds, budget: &mut usize) -> Result<bool, String> {
    charge(budget, 16)?;
    let edge = Bounds { lo: std::array::from_fn(|axis| a[axis].lo.min(b[axis].lo)), hi: std::array::from_fn(|axis| a[axis].hi.max(b[axis].hi)) };
    if !edge.overlaps(bounds) {
        return Ok(true);
    }
    let side = orient(a, b, bounds.point());
    Ok(side.finite() && (side.lo > 0.0 || side.hi < 0.0))
}

fn winding_probes(bounds: Bounds) -> impl Iterator<Item = [f64; 2]> {
    [(0.5, 0.5), (0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)].into_iter().map(move |(x, y)| {
        [
            (bounds.lo[0] * (1.0 - x) + bounds.hi[0] * x).clamp(bounds.lo[0], bounds.hi[0]),
            (bounds.lo[1] * (1.0 - y) + bounds.hi[1] * y).clamp(bounds.lo[1], bounds.hi[1]),
        ]
    })
}

struct LatticeCover<'a> {
    snapshot: &'a Snapshot,
    hull: &'a Hull,
    polygons: &'a [Polygon],
    chart_pad: f64,
    cells: usize,
}

impl LatticeCover<'_> {
    fn cell(&mut self, ranges: [[f32; 2]; 2], depth: usize, parent: Option<(Bounds, &[WindingState])>, budget: &mut usize) -> Result<bool, String> {
        charge(budget, 96)?;
        self.cells += 1;
        if self.cells > MAX_CELLS {
            return Ok(false);
        }
        let Some(bounds) = self.snapshot.rounded_ray_screen_bounds(ranges).and_then(|screen| self.snapshot.orthographic_ray_bounds(screen)) else {
            return Ok(false);
        };
        let bounds = bounds.grow(self.chart_pad);
        // Each child key is also a parent key, so both charts enclose
        // the physical ray. Intersect the two bounds before reusing proofs.
        let bounds = parent.map_or(bounds, |(outer, _)| Bounds {
            lo: std::array::from_fn(|axis| bounds.lo[axis].max(outer.lo[axis])),
            hi: std::array::from_fn(|axis| bounds.hi[axis].min(outer.hi[axis])),
        });
        if !bounds.valid() {
            return Ok(false);
        }
        let mut states = Vec::new();
        reserve(&mut states, self.polygons.len())?;
        for (i, polygon) in self.polygons.iter().enumerate() {
            charge(budget, 1)?;
            let prior = parent.and_then(|(_, states)| states.get(i));
            let state = if polygon.union_boundary && polygon.bounds.overlaps(bounds) {
                WindingState::refine(polygon, bounds, prior, budget)?
            } else {
                WindingState::default()
            };
            let covers = if !polygon.bounds.encloses(bounds) {
                false
            } else if polygon.union_boundary {
                state.contains(polygon, bounds, budget)?
            } else {
                convex_contains(&polygon.vertices, bounds, budget)?
            };
            if covers {
                return Ok(true);
            }
            states.push(state);
        }
        let size = ranges.map(screen_key_count);
        // With parent winding reuse, one more split can resolve
        // coverage using just the remaining boundary terms instead of a
        // batch of BVH queries. Keep the full exact-ray cap at max depth.
        let direct_limit = if depth < MAX_COVER_DEPTH { MAX_DISCRETE_RAYS / 2 } else { MAX_DISCRETE_RAYS };
        if size[0].checked_mul(size[1]).is_some_and(|count| count <= direct_limit as u64) {
            charge(budget, MAX_DISCRETE_RAYS)?;
            let Some((x, y)) = screen_grid(ranges) else { return Ok(false) };
            return self.snapshot.discrete_own_keys(self.hull, &x, &y, budget);
        }
        if depth >= MAX_COVER_DEPTH {
            return Ok(false);
        }
        let axis = usize::from(size[1] > size[0]);
        let [lo, hi] = ranges[axis];
        let middle = (f64::from(lo) * 0.5 + f64::from(hi) * 0.5) as f32;
        let middle = if middle < hi { middle } else { lo };
        let next = middle.next_up();
        if !(lo <= middle && middle < next && next <= hi) {
            return Ok(false);
        }
        let (mut first, mut second) = (ranges, ranges);
        first[axis][1] = middle;
        second[axis][0] = next;
        let parent = Some((bounds, states.as_slice()));
        Ok(self.cell(second, depth + 1, parent, budget)? && self.cell(first, depth + 1, parent, budget)?)
    }
}
fn covered(bounds: Bounds, polygons: &[Polygon], budget: &mut usize) -> Result<bool, String> {
    let mut stack = vec![(bounds, 0usize, [false; 4])];
    let mut cells = 0usize;
    while let Some((cell, depth, inherited_corners)) = stack.pop() {
        charge(budget, 1)?;
        cells += 1;
        if cells > MAX_CELLS {
            return Ok(false);
        }
        let mut complete = false;
        for polygon in polygons {
            if contains(polygon, cell, budget)? {
                complete = true;
                break;
            }
        }
        if complete {
            continue;
        }
        // Samples only reject a certificate early. They never certify union
        // coverage: every positive result covers all remaining rectangle cells.
        let center = [cell.lo[0] * 0.5 + cell.hi[0] * 0.5, cell.lo[1] * 0.5 + cell.hi[1] * 0.5];
        let new_corners = cell.corners().into_iter().zip(inherited_corners).filter_map(|(point, proved)| (!proved).then_some(point));
        for point in new_corners.chain(std::iter::once(center)) {
            let p = Bounds { lo: point, hi: point };
            let mut found = false;
            for polygon in polygons {
                if contains(polygon, p, budget)? {
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(false);
            }
        }
        let axis = usize::from(cell.hi[1] - cell.lo[1] > cell.hi[0] - cell.lo[0]);
        let middle = cell.lo[axis] * 0.5 + cell.hi[axis] * 0.5;
        if depth == MAX_COVER_DEPTH || middle <= cell.lo[axis] || middle >= cell.hi[axis] || cell.hi[axis] - cell.lo[axis] <= 1e-6 {
            return Ok(false);
        }
        let mut a = cell;
        let mut b = cell;
        *a.hi.get_mut(axis).ok_or("missing coverage axis")? = middle;
        *b.lo.get_mut(axis).ok_or("missing coverage axis")? = middle;
        // Every corner just passed a probe or inherited that exact proof.
        // The unchanged corners of each child keep their original bitwise
        // coordinates in Bounds::corners order. Whole-cell proofs still run
        // first, and new corners must still pass their own point probes.
        let (a_corners, b_corners) = if axis == 0 {
            ([true, false, false, true], [false, true, true, false])
        } else {
            ([true, true, false, false], [false, false, true, true])
        };
        stack.push((a, depth + 1, a_corners));
        stack.push((b, depth + 1, b_corners));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::geom::MeshGeom as _;
    fn projection(perspective: bool) -> Proj {
        let cam = solidcraft_viewport::Camera {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            distance: 20.0,
            fov: if perspective { std::f64::consts::FRAC_PI_4 } else { 0.0 },
            ..Default::default()
        };
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        Proj { cam, vp: cam.view_proj(f64::from(rect.width() / rect.height()), 100.0), rect }
    }
    fn quad(y: f64, left: f64, right: f64) -> Mesh {
        Mesh {
            positions: vec![Vec3::new(left, y, -2.0), Vec3::new(right, y, -2.0), Vec3::new(right, y, 2.0), Vec3::new(left, y, 2.0)],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        }
    }
    fn snapshot(proj: Proj, meshes: Vec<Mesh>) -> Snapshot {
        let sources = meshes
            .into_iter()
            .enumerate()
            .map(|(i, mesh)| Source { index: Arc::new(MeshIndex::new(&mesh)), mesh: Arc::new(mesh), own: i == 0 })
            .collect();
        let mut result = Snapshot {
            proj,
            chart: Chart::new(&proj).unwrap(),
            sources,
            fit: 0.01,
            tree: None,
            own_rays: Default::default(),
            memo_bypass: Default::default(),
        };
        result.prepare(&mut 10_000_000).unwrap();
        result
    }
    fn hull(y: f64) -> Vec<HPoint> {
        [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)]
            .into_iter()
            .enumerate()
            .map(|(i, (x, z))| HPoint::weighted(Vec3::new(x, y, z), [1.0, 2.0, 0.5, 1.5][i]))
            .collect()
    }
    fn tilted_hull(perspective: bool, gap: f64) -> (Snapshot, Vec<HPoint>) {
        let mut proj = projection(perspective);
        proj.cam.yaw = std::f64::consts::FRAC_PI_4;
        proj.vp = proj.cam.view_proj(f64::from(proj.rect.width() / proj.rect.height()), 100.0);
        let (right, up, back) = proj.cam.basis();
        let mesh = Mesh {
            positions: [(-4.0, -2.0), (4.0, -2.0), (4.0, 2.0), (-4.0, 2.0)].into_iter().map(|(x, y)| right * x + up * y + back * gap).collect(),
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..Default::default()
        };
        let controls = [(-1.0, -0.25, 1.0), (1.0, -0.25, 2.0), (1.0, 0.25, 0.5), (-1.0, 0.25, 1.5)]
            .into_iter()
            .map(|(x, y, w)| HPoint::weighted(right * x + up * y, w))
            .collect();
        (snapshot(proj, vec![mesh]), controls)
    }
    #[test]
    fn tilted_parallel_hull_can_prove_depth_when_the_coarse_endpoint_box_cannot() {
        let gap = 2.5;
        let (s, controls) = tilted_hull(false, gap);
        let hull = s.hull(&controls, &mut 100_000).unwrap().unwrap();
        let rays = hull.own.as_ref().unwrap();
        let (_, _, back) = s.proj.cam.basis();
        let coarse = dot(v(back), sub(rays.end, v(back * gap)));
        assert!(coarse.hi >= -rays.bias, "the old box cannot certify either coplanar triangle: {coarse:?}");
        assert!(!s.visible(Vec3::ZERO, &mut 100_000).unwrap());
        assert!(!s.continuous_occlusion(&hull, false, &mut 100_000).unwrap(), "the original endpoint box does not certify this hull");
        assert!(s.continuous_occlusion(&hull, true, &mut 100_000).unwrap(), "the tight endpoint proves both triangles and their shared diagonal");

        let (within_bias, controls) = tilted_hull(false, 0.1);
        assert!(within_bias.visible(Vec3::ZERO, &mut 100_000).unwrap());
        assert!(!within_bias.occludes_hull(&controls, &mut 100_000).unwrap(), "a closer plane within the existing depth bias is insufficient");
    }
    #[test]
    fn orthographic_ray_chart_encloses_actual_rounded_camera_origins_without_world_box_inflation() {
        let mut proj = projection(false);
        proj.cam.yaw = 0.63;
        proj.cam.pitch = 0.39;
        proj.cam.roll = -0.4;
        proj.cam.target = Vec3::new(812.5, -135.1, 120.25);
        proj.vp = proj.cam.view_proj(f64::from(proj.rect.width() / proj.rect.height()), 100.0);
        let s = snapshot(proj, vec![quad(-2.0, -2.0, 2.0)]);
        let screen = Bounds { lo: [237.5, 322.0], hi: [251.75, 327.5] };
        let composed = s.orthographic_ray_bounds(screen).unwrap();
        let rays = s.rays(v(proj.cam.target), screen, 0.0).unwrap();
        let mut coarse = Bounds::empty();
        coarse.add(s.chart.project(rays.origin).unwrap());
        assert!(coarse.encloses(composed));
        assert!((composed.hi[1] - composed.lo[1]) < 0.5 * (coarse.hi[1] - coarse.lo[1]));
        for i in 0..=16 {
            for j in 0..=16 {
                let [x, y] = [i, j].map(|n| f64::from(n) / 16.0);
                let at = egui::pos2((screen.lo[0] * (1.0 - x) + screen.hi[0] * x) as f32, (screen.lo[1] * (1.0 - y) + screen.hi[1] * y) as f32);
                let (origin, _) = proj.ray(at);
                let mut direct = Bounds::empty();
                direct.add(s.chart.project(v(origin)).unwrap());
                assert!(composed.encloses(direct), "{at:?}: {direct:?} outside {composed:?}");
            }
        }
    }
    #[test]
    fn tight_control_depth_encloses_rounded_ray_depth_through_the_hull() {
        for perspective in [false, true] {
            let (s, controls) = tilted_hull(perspective, 2.5);
            let hull = s.hull(&controls, &mut 100_000).unwrap().unwrap();
            let depth = tight_ray_depth(&hull.points, hull.world, hull.own.as_ref().unwrap());
            assert!(depth.finite());
            let (right, up, _) = s.proj.cam.basis();
            for i in -16..=16 {
                for j in -4..=4 {
                    let p = right * (f64::from(i) / 16.0) + up * (f64::from(j) / 16.0);
                    let (origin, direction) = s.proj.ray(s.proj.to_screen(p).unwrap());
                    let actual = (p - origin).dot(direction);
                    assert!(depth.lo <= actual && actual <= depth.hi, "perspective={perspective}, {p:?}: {actual} outside {depth:?}");
                }
            }
        }
    }
    #[test]
    fn projected_control_hull_enumerates_a_depth_aligned_curve_without_filling_its_world_box() {
        for perspective in [false, true] {
            let (s, _) = tilted_hull(perspective, 2.5);
            let (_, _, back) = s.proj.cam.basis();
            let controls = [HPoint::weighted(back * -1.5, 0.5), HPoint::weighted(back * 0.8, 2.0)];
            let hull = s.hull(&controls, &mut 100_000).unwrap().unwrap();
            assert!(s.screen_keys(hull.world).and_then(screen_grid).is_none());
            let (x, y) = s.discrete_keys(&hull, &mut 100_000).unwrap().unwrap();
            for i in 0..=64 {
                let t = f64::from(i) / 64.0;
                let point = (controls[0] * (1.0 - t) + controls[1] * t).project().unwrap();
                let actual = s.proj.to_screen(point).unwrap();
                assert!(x.contains(&actual.x) && y.contains(&actual.y), "perspective={perspective}, screen={actual:?}");
            }
        }
    }
    #[test]
    fn own_ray_hits_reuse_geometry_but_recompute_target_depth_bias_and_context_visibility() {
        for perspective in [false, true] {
            let mut s = snapshot(projection(perspective), vec![quad(-2.0, -2.0, 2.0)]);
            assert!(!s.visible(Vec3::ZERO, &mut 100_000).unwrap());
            assert!(s.visible(Vec3::new(0.0, -1.95, 0.0), &mut 100_000).unwrap(), "the nearer target must not inherit a hidden boolean");
            assert!(!s.visible(Vec3::ZERO, &mut 100_000).unwrap());
            {
                let memo = s.own_rays.lock().unwrap();
                assert_eq!(memo.values.len(), 1);
                assert_eq!(memo.raycasts, 1);
                assert_eq!(memo.hit_reuses, 2);
            }
            s.fit = 1.0;
            assert!(s.visible(Vec3::ZERO, &mut 100_000).unwrap(), "the target's new bias is independent of the saved closest hit");
            s.clear_own_ray_cache_for_test().unwrap();
            assert!(s.visible(Vec3::ZERO, &mut 100_000).unwrap(), "warm and cold classifications agree");
            let context = snapshot(projection(perspective), vec![Mesh::default(), quad(-2.0, -2.0, 2.0)]);
            assert!(!context.visible(Vec3::ZERO, &mut 100_000).unwrap());
            assert!(context.visible(Vec3::new(0.0, -2.0, 0.0), &mut 100_000).unwrap(), "coincident context is recomputed at the actual target");
            assert_eq!(context.own_rays.lock().unwrap().miss_reuses, 1);
        }
    }
    #[test]
    fn successful_own_ray_misses_are_memoized_but_budget_errors_never_become_misses() {
        let s = snapshot(projection(false), vec![quad(-2.0, -2.0, 2.0)]);
        let screen = s.proj.to_screen(Vec3::new(3.0, 0.0, 0.0)).unwrap();
        for invalid in [egui::pos2(f32::NAN, screen.y), egui::pos2(screen.x, f32::INFINITY)] {
            assert!(s.own_hit(invalid, &mut 1000).is_err());
        }
        assert!(s.own_rays.lock().unwrap().values.is_empty());
        assert_eq!(s.own_hit(screen, &mut 100_000).unwrap(), None);
        assert_eq!(s.own_hit(screen, &mut 1).unwrap(), None, "a successful saved miss needs no BVH traversal");
        assert_eq!(s.own_rays.lock().unwrap().miss_reuses, 1);
        s.clear_own_ray_cache_for_test().unwrap();
        assert!(s.own_hit(screen, &mut 1).is_err());
        assert!(s.own_rays.lock().unwrap().values.is_empty());
        assert_eq!(s.own_rays.lock().unwrap().errors, 1);
        assert_eq!(s.own_hit(screen, &mut 100_000).unwrap(), None);
        s.clear_own_ray_cache_for_test().unwrap();
        let hit_screen = s.proj.to_screen(Vec3::ZERO).unwrap();
        assert!(s.own_hit(hit_screen, &mut 1).is_err());
        assert!(s.own_rays.lock().unwrap().values.is_empty());
        let hit = s.own_hit(hit_screen, &mut 100_000).unwrap();
        assert!(hit.is_some());
        assert_eq!(s.own_hit(hit_screen, &mut 1).unwrap(), hit);
        let entries = s.own_rays.lock().unwrap().values.len();
        assert!(s.without_own_ray_cache_for_test(|| s.own_hit(hit_screen, &mut 1)).is_err(), "diagnostic bypass must not borrow a warm answer");
        assert_eq!(s.own_rays.lock().unwrap().values.len(), entries, "diagnostic bypass does not disturb actual query state");
        assert_eq!(s.own_hit(hit_screen, &mut 1).unwrap(), hit, "bypass scope restores caching");
    }
    #[test]
    fn own_ray_memo_uses_both_screen_bits_and_bounded_fifo_eviction_changes_cost_only() {
        let s = snapshot(projection(false), vec![quad(-2.0, -2.0, 2.0)]);
        let first = s.proj.to_screen(Vec3::ZERO).unwrap();
        let hit = s.own_hit(first, &mut 100_000).unwrap();
        assert!(hit.is_some());
        for screen in [egui::pos2(first.x.next_up(), first.y), egui::pos2(first.x, first.y.next_up())] {
            assert!(s.own_hit(screen, &mut 100_000).unwrap().is_some());
        }
        assert_eq!(s.own_rays.lock().unwrap().values.len(), 3, "neither coordinate may be dropped from the exact key");
        s.clear_own_ray_cache_for_test().unwrap();
        let mut x = first.x;
        for _ in 0..=MAX_OWN_RAY_MEMO {
            assert!(s.own_hit(egui::pos2(x, first.y), &mut 100_000).unwrap().is_some());
            x = x.next_up();
        }
        {
            let memo = s.own_rays.lock().unwrap();
            assert_eq!(memo.values.len(), MAX_OWN_RAY_MEMO);
            assert_eq!(memo.order.len(), MAX_OWN_RAY_MEMO);
            assert_eq!(memo.evictions, 1);
            assert!(!memo.values.contains_key(&[first.x.to_bits(), first.y.to_bits()]));
        }
        assert!(s.own_hit(first, &mut 1).is_err(), "an evicted result must be recomputed, not guessed");
        assert_eq!(s.own_hit(first, &mut 100_000).unwrap(), hit);
        assert_eq!(s.own_rays.lock().unwrap().values.len(), MAX_OWN_RAY_MEMO);
        assert_eq!(s.own_rays.lock().unwrap().evictions, 2);
    }
    #[test]
    fn own_ray_memos_are_snapshot_local_and_small_discrete_proofs_share_the_same_successful_hits() {
        for perspective in [false, true] {
            let front = snapshot(projection(perspective), vec![quad(-2.0, -2.0, 2.0)]);
            let rear = snapshot(projection(perspective), vec![quad(2.0, -2.0, 2.0)]);
            assert!(!front.visible(Vec3::ZERO, &mut 100_000).unwrap());
            assert!(rear.visible(Vec3::ZERO, &mut 100_000).unwrap(), "another mesh must never borrow the front snapshot's hit");
            assert_eq!(front.own_rays.lock().unwrap().values.len(), 1);
            assert_eq!(rear.own_rays.lock().unwrap().values.len(), 1);
            let point = [HPoint::weighted(Vec3::ZERO, 1.0); 2];
            let before = front.own_rays.lock().unwrap().hit_reuses;
            assert!(front.occludes_hull(&point, &mut 400).unwrap(), "complete small-ray proof must precede expensive continuous coverage");
            assert!(front.own_rays.lock().unwrap().hit_reuses > before);
            front.clear_own_ray_cache_for_test().unwrap();
            assert!(front.occludes_hull(&point, &mut 100_000).unwrap(), "cold and warm exact certificates agree");
        }
    }

    #[test]
    fn shared_tessellation_diagonal_is_covered_by_union_without_using_triangle_model_ids() {
        for perspective in [false, true] {
            let mut one = quad(-2.0, -2.0, 2.0);
            one.triangles.truncate(1);
            let one = snapshot(projection(perspective), vec![one]);
            assert!(!one.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
            let interior: Vec<_> =
                hull(0.0).into_iter().map(|point| HPoint::weighted(point.project().unwrap() * 0.25 + Vec3::new(1.0, 0.0, -1.0), 1.0)).collect();
            let ray_hull = one.hull(&interior, &mut 10_000_000).unwrap().unwrap();
            CERTIFICATE_TRACE.with(|trace| *trace.borrow_mut() = Some(CertificateStats::default()));
            let covered = one.continuous_occlusion(&ray_hull, false, &mut 10_000_000);
            let stats = CERTIFICATE_TRACE.with(|trace| trace.borrow_mut().take().unwrap());
            assert!(covered.unwrap());
            assert_eq!(stats.coverage_proved, 1);
            assert_eq!(stats.polygons, 0, "one safe triangle should cover this hull directly");

            let both = snapshot(projection(perspective), vec![quad(-2.0, -2.0, 2.0)]);
            assert!(both.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
            assert!(!both.visible(Vec3::ZERO, &mut 10_000).unwrap());
        }
    }
    #[test]
    fn rejected_depth_candidates_charge_the_examined_controls() {
        let s = snapshot(projection(false), vec![quad(2.0, -2.0, 2.0)]);
        let item = *s.tree.as_ref().unwrap().items.first().unwrap();
        let points = hull(0.0);
        let mut work = [1_000; 2];
        for (controls, remaining) in [points.clone(), points.repeat(16)].iter().zip(&mut work) {
            let hull = s.hull(controls, &mut 100_000).unwrap().unwrap();
            CERTIFICATE_TRACE.with(|trace| *trace.borrow_mut() = Some(CertificateStats::default()));
            let triangle = s.safe_triangle(item, &hull, None, remaining);
            let stats = CERTIFICATE_TRACE.with(|trace| trace.borrow_mut().take().unwrap());
            assert!(triangle.unwrap().is_none());
            assert_eq!(stats.world_depth, 1, "the triangle is behind the first control");
        }
        assert_eq!(work[0], work[1], "unexamined controls do not spend the visibility budget");
    }
    #[test]
    fn whole_world_box_depth_skips_control_tests_but_preserves_correlated_hulls() {
        let s = snapshot(projection(false), vec![quad(-2.0, -2.0, 2.0)]);
        let world = s.hull(&hull(0.0), &mut 100_000).unwrap().unwrap();
        let item = s.tree.as_ref().unwrap().items[0];
        let mut work = 64 + 8;
        assert!(s.safe_triangle(item, &world, None, &mut work).unwrap().is_some());
        assert_eq!(work, 0, "one complete-box plane test suffices for all controls");

        let gap = 0.4;
        let (own, controls) = tilted_hull(false, gap);
        let s = snapshot(own.proj, vec![Mesh::default(), (*own.sources[0].mesh).clone()]);
        let hull = s.hull(&controls, &mut 100_000).unwrap().unwrap();
        let (_, _, back) = s.proj.cam.basis();
        let coarse = dot(v(back), sub(hull.world, v(back * gap)));
        assert!(coarse.hi >= 0.0, "the world box loses the control correlations");
        assert!(s.continuous_occlusion(&hull, true, &mut 100_000).unwrap(), "per-control fallback retains the exact tilted hull proof");
    }

    #[test]
    fn duplicate_native_face_vertex_indices_do_not_open_a_display_occlusion_crack() {
        for perspective in [false, true] {
            let original = quad(-2.0, -2.0, 2.0);
            let mesh = Mesh {
                positions: vec![
                    original.positions[0],
                    original.positions[1],
                    original.positions[2],
                    original.positions[0],
                    original.positions[2],
                    original.positions[3],
                ],
                triangles: vec![[0, 1, 2], [3, 4, 5]],
                ..Default::default()
            };
            let both = snapshot(projection(perspective), vec![mesh]);
            assert!(both.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
            assert!(!both.visible(Vec3::ZERO, &mut 10_000).unwrap());
        }
        assert_eq!(vertex_key(0, Vec3::new(-0.0, 1.0, 2.0)), vertex_key(0, Vec3::new(0.0, 1.0, 2.0)));
        assert_ne!(vertex_key(0, Vec3::ZERO), vertex_key(1, Vec3::ZERO));
        assert_ne!(vertex_key(0, Vec3::ZERO), vertex_key(0, Vec3::new(f64::MIN_POSITIVE, 0.0, 0.0)));
    }
    #[test]
    fn concave_safe_triangle_union_covers_internal_seams_without_filling_its_notch() {
        let mesh = Mesh {
            positions: vec![
                Vec3::new(-2.0, -2.0, -2.0),
                Vec3::new(2.0, -2.0, -2.0),
                Vec3::new(2.0, -2.0, 0.0),
                Vec3::new(0.0, -2.0, 0.0),
                Vec3::new(0.0, -2.0, 2.0),
                Vec3::new(-2.0, -2.0, 2.0),
            ],
            triangles: vec![[0, 1, 3], [1, 2, 3], [0, 3, 5], [3, 4, 5]],
            ..Default::default()
        };
        for perspective in [false, true] {
            let s = snapshot(projection(perspective), vec![mesh.clone()]);
            let internal: Vec<_> = hull(0.0).into_iter().map(|p| HPoint::weighted(p.project().unwrap() + Vec3::new(-1.0, 0.0, -1.0), 1.0)).collect();
            assert!(s.occludes_hull(&internal, &mut 10_000_000).unwrap());
            assert!(!s.visible(Vec3::new(-1.0, 0.0, -1.0), &mut 10_000).unwrap());
            let notch: Vec<_> = hull(0.0).into_iter().map(|p| HPoint::weighted(p.project().unwrap() + Vec3::new(1.0, 0.0, 1.0), 1.0)).collect();
            assert!(!s.occludes_hull(&notch, &mut 10_000_000).unwrap());
            assert!(s.visible(Vec3::new(1.0, 0.0, 1.0), &mut 10_000).unwrap());
            assert!(!s.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap(), "a box crossing the concave notch must not become its convex hull");
        }
    }
    #[test]
    fn union_boundary_keeps_folds_separate_and_uncertain_outer_contact_uncertified() {
        let make = |x: f64, z: f64| Vertex { id: vertex_key(0, Vec3::new(x, -2.0, z)), point: [I::at(x), I::at(z)], world: Vec3::new(x, -2.0, z) };
        let a = make(-2.0, -2.0);
        let b = make(2.0, -2.0);
        let triangles = vec![
            Triangle { vertices: vec![a.clone(), b.clone(), make(1.0, 2.0)], own: true },
            Triangle { vertices: vec![a.clone(), b.clone(), make(-1.0, 2.0)], own: true },
        ];
        let separate = polygons(&triangles, &mut 1_000_000).unwrap();
        assert_eq!(separate.len(), 2, "same-direction shared edges keep separate boundary contributions");
        assert!(separate.iter().all(|p| p.union_boundary && p.vertices.len() == 3));
        let mut multiply_used = triangles;
        multiply_used.push(Triangle { vertices: vec![a.clone(), b.clone(), make(0.0, 3.0)], own: true });
        let separate = polygons(&multiply_used, &mut 1_000_000).unwrap();
        assert_eq!(separate.len(), 3);
        assert!(separate.iter().all(|p| p.union_boundary && p.vertices.len() == 3));
        let c = make(2.0, 2.0);
        let square_with_fold = [
            Triangle { vertices: vec![a.clone(), b.clone(), c.clone()], own: true },
            Triangle { vertices: vec![a.clone(), c, make(-2.0, 2.0)], own: true },
            Triangle { vertices: vec![a, b, make(1.0, -1.0)], own: true },
        ];
        let polygons = polygons(&square_with_fold, &mut 1_000_000).unwrap();
        assert_eq!(polygons.len(), 2);
        assert!(covered(Bounds { lo: [-0.25, -0.25], hi: [0.25, 0.25] }, &polygons, &mut 1_000_000).unwrap());
        assert!(!covered(Bounds { lo: [-0.25, -2.1], hi: [0.25, -1.9] }, &polygons, &mut 1_000_000).unwrap());
        for perspective in [false, true] {
            let s = snapshot(projection(perspective), vec![quad(-2.0, -2.0, 2.0)]);
            let touching = [-0.5, 0.5].map(|z| {
                let edge = Vec3::new(2.0, -2.0, z);
                let point = if perspective {
                    let eye = s.proj.cam.eye();
                    let delta = edge - eye;
                    eye + delta * (-eye.y / delta.y)
                } else {
                    Vec3::new(2.0, 0.0, z)
                };
                HPoint::weighted(point, 1.0)
            });
            assert!(!s.occludes_hull(&touching, &mut 10_000_000).unwrap(), "uncertain outer-edge contact is not a coverage certificate");
        }
    }
    #[test]
    fn whole_box_winding_skips_point_probes_and_keeps_strict_clearance() {
        let boundary = |points: &[[f64; 2]]| {
            let mut bounds = Bounds::empty();
            let vertices = points
                .iter()
                .map(|&[x, y]| {
                    let world = Vec3::new(x, -2.0, y);
                    let point = [I::at(x), I::at(y)];
                    bounds.add(point);
                    Vertex { id: vertex_key(0, world), point, world }
                })
                .collect();
            Polygon { vertices, boundary_edges: Vec::new(), bounds, union_boundary: true }
        };
        let square = boundary(&[[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]]);
        let inside = Bounds { lo: [-0.5; 2], hi: [0.5; 2] };
        assert!(union_contains(&square, inside, &mut 40).unwrap());
        let touching = Bounds { lo: [1.9, -0.5], hi: [2.0, 0.5] };
        assert!(!union_contains(&square, touching, &mut 1_000).unwrap());

        let concave = boundary(&[[-2.0, -2.0], [2.0, -2.0], [2.0, 0.0], [0.0, 0.0], [0.0, 2.0], [-2.0, 2.0]]);
        let spanning_vertex_ordinate = Bounds { lo: [-1.5, -0.5], hi: [-0.5, 0.5] };
        assert_eq!(boundary_winding(&concave, spanning_vertex_ordinate.point(), &mut 1_000).unwrap(), None);
        assert!(union_contains(&concave, spanning_vertex_ordinate, &mut 1_000).unwrap());
        let notch = Bounds { lo: [0.5, 0.5], hi: [1.0, 1.0] };
        assert!(!union_contains(&concave, notch, &mut 1_000).unwrap());
    }

    #[test]
    fn lattice_children_reuse_only_certified_winding_edges_and_keep_concave_gaps() {
        let mut bounds = Bounds::empty();
        let vertices = [[-2.0, -2.0], [2.0, -2.0], [2.0, 0.0], [0.0, 0.0], [0.0, 2.0], [-2.0, 2.0]]
            .into_iter()
            .map(|[x, y]| {
                let world = Vec3::new(x, -2.0, y);
                let point = [I::at(x), I::at(y)];
                bounds.add(point);
                Vertex { id: vertex_key(0, world), point, world }
            })
            .collect();
        let polygon = Polygon { vertices, boundary_edges: Vec::new(), bounds, union_boundary: true };
        let parent = Bounds { lo: [-1.5, -0.5], hi: [1.5, 0.5] };
        let prior = WindingState::refine(&polygon, parent, None, &mut 1_000).unwrap();
        assert!(!prior.uncertain.is_empty() && prior.uncertain.len() < polygon.vertices.len());
        assert!(!prior.contains(&polygon, parent, &mut 1_000).unwrap());
        for (child, expected) in [
            (Bounds { lo: [-1.5, 0.25], hi: [-0.5, 0.5] }, true),
            (Bounds { lo: [0.5, 0.25], hi: [1.5, 0.5] }, false),
            (Bounds { lo: [-0.25, 0.25], hi: [0.0, 0.5] }, false),
        ] {
            assert!(parent.encloses(child));
            let mut reused_budget = 1_000;
            let state = WindingState::refine(&polygon, child, Some(&prior), &mut reused_budget).unwrap();
            assert_eq!(state.contains(&polygon, child, &mut reused_budget).unwrap(), expected);
            let mut fresh_budget = 1_000;
            assert_eq!(union_contains(&polygon, child, &mut fresh_budget).unwrap(), expected);
            if expected {
                assert!(reused_budget > fresh_budget);
            }
        }
    }

    #[test]
    fn coverage_splits_reuse_parent_point_probes_without_skipping_whole_cell_proofs() {
        let rectangle = |left, right, bottom, top| {
            let vertices = [[left, bottom], [right, bottom], [right, top], [left, top]]
                .into_iter()
                .map(|[x, z]| {
                    let world = Vec3::new(x, -2.0, z);
                    Vertex { id: vertex_key(0, world), point: [I::at(x), I::at(z)], world }
                })
                .collect();
            polygon(vertices).unwrap()
        };
        let bounds = Bounds { lo: [-1.0, -1.0], hi: [1.0, 1.0] };
        let mut quadrants = [
            rectangle(-2.0, 0.25, -2.0, 0.25),
            rectangle(-2.0, 0.25, -0.25, 2.0),
            rectangle(-0.25, 2.0, -2.0, 0.25),
            rectangle(-0.25, 2.0, -0.25, 2.0),
        ];
        // The square requires an X split and then both Y splits. The budget
        // allows each of those child proofs, but not rechecking the two
        // already-qualified corners of both half boxes.
        assert!(covered(bounds, &quadrants, &mut 2_200).unwrap());
        // Parent corners and center still pass. A slit in the top right
        // quarter must remain uncertified during the child proofs.
        quadrants[1] = rectangle(-2.0, 0.1, -0.25, 2.0);
        quadrants[3] = rectangle(0.15, 2.0, -0.25, 2.0);
        assert!(!covered(bounds, &quadrants, &mut 1_000_000).unwrap());
    }
    #[test]
    fn overlapping_safe_context_components_jointly_cover_the_box() {
        let s = snapshot(projection(false), vec![Mesh::default(), quad(-2.0, -2.0, 0.2), quad(-2.0, -0.2, 2.0)]);
        assert!(s.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
        assert!(!s.visible(Vec3::ZERO, &mut 10_000).unwrap());
    }
    #[test]
    fn grazing_occluder_uses_the_same_ray_depth_bias_for_parallel_and_perspective_rays() {
        for perspective in [false, true] {
            for gap in [0.1, 0.3] {
                // y=10*x-gap. The central ray's hit gap is exactly gap,
                // while normal distance is gap/sqrt(101). Both front and
                // rear vertices stay before the perspective near plane.
                let mesh = Mesh {
                    positions: vec![
                        Vec3::new(-0.5, -5.0 - gap, -1.0),
                        Vec3::new(0.5, 5.0 - gap, -1.0),
                        Vec3::new(0.5, 5.0 - gap, 1.0),
                        Vec3::new(-0.5, -5.0 - gap, 1.0),
                    ],
                    triangles: vec![[0, 1, 2], [0, 2, 3]],
                    ..Default::default()
                };
                let s = snapshot(projection(perspective), vec![mesh]);
                let narrow =
                    [(-0.0001, -0.25), (0.0001, -0.25), (0.0001, 0.25), (-0.0001, 0.25)].map(|(x, z)| HPoint::weighted(Vec3::new(x, 0.0, z), 1.0));
                let hidden = gap > 0.15;
                assert_eq!(s.visible(Vec3::ZERO, &mut 10_000).unwrap(), !hidden);
                assert_eq!(
                    s.occludes_hull(&narrow, &mut 10_000_000).unwrap(),
                    hidden,
                    "perspective={perspective}, ray gap={gap}, normal distance={}",
                    gap / 101.0_f64.sqrt()
                );
                if hidden {
                    assert!(gap / 101.0_f64.sqrt() < 0.15, "this must challenge the old normal-distance bound");
                }
            }
        }
    }
    #[test]
    fn exhaustive_screen_keys_cover_microscopic_display_seams_without_welding_the_mesh() {
        for perspective in [false, true] {
            let mut projection = projection(perspective);
            projection.cam.target.x = 0.123456789;
            projection.vp = projection.cam.view_proj(f64::from(projection.rect.width() / projection.rect.height()), 100.0);
            // Put a genuine tiny gap on the ideal geometric ray. The picker
            // uses a rounded f32 screen ray, which lies strictly on one side.
            let eye = projection.cam.eye();
            let seam = if perspective { eye.x * (1.0 - (-2.0 - eye.y) / -eye.y) } else { 0.0 };
            let left = quad(-2.0, -2.0, seam - 1e-14);
            let right = quad(-2.0, seam + 1e-14, 2.0);
            let mesh = Mesh {
                positions: left.positions.into_iter().chain(right.positions).collect(),
                triangles: left.triangles.into_iter().chain(right.triangles.into_iter().map(|triangle| triangle.map(|id| id + 4))).collect(),
                ..Default::default()
            };
            let s = snapshot(projection, vec![mesh]);
            let world = [HPoint::weighted(Vec3::ZERO, 1.0), HPoint::weighted(Vec3::ZERO, 2.0)];
            assert!(!s.visible(Vec3::ZERO, &mut 100_000).unwrap(), "the actual rounded picker ray must hit one side, perspective={perspective}");
            let hull = s.hull(&world, &mut 100_000).unwrap().unwrap();
            CERTIFICATE_TRACE.with(|trace| *trace.borrow_mut() = Some(CertificateStats::default()));
            let continuous = s.continuous_occlusion(&hull, false, &mut 10_000_000);
            let stats = CERTIFICATE_TRACE.with(|trace| trace.borrow_mut().take().unwrap());
            assert!(stats.safe >= 2 && stats.polygons >= 2, "both panels must reach continuous coverage: {stats:?}");
            assert!(!continuous.unwrap(), "continuous coverage must preserve the actual display-mesh gap");
            assert_eq!(
                s.continuous_occlusion(&hull, true, &mut 10_000_000).unwrap(),
                !perspective,
                "only the orthographic lattice can certify coverage before the explicit rounded-ray fallback"
            );
            assert!(s.discrete_own_occlusion(&hull, &mut 1_000_000).unwrap(), "all possible rounded rays must be proven hidden");
            assert!(s.occludes_hull(&world, &mut 10_000_000).unwrap());
        }
    }
    #[test]
    fn discrete_ray_certificates_keep_real_holes_near_bias_and_the_complete_world_depth_range() {
        for perspective in [false, true] {
            let world = [HPoint::weighted(Vec3::ZERO, 1.0); 2];
            let left = quad(-2.0, -2.0, -0.01);
            let right = quad(-2.0, 0.01, 2.0);
            let gap = snapshot(
                projection(perspective),
                vec![Mesh {
                    positions: left.positions.into_iter().chain(right.positions).collect(),
                    triangles: left.triangles.into_iter().chain(right.triangles.into_iter().map(|triangle| triangle.map(|id| id + 4))).collect(),
                    ..Default::default()
                }],
            );
            let hull = gap.hull(&world, &mut 100_000).unwrap().unwrap();
            assert!(gap.visible(Vec3::ZERO, &mut 100_000).unwrap());
            assert!(!gap.discrete_own_occlusion(&hull, &mut 100_000).unwrap(), "one real missed screen key must refuse the whole certificate");
            for y in [-0.1, 2.0, -80.0 + 1e-12] {
                let s = snapshot(projection(perspective), vec![quad(y, -2.0, 2.0)]);
                let hull = s.hull(&world, &mut 100_000).unwrap().unwrap();
                assert!(!s.discrete_own_occlusion(&hull, &mut 100_000).unwrap(), "depth/near-origin policy must remain unchanged, y={y}");
            }
            let s = snapshot(projection(perspective), vec![quad(-2.0, -2.0, 2.0)]);
            let partial = [HPoint::weighted(Vec3::ZERO, 1.0), HPoint::weighted(Vec3::new(0.0, -1.95, 0.0), 2.0)];
            let hull = s.hull(&partial, &mut 100_000).unwrap().unwrap();
            assert!(!s.discrete_own_occlusion(&hull, &mut 100_000).unwrap(), "a hidden sample cannot replace the hull's nearer depth bound");
            assert!(s.discrete_own_occlusion(&hull, &mut 0).is_err());
        }
    }

    #[test]
    fn partitioned_orthographic_key_cover_refuses_a_gap_only_when_a_ray_can_enter_it() {
        let proj = projection(false);
        let center = proj.rect.center();
        let next = egui::pos2(center.x.next_up(), center.y);
        let at = |screen: egui::Pos2| {
            let (origin, direction) = proj.ray(screen);
            origin + direction * (-origin.y / direction.y)
        };
        let a = at(center).x;
        let dx = at(next).x - a;
        assert!(dx > 0.0);
        let panels = |left: f64, right: f64, depth: f64| {
            let mut mesh = quad(depth, -2.0, left);
            mesh.append(&quad(depth, right, 2.0), 0);
            snapshot(proj, vec![mesh])
        };
        let controls = [[-0.0009, -0.0009], [0.0009, -0.0009], [0.0009, 0.0009], [-0.0009, 0.0009]]
            .map(|[x, y]| HPoint::weighted(at(center + egui::vec2(x, y)), 1.0));
        let s = panels(a + 0.25 * dx, a + 0.75 * dx, -2.0);
        let ray_hull = s.hull(&controls, &mut 100_000).unwrap().unwrap();
        assert!(s.discrete_keys(&ray_hull, &mut 100_000).unwrap().is_none(), "whole box exceeds the original small-grid cap");
        assert!(!s.continuous_occlusion(&ray_hull, false, &mut 1_000_000).unwrap(), "the real continuous gap remains");
        assert!(s.occludes_hull(&controls, &mut 1_000_000).unwrap(), "the gap lies strictly between adjacent rounded rays");
        assert!(s.occludes_hull(&controls, &mut 100).is_err(), "a low budget cannot manufacture a proof");

        let hole = panels(a - 0.25 * dx, a + 0.75 * dx, -2.0);
        assert!(hole.visible(at(center), &mut 100_000).unwrap());
        assert!(!hole.occludes_hull(&controls, &mut 1_000_000).unwrap());
        let within_bias = panels(a + 0.25 * dx, a + 0.75 * dx, -0.05);
        assert!(!within_bias.occludes_hull(&controls, &mut 1_000_000).unwrap());
    }
    #[test]
    fn screen_key_ranges_follow_actual_two_stage_f32_projection_and_refuse_excessive_enumeration() {
        for perspective in [false, true] {
            let mut projection = projection(perspective);
            projection.rect = egui::Rect::from_min_size(egui::pos2(-53.25, 27.125), egui::vec2(800.0, 600.0));
            projection.vp = projection.cam.view_proj(f64::from(projection.rect.width() / projection.rect.height()), 100.0);
            let s = snapshot(projection, vec![quad(-2.0, -2.0, 2.0)]);
            let world = [I { lo: -1e-8, hi: 1e-8 }, I { lo: -0.01, hi: 0.01 }, I { lo: -1e-8, hi: 1e-8 }];
            let keys = s.screen_keys(world).unwrap();
            for x in [-1e-8, 0.0, 1e-8] {
                for y in [-0.01, 0.0, 0.01] {
                    for z in [-1e-8, 0.0, 1e-8] {
                        let actual = projection.to_screen(Vec3::new(x, y, z)).unwrap();
                        assert!(actual.x >= keys[0][0] && actual.x <= keys[0][1]);
                        assert!(actual.y >= keys[1][0] && actual.y <= keys[1][1]);
                    }
                }
            }
        }
        let start = 128.0_f32;
        let end = (0..MAX_DISCRETE_RAYS - 1).fold(start, |key, _| key.next_up());
        assert_eq!(float_keys([start, end]).unwrap().len(), MAX_DISCRETE_RAYS);
        assert!(float_keys([start, end.next_up()]).is_none());
        assert!(float_keys([f32::NAN, 1.0]).is_none());
        assert!(float_keys([1.0, f32::INFINITY]).is_none());
        assert!(float_keys([1.0, 0.0]).is_none());
        assert_eq!(float_keys([-0.0, 0.0]).unwrap().len(), 1);
    }

    #[test]
    fn partial_cover_wrong_depth_and_touching_bias_never_supply_a_certificate() {
        let s = snapshot(projection(false), vec![quad(-2.0, -2.0, 2.0)]);
        let mut partial = hull(0.0);
        partial.push(HPoint::weighted(Vec3::new(3.0, 0.0, 0.0), 1.0));
        assert!(!s.occludes_hull(&partial, &mut 10_000_000).unwrap());
        assert!(!s.occludes_hull(&hull(-1.9), &mut 10_000_000).unwrap());
        assert!(s.visible(Vec3::new(0.0, -1.9, 0.0), &mut 10_000).unwrap());
        let behind = snapshot(projection(false), vec![quad(2.0, -2.0, 2.0)]);
        assert!(!behind.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
        assert!(behind.visible(Vec3::ZERO, &mut 10_000).unwrap());
        let touching = snapshot(projection(false), vec![Mesh::default(), quad(-2.0, -2.0, 2.0)]);
        assert!(!touching.occludes_hull(&hull(-2.0), &mut 10_000_000).unwrap());
        assert!(touching.visible(Vec3::new(0.0, -2.0, 0.0), &mut 10_000).unwrap());
        let before_ray_origin = snapshot(projection(false), vec![quad(-80.0 + 1e-12, -2.0, 2.0)]);
        assert!(!before_ray_origin.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
        assert!(before_ray_origin.visible(Vec3::ZERO, &mut 10_000).unwrap());
    }
    #[test]
    fn merging_a_connected_ring_never_fills_its_projected_hole() {
        let positions = vec![
            Vec3::new(-2.0, -2.0, -2.0),
            Vec3::new(2.0, -2.0, -2.0),
            Vec3::new(2.0, -2.0, 2.0),
            Vec3::new(-2.0, -2.0, 2.0),
            Vec3::new(-0.25, -2.0, -0.25),
            Vec3::new(0.25, -2.0, -0.25),
            Vec3::new(0.25, -2.0, 0.25),
            Vec3::new(-0.25, -2.0, 0.25),
        ];
        let mut triangles = Vec::new();
        for i in 0..4u32 {
            let j = (i + 1) % 4;
            triangles.push([i, j, j + 4]);
            triangles.push([i, j + 4, i + 4]);
        }
        let s = snapshot(projection(false), vec![Mesh { positions, triangles, ..Default::default() }]);
        let ray_hull = s.hull(&hull(0.0), &mut 100_000).unwrap().unwrap();
        CERTIFICATE_TRACE.with(|trace| *trace.borrow_mut() = Some(CertificateStats::default()));
        let covered = s.continuous_occlusion(&ray_hull, false, &mut 100_000);
        let stats = CERTIFICATE_TRACE.with(|trace| trace.borrow_mut().take().unwrap());
        assert_eq!(stats.safe, 8, "all front ring triangles qualify");
        assert_eq!(stats.union_boundaries, 1, "the connected outer and inner rings share one signed boundary");
        assert!(!covered.unwrap());
        let arm = hull(0.0).into_iter().map(|p| HPoint::weighted(p.project().unwrap() * 0.5 + Vec3::new(-1.0, 0.0, 0.0), 1.0)).collect::<Vec<_>>();
        assert!(s.occludes_hull(&arm, &mut 100_000).unwrap());
        assert!(!s.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
        assert!(s.visible(Vec3::ZERO, &mut 10_000).unwrap());
    }
    #[test]
    fn a_positive_local_turn_star_is_not_a_convex_coverage_boundary() {
        let vertices = [0, 2, 4, 1, 3]
            .into_iter()
            .map(|step| {
                let angle = std::f64::consts::TAU * f64::from(step) / 5.0;
                let (y, x) = angle.sin_cos();
                Vertex { id: vertex_key(0, Vec3::new(x, -2.0, y)), point: [I::at(x), I::at(y)], world: Vec3::new(x, -2.0, y) }
            })
            .collect();
        assert!(polygon(vertices).is_none());
    }
    #[test]
    fn orthographic_own_bias_includes_the_existing_two_pixel_depth_allowance() {
        let mut proj = projection(false);
        proj.cam.distance = 2000.0;
        proj.vp = proj.cam.view_proj(f64::from(proj.rect.width() / proj.rect.height()), 100.0);
        let s = snapshot(proj, vec![quad(-2.0, -2.0, 2.0)]);
        assert!(!s.occludes_hull(&hull(0.0), &mut 10_000_000).unwrap());
        assert!(s.visible(Vec3::ZERO, &mut 10_000).unwrap());
    }
    #[test]
    fn tensor_hulls_near_plane_mixes_hostile_inputs_and_budgets_are_bounded() {
        let s = snapshot(projection(true), vec![quad(-2.0, -2.0, 2.0)]);
        let tensor = vec![HPoint::weighted(Vec3::ZERO, 1.0); 1089];
        assert!(s.occludes_hull(&tensor, &mut 10_000_000).unwrap());
        let mut mixed = hull(0.0);
        mixed.push(HPoint::weighted(Vec3::new(0.0, -21.0, 0.0), 1.0));
        assert!(!s.occludes_hull(&mixed, &mut 10_000_000).unwrap());
        assert!(s.occludes_hull(&[HPoint::weighted(Vec3::ZERO, -1.0)], &mut 10_000).is_err());
        assert!(s.occludes_hull(&hull(0.0), &mut 0).is_err());
        assert!(s.visible(Vec3::ZERO, &mut 0).is_err());
        let mut unprepared = Snapshot {
            proj: s.proj,
            chart: Chart::new(&s.proj).unwrap(),
            sources: Vec::new(),
            fit: 0.01,
            tree: None,
            own_rays: Default::default(),
            memo_bypass: Default::default(),
        };
        unprepared.sources.push(Source { mesh: Arc::new(quad(-2.0, -2.0, 2.0)), index: Arc::new(MeshIndex::default()), own: true });
        assert!(unprepared.prepare(&mut 0).is_err());
        assert!(unprepared.tree.is_none());
    }
    #[test]
    fn captured_visibility_matches_surface_visible_for_own_and_opaque_context() {
        let mut app = SolveApp::new(Default::default(), Default::default());
        app.session.execute("form.prototype.create", &serde_json::json!({"primitive":"face","segments":[1,1,1],"levels":0})).unwrap();
        let mesh = quad(-2.0, -2.0, 2.0);
        let form = app.session.forms_prototype.as_mut().unwrap();
        let surface = Arc::make_mut(&mut form.surface);
        surface.index = MeshIndex::new(&mesh);
        surface.mesh = mesh.clone();
        live_cad::install_occlusion_for_test(&mut app, mesh);
        let context_mesh = Arc::new(quad(-3.0, -0.2, 0.2));
        app.forms_prototype.context.entries.push(context::Entry {
            name: Some("Opaque".into()),
            index: Some(Arc::new(MeshIndex::new(&context_mesh))),
            mesh: context_mesh.clone(),
            color: None,
            exact: None,
            detached_companion: false,
        });
        for perspective in [false, true] {
            let proj = projection(perspective);
            let snapshot = Snapshot::capture(&app, &proj).unwrap();
            let (own_mesh, own_index) = live_cad::occlusion_snapshot(&app).unwrap();
            assert!(Arc::ptr_eq(&snapshot.sources[0].mesh, &own_mesh));
            assert!(Arc::ptr_eq(&snapshot.sources[0].index, &own_index));
            assert!(Arc::ptr_eq(&snapshot.sources[1].mesh, &context_mesh));
            assert!(Arc::ptr_eq(&snapshot.sources[1].index, app.forms_prototype.context.entries[0].index.as_ref().unwrap()));
            for point in [Vec3::ZERO, Vec3::new(0.0, -2.0, 0.0), Vec3::new(0.0, -3.0, 0.0), Vec3::new(3.0, 0.0, 0.0)] {
                assert_eq!(snapshot.visible(point, &mut 10_000).unwrap(), super::super::super::surface::visible(&app, &proj, point));
            }
        }
    }
}
