//! FFI wrappers for unit `query.poly`.
//!
//! Shim: `harness/cpp/shim/query_poly.cpp` (`sc_query_poly_eval`).
//! Upstream: `source/SMLib/src/SmPoly.cpp`, `source/SMLib/inc/SmPoly.h`.
//!
//! [`run`] builds an `SmPolyBrep` from a [`PolyDesc`], runs one [`Op`] and returns the
//! shim's sections (`key kind len values...`, kind `0` floats, `1` integers).
//!
//! Flat input (all `f64`): `op`, `brep_tol`, `n_regions` and per region its shell count;
//! `n_vertices` and per vertex `x y z tol`; `n_faces` and per face `shell tol has_normal
//! nx ny nz n_loops` and per loop `mode edge_tol k v0 .. v(k-1)` (mode `0` open without
//! end vertex, `k` edges; `1` closed, `k` edges; `2` open ending at `v(k-1)`, `k-1`
//! edges); radial rings `n` and per ring `k e0 ..`; aux data `n` and `nx ny nz u v`;
//! edge aux pairs; vertex normals `v x y z`; composite faces `k f0 ..`; original face
//! labels `f label`; original edge use labels `e label`; `ok_back_ptrs`; then the op's
//! arguments (see [`Op`]).
//!
//! The wrapper rejects what would make upstream read invalid memory: indices out of
//! range, a region with more than [`MAX_SHELLS_PER_REGION`] shells, loops without vertices (or open-with-end loops with fewer than two), radial
//! rings with fewer than two edges or sharing an edge, an edge given aux data twice
//! (the replaced data may be freed), a face in two composite faces (deleting it would
//! leave a dangling member), `CalculateBoundingBox` asked for a pseudo box without a
//! point array (upstream dereferences the null pointer), labels outside `1..=3`, and
//! mutations that touch an already deleted object.
//!
//! [`Op::OwnerDegenerate`] puts explicit real face records immediately after `op`,
//! before the otherwise unchanged poly B-rep block:
//! `n_owners`, then `label surface_kind` (`0` no surface, `1` plane, `2` Bezier).
//! A plane is `origin[3] x[3] y[3] scale[2] domain[4]`; a non-rational Bezier
//! patch is `nu nv` followed by `nu * nv` XYZ control points, u rows first.
//! This new op does not change any legacy input or output layout.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_query_poly_eval(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// One loop of a face.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopDesc {
    /// `0` open without end vertex, `1` closed, `2` open with end vertex.
    pub mode: u8,
    /// Tolerance passed to `AddPolyEdge`.
    pub edge_tol: f64,
    /// Vertex indices.
    pub verts: Vec<usize>,
}

/// One face.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceDesc {
    /// Global shell index.
    pub shell: usize,
    /// Face tolerance.
    pub tol: f64,
    /// Optional constructor normal.
    pub normal: Option<[f64; 3]>,
    /// Loops, outer first.
    pub loops: Vec<LoopDesc>,
}

/// A poly B-rep description.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PolyDesc {
    /// Poly B-rep tolerance.
    pub tol: f64,
    /// Shell count of each region.
    pub regions: Vec<usize>,
    /// `x y z tol` per vertex.
    pub vertices: Vec<[f64; 4]>,
    /// Faces.
    pub faces: Vec<FaceDesc>,
    /// Radial rings of edge indices (edges in creation order).
    pub radials: Vec<Vec<usize>>,
    /// Aux data `nx ny nz u v`.
    pub aux: Vec<[f64; 5]>,
    /// `(edge, aux)` assignments.
    pub edge_aux: Vec<(usize, usize)>,
    /// `(vertex, normal)` face normals.
    pub vertex_normals: Vec<(usize, [f64; 3])>,
    /// Composite faces (member plain faces, first one constructs).
    pub cpolys: Vec<Vec<usize>>,
    /// `(face, label)` original faces.
    pub orig_faces: Vec<(usize, i64)>,
    /// `(edge, label)` original edge uses.
    pub orig_edgeuses: Vec<(usize, i64)>,
    /// `m_bOKBackPtrs`.
    pub ok_back_ptrs: bool,
}

