//! Runner for unit `query.poly_copy`: `SmApiPolyBrepCopy` (the `SmPolyBrep` copy
//! constructor: `SmPolyBrepData::FromPolyBrep` and `SmPolyBrep::MakeTopologyFromData`).
//!
//! C++: `solidcore_ffi::units::query_poly_copy` (shim
//! `harness/cpp/shim/query_poly_copy.cpp`, which documents the block).
//! Rust: [`solidcore::query::poly::copy::poly_brep_copy`].
//!
//! A case is the shim's block: a poly B-rep (regions, shells, vertices, faces with
//! closed, open and last-end-vertex loops, radial rings, aux data, attributes on the
//! poly B-rep). Both sides build it, copy it and dump the copy: tolerance, every
//! region / shell / face (tolerance, stored normal and deviation) / loop (orientation,
//! closed) / edge (tolerance, start vertex, radial and vertex ring links, aux data) /
//! vertex (point, tolerance, edge ring head) by index, the back pointer flag, the
//! composite faces, the copied attributes (id, behaviour, which source attribute), and
//! the source faces' cached normals. Generated cases cover closed meshes, open and
//! unfinished loops, faces without loops, several regions and shells, faces with holes,
//! given normals, three-face radial rings, missing rings, shared aux data, attributes of
//! every behaviour (temporary ones are not copied), degenerate faces (collinear or
//! repeated points, a one-edge open loop: the `(1, 1, 1)` normals), at tiny, unit and
//! huge scales. Uncompared (not copied; follow-up): composite faces, back pointers,
//! original keys and vertex normals of the source.

// The generator indexes the meshes it has just built (as other runners do).
#![allow(clippy::indexing_slicing)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::query::poly::copy::{poly_brep_copy, PolyCopyAttributes};
use solidcore::query::poly::{PolyBrep, PolyEdgeId, PolyShellId, PolyVertexId};
use solidcore_ffi::units::query_poly_copy as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 500;

/// `query.poly_copy` runner.
pub struct Runner;

/// The shim's input block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    block: Vec<f64>,
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn output(flat: Vec<f64>) -> Output {
    Output::new().values("out", flat)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::eval(&i.block) {
        Ok(v) => Ok(output(v)),
        Err(solidcore_ffi::FfiError::InvalidArgument(r)) => Ok(Output::rejected(r)),
        Err(e) => Err(herr(e)),
    }
}

// ---- Rust side ----------------------------------------------------------------------

const SOURCE: ObjectKey = ObjectKey(1);
const COPY: ObjectKey = ObjectKey(2);

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = *self.v.get(self.at).ok_or_else(|| herr("short block"))?;
        self.at += 1;
        Ok(x)
    }
    fn int(&mut self) -> Result<usize, HarnessError> {
        Ok(self.num()? as usize)
    }
    fn vec(&mut self) -> Result<Vec3, HarnessError> {
        Ok(Vec3::new(self.num()?, self.num()?, self.num()?))
    }
}

fn rs<T>(r: solidcore::Result<T>) -> Result<T, HarnessError> {
    r.map_err(|e| herr(format!("rust build: {e}")))
}

fn pick<T: Copy>(v: &[T], i: usize) -> Result<T, HarnessError> {
    v.get(i).copied().ok_or_else(|| herr("index"))
}

