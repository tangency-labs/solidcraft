//! Runner for unit `query.poly_properties`: `SmApiPolyBrepIsManifoldSolid`,
//! `SmApiPolyBrepComputeVolume`, `SmApiPolyBrepComputeMassProperties`,
//! `SmPolyBrep::ComputeProperties` and `SmPolyFace::ComputeProperties`.
//!
//! C++: `solidcore_ffi::units::query_poly_properties` (shim
//! `harness/cpp/shim/query_poly_properties.cpp`, which documents the block).
//! Rust: [`solidcore::query::poly_properties`].
//!
//! A case is the shim's block: the operation, the properties asked for, the origin and
//! density, and a poly B-rep (vertices, faces of closed loops, radial rings). Both sides
//! build their own poly B-rep from it. The output is the status and every value the
//! operation returns (per face for the face operation). Generated cases cover closed
//! meshes (tetrahedra, cubes, triangulated cubes, octahedra, prisms, latitude-longitude
//! spheres, tori) under random frames at tiny, unit and huge scales, with outward,
//! reversed and mixed winding, a missing face, a missing radial link, a three-face edge,
//! non-planar quads, faces with holes, a zero-length first edge, given normals, invalid
//! and boundary densities and origins, and every `SmMassPropertiesType`.

// The generator indexes the meshes it has just built (as other runners do).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::query::poly::{PolyBrep, PolyEdgeId};
use solidcore::query::poly_properties::{
    poly_brep_compute_mass_properties, poly_brep_compute_volume, poly_brep_is_manifold_solid,
    MassPropertiesType, PolyProperties,
};
use solidcore::SolidError;
use solidcore_ffi::units::query_poly_properties as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `query.poly_properties` runner.
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

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        // SM_ERR_INVALID_INPUT
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        _ => -1.0,
    }
}

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

fn which_of(v: usize) -> Result<MassPropertiesType, HarnessError> {
    Ok(match v {
        0 => MassPropertiesType::Area,
        1 => MassPropertiesType::Volume,
        2 => MassPropertiesType::Centroid,
        3 => MassPropertiesType::Moments,
        4 => MassPropertiesType::All,
        _ => return Err(herr("bad properties type")),
    })
}

fn rs<T>(r: solidcore::Result<T>) -> Result<T, HarnessError> {
    r.map_err(|e| herr(format!("rust build: {e}")))
}

