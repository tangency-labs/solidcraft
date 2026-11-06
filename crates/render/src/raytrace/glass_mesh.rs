//! Cooperative certification of the exact f64 boundary consumed by CPU Path.
//! Coordinates are never tolerance-welded, snapped or repaired. Every shared edge must have
//! identical finite endpoints (ClosedMesh canonicalises signed zero; declared boxes
//! retain the legacy raw-bit edge policy); geometric contact is
//! permitted only along the same body's actual shared topological simplex.
use super::{Bounds, Triangle, WorkBudget};
use crate::{
    ProductScene,
    job::{self, CancelToken, DielectricBoundary, RaytraceOptions},
};
use robust::{Coord, Coord3D, orient2d, orient3d};
use solidcore_record::Vec3;
use std::mem::size_of;

fn invalid(message: &str) -> job::JobError {
    job::JobError::new(job::ErrorCode::InvalidScene, message)
}
fn memory() -> job::JobError {
    job::JobError::new(job::ErrorCode::LimitExceeded, "CPU glass certification memory exceeds admission")
}
fn reserved<T>(n: usize) -> job::Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(n).map_err(|_| job::JobError::new(job::ErrorCode::AllocationFailed, "CPU glass certification allocation failed"))?;
    if values.capacity() != n {
        return Err(memory());
    }
    Ok(values)
}
// Keep adaptive determinant expansions away from IEEE underflow. This is a
// conservative certificate domain, not a snap tolerance or a changed ray bound.
// With coordinates in [2^-128, 1e12] (or exactly zero), each nonzero f64
// difference is at least 2^-180 and the degree-three expansion products remain
// normal. Original legacy optics have no new coordinate restriction.
fn predicate_domain(points: &[Vec3]) -> job::Result<()> {
    let minimum = f64::from_bits((1023 - 128) << 52);
    if points.iter().flat_map(|p| [p.x, p.y, p.z]).any(|v| !v.is_finite() || v.abs() > super::MAX_COORDINATE_MM || (v != 0.0 && v.abs() < minimum)) {
        return Err(invalid("CPU exact glass certificate requires zero or coordinate magnitudes at least 2^-128 mm within the existing world bound"));
    }
    Ok(())
}
fn point(p: Vec3) -> Coord3D<f64> {
    Coord3D { x: p.x, y: p.y, z: p.z }
}
fn side(t: [Vec3; 3], p: Vec3) -> f64 {
    orient3d(point(t[0]), point(t[1]), point(t[2]), point(p))
}
fn key(p: Vec3) -> [u64; 3] {
    [p.x, p.y, p.z].map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}
fn target_key(p: Vec3, medium: usize, options: &RaytraceOptions) -> job::Result<[u64; 3]> {
    let input = options.dielectrics.get(medium).ok_or_else(|| invalid("CPU glass coordinate target"))?;
    Ok(if input.boundary == DielectricBoundary::ClosedBox { [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()] } else { key(p) })
}
fn same(a: Vec3, b: Vec3) -> bool {
    key(a) == key(b)
}
fn projection(t: [Vec3; 3]) -> usize {
    for drop in 0..3 {
        let q = t.map(|p| project(p, drop));
        if orient2d(q[0], q[1], q[2]) != 0.0 {
            return drop;
        }
    }
    2
}
fn project(p: Vec3, drop: usize) -> Coord<f64> {
    match drop {
        0 => Coord { x: p.y, y: p.z },
        1 => Coord { x: p.z, y: p.x },
        _ => Coord { x: p.x, y: p.y },
    }
}
fn in_triangle(t: [Vec3; 3], p: Vec3, drop: usize) -> bool {
    let q = t.map(|p| project(p, drop));
    let p = project(p, drop);
    let s = [orient2d(q[0], q[1], p), orient2d(q[1], q[2], p), orient2d(q[2], q[0], p)];
    !s.iter().any(|v| *v < 0.0) || !s.iter().any(|v| *v > 0.0)
}
fn on_segment(a: Coord<f64>, b: Coord<f64>, p: Coord<f64>) -> bool {
    orient2d(a, b, p) == 0.0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}
