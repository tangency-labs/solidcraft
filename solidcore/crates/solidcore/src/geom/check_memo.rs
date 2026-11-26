//! Not upstream: the incremental check, from yack learnings. A body made by an operation
//! keeps most of its input's faces, edges and vertices as they were, and a design's
//! features each make the next one's input, so a check after each step met the same
//! geometry again and tested it again: on a body of 4400 faces it ran the same 45,000
//! vertex use solves per check, with about 16 vertex uses new each time (T-1208).
//!
//! The geometric tests of a face, an edge or a vertex read only its neighbourhood: a face
//! its surface, loops, trims, edges and vertices; an edge its curve, vertices and the faces
//! round it; a vertex its point and the faces and edges at it. Each such neighbourhood has a
//! content digest ([`crate::geom::content`]): what it is, not which entities it is made of,
//! so the same neighbourhood in a later body, built anew by the operation in between, has
//! the same digest. A check that finds a body valid with no report at all keeps every digest
//! of it as clean; a later check skips the geometric tests of the entities whose digests are
//! clean, since they would pass again. Its structural tests (links, owners, counts) and the
//! body-wide tests still run every time.
//!
//! A digest is taken under everything else the tests read: the check level and scope and
//! the kernel's inputs. An entity whose geometry has no digest (a curve or surface holding
//! other surfaces or volumes, a blend's definition) is never clean. The memo is off with
//! [`Kernel::memoize_curve_caches`], which gives the check's full, uncached verify mode
//! ([`crate::topology::brep::validation::check_brep_full`]).

use std::collections::{HashMap, HashSet};

use crate::geom::content::{Content, ContentId, Digest};
use crate::topology::{Brep, EdgeId, FaceId, LoopId, VertexId};
use crate::Kernel;

/// The most clean digests a kernel keeps (16 bytes each); past it the set starts again.
const MOST_CLEAN: usize = 1 << 20;

/// The clean digests a kernel keeps, and the digests of the body its check is running on.
#[derive(Default)]
pub struct CheckMemo {
    clean: HashSet<ContentId>,
    current: Option<Stars>,
    last_clean: [usize; 3],
}

impl std::fmt::Debug for CheckMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CheckMemo({} clean)", self.clean.len())
    }
}

impl CheckMemo {
    /// The clean digests the memo keeps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.clean.len()
    }

    /// How many faces, edges and vertices the last check began found clean, and so skipped
    /// the geometric tests of.
    #[must_use]
    pub fn last_clean(&self) -> [usize; 3] {
        self.last_clean
    }

    /// Whether it keeps none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.clean.is_empty()
    }
}

/// The digests of one body's faces, edges and vertices, and which of them were clean when
/// its check began.
#[derive(Default)]
pub(crate) struct Stars {
    faces: HashMap<FaceId, ContentId>,
    edges: HashMap<EdgeId, ContentId>,
    vertices: HashMap<VertexId, ContentId>,
    loops: HashMap<LoopId, FaceId>,
    clean_faces: HashSet<FaceId>,
    clean_edges: HashSet<EdgeId>,
    clean_vertices: HashSet<VertexId>,
}

impl Stars {
    /// The digests of `b`'s entities under `salt` (what else the tests read). An entity
    /// whose neighbourhood has no digest, or that a broken link leaves unreadable, has none.
    pub(crate) fn of(b: &Brep, salt: ContentId) -> Self {
        let mut out = Self::default();
        let salt = body_salt(b, salt);
        let parts = Parts::of(b);
        for f in b.face_list().iter().copied() {
            if let Some(d) = face_digest(b, f, salt, &parts) {
                out.faces.insert(f, d);
            }
            if let Ok(face) = b.face(f) {
                for &l in &face.loops {
                    out.loops.insert(l, f);
                }
            }
        }
        for e in b.edge_list().iter().copied() {
            if let Some(d) = edge_digest(b, e, salt, &parts, &out.faces) {
                out.edges.insert(e, d);
            }
        }
        for v in b.vertex_list().iter().copied() {
            if let Some(d) = vertex_digest(b, v, salt, &parts) {
                out.vertices.insert(v, d);
            }
        }
        out
    }

    /// Marks which entities `clean` names.
    fn mark(&mut self, clean: &HashSet<ContentId>) {
        self.clean_faces = self
            .faces
            .iter()
            .filter(|(_, d)| clean.contains(d))
            .map(|(&f, _)| f)
            .collect();
        self.clean_edges = self
            .edges
            .iter()
            .filter(|(_, d)| clean.contains(d))
            .map(|(&e, _)| e)
            .collect();
        self.clean_vertices = self
            .vertices
            .iter()
            .filter(|(_, d)| clean.contains(d))
            .map(|(&v, _)| v)
            .collect();
    }