/// The source poly B-rep (the shim's construction order) and its attributes.
fn build(
    k: &Kernel,
    rd: &mut Rd<'_>,
    store: &mut AttributeStore,
) -> Result<(PolyBrep, Vec<solidcore::geom::attribute::AttributeHandle>), HarnessError> {
    let tol = rd.num()?;
    let mut b = PolyBrep::new(k, tol);
    let nr = rd.int()?;
    let mut regions = Vec::new();
    for _ in 0..nr {
        regions.push(rs(b.add_region())?);
    }
    let mut shells: Vec<PolyShellId> = Vec::new();
    for &r in &regions {
        let ns = rd.int()?;
        for _ in 0..ns {
            shells.push(rs(b.add_shell(r))?);
        }
    }
    let nv = rd.int()?;
    let mut verts: Vec<PolyVertexId> = Vec::new();
    for _ in 0..nv {
        let p = rd.vec()?;
        let t = rd.num()?;
        verts.push(rs(b.add_vertex(k, p, t))?);
    }
    let mut edges: Vec<PolyEdgeId> = Vec::new();
    let nf = rd.int()?;
    for _ in 0..nf {
        let shell = pick(&shells, rd.int()?)?;
        let ftol = rd.num()?;
        let has_n = rd.num()? != 0.0;
        let n = rd.vec()?;
        let face = rs(b.add_face(k, shell, ftol, has_n.then_some(n)))?;
        let nl = rd.int()?;
        for _ in 0..nl {
            let mode = rd.int()?;
            let etol = rd.num()?;
            let kk = rd.int()?;
            let vs = (0..kk)
                .map(|_| pick(&verts, rd.int()?))
                .collect::<Result<Vec<_>, _>>()?;
            let lp = rs(b.start_poly_edge_loop(face))?;
            // (the FFI guard refuses these loops: no vertex, or an open loop without an
            // edge; `(j + 1) % kk` and `kk - 1` need them)
            if kk == 0 || (mode == 2 && kk < 2) {
                return Err(herr("loop without vertices, or open loop without edges"));
            }
            let ne = if mode == 2 { kk - 1 } else { kk };
            for j in 0..ne {
                let (v0, v1) = (vs[j], vs[(j + 1) % kk]);
                let (p0, p1) = (rs(b.vertex_point(v0))?, rs(b.vertex_point(v1))?);
                edges.push(rs(b.add_poly_edge(k, lp, etol, p0, p1, v0))?);
            }
            if mode == 1 {
                // (the status is ignored by the shim)
                let _ = b.finish_poly_edge_loop(lp);
            } else if mode == 2 {
                rs(b.set_last_end_poly_vertex(lp, Some(vs[kk - 1])))?;
            }
        }
    }
    let ng = rd.int()?;
    for _ in 0..ng {
        let kk = rd.int()?;
        let ring = (0..kk)
            .map(|_| pick(&edges, rd.int()?))
            .collect::<Result<Vec<_>, _>>()?;
        rs(b.link_radials(&ring))?;
    }
    let na = rd.int()?;
    let mut aux = Vec::new();
    for _ in 0..na {
        let n = rd.vec()?;
        let uv = Vec2::new(rd.num()?, rd.num()?);
        aux.push(rs(b.add_aux_data(n, uv))?);
    }
    let nea = rd.int()?;
    for _ in 0..nea {
        let e = pick(&edges, rd.int()?)?;
        let a = pick(&aux, rd.int()?)?;
        rs(b.edge_set_aux_data(e, Some(a)))?;
    }
    let nat = rd.int()?;
    let mut handles = Vec::new();
    for _ in 0..nat {
        let id = rd.int()? as u64;
        let value = rd.num()? as i64;
        let beh = rs(Behavior::from_code(rd.int()? as i32))?;
        let h = rs(store.insert(Attribute::long(id, value, beh)))?;
        rs(store.add_attribute(SOURCE, h))?;
        handles.push(h);
    }
    Ok((b, handles))
}

fn ix<T: Ord>(m: &BTreeMap<T, usize>, x: Option<T>) -> f64 {
    match x {
        None => -1.0,
        Some(x) => m.get(&x).map_or(-2.0, |&i| i as f64),
    }
}

