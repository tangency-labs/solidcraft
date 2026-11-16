//! The tiled-box proof as bounded, cancellable continuations. Its geometry and
//! raw-bit edge policy match the independent synchronous dielectric certifier.
use super::{Bounds, Result, TraceError, Triangle, WorkBudget};
use crate::job::CancelToken;
use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;
use std::{cmp::Ordering, mem::size_of};

type Edge = ([u64; 3], [u64; 3]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Load,
    Edges,
    Sort,
    Duplicates,
    Reverse,
    Area,
    Done,
}

fn reserve<T>(count: usize) -> Result<Vec<T>> {
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| TraceError::AllocationFailed)?;
    if values.capacity() != count {
        return Err(TraceError::MemoryLimit);
    }
    Ok(values)
}

/// No source references or certificate may escape independently: the owning
/// image/preparation supplies its same immutable mesh at each continuation.
pub(super) struct BoxTask {
    triangles: Vec<Triangle>,
    edges: Vec<Edge>,
    scratch: Vec<Edge>,
    count: usize,
    stage: Stage,
    lo: Vec3,
    hi: Vec3,
    area: f64,
    i: usize,
    width: usize,
    begin: usize,
    left: usize,
    right: usize,
    end: usize,
    out: usize,
    searching: bool,
    search_lo: usize,
    search_hi: usize,
}

impl BoxTask {
    pub(super) fn required_bytes(count: usize) -> Result<u64> {
        if !(12..=super::dielectric::MAX_BOX_TRIANGLES).contains(&count) {
            return Err(TraceError::InvalidInput("unsupported dielectric boundary tessellation"));
        }
        count
            .checked_mul(size_of::<Triangle>() + 6 * size_of::<Edge>())
            .and_then(|bytes| bytes.checked_add(size_of::<Self>()))
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(TraceError::MemoryLimit)
    }

    /// Caller has admitted and, for a host preparation, reserved required_bytes
    /// before this fallible allocation. No allocator growth occurs during step.
    pub(super) fn new(count: usize) -> Result<Self> {
        Self::required_bytes(count)?;
        let edges = count.checked_mul(3).ok_or(TraceError::MemoryLimit)?;
        Ok(Self {
            triangles: reserve(count)?,
            edges: reserve(edges)?,
            scratch: reserve(edges)?,
            count,
            stage: Stage::Load,
            lo: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            hi: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            area: 0.0,
            i: 0,
            width: 1,
            begin: 0,
            left: 0,
            right: 0,
            end: 0,
            out: 0,
            searching: false,
            search_lo: 0,
            search_hi: 0,
        })
    }

    pub(super) fn result(&self) -> Result<Bounds> {
        if self.stage != Stage::Done {
            return Err(TraceError::BuildIncomplete);
        }
        Bounds::new(self.lo, self.hi)
    }

    /// Reuse the prepaid largest-target buffers; clearing Copy elements does not
    /// run an input-sized destructor loop or allocate another target's scratch.
    pub(super) fn reset(&mut self, count: usize) -> Result<()> {
        Self::required_bytes(count)?;
        let edges = count.checked_mul(3).ok_or(TraceError::MemoryLimit)?;
        if count > self.triangles.capacity() || edges > self.edges.capacity() || edges > self.scratch.capacity() {
            return Err(TraceError::MemoryLimit);
        }
        self.triangles.clear();
        self.edges.clear();
        self.scratch.clear();
        self.count = count;
        self.stage = Stage::Load;
        self.lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        self.hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        self.area = 0.0;
        self.i = 0;
        self.width = 1;
        self.begin = 0;
        self.left = 0;
        self.right = 0;
        self.end = 0;
        self.out = 0;
        self.searching = false;
        self.search_lo = 0;
        self.search_hi = 0;
        Ok(())
    }