/// The poly B-rep of the block (the shim's construction order).
fn build(k: &Kernel, rd: &mut Rd<'_>, tol: f64) -> Result<PolyBrep, HarnessError> {
    let mut b = PolyBrep::new(k, tol);
    let region = rs(b.add_region())?;
    let shell = rs(b.add_shell(region))?;
    let nv = rd.int()?;
    let mut verts = Vec::new();
    for _ in 0..nv {
        let p = rd.vec()?;
        let t = rd.num()?;
        verts.push(rs(b.add_vertex(k, p, t))?);
    }
    let mut edges: Vec<PolyEdgeId> = Vec::new();
    let nf = rd.int()?;
    for _ in 0..nf {
        let ftol = rd.num()?;
        let has_n = rd.num()? != 0.0;
        let n = rd.vec()?;
        let face = rs(b.add_face(k, shell, ftol, has_n.then_some(n)))?;
        let nl = rd.int()?;
        for _ in 0..nl {
            let etol = rd.num()?;
            let kk = rd.int()?;
            let vs = (0..kk)
                .map(|_| {
                    let i = rd.int()?;
                    verts.get(i).copied().ok_or_else(|| herr("vertex index"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let lp = rs(b.start_poly_edge_loop(face))?;
            for j in 0..kk {
                let (v0, v1) = (vs[j], vs[(j + 1) % kk]);
                let (p0, p1) = (rs(b.vertex_point(v0))?, rs(b.vertex_point(v1))?);
                edges.push(rs(b.add_poly_edge(k, lp, etol, p0, p1, v0))?);
            }
            // (the status is ignored by the shim)
            let _ = b.finish_poly_edge_loop(lp);
        }
    }
    let ng = rd.int()?;
    for _ in 0..ng {
        let kk = rd.int()?;
        let ring = (0..kk)
            .map(|_| {
                let i = rd.int()?;
                edges.get(i).copied().ok_or_else(|| herr("edge index"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        rs(b.link_radials(&ring))?;
    }
    Ok(b)
}

fn put_props(v: &mut Vec<f64>, p: &PolyProperties) {
    v.extend([p.area, p.volume]);
    v.extend(p.barycenter.to_array());
    v.extend(p.moments[0].to_array());
    v.extend(p.moments[1].to_array());
}

/// Pushes `0` and the values on success, the status on failure; `Unported` defers.
fn push<T>(
    v: &mut Vec<f64>,
    r: solidcore::Result<T>,
    f: impl FnOnce(&mut Vec<f64>, T),
) -> Result<(), Box<Output>> {
    match r {
        Ok(x) => {
            v.push(0.0);
            f(v, x);
            Ok(())
        }
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                Err(Box::new(o))
            } else {
                v.push(status(&e));
                Ok(())
            }
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut rd = Rd {
        v: &i.block,
        at: 0,
    };
    let op = rd.int()?;
    let which = which_of(rd.int()?)?;
    let origin = rd.vec()?;
    let density = rd.num()?;
    let tol = rd.num()?;
    let mut b = build(&k, &mut rd, tol)?;
    let mut v = Vec::new();
    let r = match op {
        0 => push(&mut v, poly_brep_is_manifold_solid(&b), |v, m| {
            v.push(f64::from(u8::from(m)));
        }),
        1 => push(&mut v, poly_brep_compute_volume(&k, &mut b), Vec::push),
        2 => push(
            &mut v,
            poly_brep_compute_mass_properties(&k, &mut b, density, origin),
            |v, m| {
                v.extend([m.area, m.volume, m.mass]);
                v.extend(m.centroid.to_array());
                v.extend(m.moments_of_inertia.to_array());
                v.extend(m.products_of_inertia.to_array());
            },
        ),
        3 => push(&mut v, b.compute_properties(&k, origin, which), |v, p| {
            put_props(v, &p);
        }),
        _ => {
            let faces = match b.poly_faces(None) {
                Ok(f) => f,
                Err(e) => return Ok(Output::err(e.to_string())),
            };
            v.extend([0.0, faces.len() as f64]);
            let mut r = Ok(());
            for f in faces {
                let p = b.face_compute_properties(&k, f, origin, which);
                r = push(&mut v, p, |v, p| put_props(v, &p));
                if r.is_err() {
                    break;
                }
            }
            r
        }
    };
    match r {
        Ok(()) => Ok(output(v)),
        Err(deferred) => Ok(*deferred),
    }
}

// ---- generation ---------------------------------------------------------------------

/// A mesh: points and faces (each a list of loops of point indices).
#[derive(Default)]
struct Mesh {
    pts: Vec<[f64; 3]>,
    faces: Vec<Vec<Vec<usize>>>,
}

impl Mesh {
    fn tri(&mut self, a: usize, b: usize, c: usize) {
        self.faces.push(vec![vec![a, b, c]]);
    }
    fn quad(&mut self, a: usize, b: usize, c: usize, d: usize) {
        self.faces.push(vec![vec![a, b, c, d]]);
    }
}

fn cube(triangulated: bool) -> Mesh {
    let mut m = Mesh {
        pts: vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
        ..Mesh::default()
    };
    for q in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        if triangulated {
            m.tri(q[0], q[1], q[2]);
            m.tri(q[0], q[2], q[3]);
        } else {
            m.quad(q[0], q[1], q[2], q[3]);
        }
    }
    m
}

fn tetra() -> Mesh {
    let mut m = Mesh {
        pts: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        ..Mesh::default()
    };
    m.tri(0, 2, 1);
    m.tri(0, 1, 3);
    m.tri(1, 2, 3);
    m.tri(2, 0, 3);
    m
}

fn octahedron() -> Mesh {
    let mut m = Mesh {
        pts: vec![
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ],
        ..Mesh::default()
    };
    for i in 0..4 {
        let j = (i + 1) % 4;
        m.tri(i, j, 4);
        m.tri(j, i, 5);
    }
    m
}

/// A prism over a regular `n`-gon, its caps as single polygons.
fn prism(n: usize, h: f64) -> Mesh {
    let mut m = Mesh::default();
    for z in [0.0, h] {
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            m.pts.push([a.cos(), a.sin(), z]);
        }
    }
    m.faces.push(vec![(0..n).rev().collect()]);
    m.faces.push(vec![(n..2 * n).collect()]);
    for i in 0..n {
        let j = (i + 1) % n;
        m.quad(i, j, n + j, n + i);
    }
    m
}

/// A latitude-longitude sphere with poles (triangles at the poles, quads elsewhere).
fn sphere(nu: usize, nv: usize) -> Mesh {
    let mut m = Mesh::default();
    m.pts.push([0.0, 0.0, -1.0]);
    for j in 1..nv {
        let t = std::f64::consts::PI * j as f64 / nv as f64 - std::f64::consts::FRAC_PI_2;
        for i in 0..nu {
            let a = std::f64::consts::TAU * i as f64 / nu as f64;
            m.pts.push([t.cos() * a.cos(), t.cos() * a.sin(), t.sin()]);
        }
    }
    m.pts.push([0.0, 0.0, 1.0]);
    let top = m.pts.len() - 1;
    let at = |j: usize, i: usize| 1 + (j - 1) * nu + i % nu;
    for i in 0..nu {
        m.tri(0, at(1, i + 1), at(1, i));
        m.tri(top, at(nv - 1, i), at(nv - 1, i + 1));
    }
    for j in 1..nv - 1 {
        for i in 0..nu {
            m.quad(at(j, i), at(j, i + 1), at(j + 1, i + 1), at(j + 1, i));
        }
    }
    m
}

/// A torus of quads.
fn torus(nu: usize, nv: usize, r: f64) -> Mesh {
    let mut m = Mesh::default();
    for j in 0..nv {
        let b = std::f64::consts::TAU * j as f64 / nv as f64;
        for i in 0..nu {
            let a = std::f64::consts::TAU * i as f64 / nu as f64;
            let rr = 1.0 + r * b.cos();
            m.pts.push([rr * a.cos(), rr * a.sin(), r * b.sin()]);
        }
    }
    let at = |j: usize, i: usize| (j % nv) * nu + i % nu;
    for j in 0..nv {
        for i in 0..nu {
            m.quad(at(j, i), at(j, i + 1), at(j + 1, i + 1), at(j + 1, i));
        }
    }
    m
}

/// A square face with a square hole (two loops), alone: an open mesh.
fn frame_face() -> Mesh {
    Mesh {
        pts: vec![
            [0.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [3.0, 3.0, 0.0],
            [0.0, 3.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 2.0, 0.0],
            [2.0, 2.0, 0.0],
            [2.0, 1.0, 0.0],
        ],
        faces: vec![vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7]]],
    }
}

/// The directed edges of the mesh in creation order: `(from, to)`.
fn directed(m: &Mesh) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for f in &m.faces {
        for l in f {
            for j in 0..l.len() {
                out.push((l[j], l[(j + 1) % l.len()]));
            }
        }
    }
    out
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let shape = rng.below(9);
    let mut m = match shape {
        0 => tetra(),
        1 => cube(false),
        2 => cube(true),
        3 => octahedron(),
        4 => prism(3 + rng.below(6) as usize, rng.uniform(0.2, 3.0)),
        5 => sphere(3 + rng.below(8) as usize, 2 + rng.below(6) as usize),
        6 => torus(3 + rng.below(6) as usize, 3 + rng.below(6) as usize, rng.uniform(0.1, 0.6)),
        7 => frame_face(),
        _ => cube(rng.chance(0.5)),
    };
    // non-planar quads
    if rng.chance(0.15) {
        for p in &mut m.pts {
            p[2] += rng.uniform(-0.05, 0.05);
        }
    }
    let defect = rng.below(12);
    match defect {
        // reversed winding
        0 | 1 => {
            for f in &mut m.faces {
                for l in f.iter_mut() {
                    l.reverse();
                }
            }
        }
        // one face reversed (mixed winding)
        2 => {
            let i = rng.below(m.faces.len() as u64) as usize;
            for l in &mut m.faces[i] {
                l.reverse();
            }
        }
        // a face missing (open)
        3 if m.faces.len() > 1 => {
            let i = rng.below(m.faces.len() as u64) as usize;
            m.faces.remove(i);
        }
        // a zero-length first edge: the first vertex of a loop repeated
        4 => {
            let i = rng.below(m.faces.len() as u64) as usize;
            let l = &mut m.faces[i][0];
            let v = l[0];
            l.insert(1, v);
        }
        _ => {}
    }

    // frame and scale
    let s = match rng.below(8) {
        0 => 10f64.powi(rng.range_i64(-6, -2) as i32),
        1 => 10f64.powi(rng.range_i64(2, 6) as i32),
        _ => rng.uniform(0.5, 4.0),
    };
    let a = rng.uniform(0.0, 6.0);
    let (ca, sa) = (a.cos(), a.sin());
    let shift = [rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s];
    let far = if rng.chance(0.1) { 1e3 * s } else { 0.0 };
    let pts: Vec<[f64; 3]> = m
        .pts
        .iter()
        .map(|p| {
            let (x, y, z) = (p[0] * s, p[1] * s, p[2] * s);
            [ca * x - sa * y + shift[0] + far, sa * x + ca * y + shift[1], z + shift[2]]
        })
        .collect();

    let op = rng.below(5) as f64;
    let which = rng.below(5) as f64;
    let origin = match rng.below(6) {
        0 => [0.0, 0.0, 0.0],
        // not a valid double: only the mass properties check it (the other operations
        // would overflow)
        1 if op == 2.0 => [1e300, 0.0, 0.0],
        2 => [far + 1e3 * s, 0.0, 0.0],
        _ => [rng.uniform(-3.0, 3.0) * s, rng.uniform(-3.0, 3.0) * s, rng.uniform(-3.0, 3.0) * s],
    };
    let density = match rng.below(8) {
        0 => 0.0,
        1 => 1e-13,
        2 => 1e-12,
        3 => -1.0,
        4 => 1e300,
        _ => rng.uniform(0.1, 10.0),
    };
    let tol = if rng.chance(0.8) { 1e-5 } else { rng.uniform(1e-9, 1e-3) };
    let mut b = vec![op, which, origin[0], origin[1], origin[2], density, tol];
    b.push(pts.len() as f64);
    for p in &pts {
        b.extend(p);
        b.push(tol);
    }
    b.push(m.faces.len() as f64);
    for f in &m.faces {
        b.push(tol);
        if rng.chance(0.1) {
            // a given (unit or not) normal
            b.extend([1.0, rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), 1.0]);
        } else {
            b.extend([0.0, 0.0, 0.0, 0.0]);
        }
        b.push(f.len() as f64);
        for l in f {
            b.push(tol);
            b.push(l.len() as f64);
            b.extend(l.iter().map(|&i| i as f64));
        }
    }
    // radial rings: each directed edge with its reverse, or several for a shared edge
    let d = directed(&m);
    let mut used = vec![false; d.len()];
    let mut rings: Vec<Vec<usize>> = Vec::new();
    for (i, &(p, q)) in d.iter().enumerate() {
        if used[i] {
            continue;
        }
        let mut ring = vec![i];
        for (j, &(p2, q2)) in d.iter().enumerate().skip(i + 1) {
            if !used[j] && ((p2 == q && q2 == p) || (p2 == p && q2 == q)) {
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
    match rng.below(10) {
        // a missing radial link
        0 if !rings.is_empty() => {
            let i = rng.below(rings.len() as u64) as usize;
            rings.remove(i);
        }
        // three faces at an edge: merge two rings
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
    Inputs { block: b }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.poly_properties"
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