/// The shim's dump of `b`.
fn dump(
    b: &PolyBrep,
    store: &AttributeStore,
    src_attrs: &[solidcore::geom::attribute::AttributeHandle],
    o: &mut Vec<f64>,
) -> solidcore::Result<()> {
    o.push(b.tolerance());
    let mut edges: Vec<PolyEdgeId> = Vec::new();
    let regions = b.poly_regions();
    o.push(regions.len() as f64);
    for r in regions {
        let shells = b.region_poly_shells(r)?;
        o.push(shells.len() as f64);
        for s in shells {
            let faces = b.shell_poly_faces(s)?;
            o.push(faces.len() as f64);
            for f in faces {
                o.push(b.face_tolerance(f)?);
                o.extend(b.face_stored_normal(f)?.to_array());
                o.push(b.face_max_deviation(f)?);
                let loops = b.face_poly_loops(f)?;
                o.push(loops.len() as f64);
                for l in loops {
                    let es = b.loop_poly_edges(l)?;
                    o.push(f64::from(b.loop_orientation(l)? as i32));
                    o.push(f64::from(u8::from(b.loop_is_closed(l)?)));
                    o.push(es.len() as f64);
                    edges.extend(es);
                }
            }
        }
    }
    let verts = b.poly_vertices();
    let mut emap = BTreeMap::new();
    for (i, &e) in edges.iter().enumerate() {
        emap.entry(e).or_insert(i);
    }
    let mut vmap = BTreeMap::new();
    for (i, &v) in verts.iter().enumerate() {
        vmap.entry(v).or_insert(i);
    }
    o.push(edges.len() as f64);
    for &e in &edges {
        let l = b.edge_links(e)?;
        o.push(b.edge_tolerance(e)?);
        o.push(ix(&vmap, b.edge_start_poly_vertex(e)?));
        o.push(ix(&emap, Some(l.next_radial)));
        o.push(ix(&emap, Some(l.last_radial)));
        o.push(ix(&emap, l.next_at_v));
        o.push(ix(&emap, l.prev_at_v));
        o.push(f64::from(u8::from(b.edge_aux_data(e)?.is_some())));
    }
    o.push(verts.len() as f64);
    for &v in &verts {
        o.extend(b.vertex_point(v)?.to_array());
        o.push(b.vertex_tolerance(v)?);
        o.push(ix(&emap, b.vertex_edge_list(v)?));
    }
    o.push(f64::from(u8::from(b.ok_back_ptrs())));
    o.push(b.cpoly_faces().len() as f64);
    let at = store.attributes(COPY);
    o.push(at.len() as f64);
    for h in at {
        let a = store
            .get(h)
            .ok_or_else(|| solidcore::SolidError::sm_err("difftest::query_poly_copy::dump"))?;
        o.push(a.attribute_id() as f64);
        o.push(f64::from(a.behavior().code()));
        o.push(src_attrs.iter().position(|&x| x == h).map_or(-2.0, |i| i as f64));
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut rd = Rd {
        v: &i.block,
        at: 0,
    };
    let mut store = AttributeStore::new();
    let (mut src, handles) = build(&k, &mut rd, &mut store)?;
    let copy = match poly_brep_copy(
        &k,
        &mut src,
        Some(PolyCopyAttributes {
            store: &mut store,
            source: SOURCE,
            copy: COPY,
        }),
    ) {
        Ok(c) => c,
        Err(e) => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
        }
    };
    let mut o = vec![0.0];
    if let Err(e) = dump(&copy, &store, &handles, &mut o) {
        return Err(herr(format!("dump: {e}")));
    }
    let faces = src.poly_faces(None).map_err(herr)?;
    o.push(faces.len() as f64);
    for f in faces {
        o.extend(src.face_stored_normal(f).map_err(herr)?.to_array());
        o.push(src.face_max_deviation(f).map_err(herr)?);
    }
    Ok(output(o))
}

// ---- generation ---------------------------------------------------------------------

/// A face loop: its mode and point indices.
type MeshLoop = (usize, Vec<usize>);

/// A mesh: points and faces (shell index, loops of point indices with their mode).
#[derive(Default)]
struct Mesh {
    pts: Vec<[f64; 3]>,
    faces: Vec<(usize, Vec<MeshLoop>)>,
}

fn cube(shell: usize, at: [f64; 3], triangulated: bool, m: &mut Mesh) {
    let base = m.pts.len();
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ] {
        m.pts.push([p[0] + at[0], p[1] + at[1], p[2] + at[2]]);
    }
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        let q = q.map(|i| base + i);
        if triangulated {
            m.faces.push((shell, vec![(1, vec![q[0], q[1], q[2]])]));
            m.faces.push((shell, vec![(1, vec![q[0], q[2], q[3]])]));
        } else {
            m.faces.push((shell, vec![(1, q.to_vec())]));
        }
    }
}