    fn all(&self) -> impl Iterator<Item = ContentId> + '_ {
        self.faces
            .values()
            .chain(self.edges.values())
            .chain(self.vertices.values())
            .copied()
    }
}

/// What every digest of one check is taken under: the level, the scope and the kernel's
/// inputs (tolerances, context scalars, fixes and the blend net tolerance). `None` when the
/// memo is off.
pub(crate) fn salt(k: &Kernel, level: u8, stitched: bool) -> Option<ContentId> {
    let kernel = crate::curve::curve::p5::CurveMemoKey::of_kernel(k)?;
    let mut d = Digest::new();
    d.tag(0x5A17);
    d.hashed(&level);
    d.bool(stitched);
    // the kernel inputs are plain numbers and flags; their derived form writes every one
    let _ = std::fmt::write(&mut d, format_args!("{kernel:?}"));
    d.f64(k.blend_net_tolerance.get());
    Some(d.finish())
}

/// `salt` with what the tests read of the body as a whole: its tolerance (the floor of
/// the vertex gap tests and the limit of the tolerance a check may grow), whether it is
/// only validated and whether editing is enabled (which decide whether a trim may be raised).
fn body_salt(b: &Brep, salt: ContentId) -> ContentId {
    let mut d = Digest::new();
    d.tag(0xB0D7);
    d.hashed(&salt);
    d.f64(b.tolerance());
    d.bool(b.validate_only());
    d.bool(b.editing_enabled());
    d.finish()
}

/// Starts the check of a body whose digests are `stars`: which of them are clean is read
/// from the kernel's memo, for [`face_clean`], [`edge_clean`] and [`vertex_clean`].
pub(crate) fn begin(k: &Kernel, mut stars: Stars) {
    let mut memo = k.check_memo.borrow_mut();
    stars.mark(&memo.clean);
    memo.last_clean = [
        stars.clean_faces.len(),
        stars.clean_edges.len(),
        stars.clean_vertices.len(),
    ];
    memo.current = Some(stars);
}

/// Ends the check begun by [`begin`]. When the body was found valid with no report at all,
/// its digests as the check began on it and `after` (its digests as the check left it) are
/// kept as clean: each describes everything the skipped tests read, and the check passed
/// both. The first is the one the next operation's body usually shares: the gaps its kept
/// entities cached are the ones the operation cached, not the ones a check filled in.
pub(crate) fn end(k: &Kernel, after: Option<Stars>) {
    let mut memo = k.check_memo.borrow_mut();
    let before = memo.current.take();
    if let Some(after) = after {
        let mut ids: Vec<ContentId> = after.all().collect();
        if let Some(before) = &before {
            ids.extend(before.all());
        }
        if memo.clean.len() + ids.len() > MOST_CLEAN {
            memo.clean.clear();
        }
        memo.clean.extend(ids);
    }
}

/// Whether face `f`'s geometric tests passed on a body before (and so are skipped).
pub(crate) fn face_clean(k: &Kernel, f: FaceId) -> bool {
    k.check_memo
        .borrow()
        .current
        .as_ref()
        .is_some_and(|s| s.clean_faces.contains(&f))
}

/// Whether loop `l`'s geometric tests passed on a body before: its face's did.
pub(crate) fn loop_clean(k: &Kernel, l: LoopId) -> bool {
    let memo = k.check_memo.borrow();
    memo.current
        .as_ref()
        .and_then(|s| s.loops.get(&l).map(|f| s.clean_faces.contains(f)))
        .unwrap_or(false)
}

/// Whether edge `e`'s geometric tests passed on a body before.
pub(crate) fn edge_clean(k: &Kernel, e: EdgeId) -> bool {
    k.check_memo
        .borrow()
        .current
        .as_ref()
        .is_some_and(|s| s.clean_edges.contains(&e))
}

/// Whether vertex `v`'s geometric tests passed on a body before.
pub(crate) fn vertex_clean(k: &Kernel, v: VertexId) -> bool {
    k.check_memo
        .borrow()
        .current
        .as_ref()
        .is_some_and(|s| s.clean_vertices.contains(&v))
}

