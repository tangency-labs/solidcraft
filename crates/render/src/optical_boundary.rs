//! Exact optical boundary predicates and a cooperative conservative contact hierarchy.
use crate::job;
use robust::{Coord, Coord3D, orient2d, orient3d};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
fn invalid(message: &str) -> job::JobError {
    job::JobError::new(job::ErrorCode::InvalidScene, message)
}
pub(crate) fn predicate_domain(points: &[Vec3]) -> job::Result<()> {
    let minimum = f64::from_bits((1023 - 128) << 52);
    if points.iter().flat_map(|p| [p.x, p.y, p.z]).any(|v| !v.is_finite() || v.abs() > 1e12 || (v != 0.0 && v.abs() < minimum)) {
        return Err(invalid("CPU exact glass certificate requires zero or coordinate magnitudes at least 2^-128 mm within the existing world bound"));
    }
    Ok(())
}
pub(crate) fn point(p: Vec3) -> Coord3D<f64> {
    Coord3D { x: p.x, y: p.y, z: p.z }
}
pub(crate) fn side(t: [Vec3; 3], p: Vec3) -> f64 {
    orient3d(point(t[0]), point(t[1]), point(t[2]), point(p))
}
pub(crate) fn key(p: Vec3) -> [u64; 3] {
    [p.x, p.y, p.z].map(|v| if v == 0.0 { 0 } else { v.to_bits() })
}

pub(crate) fn same(a: Vec3, b: Vec3) -> bool {
    key(a) == key(b)
}
pub(crate) fn projection(t: [Vec3; 3]) -> usize {
    for drop in 0..3 {
        let q = t.map(|p| project(p, drop));
        if orient2d(q[0], q[1], q[2]) != 0.0 {
            return drop;
        }
    }
    2
}
pub(crate) fn project(p: Vec3, drop: usize) -> Coord<f64> {
    match drop {
        0 => Coord { x: p.y, y: p.z },
        1 => Coord { x: p.z, y: p.x },
        _ => Coord { x: p.x, y: p.y },
    }
}
pub(crate) fn in_triangle(t: [Vec3; 3], p: Vec3, drop: usize) -> bool {
    let q = t.map(|p| project(p, drop));
    let p = project(p, drop);
    let s = [orient2d(q[0], q[1], p), orient2d(q[1], q[2], p), orient2d(q[2], q[0], p)];
    !s.iter().any(|v| *v < 0.0) || !s.iter().any(|v| *v > 0.0)
}
pub(crate) fn on_segment(a: Coord<f64>, b: Coord<f64>, p: Coord<f64>) -> bool {
    orient2d(a, b, p) == 0.0 && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}