/// Explicit geometry attached to a real `SmFace` by the owner-aware op.
#[derive(Debug, Clone, PartialEq)]
pub enum OwnerSurface {
    /// An actual `SmPlane`, with its axes, scales and natural UV domain.
    Plane {
        /// Placement origin.
        origin: [f64; 3],
        /// Placement x axis.
        x: [f64; 3],
        /// Placement y axis.
        y: [f64; 3],
        /// Parameter scales.
        scale: [f64; 2],
        /// `[u_min, v_min, u_max, v_max]`.
        domain: [f64; 4],
    },
    /// A non-rational single-span B-spline on `[0, 1]^2`, with 2..=4 points
    /// in each direction. Every row is one u index; columns are v indices.
    Bezier {
        /// XYZ control net (not homogeneous coordinates).
        net: Vec<Vec<[f64; 3]>>,
    },
}

/// A real original face, explicitly distinguished from an unresolved legacy label.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnerFace {
    /// Existing original-face label, in `1..=3`.
    pub label: i64,
    /// `None` constructs a real face with a null surface, not an unresolved owner.
    pub surface: Option<OwnerSurface>,
}

/// Validates only the new explicit owner geometry, without calling C++.
///
/// # Errors
///
/// Rejects duplicate/out-of-range labels, non-finite geometry, reversed plane
/// domains, or a non-rectangular/unsupported Bezier net. No legacy guard changes.
pub fn check_owner_faces(owners: &[OwnerFace]) -> Result<(), FfiError> {
    let finite = |xs: &[f64]| -> Result<(), FfiError> {
        if xs.iter().all(|x| x.is_finite()) {
            Ok(())
        } else {
            Err(bad("non-finite owner geometry"))
        }
    };
    let mut seen = Vec::new();
    for owner in owners {
        label(owner.label)?;
        if seen.contains(&owner.label) {
            return Err(bad("duplicate owner label"));
        }
        seen.push(owner.label);
        match &owner.surface {
            None => {}
            Some(OwnerSurface::Plane {
                origin,
                x,
                y,
                scale,
                domain,
            }) => {
                finite(origin)?;
                finite(x)?;
                finite(y)?;
                finite(scale)?;
                finite(domain)?;
                if domain[0] > domain[2] || domain[1] > domain[3] {
                    return Err(bad("reversed owner plane domain"));
                }
            }
            Some(OwnerSurface::Bezier { net }) => {
                let nv = net.first().map_or(0, Vec::len);
                if !(2..=4).contains(&net.len())
                    || !(2..=4).contains(&nv)
                    || net.iter().any(|row| row.len() != nv)
                {
                    return Err(bad(
                        "owner Bezier net must be rectangular, 2..=4 per direction",
                    ));
                }
                for point in net.iter().flatten() {
                    finite(point)?;
                }
            }
        }
    }
    Ok(())
}

fn encode_owner_faces(v: &mut Vec<f64>, owners: &[OwnerFace]) -> Result<(), FfiError> {
    check_owner_faces(owners)?;
    v.push(owners.len() as f64);
    for owner in owners {
        v.push(label(owner.label)?);
        match &owner.surface {
            None => v.push(0.0),
            Some(OwnerSurface::Plane {
                origin,
                x,
                y,
                scale,
                domain,
            }) => {
                v.push(1.0);
                v.extend_from_slice(origin);
                v.extend_from_slice(x);
                v.extend_from_slice(y);
                v.extend_from_slice(scale);
                v.extend_from_slice(domain);
            }
            Some(OwnerSurface::Bezier { net }) => {
                v.push(2.0);
                v.push(net.len() as f64);
                v.push(net.first().map_or(0, Vec::len) as f64);
                for point in net.iter().flatten() {
                    v.extend_from_slice(point);
                }
            }
        }
    }
    Ok(())
}

/// One contains query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainsQuery {
    /// `false`: loop `ContainsPoint`; `true`: face `PointInPolygon`.
    pub face: bool,
    /// Loop or face index.
    pub id: usize,
    /// The point.
    pub point: [f64; 3],
    /// Tolerance (loops only).
    pub tol: f64,
    /// `bWorkIn3D`.
    pub work_in_3d: bool,
}