fn opposite(a: f64, b: f64) -> bool {
    (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0)
}
fn shared_vertex(p: Vec3, a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    a.into_iter().any(|x| same(x, p)) && b.into_iter().any(|x| same(x, p))
}
fn coplanar_edge_bad(edges: [[Vec3; 2]; 2], triangles: [[Vec3; 3]; 2], allow_shared: bool, drop: usize) -> bool {
    let [[a, b], [c, d]] = edges;
    let [ta, tb] = triangles;
    let [pa, pb, pc, pd] = [a, b, c, d].map(|p| project(p, drop));
    let s = [orient2d(pa, pb, pc), orient2d(pa, pb, pd), orient2d(pc, pd, pa), orient2d(pc, pd, pb)];
    if opposite(s[0], s[1]) && opposite(s[2], s[3]) {
        return true;
    }
    for (p, q, r) in [(a, pc, pd), (b, pc, pd), (c, pa, pb), (d, pa, pb)] {
        if on_segment(q, r, project(p, drop)) && (!allow_shared || !shared_vertex(p, ta, tb)) {
            return true;
        }
    }
    // If collinear overlap has positive length, it is legal only for the exact
    // shared topological edge, not two incident edges extending the same way.
    if s.into_iter().all(|v| v == 0.0) {
        let axis_x = (pa.x - pb.x).abs() >= (pa.y - pb.y).abs();
        let value = |p: Coord<f64>| if axis_x { p.x } else { p.y };
        let lo = value(pa).min(value(pb)).max(value(pc).min(value(pd)));
        let hi = value(pa).max(value(pb)).min(value(pc).max(value(pd)));
        if lo < hi && !(allow_shared && ((same(a, c) && same(b, d)) || (same(a, d) && same(b, c)))) {
            return true;
        }
    }
    false
}
/// True if the two exact triangles meet beyond their permitted shared simplex.
/// This uses signs of adaptive exact predicates; no distance/angle tolerance.
pub(super) fn forbidden_contact(a: [Vec3; 3], b: [Vec3; 3], allow_shared: bool) -> bool {
    let sa = a.map(|p| side(b, p));
    let sb = b.map(|p| side(a, p));
    if sa.iter().all(|v| *v > 0.0) || sa.iter().all(|v| *v < 0.0) || sb.iter().all(|v| *v > 0.0) || sb.iter().all(|v| *v < 0.0) {
        return false;
    }
    let common = a.iter().filter(|p| b.iter().any(|q| same(**p, *q))).count();
    if common == 3 {
        return true;
    }
    for (t, u, s) in [(a, b, sa), (b, a, sb)] {
        let drop = projection(u);
        for (p, sign) in t.into_iter().zip(s) {
            if sign == 0.0 && in_triangle(u, p, drop) && (!allow_shared || !shared_vertex(p, a, b)) {
                return true;
            }
        }
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            let p = t[i];
            let q = t[j];
            if opposite(s[i], s[j]) {
                let w = [side([p, q, u[0]], u[1]), side([p, q, u[1]], u[2]), side([p, q, u[2]], u[0])];
                if !w.iter().any(|v| *v < 0.0) || !w.iter().any(|v| *v > 0.0) {
                    return true;
                }
            } else if s[i] == 0.0 && s[j] == 0.0 {
                for (k, l) in [(0, 1), (1, 2), (2, 0)] {
                    if coplanar_edge_bad([[p, q], [u[k], u[l]]], [a, b], allow_shared, drop) {
                        return true;
                    }
                }
            }
        }
    }
    false
}
#[derive(Clone, Copy)]
struct Face {
    p: [Vec3; 3],
    medium: usize,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    medium: usize,
    a: [u64; 3],
    b: [u64; 3],
    face: usize,
    forward: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Region {
    pub bounds: Bounds,
    pub parent: Option<usize>,
    begin: usize,
    end: usize,
    pub primitive_begin: usize,
    pub primitive_end: usize,
}
impl Default for Region {
    fn default() -> Self {
        Self { bounds: Bounds::empty(), parent: None, begin: 0, end: 0, primitive_begin: 0, primitive_end: 0 }
    }
}
#[derive(Clone, Copy)]
enum Stage {
    Index,
    Load,
    BoxAxes,
    Edges,
    Sort,
    Pairs,
    Connected,
    CornerInit,
    CornerEdges,
    CornerRecords,
    VertexLinks,
    Contact,
    Sources,
    Volume,
    Parents,
    Convex,
    Inside,
    Done,
}
/// All vectors reserve their admitted exact capacity before preparation begins.
/// Sorting is an iterative merge: a continuation copies at most one edge.
pub(super) struct Task {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    scratch: Vec<Edge>,
    parents: Vec<usize>,
    ranks: Vec<u8>,
    regions: [Region; 8],
    count: usize,
    stage: Stage,
    vertex_sort: bool,
    medium: usize,
    triangle: usize,
    i: usize,
    j: usize,
    width: usize,
    begin: usize,
    left: usize,
    right: usize,
    end: usize,
    out: usize,
    pair: (usize, usize),
    vertex: usize,
    outer: usize,
    inner: usize,
    volume: super::glass_volume::SignedVolume,
}
fn selected_count(scene: &ProductScene, options: &RaytraceOptions) -> job::Result<usize> {
    let mut count = 0_usize;
    for input in &options.dielectrics {
        let body = scene.meshes.get(input.body_index).filter(|b| b.name == input.body_name).ok_or_else(|| invalid("CPU glass target changed"))?;
        if input.boundary != DielectricBoundary::ThinSheet {
            count = count.checked_add(body.mesh.triangles.len()).ok_or_else(memory)?;
        }
    }
    if count as u64 > options.budget.limits().max_triangles {
        return Err(memory());
    }
    Ok(count)
}
impl Task {
    pub(super) fn required_bytes(scene: &ProductScene, options: &RaytraceOptions) -> job::Result<u64> {
        let n = selected_count(scene, options)?;
        n.checked_mul(size_of::<Face>() + 6 * size_of::<Edge>() + 3 * size_of::<usize>() + 3 * size_of::<u8>())
            .and_then(|n| n.checked_add(size_of::<Self>()))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(memory)
    }
    pub(super) fn new(scene: &ProductScene, options: &RaytraceOptions) -> job::Result<Self> {
        options.validate()?;
        let n = selected_count(scene, options)?;
        let edges = n.checked_mul(3).ok_or_else(memory)?;
        Ok(Self {
            faces: reserved(n)?,
            edges: reserved(edges)?,
            scratch: reserved(edges)?,
            parents: reserved(edges)?,
            ranks: reserved(edges)?,
            regions: [Region::default(); 8],
            count: options.dielectrics.len(),
            stage: Stage::Index,
            vertex_sort: false,
            medium: 0,
            triangle: 0,
            i: 0,
            j: 1,
            width: 1,
            begin: 0,
            left: 0,
            right: 0,
            end: 0,
            out: 0,
            pair: (0, 1),
            vertex: 0,
            outer: 0,
            inner: 0,
            volume: Default::default(),
        })
    }
    pub(super) fn ready(&self) -> bool {
        matches!(self.stage, Stage::Done)
    }
    pub(super) fn region(&self, index: usize) -> job::Result<Region> {
        self.regions.get(index).copied().filter(|_| self.ready()).ok_or_else(|| invalid("CPU glass certificate missing"))
    }
    fn root(&self, mut n: usize) -> job::Result<usize> {
        // Union by rank bounds the tree by log2(number of corners). The explicit bound also protects
        // malformed private state; every parent chase is finite and cancellable
        // at the surrounding per-edge continuation.
        for _ in 0..64 {
            let p = *self.parents.get(n).ok_or_else(|| invalid("CPU glass component index"))?;
            if p == n {
                return Ok(n);
            }
            n = p;
        }
        Err(invalid("CPU glass component chain exceeds its bounded certificate"))
    }
    fn join(&mut self, ra: usize, rb: usize) -> job::Result<()> {
        if ra != rb {
            let ar = *self.ranks.get(ra).ok_or_else(|| invalid("CPU glass union rank"))?;
            let br = *self.ranks.get(rb).ok_or_else(|| invalid("CPU glass union rank"))?;
            let (keep, merge) = if ar < br { (rb, ra) } else { (ra, rb) };
            *self.parents.get_mut(merge).ok_or_else(|| invalid("CPU glass union index"))? = keep;
            if ar == br {
                let r = self.ranks.get_mut(keep).ok_or_else(|| invalid("CPU glass union rank"))?;
                *r = r.checked_add(1).ok_or_else(|| invalid("CPU glass union rank overflow"))?;
            }
        }
        Ok(())
    }
    fn next_pair(&mut self) {
        self.pair.1 += 1;
        if self.pair.1 >= self.faces.len() {
            self.pair.0 += 1;
            self.pair.1 = self.pair.0 + 1;
        }
    }
    pub(super) fn step(&mut self, scene: &ProductScene, options: &RaytraceOptions, work: &mut WorkBudget, cancel: &CancelToken) -> job::Result<()> {
        work.tick(cancel).map_err(super::image::error)?;
        match self.stage {
            Stage::Index => {
                if let Some(body) = scene.meshes.get(self.medium) {
                    for (index, input) in options.dielectrics.iter().enumerate() {
                        if input.body_index == self.medium {
                            let region = self.regions.get_mut(index).ok_or_else(|| invalid("CPU glass primitive range"))?;
                            region.primitive_begin = self.i;
                            region.primitive_end = self.i.checked_add(body.mesh.triangles.len()).ok_or_else(memory)?;
                            match input.boundary {
                                DielectricBoundary::ThinSheet => {
                                    super::dielectric::certify_sheet(&body.mesh).map_err(super::image::error)?;
                                }
                                DielectricBoundary::ClosedBox => {
                                    if !(12..=super::dielectric::MAX_BOX_TRIANGLES).contains(&body.mesh.triangles.len()) {
                                        return Err(invalid("unsupported dielectric boundary tessellation"));
                                    }
                                }
                                DielectricBoundary::ClosedMesh => {}
                            }
                        }
                    }
                    self.i = self.i.checked_add(body.mesh.triangles.len()).ok_or_else(memory)?;
                    self.medium += 1;
                } else {
                    self.i = 0;
                    self.medium = 0;
                    self.stage = Stage::Load;
                }
            }
            Stage::Load => {
                let Some(input) = options.dielectrics.get(self.medium) else {
                    self.i = 0;
                    self.stage = if options.dielectrics.iter().any(|input| input.boundary == DielectricBoundary::ClosedBox) {
                        Stage::BoxAxes
                    } else {
                        Stage::Edges
                    };
                    return Ok(());
                };
                let body =
                    scene.meshes.get(input.body_index).filter(|b| b.name == input.body_name).ok_or_else(|| invalid("CPU glass target changed"))?;
                let region = self.regions.get_mut(self.medium).ok_or_else(|| invalid("CPU glass region index"))?;
                if self.triangle == 0 {
                    region.begin = self.faces.len();
                }
                let Some(indices) = body.mesh.triangles.get(self.triangle) else {
                    region.end = self.faces.len();
                    if input.boundary != DielectricBoundary::ThinSheet && region.end == region.begin {
                        return Err(invalid("closed CPU glass has no triangles"));
                    }
                    self.medium += 1;
                    self.triangle = 0;
                    return Ok(());
                };
                let mut p = [Vec3::ZERO; 3];
                for (v, index) in p.iter_mut().zip(indices) {
                    *v = *body.mesh.positions.get(*index as usize).ok_or_else(|| invalid("CPU glass vertex index"))?;
                }
                predicate_domain(&p)?;
                if Triangle::new(p).map_err(super::image::error)?.geometric_normal().is_none() {
                    return Err(invalid("CPU glass boundary contains a degenerate triangle"));
                }
                region.bounds = region.bounds.include(Triangle::new(p).map_err(super::image::error)?.bounds());
                if input.boundary != DielectricBoundary::ThinSheet {
                    if self.faces.len() == self.faces.capacity() {
                        return Err(memory());
                    }
                    self.parents.push(self.faces.len());
                    self.ranks.push(0);
                    self.faces.push(Face { p, medium: self.medium });
                }
                self.triangle += 1;
            }
            Stage::BoxAxes => {
                if let Some(face) = self.faces.get(self.i) {
                    let input = options.dielectrics.get(face.medium).ok_or_else(|| invalid("CPU box plane target"))?;
                    if input.boundary == DielectricBoundary::ClosedBox {
                        let normal = Triangle::new(face.p).map_err(super::image::error)?.geometric_normal().ok_or_else(|| invalid("box normal"))?;
                        let axis = if normal.x.abs() == 1.0 {
                            0
                        } else if normal.y.abs() == 1.0 {
                            1
                        } else if normal.z.abs() == 1.0 {
                            2
                        } else {
                            return Err(invalid("box face is not axis aligned"));
                        };
                        let bounds = self.regions.get(face.medium).ok_or_else(|| invalid("CPU box plane bounds"))?.bounds;
                        let positive = super::component(normal, axis) > 0.0;
                        // Region bounds union Triangle::bounds, whose finite extrema
                        // each take exactly one outward representable step. Undo
                        // only that padding for the raw face-plane comparison;
                        // keep the stored bounds and raw endpoint edge keys intact.
                        // The inverse is value-exact, including subnormals. ±0
                        // compare equally here; signed-zero topology still uses
                        // the original referenced coordinates' unchanged bits.
                        let padded = super::component(if positive { bounds.max() } else { bounds.min() }, axis);
                        let plane = if positive { padded.next_down() } else { padded.next_up() };
                        if face.p.iter().any(|p| super::component(*p, axis) != plane) {
                            return Err(invalid("box winding or face plane"));
                        }
                        self.i += 1;
                    } else {
                        self.i = self.regions.get(face.medium).ok_or_else(|| invalid("CPU box skip region"))?.end;
                    }
                } else {
                    self.i = 0;
                    self.stage = Stage::Edges;
                }
            }
            Stage::Edges => {
                if let Some(face) = self.faces.get(self.i) {
                    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                        let a = target_key(face.p[a], face.medium, options)?;
                        let b = target_key(face.p[b], face.medium, options)?;
                        if a == b {
                            return Err(invalid("CPU glass collapsed edge"));
                        }
                        self.edges.push(Edge { medium: face.medium, a: a.min(b), b: a.max(b), face: self.i, forward: a < b });
                        self.scratch.push(Edge::default());
                    }
                    self.i += 1;
                } else {
                    self.stage = Stage::Sort;
                    self.begin = 0;
                    self.out = 0;
                }
            }
            Stage::Sort => {
                if self.width >= self.edges.len() {
                    self.stage = if self.vertex_sort { Stage::VertexLinks } else { Stage::Pairs };
                    self.i = 0;
                    return Ok(());
                }
                if self.out == self.begin {
                    self.left = self.begin;
                    self.right = (self.begin + self.width).min(self.edges.len());
                    self.end = (self.right + self.width).min(self.edges.len());
                }
                let middle = (self.begin + self.width).min(self.edges.len());
                let take_left = self.left < middle && (self.right >= self.end || self.edges.get(self.left) <= self.edges.get(self.right));
                let at = if take_left {
                    let n = self.left;
                    self.left += 1;
                    n
                } else {
                    let n = self.right;
                    self.right += 1;
                    n
                };
                let value = *self.edges.get(at).ok_or_else(|| invalid("CPU glass merge input"))?;
                *self.scratch.get_mut(self.out).ok_or_else(|| invalid("CPU glass merge output"))? = value;
                self.out += 1;
                if self.out == self.end {
                    self.begin = self.end;
                    if self.begin == self.edges.len() {
                        std::mem::swap(&mut self.edges, &mut self.scratch);
                        self.width = self.width.checked_mul(2).ok_or_else(memory)?;
                        self.begin = 0;
                        self.out = 0;
                    }
                }
            }
            Stage::Pairs => {
                let Some(a) = self.edges.get(self.i).copied() else {
                    self.stage = Stage::Connected;
                    self.i = 0;
                    return Ok(());
                };
                let b = *self.edges.get(self.i + 1).ok_or_else(|| invalid("CPU glass requires closed edges"))?;
                let group = |x: Edge| (x.medium, x.a, x.b);
                if group(a) != group(b) || a.forward == b.forward || self.edges.get(self.i + 2).is_some_and(|x| group(*x) == group(a)) {
                    return Err(invalid("CPU glass requires exactly two oppositely wound faces per edge"));
                }
                let ra = self.root(a.face)?;
                let rb = self.root(b.face)?;
                self.join(ra, rb)?;
                self.i += 2;
            }
            Stage::Connected => {
                if let Some(face) = self.faces.get(self.i) {
                    let region = self.regions.get(face.medium).ok_or_else(|| invalid("CPU glass region missing"))?;
                    if self.root(self.i)? != self.root(region.begin)? {
                        return Err(invalid("each CPU closed glass body must contain one connected shell"));
                    }
                    self.i += 1;
                } else {
                    self.stage = Stage::CornerInit;
                    self.i = 0;
                }
            }
            Stage::CornerInit => {
                let count = self.faces.len().checked_mul(3).ok_or_else(memory)?;
                if self.i == count {
                    self.stage = Stage::CornerEdges;
                    self.i = 0;
                    self.vertex = 0;
                    return Ok(());
                }
                if let Some(parent) = self.parents.get_mut(self.i) {
                    *parent = self.i;
                } else {
                    self.parents.push(self.i);
                }
                if let Some(rank) = self.ranks.get_mut(self.i) {
                    *rank = 0;
                } else {
                    self.ranks.push(0);
                }
                self.i += 1;
            }
            Stage::CornerEdges => {
                let Some(a) = self.edges.get(self.i).copied() else {
                    self.stage = Stage::CornerRecords;
                    self.i = 0;
                    return Ok(());
                };
                let b = *self.edges.get(self.i + 1).ok_or_else(|| invalid("CPU glass corner edge pair"))?;
                let coordinate = if self.vertex == 0 { a.a } else { a.b };
                let corner = |edge: Edge| -> job::Result<usize> {
                    let face = self.faces.get(edge.face).ok_or_else(|| invalid("CPU glass corner face"))?;
                    let mut local = None;
                    for (index, p) in face.p.into_iter().enumerate() {
                        if target_key(p, face.medium, options)? == coordinate {
                            local = Some(index);
                            break;
                        }
                    }
                    let local = local.ok_or_else(|| invalid("CPU glass corner endpoint"))?;
                    edge.face.checked_mul(3).and_then(|n| n.checked_add(local)).ok_or_else(memory)
                };
                let ra = self.root(corner(a)?)?;
                let rb = self.root(corner(b)?)?;
                self.join(ra, rb)?;
                self.vertex += 1;
                if self.vertex == 2 {
                    self.vertex = 0;
                    self.i += 2;
                }
            }
            Stage::CornerRecords => {
                let Some(face) = self.faces.get(self.i / 3).copied() else {
                    self.vertex_sort = true;
                    self.width = 1;
                    self.begin = 0;
                    self.out = 0;
                    self.stage = Stage::Sort;
                    return Ok(());
                };
                let coordinate = target_key(*face.p.get(self.i % 3).ok_or_else(|| invalid("CPU glass corner position"))?, face.medium, options)?;
                *self.edges.get_mut(self.i).ok_or_else(|| invalid("CPU glass corner record"))? =
                    Edge { medium: face.medium, a: coordinate, b: [0; 3], face: self.i, forward: false };
                self.i += 1;
            }
            Stage::VertexLinks => {
                let Some(a) = self.edges.get(self.i).copied() else {
                    self.stage = Stage::Contact;
                    self.pair = (0, 1);
                    return Ok(());
                };
                if let Some(b) = self.i.checked_sub(1).and_then(|i| self.edges.get(i)).copied()
                    && (a.medium, a.a) == (b.medium, b.a)
                    && self.root(a.face)? != self.root(b.face)?
                {
                    return Err(invalid("CPU glass vertex link must be one connected cycle; pinched or touching fans are unsupported"));
                }
                self.i += 1;
            }
            Stage::Contact => {
                let Some(a) = self.faces.get(self.pair.0).copied() else {
                    self.stage = Stage::Sources;
                    self.medium = 0;
                    self.triangle = 0;
                    self.vertex = 0;
                    return Ok(());
                };
                let Some(b) = self.faces.get(self.pair.1).copied() else {
                    self.next_pair();
                    return Ok(());
                };
                let ba = Triangle::new(a.p).map_err(super::image::error)?.bounds();
                let bb = Triangle::new(b.p).map_err(super::image::error)?.bounds();
                if !disjoint(ba, bb) && forbidden_contact(a.p, b.p, a.medium == b.medium) {
                    return Err(invalid("CPU glass boundaries intersect, touch, overlap or self-intersect"));
                }
                self.next_pair();
            }
            Stage::Sources => {
                let Some(input) = options.dielectrics.get(self.medium) else {
                    self.stage = Stage::Volume;
                    self.medium = 0;
                    self.vertex = 0;
                    return Ok(());
                };
                let body = scene.meshes.get(input.body_index).ok_or_else(|| invalid("CPU glass source-contact body"))?;
                let Some(indices) = body.mesh.triangles.get(self.triangle) else {
                    self.medium += 1;
                    self.triangle = 0;
                    self.vertex = 0;
                    return Ok(());
                };
                let mut p = [Vec3::ZERO; 3];
                for (p, index) in p.iter_mut().zip(indices) {
                    *p = *body.mesh.positions.get(*index as usize).ok_or_else(|| invalid("CPU glass source-contact vertex"))?;
                }
                let areas = options.area_lights.as_ref().map_or(0, Vec::len);
                if let Some(light) = options.area_lights.as_ref().and_then(|lights| lights.get(self.vertex)) {
                    let vector = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
                    let c = vector(light.center_mm);
                    let u = vector(light.edge_u_mm) * 0.5;
                    let v = vector(light.edge_v_mm) * 0.5;
                    let q = [c - u - v, c + u - v, c + u + v, c - u + v];
                    predicate_domain(&q)?;
                    if forbidden_contact(p, [q[0], q[1], q[2]], false) || forbidden_contact(p, [q[0], q[2], q[3]], false) {
                        return Err(invalid("CPU glass cannot touch an analytic area emitter"));
                    }
                    self.vertex += 1;
                } else if let Some(light) =
                    self.vertex.checked_sub(areas).and_then(|at| options.punctual_lights.as_ref().and_then(|lights| lights.get(at)))
                {
                    let q = Vec3::new(light.position_mm[0], light.position_mm[1], light.position_mm[2]);
                    predicate_domain(&[q])?;
                    if side(p, q) == 0.0 && in_triangle(p, q, projection(p)) {
                        return Err(invalid("CPU point or spot source lies on a glass boundary"));
                    }
                    self.vertex += 1;
                } else {
                    self.triangle += 1;
                    self.vertex = 0;
                }
            }
            Stage::Volume => {
                let Some(input) = options.dielectrics.get(self.medium) else {
                    self.stage = Stage::Parents;
                    self.outer = 0;
                    self.inner = 1;
                    return Ok(());
                };
                let region = *self.regions.get(self.medium).ok_or_else(|| invalid("CPU exact glass volume region"))?;
                if input.boundary == DielectricBoundary::ThinSheet {
                    self.medium += 1;
                    return Ok(());
                }
                if self.vertex == 0 {
                    self.i = region.begin;
                    self.volume = Default::default();
                    self.vertex = 1;
                }
                if self.i == region.end {
                    if !self.volume.positive() {
                        return Err(invalid("CPU glass must enclose positive exact outward-oriented volume"));
                    }
                    self.medium += 1;
                    self.vertex = 0;
                    return Ok(());
                }
                let p = self.faces.get(self.i).ok_or_else(|| invalid("CPU exact glass volume face"))?.p;
                self.volume.add_term(p, self.vertex - 1)?;
                self.vertex += 1;
                if self.vertex == 7 {
                    self.vertex = 1;
                    self.i += 1;
                }
            }
            Stage::Parents => {
                if self.outer >= self.count {
                    self.finish();
                    return Ok(());
                }
                if self.inner >= self.count {
                    self.outer += 1;
                    self.inner = self.outer + 1;
                    return Ok(());
                }
                let a = self.regions[self.outer];
                let b = self.regions[self.inner];
                if disjoint(a.bounds, b.bounds) {
                    self.inner += 1;
                    return Ok(());
                }
                let oa = options.dielectrics.get(self.outer).ok_or_else(|| invalid("CPU glass outer options"))?;
                let ob = options.dielectrics.get(self.inner).ok_or_else(|| invalid("CPU glass inner options"))?;
                if oa.boundary == DielectricBoundary::ThinSheet || ob.boundary == DielectricBoundary::ThinSheet {
                    return Err(invalid("CPU thin sheets may not touch or overlap a closed medium's bounds"));
                }
                let (parent, child) = if contains(a.bounds, b.bounds) {
                    (self.outer, self.inner)
                } else if contains(b.bounds, a.bounds) {
                    (self.inner, self.outer)
                } else {
                    return Err(invalid("CPU glass overlapping bounds have no conservative nesting certificate"));
                };
                self.pair = (parent, child);
                self.i = self.regions[parent].begin;
                self.j = self.regions[parent].begin;
                self.vertex = 0;
                self.stage = Stage::Convex;
            }
            Stage::Convex | Stage::Inside => {
                let (parent, child) = self.pair;
                let pr = self.regions[parent];
                let cr = self.regions[child];
                let inside = matches!(self.stage, Stage::Inside);
                let points = if inside { cr } else { pr };
                if self.i >= pr.end {
                    if !inside {
                        self.stage = Stage::Inside;
                        self.i = pr.begin;
                        self.j = cr.begin;
                        self.vertex = 0;
                        return Ok(());
                    }
                    let old = self.regions[child].parent;
                    if old.is_none_or(|old| contains(self.regions[old].bounds, pr.bounds)) {
                        self.regions[child].parent = Some(parent);
                    }
                    self.inner += 1;
                    self.stage = Stage::Parents;
                    return Ok(());
                }
                let face = self.faces.get(self.i).ok_or_else(|| invalid("CPU glass nesting face"))?;
                let p = self.faces.get(self.j).and_then(|f| f.p.get(self.vertex)).copied().ok_or_else(|| invalid("CPU glass nesting point"))?;
                let sign = side(face.p, p);
                if (inside && sign <= 0.0) || (!inside && sign < 0.0) {
                    return Err(invalid("CPU nested glass requires a convex outward parent and strictly contained child"));
                }
                self.vertex += 1;
                if self.vertex == 3 {
                    self.vertex = 0;
                    self.j += 1;
                }
                if self.j >= points.end {
                    self.i += 1;
                    self.j = points.begin;
                }
            }
            Stage::Done => return Err(invalid("CPU glass certification already complete")),
        }
        Ok(())
    }
    fn finish(&mut self) {
        self.stage = Stage::Done;
        self.faces = Vec::new();
        self.edges = Vec::new();
        self.scratch = Vec::new();
        self.parents = Vec::new();
        self.ranks = Vec::new();
    }
}
fn disjoint(a: Bounds, b: Bounds) -> bool {
    let a0 = a.min();
    let a1 = a.max();
    let b0 = b.min();
    let b1 = b.max();
    a1.x < b0.x || b1.x < a0.x || a1.y < b0.y || b1.y < a0.y || a1.z < b0.z || b1.z < a0.z
}
fn contains(a: Bounds, b: Bounds) -> bool {
    let a0 = a.min();
    let a1 = a.max();
    let b0 = b.min();
    let b1 = b.max();
    a0.x <= b0.x && a0.y <= b0.y && a0.z <= b0.z && a1.x >= b1.x && a1.y >= b1.y && a1.z >= b1.z
}

