//! Cooperative certification of the exact f64 boundary consumed by CPU Path.
//! Coordinates are never tolerance-welded, snapped or repaired. Every shared edge must have
//! identical finite endpoints (ClosedMesh and sheets canonicalise signed zero; declared
//! boxes retain the legacy raw-bit edge policy). Sheets may have manifold boundary
//! edges, concavity and holes; geometric contact is
//! permitted only along the same body's actual shared topological simplex.
use super::{Bounds, Triangle, WorkBudget};
use crate::optical_boundary::{coplanar_edge_bad, in_triangle, key, opposite, predicate_domain, project, projection, side};
pub(super) use crate::optical_boundary::{forbidden_contact, origin_crossing};
use crate::{
    ProductScene,
    job::{self, CancelToken, DielectricBoundary, RaytraceOptions},
};
use robust::orient2d;
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
fn target_key(p: Vec3, medium: usize, options: &RaytraceOptions) -> job::Result<[u64; 3]> {
    let input = options.dielectrics.get(medium).ok_or_else(|| invalid("CPU glass coordinate target"))?;
    Ok(if input.boundary == DielectricBoundary::ClosedBox { [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()] } else { key(p) })
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
    ContactBuild,
    Contact,
    Sources,
    Volume,
    Parents,
    WindingA,
    WindingB,
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
    regions: Vec<Region>,
    count: usize,
    stage: Stage,
    vertex_sort: bool,
    medium: usize,
    triangle: usize,
    i: usize,
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
    contact_index: Option<crate::optical_boundary::ContactIndex>,
    contact_query: Option<crate::optical_boundary::ContactQuery>,
    winding: i32,
    inside_a: bool,
}
fn selected_count(scene: &ProductScene, options: &RaytraceOptions) -> job::Result<usize> {
    let mut count = 0_usize;
    for input in &options.dielectrics {
        let body = scene.meshes.get(input.body_index).filter(|b| b.name == input.body_name).ok_or_else(|| invalid("CPU glass target changed"))?;
        count = count.checked_add(body.mesh.triangles.len()).ok_or_else(memory)?;
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
            .and_then(|n| n.checked_add(options.dielectrics.len() * size_of::<Region>()))
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(memory)?
            .checked_add(crate::optical_boundary::ContactIndex::required_bytes(n)?)
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
            regions: {
                let mut regions = reserved(options.dielectrics.len())?;
                regions.resize(options.dielectrics.len(), Region::default());
                regions
            },
            count: options.dielectrics.len(),
            stage: Stage::Index,
            vertex_sort: false,
            medium: 0,
            triangle: 0,
            i: 0,
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
            contact_index: None,
            contact_query: None,
            winding: 0,
            inside_a: false,
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
                                // Path sheets use this task's charged planar/manifold proof.
                                DielectricBoundary::ThinSheet => {}
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
                    if region.end == region.begin {
                        return Err(invalid("CPU glass boundary has no triangles"));
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
                if input.boundary == DielectricBoundary::ThinSheet
                    && let Some(first) = self.faces.get(region.begin)
                {
                    if !same_plane(first.p, p) {
                        return Err(invalid("CPU thin sheet must have one exact planar boundary"));
                    }
                    let drop = projection(first.p);
                    let a = first.p.map(|p| project(p, drop));
                    let b = p.map(|p| project(p, drop));
                    let winding = orient2d(b[0], b[1], b[2]);
                    if winding == 0.0 || opposite(orient2d(a[0], a[1], a[2]), winding) {
                        return Err(invalid("CPU thin sheet triangles must have consistent winding"));
                    }
                }
                if self.faces.len() == self.faces.capacity() {
                    return Err(memory());
                }
                self.parents.push(self.faces.len());
                self.ranks.push(0);
                self.faces.push(Face { p, medium: self.medium });
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
                let group = |x: Edge| (x.medium, x.a, x.b);
                let Some(b) = self.edges.get(self.i + 1).copied().filter(|b| group(a) == group(*b)) else {
                    if options.dielectrics.get(a.medium).is_some_and(|input| input.boundary == DielectricBoundary::ThinSheet) {
                        self.i += 1;
                        return Ok(());
                    }
                    return Err(invalid("CPU glass requires closed edges"));
                };
                if a.forward == b.forward || self.edges.get(self.i + 2).is_some_and(|x| group(*x) == group(a)) {
                    return Err(invalid("CPU glass requires at most two oppositely wound faces per edge; closed boundaries require exactly two"));
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
                        return Err(invalid("each CPU glass body must contain one edge-connected boundary"));
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
                let Some(b) = self.edges.get(self.i + 1).copied().filter(|b| (a.medium, a.a, a.b) == (b.medium, b.a, b.b)) else {
                    // Sheet boundary edges have one face and do not join two
                    // corner records. Closed-edge completeness was proved above.
                    self.i += 1;
                    self.vertex = 0;
                    return Ok(());
                };
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
                    self.contact_index = Some(crate::optical_boundary::ContactIndex::new(self.faces.len())?);
                    self.stage = Stage::ContactBuild;
                    self.pair = (0, 0);
                    return Ok(());
                };
                if let Some(b) = self.i.checked_sub(1).and_then(|i| self.edges.get(i)).copied()
                    && (a.medium, a.a) == (b.medium, b.a)
                    && self.root(a.face)? != self.root(b.face)?
                {
                    return Err(invalid(
                        "CPU glass vertex link must be one connected cycle or sheet boundary path; pinched or touching fans are unsupported",
                    ));
                }
                self.i += 1;
            }
            Stage::ContactBuild => {
                let index = self.contact_index.as_mut().ok_or_else(|| invalid("missing CPU optical contact hierarchy"))?;
                if index.step(|i| self.faces.get(i).map(|face| face.p), cancel)? {
                    self.stage = Stage::Contact;
                }
            }
            Stage::Contact => {
                let Some(a) = self.faces.get(self.pair.0).copied() else {
                    self.contact_index = None;
                    self.contact_query = None;
                    self.stage = Stage::Sources;
                    self.medium = 0;
                    self.triangle = 0;
                    self.vertex = 0;
                    return Ok(());
                };
                let index = self.contact_index.as_ref().ok_or_else(|| invalid("missing CPU optical contact hierarchy"))?;
                if self.contact_query.is_none() {
                    self.contact_query = Some(index.query(a.p)?);
                }
                let query = self.contact_query.as_mut().ok_or_else(|| invalid("missing CPU optical contact traversal"))?;
                match index.query_step(query, cancel)? {
                    None => {
                        self.contact_query = None;
                        self.pair.0 += 1;
                    }
                    Some(Some(other)) if other > self.pair.0 => {
                        let b = self.faces.get(other).ok_or_else(|| invalid("missing CPU optical contact candidate"))?;
                        if forbidden_contact(a.p, b.p, a.medium == b.medium) {
                            let first = options.dielectrics.get(a.medium).map_or("unknown", |input| input.body_name.as_str());
                            let second = options.dielectrics.get(b.medium).map_or("unknown", |input| input.body_name.as_str());
                            return Err(invalid(&format!("CPU glass boundaries intersect, touch, overlap or self-intersect: {first} / {second}")));
                        }
                    }
                    Some(_) => {}
                }
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
                    for child in 0..self.count {
                        let mut parent = self.regions[child].parent;
                        let mut depth = 1;
                        while let Some(at) = parent {
                            depth += 1;
                            if depth > job::MAX_NESTED_MEDIA {
                                return Err(job::JobError::new(job::ErrorCode::LimitExceeded, "at most 16 nested dielectric layers are supported"));
                            }
                            parent = self.regions.get(at).ok_or_else(|| invalid("CPU glass parent identity"))?.parent;
                        }
                    }
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
                self.i = a.begin;
                self.winding = 0;
                self.stage = Stage::WindingA;
            }
            Stage::WindingA | Stage::WindingB => {
                let second = matches!(self.stage, Stage::WindingB);
                let (container, sample) = if second { (self.inner, self.outer) } else { (self.outer, self.inner) };
                let region = self.regions[container];
                if self.i < region.end {
                    let face = self.faces.get(self.i).ok_or_else(|| invalid("CPU glass winding face"))?;
                    let point = self.faces.get(self.regions[sample].begin).ok_or_else(|| invalid("CPU glass winding sample"))?.p[0];
                    self.winding += i32::from(origin_crossing(face.p, point)?);
                    self.i += 1;
                } else {
                    if !(0..=1).contains(&self.winding) {
                        return Err(invalid("CPU glass boundary has ambiguous containment winding"));
                    }
                    if !second {
                        self.inside_a = self.winding == 1;
                        self.winding = 0;
                        self.i = self.regions[self.inner].begin;
                        self.stage = Stage::WindingB;
                    } else {
                        let inside_b = self.winding == 1;
                        if self.inside_a && inside_b {
                            return Err(invalid("CPU glass containment cycle"));
                        }
                        let relation = if self.inside_a {
                            Some((self.outer, self.inner))
                        } else if inside_b {
                            Some((self.inner, self.outer))
                        } else {
                            None
                        };
                        if let Some((parent, child)) = relation {
                            if !contains(self.regions[parent].bounds, self.regions[child].bounds) {
                                return Err(invalid("CPU glass containment contradicts its certified bounds"));
                            }
                            let old = self.regions[child].parent;
                            if old.is_none_or(|old| contains(self.regions[old].bounds, self.regions[parent].bounds)) {
                                self.regions[child].parent = Some(parent);
                            }
                        }
                        self.inner += 1;
                        self.stage = Stage::Parents;
                    }
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