/// One mutation command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mutation {
    /// `SmPolyVertex::SetPoint`.
    SetPoint(usize, [f64; 3]),
    /// `SmPolyVertex::SetTolerance(SmZoneTol3d(t))`.
    SetVertexTolerance(usize, f64),
    /// `delete pPolyEdge`.
    DeleteEdge(usize),
    /// `delete pPolyFace` (plain or composite).
    DeleteFace(usize),
    /// `SmCPolyFace::RemovePolyFace(face)`.
    CPolyRemove(usize, usize),
    /// `SmPolyFace::SetNormalUnInit`.
    SetNormalUninit(usize),
    /// `SmPolyEdge::SetTolerance(SmZoneTol3d(t))`.
    SetEdgeTolerance(usize, f64),
    /// `SmPolyFace::SetTolerance(SmZoneTol3d(t))`.
    SetFaceTolerance(usize, f64),
    /// `SmPolyFace::SetOriginalFace(label, FALSE)`.
    SetOriginalFace(usize, i64),
    /// `SmPolyEdge::SetAuxData(NULL)`.
    ClearAux(usize),
}

/// The operation run after building.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Only the dump.
    Dump,
    /// `SmPolyBrep::CalculateBoundingBox` plus every object's box.
    BBox {
        /// Ask for the pseudo box.
        pseudo: bool,
        /// `None`: no array; `Some(empty)`: empty array; else the given points.
        points: Option<Vec<[f64; 3]>>,
        /// Ask for the centroid.
        centroid: bool,
        /// Pseudo box normal.
        normal: Option<[f64; 3]>,
    },
    /// `GetNormal(force, update)` calls `(face, force, update)`.
    Normal(Vec<(usize, bool, bool)>),
    /// `ComputeSphereBound(force)` calls `(face, force)`.
    Sphere(Vec<(usize, bool)>),
    /// Contains queries.
    Contains(Vec<ContainsQuery>),
    /// `IsDegenerate(opt xsect tol)` calls.
    Degenerate(Vec<(usize, Option<f64>)>),
    /// Real-face `IsDegenerate` calls plus independent raw/mapped endpoint and
    /// midpoint samples, natural domains and parameter classifications.
    OwnerDegenerate {
        /// Explicit owner provenance; omitted labels stay unresolved.
        owners: Vec<OwnerFace>,
        /// `(edge, optional intersection tolerance)` calls.
        calls: Vec<(usize, Option<f64>)>,
    },
    /// `Transform(SmAxis2Placement(o, x, y), opt scale)`.
    Transform {
        /// Origin.
        origin: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Scale.
        scale: Option<[f64; 3]>,
    },
    /// Mutations in order.
    Mutate(Vec<Mutation>),
    /// `GetType` and `IsKindOf(code)` of the brep, first region, first shell, every face,
    /// first loop, first edge, first vertex.
    Kinds(Vec<i64>),
}

/// One output section.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Section key.
    pub key: i32,
    /// Integer values (else floats).
    pub ints: bool,
    /// Values.
    pub values: Vec<f64>,
}

/// Largest shell count of one region the wrapper accepts. The shim converts each count
/// with `(int64_t)` and allocates that many shells, so a fuzzed `2^63 - 1` wraps to a
/// negative count (no shells, later shell indices read past the shim's array: SIGSEGV)
/// and `2^31` allocates billions of objects.
pub const MAX_SHELLS_PER_REGION: usize = 1 << 16;

fn idx(i: usize, n: usize, what: &str) -> Result<f64, FfiError> {
    if i < n {
        Ok(i as f64)
    } else {
        Err(bad(format!("{what} index {i} out of range {n}")))
    }
}

fn label(l: i64) -> Result<f64, FfiError> {
    if (1..=3).contains(&l) {
        Ok(l as f64)
    } else {
        Err(bad("label outside 1..=3"))
    }
}