/// Source-plane exclusion is limited to the producing body and the exact f64
/// plane, so a parallel opposite interface or another body's occluder remains.
pub(super) fn same_plane(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    b.into_iter().all(|p| side(a, p) == 0.0)
}

/// Exact positive-X oriented crossing with half-open projected edge ownership.
/// Shared projected edges with equal normal signs contribute once; silhouettes
/// contribute opposite signs (or neither). Thus a closed oriented manifold has
/// winding0 outside and1 inside, including rays through edges/vertices. A point
/// actually on the surface is refused instead of assigned to an arbitrary side.
pub(super) fn origin_crossing(t: [Vec3; 3], origin: Vec3) -> job::Result<i8> {
    predicate_domain(&[origin])?;
    let sign = side(t, origin);
    if sign == 0.0 && in_triangle(t, origin, projection(t)) {
        return Err(invalid("CPU camera or pupil origin lies exactly on a Glass boundary"));
    }
    let mut q = t.map(|p| project(p, 0));
    let p = project(origin, 0);
    let area = orient2d(q[0], q[1], q[2]);
    if area == 0.0 || sign == 0.0 || opposite(sign, area) {
        return Ok(0);
    }
    let contribution = if area > 0.0 { 1 } else { -1 };
    if area < 0.0 {
        q.swap(1, 2);
    }
    for (a, b) in [(0, 1), (1, 2), (2, 0)] {
        let a = q[a];
        let b = q[b];
        let edge = orient2d(a, b, p);
        let owns = b.y > a.y || (b.y == a.y && b.x < a.x);
        if edge < 0.0 || (edge == 0.0 && !owns) {
            return Ok(0);
        }
    }
    Ok(contribution)
}