/// A square face with a square hole (two loops).
fn frame_face(shell: usize, m: &mut Mesh) {
    let base = m.pts.len();
    for p in [
        [0.0, 0.0, 0.0],
        [3.0, 0.0, 0.0],
        [3.0, 3.0, 0.0],
        [0.0, 3.0, 0.0],
        [1.0, 1.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, 2.0, 0.0],
        [2.0, 1.0, 0.0],
    ] {
        m.pts.push(p);
    }
    m.faces.push((
        shell,
        vec![
            (1, (0..4).map(|i| base + i).collect()),
            (1, (4..8).map(|i| base + i).collect()),
        ],
    ));
}

/// A fan of triangles around a centre point (an open surface).
fn fan(shell: usize, n: usize, m: &mut Mesh) {
    let base = m.pts.len();
    m.pts.push([0.0, 0.0, 0.0]);
    for i in 0..n {
        let a = std::f64::consts::TAU * i as f64 / n as f64;
        m.pts.push([a.cos(), a.sin(), 0.1 * (i % 2) as f64]);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.faces.push((shell, vec![(1, vec![base, base + 1 + i, base + 1 + j])]));
    }
}

/// A degenerate face (`kind`): a triangle of collinear points or with a repeated
/// point, a quad of collinear points or with a repeated point (`GetNormal`'s
/// `(1, 1, 1)` reset and degenerate N-gon paths), or a two-vertex open loop (one edge,
/// mode 2: the default normal).
fn sliver(shell: usize, kind: u64, m: &mut Mesh) {
    let base = m.pts.len();
    let (pts, mode): (Vec<[f64; 3]>, usize) = match kind {
        0 => (vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 1),
        1 => (vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 0.0]], 1),
        2 => (
            vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 2.0, 0.0], [3.0, 3.0, 0.0]],
            1,
        ),
        3 => (
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            1,
        ),
        _ => (vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]], 2),
    };
    let n = pts.len();
    for p in pts {
        m.pts.push([p[0], p[1] + 5.0, p[2]]);
    }
    m.faces.push((shell, vec![(mode, (base..base + n).collect())]));
}