/// The digest of face `f`'s neighbourhood: its surface, tolerance and parameter domain, and
/// loop by loop, edge use by edge use, each use's sense and trim, its edge's curve, interval
/// and tolerance, and the point and tolerance of the vertex it starts at.
fn face_digest(b: &Brep, f: FaceId, salt: ContentId, parts: &Parts) -> Option<ContentId> {
    use crate::topology::edgeuse::p1 as eu1;
    let mut d = Digest::new();
    d.tag(0xF);
    d.hashed(&salt);
    let face = b.face(f).ok()?;
    face.surface.as_ref()?.content(&mut d)?;
    d.f64(crate::topology::face::p2::tolerance(b, f).ok()?);
    face.uv_domain.content(&mut d)?;
    d.hashed(&face.rectangular_trim);
    let fu = crate::topology::face::p2::upward_faceuse(b, f).ok()??;
    let loopuses = eu1::faceuse_loopuses(b, fu, false, None).ok()?;
    d.usize(loopuses.len());
    for lu in loopuses {
        d.hashed(&b.loopuse(lu).ok()?.orientation);
        let edgeuses = eu1::loopuse_edgeuses(b, lu).ok()?;
        d.usize(edgeuses.len());
        if edgeuses.is_empty() {
            // a vertex loop
            for vu in crate::topology::loopuse::vertexuses(b, lu, None).ok()? {
                parts.vertex(b, crate::topology::vertexuse::vertex(b, vu?).ok()??, &mut d)?;
            }
        }
        for eu in edgeuses {
            d.hashed(&eu1::orientation(b, eu).ok()?);
            let (trim, holder) = eu1::uv_trim_curve(b, eu).ok()?;
            trim.content(&mut d)?;
            d.bool(holder == Some(eu));
            let e = eu1::edge(b, eu).ok()??;
            parts.edge(b, e, &mut d)?;
            let vu = eu1::vertexuse(b, eu).ok()??;
            parts.vertex(b, crate::topology::vertexuse::vertex(b, vu).ok()??, &mut d)?;
        }
    }
    Some(d.finish())
}

/// The digest of edge `e`'s neighbourhood: its curve, interval, tolerance and vertices, and
/// use by use round it, the use's sense, trim and loop membership and its face's digest.
fn edge_digest(
    b: &Brep,
    e: EdgeId,
    salt: ContentId,
    parts: &Parts,
    faces: &HashMap<FaceId, ContentId>,
) -> Option<ContentId> {
    use crate::topology::edgeuse::p1 as eu1;
    let mut d = Digest::new();
    d.tag(0xE);
    d.hashed(&salt);
    parts.edge(b, e, &mut d)?;
    let edge = b.edge(e).ok()?;
    for v in edge.vertices {
        parts.vertex(b, v, &mut d)?;
    }
    d.bool(edge.vertices[0] == edge.vertices[1]);
    let uses = &edge.edgeuses;
    d.usize(uses.len());
    let place =
        |x: Option<crate::topology::EdgeuseId>| x.and_then(|x| uses.iter().position(|&u| u == x));
    for &eu in uses {
        d.hashed(&eu1::orientation(b, eu).ok()?);
        d.bool(eu1::is_loop_edgeuse(b, eu).ok()?);
        d.hashed(&place(eu1::radial(b, eu).ok()?));
        d.hashed(&place(eu1::mate(b, eu).ok()?));
        let (trim, holder) = eu1::uv_trim_curve(b, eu).ok()?;
        trim.content(&mut d)?;
        d.bool(holder == Some(eu));
        match eu1::face(b, eu).ok()? {
            Some(f) => d.hashed(faces.get(&f)?),
            None => d.bool(false),
        }
    }
    Some(d.finish())
}