fn encode(d: &PolyDesc, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut v: Vec<f64> = Vec::new();
    let op_code = match op {
        Op::Dump => 0.0,
        Op::BBox { .. } => 1.0,
        Op::Normal(_) => 2.0,
        Op::Sphere(_) => 3.0,
        Op::Contains(_) => 4.0,
        Op::Degenerate(_) => 5.0,
        Op::Transform { .. } => 6.0,
        Op::Mutate(_) => 7.0,
        Op::Kinds(_) => 8.0,
        Op::OwnerDegenerate { .. } => 9.0,
    };
    v.push(op_code);
    if let Op::OwnerDegenerate { owners, .. } = op {
        // Construct genuine owners before poly faces, without transient fake
        // pointers for any explicitly resolved label. Legacy bytes are unchanged.
        encode_owner_faces(&mut v, owners)?;
        if d.vertices.iter().flatten().any(|x| !x.is_finite()) {
            return Err(bad("non-finite owner-op poly vertex"));
        }
    }
    v.push(d.tol);
    v.push(d.regions.len() as f64);
    let mut n_shells = 0usize;
    for &s in &d.regions {
        if s > MAX_SHELLS_PER_REGION {
            return Err(bad("too many shells in a region"));
        }
        v.push(s as f64);
        n_shells += s;
    }
    let nv = d.vertices.len();
    v.push(nv as f64);
    for p in &d.vertices {
        v.extend_from_slice(p);
    }
    let nf = d.faces.len();
    v.push(nf as f64);
    let mut n_loops = 0usize;
    let mut n_edges = 0usize;
    for f in &d.faces {
        v.push(idx(f.shell, n_shells, "shell")?);
        v.push(f.tol);
        v.push(if f.normal.is_some() { 1.0 } else { 0.0 });
        v.extend_from_slice(&f.normal.unwrap_or([0.0; 3]));
        v.push(f.loops.len() as f64);
        for l in &f.loops {
            let k = l.verts.len();
            if k == 0 || (l.mode == 2 && k < 2) || l.mode > 2 {
                return Err(bad("loop vertex count / mode"));
            }
            v.push(f64::from(l.mode));
            v.push(l.edge_tol);
            v.push(k as f64);
            for &x in &l.verts {
                v.push(idx(x, nv, "vertex")?);
            }
            n_loops += 1;
            n_edges += if l.mode == 2 { k - 1 } else { k };
        }
    }
    let mut used = vec![false; n_edges];
    v.push(d.radials.len() as f64);
    for g in &d.radials {
        if g.len() < 2 {
            return Err(bad("radial ring shorter than 2"));
        }
        v.push(g.len() as f64);
        for &e in g {
            v.push(idx(e, n_edges, "edge")?);
            let u = used.get_mut(e).ok_or_else(|| bad("edge"))?;
            if *u {
                return Err(bad("edge in two radial rings"));
            }
            *u = true;
        }
    }
    let na = d.aux.len();
    v.push(na as f64);
    for a in &d.aux {
        v.extend_from_slice(a);
    }
    let mut has_aux = vec![false; n_edges];
    v.push(d.edge_aux.len() as f64);
    for &(e, a) in &d.edge_aux {
        v.push(idx(e, n_edges, "edge")?);
        v.push(idx(a, na, "aux")?);
        let u = has_aux.get_mut(e).ok_or_else(|| bad("edge"))?;
        if *u {
            return Err(bad("edge given aux data twice"));
        }
        *u = true;
    }
    v.push(d.vertex_normals.len() as f64);
    for (x, n) in &d.vertex_normals {
        v.push(idx(*x, nv, "vertex")?);
        v.extend_from_slice(n);
    }
    let mut in_cpoly = vec![false; nf];
    v.push(d.cpolys.len() as f64);
    for c in &d.cpolys {
        if c.is_empty() {
            return Err(bad("empty composite face"));
        }
        v.push(c.len() as f64);
        let mut seen: Vec<usize> = Vec::new();
        for &f in c {
            v.push(idx(f, nf, "face")?);
            if !seen.contains(&f) {
                let u = in_cpoly.get_mut(f).ok_or_else(|| bad("face"))?;
                if *u {
                    return Err(bad("face in two composite faces"));
                }
                *u = true;
                seen.push(f);
            }
        }
    }
    let n_all_faces = nf + d.cpolys.len();
    v.push(d.orig_faces.len() as f64);
    for &(f, l) in &d.orig_faces {
        v.push(idx(f, nf, "face")?);
        v.push(label(l)?);
    }
    v.push(d.orig_edgeuses.len() as f64);
    for &(e, l) in &d.orig_edgeuses {
        v.push(idx(e, n_edges, "edge")?);
        v.push(label(l)?);
    }
    v.push(if d.ok_back_ptrs { 1.0 } else { 0.0 });

    let b = |x: bool| if x { 1.0 } else { 0.0 };
    match op {
        Op::Dump => {}
        Op::BBox {
            pseudo,
            points,
            centroid,
            normal,
        } => {
            if *pseudo && points.is_none() {
                return Err(bad(
                    "pseudo box without a point array (upstream null dereference)",
                ));
            }
            v.push(b(*pseudo));
            match points {
                None => v.push(0.0),
                Some(p) if p.is_empty() => v.push(1.0),
                Some(p) => {
                    v.push(2.0);
                    v.push(p.len() as f64);
                    for q in p {
                        v.extend_from_slice(q);
                    }
                }
            }
            v.push(b(*centroid));
            v.push(b(normal.is_some()));
            v.extend_from_slice(&normal.unwrap_or([0.0; 3]));
        }
        Op::Normal(calls) => {
            v.push(calls.len() as f64);
            for &(f, force, upd) in calls {
                v.push(idx(f, n_all_faces, "face")?);
                v.push(b(force));
                v.push(b(upd));
            }
        }
        Op::Sphere(calls) => {
            v.push(calls.len() as f64);
            for &(f, force) in calls {
                v.push(idx(f, n_all_faces, "face")?);
                v.push(b(force));
            }
        }
        Op::Contains(qs) => {
            v.push(qs.len() as f64);
            for q in qs {
                v.push(b(q.face));
                v.push(if q.face {
                    idx(q.id, n_all_faces, "face")?
                } else {
                    idx(q.id, n_loops, "loop")?
                });
                v.extend_from_slice(&q.point);
                v.push(q.tol);
                v.push(b(q.work_in_3d));
            }
        }
        Op::Degenerate(qs) => {
            v.push(qs.len() as f64);
            for &(e, t) in qs {
                v.push(idx(e, n_edges, "edge")?);
                v.push(b(t.is_some()));
                v.push(t.unwrap_or(0.0));
            }
        }
        Op::OwnerDegenerate { calls, .. } => {
            v.push(calls.len() as f64);
            for &(e, t) in calls {
                v.push(idx(e, n_edges, "edge")?);
                v.push(b(t.is_some()));
                v.push(t.unwrap_or(0.0));
            }
        }
        Op::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.push(b(scale.is_some()));
            v.extend_from_slice(&scale.unwrap_or([0.0; 3]));
        }
        Op::Mutate(cmds) => {
            let mut edge_alive = vec![true; n_edges];
            let mut face_alive = vec![true; n_all_faces];
            let live_edge = |a: &[bool], e: usize| -> Result<f64, FfiError> {
                idx(e, n_edges, "edge")?;
                if a.get(e).copied().unwrap_or(false) {
                    Ok(e as f64)
                } else {
                    Err(bad("deleted edge"))
                }
            };
            let live_face = |a: &[bool], f: usize| -> Result<f64, FfiError> {
                idx(f, n_all_faces, "face")?;
                if a.get(f).copied().unwrap_or(false) {
                    Ok(f as f64)
                } else {
                    Err(bad("deleted face"))
                }
            };
            v.push(cmds.len() as f64);
            for c in cmds {
                let rec: [f64; 6] = match *c {
                    Mutation::SetPoint(x, p) => [1.0, idx(x, nv, "vertex")?, 0.0, p[0], p[1], p[2]],
                    Mutation::SetVertexTolerance(x, t) => {
                        [2.0, idx(x, nv, "vertex")?, 0.0, t, 0.0, 0.0]
                    }
                    Mutation::DeleteEdge(e) => {
                        let r = [3.0, live_edge(&edge_alive, e)?, 0.0, 0.0, 0.0, 0.0];
                        if let Some(a) = edge_alive.get_mut(e) {
                            *a = false;
                        }
                        r
                    }
                    Mutation::DeleteFace(f) => {
                        let r = [4.0, live_face(&face_alive, f)?, 0.0, 0.0, 0.0, 0.0];
                        if let Some(a) = face_alive.get_mut(f) {
                            *a = false;
                        }
                        r
                    }
                    Mutation::CPolyRemove(c, f) => {
                        if c < nf {
                            return Err(bad("not a composite face"));
                        }
                        [
                            5.0,
                            live_face(&face_alive, c)?,
                            idx(f, n_all_faces, "face")?,
                            0.0,
                            0.0,
                            0.0,
                        ]
                    }
                    Mutation::SetNormalUninit(f) => {
                        [6.0, live_face(&face_alive, f)?, 0.0, 0.0, 0.0, 0.0]
                    }
                    Mutation::SetEdgeTolerance(e, t) => {
                        [7.0, live_edge(&edge_alive, e)?, 0.0, t, 0.0, 0.0]
                    }
                    Mutation::SetFaceTolerance(f, t) => {
                        [8.0, live_face(&face_alive, f)?, 0.0, t, 0.0, 0.0]
                    }
                    Mutation::SetOriginalFace(f, l) => {
                        [9.0, live_face(&face_alive, f)?, label(l)?, 0.0, 0.0, 0.0]
                    }
                    Mutation::ClearAux(e) => [10.0, live_edge(&edge_alive, e)?, 0.0, 0.0, 0.0, 0.0],
                };
                v.extend_from_slice(&rec);
            }
        }
        Op::Kinds(codes) => {
            v.push(codes.len() as f64);
            for &c in codes {
                v.push(c as f64);
            }
        }
    }
    Ok(v)
}