/// The directed edges of the mesh in creation order (`(from, to)`; open loops of mode 2
/// lack their closing edge).
fn directed(m: &Mesh) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (_, f) in &m.faces {
        for (mode, l) in f {
            let ne = if *mode == 2 { l.len() - 1 } else { l.len() };
            for j in 0..ne {
                out.push((l[j], l[(j + 1) % l.len()]));
            }
        }
    }
    out
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let mut m = Mesh::default();
    let regions: Vec<usize> = match rng.below(5) {
        0 => vec![],
        1 => vec![2],
        2 => vec![1, 2],
        _ => vec![1],
    };
    let n_shells: usize = regions.iter().sum();
    if n_shells > 0 {
        let pick_shell = |rng: &mut Rng| rng.below(n_shells as u64) as usize;
        let parts = 1 + rng.below(3) as usize;
        for p in 0..parts {
            let shell = pick_shell(rng);
            match rng.below(5) {
                0 => frame_face(shell, &mut m),
                1 => fan(shell, 3 + rng.below(6) as usize, &mut m),
                _ => cube(shell, [2.0 * p as f64, 0.0, 0.0], rng.chance(0.5), &mut m),
            }
        }
        // loop modes: some open (mode 0) or with a last end vertex (mode 2)
        for (_, f) in &mut m.faces {
            for (mode, l) in f.iter_mut() {
                *mode = match rng.below(12) {
                    0 => 0,
                    1 if l.len() >= 2 => 2,
                    _ => 1,
                };
            }
        }
        // degenerate faces: GetNormal's zero cross product and degenerate N-gon paths
        if rng.chance(0.3) {
            let shell = pick_shell(rng);
            sliver(shell, rng.below(5), &mut m);
        }
        // a face without loops
        if rng.chance(0.08) {
            let shell = pick_shell(rng);
            m.faces.push((shell, Vec::new()));
        }
    }

    let s = match rng.below(8) {
        0 => 10f64.powi(rng.range_i64(-6, -2) as i32),
        1 => 10f64.powi(rng.range_i64(2, 6) as i32),
        _ => rng.uniform(0.5, 4.0),
    };
    let tol = if rng.chance(0.8) { 1e-5 * s.max(1.0) } else { rng.uniform(1e-9, 1e-3) };
    let mut b = vec![tol, regions.len() as f64];
    b.extend(regions.iter().map(|&n| n as f64));
    // unused vertices too
    let extra = if rng.chance(0.1) { 2 } else { 0 };
    b.push((m.pts.len() + extra) as f64);
    for p in &m.pts {
        b.extend([p[0] * s, p[1] * s, p[2] * s]);
        b.push(if rng.chance(0.1) { 2.0 * tol } else { tol });
    }
    for _ in 0..extra {
        b.extend([rng.uniform(-1.0, 1.0) * s, 0.0, 0.0, tol]);
    }
    b.push(m.faces.len() as f64);
    for (shell, loops) in &m.faces {
        b.push(*shell as f64);
        b.push(if rng.chance(0.1) { 3.0 * tol } else { tol });
        if rng.chance(0.15) {
            b.extend([1.0, rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), 1.0]);
        } else {
            b.extend([0.0, 0.0, 0.0, 0.0]);
        }
        b.push(loops.len() as f64);
        for (mode, l) in loops {
            b.push(*mode as f64);
            b.push(tol);
            b.push(l.len() as f64);
            b.extend(l.iter().map(|&i| i as f64));
        }
    }
    // radial rings: each directed edge with its reverse (or a same-direction twin)
    let d = directed(&m);
    let mut used = vec![false; d.len()];
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for (i, &(p, q)) in d.iter().enumerate() {
        if used[i] {
            continue;
        }
        let mut ring = vec![i];
        for (j, &(p2, q2)) in d.iter().enumerate().skip(i + 1) {
            if !used[j] && ((p2 == q && q2 == p) || (p2 == p && q2 == q)) && ring.len() < 4 {
                ring.push(j);
            }
        }
        if ring.len() > 1 {
            for &e in &ring {
                used[e] = true;
            }
            rings.push(ring);
        }
    }
    match rng.below(8) {
        0 if !rings.is_empty() => {
            let i = rng.below(rings.len() as u64) as usize;
            rings.remove(i);
        }
        1 if rings.len() > 1 => {
            let extra = rings.remove(1);
            if rings[0].len() + extra.len() <= ffi::MAX_RING {
                rings[0].extend(extra);
            }
        }
        _ => {}
    }
    b.push(rings.len() as f64);
    for r in &rings {
        b.push(r.len() as f64);
        b.extend(r.iter().map(|&e| e as f64));
    }
    // aux data, some shared by several edges
    let na = if d.is_empty() { 0 } else { rng.below(4) as usize };
    b.push(na as f64);
    for _ in 0..na {
        b.extend([0.0, 0.0, 1.0, rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)]);
    }
    let nea = if na == 0 { 0 } else { rng.below(d.len().min(6) as u64 + 1) as usize };
    let mut aux_edges: Vec<usize> = (0..d.len()).collect();
    aux_edges.truncate(nea);
    b.push(aux_edges.len() as f64);
    for e in aux_edges {
        b.extend([e as f64, rng.below(na as u64) as f64]);
    }
    // attributes on the poly B-rep, every behaviour
    let nat = rng.below(4) as usize;
    b.push(nat as f64);
    for _ in 0..nat {
        b.extend([
            (1 + rng.below(5)) as f64,
            rng.range_i64(-100, 100) as f64,
            rng.below(5) as f64,
        ]);
    }
    Inputs { block: b }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.poly_copy"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