/// The digest of vertex `v`'s neighbourhood, as far as its tests read: its point and
/// tolerance (with its uses' UV points and cached gaps) and, use by use, the kind of the
/// use's owner, which of the vertex's faces it is on and that face's surface, and the edge
/// uses the gap and sector tests read at the vertex: the use's own edge use, its clockwise
/// neighbour (the one ending at the vertex), their mates and the mates' clockwise
/// neighbours, each with its sense, trim and edge geometry. Not the rest of the faces' or
/// edges' boundaries, which no vertex test reads, so an edit elsewhere on a face leaves its
/// other vertices clean.
fn vertex_digest(b: &Brep, v: VertexId, salt: ContentId, parts: &Parts) -> Option<ContentId> {
    use crate::topology::edgeuse::p1 as eu1;
    use crate::topology::{vertexuse as tvu, VertexuseOwner};
    let mut d = Digest::new();
    d.tag(0x77);
    d.hashed(&salt);
    parts.vertex(b, v, &mut d)?;
    let mut ring = Ring::default();
    let uses = &b.vertex(v).ok()?.vertexuses;
    d.usize(uses.len());
    for &vu in uses {
        let owner = b.vertexuse(vu).ok()?.owner;
        d.tag(match owner {
            VertexuseOwner::Edgeuse(_) => 1,
            VertexuseOwner::Loopuse(_) => 2,
            VertexuseOwner::Shell(_) => 3,
            VertexuseOwner::Unknown => 4,
        });
        match tvu::faceuse(b, vu).ok()? {
            Some(fu) => ring.face(b, parts, b.faceuse(fu).ok()?.face?, &mut d)?,
            None => d.bool(false),
        }
        let eu = tvu::edgeuse(b, vu).ok()?;
        let cw = match eu {
            Some(eu) => b.edgeuse(eu).ok()?.cw,
            None => None,
        };
        let mate = |x: Option<crate::topology::EdgeuseId>| match x {
            Some(x) => eu1::mate(b, x).ok(),
            None => Some(None),
        };
        let cw_of = |x: Option<crate::topology::EdgeuseId>| match x {
            Some(x) => b.edgeuse(x).ok().map(|r| r.cw),
            None => Some(None),
        };
        let eu_mate = mate(eu)?;
        let cw_mate = mate(cw)?;
        for x in [eu, cw, eu_mate, cw_mate, cw_of(eu_mate)?, cw_of(cw_mate)?] {
            ring.edgeuse(b, parts, x, &mut d)?;
        }
    }
    Some(d.finish())
}

/// The faces and edge uses a vertex digest has met, numbered in the order it met them:
/// the digest writes each one's number, so it says which uses share a face or are the same
/// edge use, and writes each one's content the first time.
#[derive(Default)]
struct Ring {
    faces: Vec<FaceId>,
    edgeuses: Vec<crate::topology::EdgeuseId>,
}

impl Ring {
    /// Writes face `f`'s number and, the first time, its surface digest.
    fn face(&mut self, b: &Brep, parts: &Parts, f: FaceId, d: &mut Digest) -> Option<()> {
        if let Some(i) = self.faces.iter().position(|&x| x == f) {
            d.usize(i);
            return Some(());
        }
        d.usize(self.faces.len());
        self.faces.push(f);
        parts.surface(b, f, d)
    }

    /// Writes edge use `x`'s number and, the first time, its sense, loop membership, trim,
    /// edge geometry and face; `false` for none.
    fn edgeuse(
        &mut self,
        b: &Brep,
        parts: &Parts,
        x: Option<crate::topology::EdgeuseId>,
        d: &mut Digest,
    ) -> Option<()> {
        use crate::topology::edgeuse::p1 as eu1;
        let Some(x) = x else {
            d.bool(false);
            return Some(());
        };
        d.bool(true);
        if let Some(i) = self.edgeuses.iter().position(|&y| y == x) {
            d.usize(i);
            return Some(());
        }
        d.usize(self.edgeuses.len());
        self.edgeuses.push(x);
        d.hashed(&eu1::orientation(b, x).ok()?);
        d.bool(eu1::is_loop_edgeuse(b, x).ok()?);
        let (trim, holder) = eu1::uv_trim_curve(b, x).ok()?;
        trim.content(d)?;
        d.bool(holder == Some(x));
        match eu1::edge(b, x).ok()? {
            Some(e) => parts.edge(b, e, d)?,
            None => d.bool(false),
        }
        match eu1::face(b, x).ok()? {
            Some(f) => self.face(b, parts, f, d)?,
            None => d.bool(false),
        }
        Some(())
    }
}

/// The digests of each edge's geometry ([`edge_geometry`]) and each vertex's point
/// ([`vertex_point`]), taken once per [`Stars::of`]: a face, an edge and a vertex digest
/// each write an edge's or a vertex's digest where they wrote its content. `None` for one
/// whose content has none.
struct Parts {
    edges: HashMap<EdgeId, Option<ContentId>>,
    vertices: HashMap<VertexId, Option<ContentId>>,
    surfaces: HashMap<FaceId, Option<ContentId>>,
}

impl Parts {
    fn of(b: &Brep) -> Self {
        let edges = b
            .edge_list()
            .iter()
            .map(|&e| (e, edge_geometry_id(b, e)))
            .collect();
        let vertices = b
            .vertex_list()
            .iter()
            .map(|&v| (v, vertex_point_id(b, v)))
            .collect();
        let surfaces = b
            .face_list()
            .iter()
            .map(|&f| (f, surface_id(b, f)))
            .collect();
        Self {
            edges,
            vertices,
            surfaces,
        }
    }