/// Builds the poly B-rep upstream, runs `op` and returns the output sections.
pub fn run(d: &PolyDesc, op: &Op) -> Result<Vec<Section>, FfiError> {
    let input = encode(d, op)?;
    let out = call_with_buffer(4096, |buf, len| {
        // SAFETY: input valid for input.len() reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_query_poly_eval(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let mut secs = Vec::new();
    let mut at = 0usize;
    while at < out.len() {
        let &[key, kind, n] = out
            .get(at..at + 3)
            .ok_or_else(|| bad("truncated section"))?
        else {
            return Err(bad("truncated section"));
        };
        let (key, n) = (key as i32, n as usize);
        let vals = out
            .get(at + 3..at + 3 + n)
            .ok_or_else(|| bad("truncated section"))?;
        secs.push(Section {
            key,
            ints: kind != 0.0,
            values: vals.to_vec(),
        });
        at += 3 + n;
    }
    Ok(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_owner_guards_are_bounded_and_do_not_invent_geometry() {
        let bare = OwnerFace {
            label: 1,
            surface: None,
        };
        assert!(check_owner_faces(&[]).is_ok()); // unresolved is not a null surface
        assert!(check_owner_faces(std::slice::from_ref(&bare)).is_ok());
        assert!(check_owner_faces(&[bare.clone(), bare]).is_err());
        for label in [0, 4, i64::MAX] {
            assert!(check_owner_faces(&[OwnerFace {
                label,
                surface: None
            }])
            .is_err());
        }
        let patch = |net| OwnerFace {
            label: 2,
            surface: Some(OwnerSurface::Bezier { net }),
        };
        assert!(check_owner_faces(&[patch(vec![vec![[0.0; 3]; 2]; 3])]).is_ok());
        for net in [
            Vec::new(),
            vec![vec![[0.0; 3]; 2]],
            vec![vec![[0.0; 3]; 2]; 5],
            vec![vec![[0.0; 3]; 5]; 2],
            vec![vec![[0.0; 3]; 2], vec![[0.0; 3]; 3]],
            vec![vec![[f64::NAN, 0.0, 0.0]; 2]; 2],
        ] {
            assert!(check_owner_faces(&[patch(net)]).is_err());
        }
    }
}