pub(crate) fn opposite(a: f64, b: f64) -> bool {
    (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0)
}
pub(crate) fn shared_vertex(p: Vec3, a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    a.into_iter().any(|x| same(x, p)) && b.into_iter().any(|x| same(x, p))
}
pub(crate) fn coplanar_edge_bad(edges: [[Vec3; 2]; 2], triangles: [[Vec3; 3]; 2], allow_shared: bool, drop: usize) -> bool {
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
pub fn forbidden_contact(a: [Vec3; 3], b: [Vec3; 3], allow_shared: bool) -> bool {
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
pub fn origin_crossing(t: [Vec3; 3], origin: Vec3) -> job::Result<i8> {
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

#[derive(Clone, Copy)]
struct Box3 {
    lo: Vec3,
    hi: Vec3,
}
impl Default for Box3 {
    fn default() -> Self {
        Self { lo: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY), hi: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }
}
impl Box3 {
    fn add(&mut self, other: Self) {
        self.lo = self.lo.min(other.lo);
        self.hi = self.hi.max(other.hi);
    }
    fn overlaps(self, other: Self) -> bool {
        self.lo.x <= other.hi.x
            && other.lo.x <= self.hi.x
            && self.lo.y <= other.hi.y
            && other.lo.y <= self.hi.y
            && self.lo.z <= other.hi.z
            && other.lo.z <= self.hi.z
    }
    fn triangle(p: [Vec3; 3]) -> Self {
        let mut b = Self::default();
        for p in p {
            b.lo = b.lo.min(p);
            b.hi = b.hi.max(p);
        }
        b
    }
}
#[derive(Clone, Copy, Default)]
struct Key {
    code: u64,
    ordinal: usize,
}
#[derive(Clone, Copy)]
enum Build {
    Bounds(usize),
    Keys(usize),
    Sort,
    Leaves(usize),
    Branches(usize),
    Ready,
}
/// A bounded Morton hierarchy for exact boundary contact candidates. The hierarchy
/// never modifies the points used by predicates. Each build/query step processes
/// one record, merge item, node or primitive and checks cancellation.
pub struct ContactIndex {
    nodes: Vec<Box3>,
    order: Vec<Key>,
    scratch: Vec<Key>,
    leaves: usize,
    bounds: Box3,
    stage: Build,
    width: usize,
    begin: usize,
    left: usize,
    right: usize,
    end: usize,
    out: usize,
}
fn allocation() -> job::JobError {
    job::JobError::new(job::ErrorCode::AllocationFailed, "optical contact index allocation failed")
}
fn capacity() -> job::JobError {
    job::JobError::new(job::ErrorCode::LimitExceeded, "optical contact index exceeds its bounded representation")
}
fn vector<T: Clone>(count: usize, initial: T) -> job::Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| allocation())?;
    if v.capacity() != count {
        return Err(capacity());
    }
    v.resize(count, initial);
    Ok(v)
}
impl ContactIndex {
    pub fn required_bytes(count: usize) -> job::Result<u64> {
        if count > job::MAX_CPU_TRIANGLES {
            return Err(capacity());
        }
        let leaves = count.max(1).checked_next_power_of_two().ok_or_else(capacity)?;
        leaves
            .checked_mul(2)
            .and_then(|n| n.checked_mul(std::mem::size_of::<Box3>()))
            .and_then(|n| count.checked_mul(2 * std::mem::size_of::<Key>()).and_then(|m| n.checked_add(m)))
            .and_then(|n| n.checked_add(std::mem::size_of::<Self>() + std::mem::size_of::<ContactQuery>()))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(capacity)
    }
    pub fn new(count: usize) -> job::Result<Self> {
        Self::required_bytes(count)?;
        let leaves = count.max(1).checked_next_power_of_two().ok_or_else(capacity)?;
        Ok(Self {
            nodes: vector(leaves * 2, Box3::default())?,
            order: vector(count, Key::default())?,
            scratch: vector(count, Key::default())?,
            leaves,
            bounds: Box3::default(),
            stage: Build::Bounds(0),
            width: 1,
            begin: 0,
            left: 0,
            right: 0,
            end: 0,
            out: 0,
        })
    }
    pub fn step(&mut self, triangle: impl Fn(usize) -> Option<[Vec3; 3]>, cancel: &job::CancelToken) -> job::Result<bool> {
        cancel.check()?;
        match self.stage {
            Build::Bounds(i) => {
                if i == self.order.len() {
                    self.stage = Build::Keys(0);
                } else {
                    let p = triangle(i).ok_or_else(|| invalid("missing optical contact triangle"))?;
                    predicate_domain(&p)?;
                    self.bounds.add(Box3::triangle(p));
                    self.stage = Build::Bounds(i + 1);
                }
            }
            Build::Keys(i) => {
                if i == self.order.len() {
                    self.stage = Build::Sort;
                } else {
                    let b = Box3::triangle(triangle(i).ok_or_else(|| invalid("missing optical contact triangle"))?);
                    let c = (b.lo + b.hi) * 0.5;
                    let span = self.bounds.hi - self.bounds.lo;
                    let coordinate = |p: f64, lo: f64, s: f64| if s > 0.0 { (((p - lo) / s).clamp(0.0, 1.0) * 2097151.0) as u64 } else { 0 };
                    let q = [
                        coordinate(c.x, self.bounds.lo.x, span.x),
                        coordinate(c.y, self.bounds.lo.y, span.y),
                        coordinate(c.z, self.bounds.lo.z, span.z),
                    ];
                    let mut code = 0;
                    for bit in 0..21 {
                        for (axis, value) in q.iter().enumerate() {
                            code |= ((value >> bit) & 1) << (3 * bit + axis);
                        }
                    }
                    self.order[i] = Key { code, ordinal: i };
                    self.stage = Build::Keys(i + 1);
                }
            }
            Build::Sort => {
                let n = self.order.len();
                if self.width >= n {
                    self.stage = Build::Leaves(0);
                } else if self.begin >= n {
                    std::mem::swap(&mut self.order, &mut self.scratch);
                    self.width = self.width.saturating_mul(2);
                    self.begin = 0;
                    self.out = 0;
                } else {
                    if self.out == self.begin {
                        self.left = self.begin;
                        self.right = (self.begin + self.width).min(n);
                        self.end = (self.begin + self.width.saturating_mul(2)).min(n);
                    }
                    let middle = (self.begin + self.width).min(n);
                    let take_left = self.left < middle
                        && (self.right >= self.end
                            || (self.order[self.left].code, self.order[self.left].ordinal)
                                <= (self.order[self.right].code, self.order[self.right].ordinal));
                    self.scratch[self.out] = if take_left {
                        let v = self.order[self.left];
                        self.left += 1;
                        v
                    } else {
                        let v = self.order[self.right];
                        self.right += 1;
                        v
                    };
                    self.out += 1;
                    if self.out == self.end {
                        self.begin = self.end;
                    }
                }
            }
            Build::Leaves(i) => {
                if i == self.order.len() {
                    self.stage = Build::Branches(self.leaves - 1);
                } else {
                    self.nodes[self.leaves + i] =
                        Box3::triangle(triangle(self.order[i].ordinal).ok_or_else(|| invalid("missing optical contact leaf"))?);
                    self.stage = Build::Leaves(i + 1);
                }
            }
            Build::Branches(i) => {
                if i == 0 {
                    self.stage = Build::Ready;
                } else {
                    let mut b = self.nodes[2 * i];
                    b.add(self.nodes[2 * i + 1]);
                    self.nodes[i] = b;
                    self.stage = Build::Branches(i - 1);
                }
            }
            Build::Ready => return Ok(true),
        }
        Ok(matches!(self.stage, Build::Ready))
    }
    /// A fresh traversal of all triangles whose exact bounds overlap the query.
    pub fn query(&self, triangle: [Vec3; 3]) -> job::Result<ContactQuery> {
        if !matches!(self.stage, Build::Ready) {
            return Err(invalid("optical contact index is incomplete"));
        }
        Ok(ContactQuery { bounds: Box3::triangle(triangle), stack: [0; 32], length: 1 })
    }
    /// None means the traversal is complete. Some(None) consumed one branch;
    /// Some(Some(index)) is one conservative candidate, never a contact decision.
    pub fn query_step(&self, query: &mut ContactQuery, cancel: &job::CancelToken) -> job::Result<Option<Option<usize>>> {
        cancel.check()?;
        if query.length == 0 {
            return Ok(None);
        }
        query.length -= 1;
        let at = query.stack[query.length];
        let at = if at == 0 { 1 } else { at };
        let node = *self.nodes.get(at).ok_or_else(|| invalid("optical contact node"))?;
        if !node.overlaps(query.bounds) {
            return Ok(Some(None));
        }
        if at >= self.leaves {
            return Ok(Some(self.order.get(at - self.leaves).map(|k| k.ordinal)));
        }
        for child in [2 * at + 1, 2 * at] {
            *query.stack.get_mut(query.length).ok_or_else(capacity)? = child;
            query.length += 1;
        }
        Ok(Some(None))
    }
}
/// Fixed traversal storage, included in contact admission; no per-pair allocation.
pub struct ContactQuery {
    bounds: Box3,
    stack: [usize; 32],
    length: usize,
}