    /// Writes face `f`'s surface digest; `None` when it has none.
    fn surface(&self, b: &Brep, f: FaceId, d: &mut Digest) -> Option<()> {
        let id = match self.surfaces.get(&f) {
            Some(id) => (*id)?,
            None => surface_id(b, f)?,
        };
        d.hashed(&id);
        Some(())
    }

    /// Writes edge `e`'s geometry digest; `None` when it has none.
    fn edge(&self, b: &Brep, e: EdgeId, d: &mut Digest) -> Option<()> {
        let id = match self.edges.get(&e) {
            Some(id) => (*id)?,
            None => edge_geometry_id(b, e)?,
        };
        d.hashed(&id);
        Some(())
    }

    /// Writes vertex `v`'s point digest; `None` when it has none.
    fn vertex(&self, b: &Brep, v: VertexId, d: &mut Digest) -> Option<()> {
        let id = match self.vertices.get(&v) {
            Some(id) => (*id)?,
            None => vertex_point_id(b, v)?,
        };
        d.hashed(&id);
        Some(())
    }
}

fn edge_geometry_id(b: &Brep, e: EdgeId) -> Option<ContentId> {
    let mut d = Digest::new();
    d.tag(0xE6);
    edge_geometry(b, e, &mut d)?;
    Some(d.finish())
}

/// Face `f`'s surface, tolerance, parameter domain and whether its domain is its trim.
fn surface_id(b: &Brep, f: FaceId) -> Option<ContentId> {
    let mut d = Digest::new();
    d.tag(0x5F);
    let face = b.face(f).ok()?;
    face.surface.as_ref()?.content(&mut d)?;
    d.f64(crate::topology::face::p2::tolerance(b, f).ok()?);
    face.uv_domain.content(&mut d)?;
    d.hashed(&face.rectangular_trim);
    Some(d.finish())
}

fn vertex_point_id(b: &Brep, v: VertexId) -> Option<ContentId> {
    let mut d = Digest::new();
    d.tag(0x76);
    vertex_point(b, v, &mut d)?;
    Some(d.finish())
}

/// Writes edge `e`'s curve, interval and tolerance, and the gaps cached on its uses.
fn edge_geometry(b: &Brep, e: EdgeId, d: &mut Digest) -> Option<()> {
    let edge = b.edge(e).ok()?;
    edge.curve.as_ref()?.content(d)?;
    edge.interval.content(d)?;
    d.f64(crate::topology::edge::tolerance(b, e).ok()?);
    // the gaps cached on the edge's uses: the check reads them where they are set instead
    // of measuring again, so the same curve with other cached gaps may get another answer
    d.usize(edge.edgeuses.len());
    for &eu in &edge.edgeuses {
        let u = b.edgeuse(eu).ok()?;
        let f = u.max_edge_face_gap;
        d.f64(f.gap_3d);
        d.hashed(&f.edgeuse.map(|x| x == eu));
        d.f64(f.edge_t);
        d.f64(f.face_uv.x);
        d.f64(f.face_uv.y);
        let g = u.edge_ccw_edge_gap;
        d.f64(g.gap_3d);
        d.hashed(&g.edgeuse0.map(|x| x == eu));
        d.bool(g.edgeuse1.is_some());
        d.f64(g.edge_t0);
        d.f64(g.edge_t1);
    }
    Some(())
}

/// Writes vertex `v`'s point and tolerance, and its uses' UV points and cached gaps.
fn vertex_point(b: &Brep, v: VertexId, d: &mut Digest) -> Option<()> {
    let vertex = b.vertex(v).ok()?;
    vertex.point.content(d)?;
    d.f64(vertex.tolerance);
    // the UV points and the gaps cached on the vertex's uses: the check reads a set gap
    // instead of measuring again, so the same point with other cached gaps may get
    // another answer
    d.usize(vertex.vertexuses.len());
    for &vu in &vertex.vertexuses {
        let u = b.vertexuse(vu).ok()?;
        d.f64(u.uv_point.x);
        d.f64(u.uv_point.y);
        let e = u.vertex_edge_gap;
        d.f64(e.gap_3d);
        d.hashed(&e.vertexuse.map(|x| x == vu));
        d.f64(e.edge_t);
        let f = u.vertex_face_gap;
        d.f64(f.gap_3d);
        d.hashed(&f.vertexuse.map(|x| x == vu));
        d.f64(f.face_uv.x);
        d.f64(f.face_uv.y);
    }
    Some(())
}