/// Exact contact between a finite pre-clip segment and one optical triangle.
/// Touches, endpoints and coplanar stretches count: none is silently classified
/// as a constant medium. The existing camera's clipped projection is not changed.
pub(super) fn origin_segment_contact(t: [Vec3; 3], a: Vec3, b: Vec3) -> job::Result<bool> {
    predicate_domain(&[a, b])?;
    let sa = side(t, a);
    let sb = side(t, b);
    if (sa > 0.0 && sb > 0.0) || (sa < 0.0 && sb < 0.0) {
        return Ok(false);
    }
    let drop = projection(t);
    if (sa == 0.0 && in_triangle(t, a, drop)) || (sb == 0.0 && in_triangle(t, b, drop)) {
        return Ok(true);
    }
    if opposite(sa, sb) {
        let signs = [side([a, b, t[0]], t[1]), side([a, b, t[1]], t[2]), side([a, b, t[2]], t[0])];
        return Ok(!signs.iter().any(|s| *s < 0.0) || !signs.iter().any(|s| *s > 0.0));
    }
    if sa == 0.0 && sb == 0.0 {
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            if coplanar_edge_bad([[a, b], [t[i], t[j]]], [t, t], false, drop) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A scattered origin must not round back onto its producing/endpoint plane.
/// This conservative precision check uses the same exact predicate domain as
/// the certificate; it never nudges a point to manufacture an interior sample.
pub(super) fn origin_on_plane(triangle: [Vec3; 3], point: Vec3) -> job::Result<bool> {
    predicate_domain(&triangle)?;
    predicate_domain(&[point])?;
    Ok(side(triangle, point) == 0.0)
}