    /// One charged triangle, edge copy/comparison, or fixed-state transition.
    /// Cancellation/exhaustion debit precedes every state or buffer mutation.
    pub(super) fn step(&mut self, mesh: &Mesh, work: &mut WorkBudget, cancel: &CancelToken) -> Result<bool> {
        if self.stage == Stage::Done {
            return super::check_cancel(cancel).map(|()| true);
        }
        work.tick(cancel)?;
        if mesh.triangles.len() != self.count {
            return Err(TraceError::InvalidInput("dielectric captured mesh changed"));
        }
        match self.stage {
            Stage::Load => {
                if let Some(indices) = mesh.triangles.get(self.i) {
                    let triangle = super::dielectric::triangle(mesh, indices)?;
                    for p in triangle.vertices() {
                        self.lo = self.lo.min(p);
                        self.hi = self.hi.max(p);
                    }
                    if self.triangles.len() == self.triangles.capacity() {
                        return Err(TraceError::MemoryLimit);
                    }
                    self.triangles.push(triangle);
                    self.i += 1;
                } else {
                    if self.lo.x >= self.hi.x || self.lo.y >= self.hi.y || self.lo.z >= self.hi.z {
                        return Err(TraceError::InvalidInput("closed box needs positive extents"));
                    }
                    Bounds::new(self.lo, self.hi)?;
                    self.i = 0;
                    self.stage = Stage::Edges;
                }
            }
            Stage::Edges => {
                if let Some(triangle) = self.triangles.get(self.i) {
                    let vertices = triangle.vertices();
                    let normal = triangle.geometric_normal().ok_or(TraceError::InvalidInput("box normal"))?;
                    let axis = if normal.x.abs() == 1.0 {
                        0
                    } else if normal.y.abs() == 1.0 {
                        1
                    } else if normal.z.abs() == 1.0 {
                        2
                    } else {
                        return Err(TraceError::InvalidInput("box face is not axis aligned"));
                    };
                    let positive = super::component(normal, axis) > 0.0;
                    let plane = super::component(if positive { self.hi } else { self.lo }, axis);
                    if vertices.iter().any(|p| super::component(*p, axis) != plane) {
                        return Err(TraceError::InvalidInput("box winding or face plane"));
                    }
                    let [a, b, c] = vertices;
                    self.area += (b - a).cross(c - a).len() * 0.5;
                    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
                    if self.edges.capacity().saturating_sub(self.edges.len()) < 3 || self.scratch.capacity().saturating_sub(self.scratch.len()) < 3 {
                        return Err(TraceError::MemoryLimit);
                    }
                    self.edges.extend([(key(a), key(b)), (key(b), key(c)), (key(c), key(a))]);
                    self.scratch.extend([Edge::default(); 3]);
                    self.i += 1;
                } else {
                    self.i = 0;
                    self.stage = Stage::Sort;
                }
            }
            Stage::Sort => {
                if self.width >= self.edges.len() {
                    self.i = 0;
                    self.stage = Stage::Duplicates;
                    return Ok(false);
                }
                if self.out == self.begin {
                    self.left = self.begin;
                    self.right = (self.begin + self.width).min(self.edges.len());
                    self.end = (self.right + self.width).min(self.edges.len());
                }
                let middle = (self.begin + self.width).min(self.edges.len());
                let take_left = self.left < middle && (self.right >= self.end || self.edges.get(self.left) <= self.edges.get(self.right));
                let at = if take_left {
                    let at = self.left;
                    self.left += 1;
                    at
                } else {
                    let at = self.right;
                    self.right += 1;
                    at
                };
                let value = *self.edges.get(at).ok_or(TraceError::Internal("box merge input"))?;
                *self.scratch.get_mut(self.out).ok_or(TraceError::Internal("box merge output"))? = value;
                self.out += 1;
                if self.out == self.end {
                    self.begin = self.end;
                    if self.begin == self.edges.len() {
                        std::mem::swap(&mut self.edges, &mut self.scratch);
                        self.width = self.width.checked_mul(2).ok_or(TraceError::MemoryLimit)?;
                        self.begin = 0;
                        self.out = 0;
                    }
                }
            }
            Stage::Duplicates => {
                if let Some(next) = self.edges.get(self.i + 1) {
                    if self.edges.get(self.i) == Some(next) {
                        return Err(TraceError::InvalidInput("incomplete or ambiguous closed box face"));
                    }
                    self.i += 1;
                } else {
                    self.i = 0;
                    self.stage = Stage::Reverse;
                }
            }
            Stage::Reverse => {
                let Some((a, b)) = self.edges.get(self.i) else {
                    self.stage = Stage::Area;
                    return Ok(false);
                };
                if !self.searching {
                    self.search_lo = 0;
                    self.search_hi = self.edges.len();
                    self.searching = true;
                } else if self.search_lo < self.search_hi {
                    let mid = self.search_lo + (self.search_hi - self.search_lo) / 2;
                    match self.edges.get(mid).ok_or(TraceError::Internal("box reverse edge search"))?.cmp(&(*b, *a)) {
                        Ordering::Less => self.search_lo = mid + 1,
                        Ordering::Greater => self.search_hi = mid,
                        Ordering::Equal => {
                            self.searching = false;
                            self.i += 1;
                        }
                    }
                } else {
                    return Err(TraceError::InvalidInput("incomplete or ambiguous closed box face"));
                }
            }
            Stage::Area => {
                let size = self.hi - self.lo;
                let boundary = 2.0 * (size.x * size.y + size.y * size.z + size.z * size.x);
                if self.area.partial_cmp(&(1.5 * boundary)) != Some(Ordering::Less) {
                    return Err(TraceError::InvalidInput("closed box boundary covered more than once"));
                }
                self.stage = Stage::Done;
            }
            Stage::Done => {}
        }
        Ok(self.stage == Stage::Done)
    }

    #[cfg(test)]
    pub(super) fn allocated_bytes(&self) -> u64 {
        (size_of::<Self>()
            + self.triangles.capacity() * size_of::<Triangle>()
            + (self.edges.capacity() + self.scratch.capacity()) * size_of::<Edge>()) as u64
    }

    #[cfg(test)]
    pub(super) fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (
            self.stage as u8,
            self.count,
            self.i,
            self.width,
            self.begin,
            self.left,
            self.right,
            self.end,
            self.out,
            self.searching,
            self.search_lo,
            self.search_hi,
        )
            .hash(&mut hash);
        [self.lo.x, self.lo.y, self.lo.z, self.hi.x, self.hi.y, self.hi.z, self.area].map(f64::to_bits).hash(&mut hash);
        for triangle in &self.triangles {
            for p in triangle.vertices() {
                [p.x, p.y, p.z].map(f64::to_bits).hash(&mut hash);
            }
        }
        self.edges.hash(&mut hash);
        self.scratch.hash(&mut hash);
        hash.finish()
    }

    #[cfg(test)]
    pub(super) fn phase(&self) -> &'static str {
        match self.stage {
            Stage::Load => "load",
            Stage::Edges => "edges",
            Stage::Sort => "sort",
            Stage::Duplicates => "duplicates",
            Stage::Reverse => "reverse",
            Stage::Area => "area",
            Stage::Done => "done",
        }
    }
}
