//! Runner for unit `geom.tools`: point / vertex / coincident-edge / edge / face lookups.
//!
//! C++: `solidcore_ffi::units::geom_tools` (upstream `SmTools::*`,
//! `source/SMLib/src/SmTools.cpp`).
//! Rust: `solidcore::geom::tools`.
//!
//! A case is one call, tagged by `op`:
//!
//! | `op`         | inputs                                                        | outputs |
//! |--------------|---------------------------------------------------------------|---------|
//! | `point`      | `pts`, `target`, `tol`                                        | `status`, `found`, `index`, `dist` = [closest, second] |
//! | `vertex`     | `vertices` (`[x,y,z,tol]` each), `pnt`, `tol`                 | `status`, `found`, `index`, `dist` |
//! | `coincident` | `file` (ASCII `.smb`), `n_edges`, `edge`, `copy_curve`, `swap`, `tol`, `start` | `status`, `found`, `same_orient`, `index`, `max_dev` |
//! | `edge`       | `file`, `pnt`                                                 | `status` (+ `index`, `param` on success) |
//! | `face`       | `file`, `pnt`                                                 | `status` (+ `face` on success) |
//!
//! `coincident` uses the curve, interval and end vertices of edge `edge` of the B-rep
//! (`SmBrep::GetEdges` order) as the target and searches every edge from `start`; with
//! `copy_curve` the target is a copy of the curve (no owner), else the edge's own curve
//! (the edge owning it is skipped). The Rust side imports the file's B-rep through the
//! `topology.brep_p1` export (`file_brep`), as for `edge` / `face`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::error::SolidError;
use solidcore::geom::tools::{self, CoincidentEdgeQuery};
use solidcore::math::{Point3, BIG_DOUBLE};
use solidcore::topology::{Brep, VertexId};
use solidcore_ffi::units::geom_tools as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};


/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `geom.tools` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Point {
        pts: Vec<[f64; 3]>,
        target: usize,
        tol: f64,
    },
    Vertex {
        vertices: Vec<[f64; 4]>,
        pnt: [f64; 3],
        tol: f64,
    },
    Coincident {
        file: String,
        n_edges: usize,
        edge: usize,
        copy_curve: bool,
        swap: bool,
        tol: f64,
        start: usize,
    },
    Edge {
        file: String,
        pnt: [f64; 3],
    },
    Face {
        file: String,
        pnt: [f64; 3],
    },
}

fn harness(e: &dyn std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn status_only(status: i32) -> Output {
    Output::new().int("status", i64::from(status))
}

fn point_out(found: bool, index: i64, dist: Vec<f64>) -> Output {
    status_only(0)
        .int("found", i64::from(found))
        .int("index", index)
        .values("dist", dist)
}

fn coincident_out(found: bool, same: bool, index: i64, max_dev: f64) -> Output {
    status_only(0)
        .int("found", i64::from(found))
        .int("same_orient", i64::from(same))
        .int("index", index)
        .scalar("max_dev", max_dev)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Point { pts, target, tol } => {
            let c = ffi::find_point(pts, *target, *tol).map_err(|e| harness(&e))?;
            if c.status != 0 {
                return Ok(status_only(c.status));
            }
            let ([found, index], dist) = c.out;
            Ok(point_out(found != 0, index, dist.to_vec()))
        }
        Inputs::Vertex { vertices, pnt, tol } => {
            let v: Vec<([f64; 3], f64)> = vertices
                .iter()
                .map(|&[x, y, z, t]| ([x, y, z], t))
                .collect();
            let c = ffi::find_vertex(&v, *pnt, *tol).map_err(|e| harness(&e))?;
            if c.status != 0 {
                return Ok(status_only(c.status));
            }
            let ([found, index], d) = c.out;
            Ok(point_out(found != 0, index, vec![d]))
        }
        Inputs::Coincident {
            file,
            n_edges,
            edge,
            copy_curve,
            swap,
            tol,
            start,
        } => {
            let c = ffi::find_coincident_edge(file, *edge, *copy_curve, *swap, *tol, *start)
                .map_err(|e| harness(&e))?;
            let ([n, found, same, index], dev) = c.out;
            if usize::try_from(n).ok() != Some(*n_edges) {
                return Err(harness(&format!("file has {n} edges, case says {n_edges}")));
            }
            if c.status != 0 {
                return Ok(status_only(c.status));
            }
            Ok(coincident_out(found != 0, same != 0, index, dev))
        }
        Inputs::Edge { file, pnt } => {
            let c = ffi::find_edge(file, *pnt).map_err(|e| harness(&e))?;
            if c.status != 0 {
                return Ok(status_only(c.status));
            }
            let ([_, index], param) = c.out;
            Ok(status_only(0).int("index", index).scalar("param", param))
        }
        Inputs::Face { file, pnt } => {
            let c = ffi::find_face(file, *pnt).map_err(|e| harness(&e))?;
            if c.status != 0 {
                return Ok(status_only(c.status));
            }
            Ok(status_only(0).int("face", c.out[1]))
        }
    }
}

fn p3([x, y, z]: [f64; 3]) -> Point3 {
    Point3::new(x, y, z)
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

fn index_i64(i: usize) -> Result<i64, HarnessError> {
    i64::try_from(i).map_err(|e| harness(&e))
}

fn rust(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    match i {
        Inputs::Point { pts, target, tol } => {
            let pts: Vec<Point3> = pts.iter().copied().map(p3).collect();
            match tools::find_first_point_within_tol_to_pnt(&pts, *target, *tol) {
                Ok(m) => Ok(point_out(
                    m.found,
                    index_i64(m.index)?,
                    vec![m.closest_distance, m.second_closest_distance],
                )),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        Inputs::Vertex { vertices, pnt, tol } => {
            let mut brep = Brep::new();
            let ids: Vec<VertexId> = vertices
                .iter()
                .map(|&[x, y, z, t]| {
                    let v = brep.add_vertex(p3([x, y, z]))?;
                    // the shim assigns the double to the `SmZoneTol3d` member
                    // (`operator=(double)` takes `fabs` only)
                    brep.vertex_mut(v)?.tolerance = solidcore::geom::tol::tol3d(t);
                    Ok(v)
                })
                .collect::<Result<_, SolidError>>()
                .map_err(|e| harness(&e))?;
            match tools::find_first_vertex_within_tol_to_pnt(&brep, &ids, p3(*pnt), *tol) {
                Ok(m) => Ok(point_out(m.found, index_i64(m.index)?, vec![m.distance])),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        Inputs::Coincident {
            file,
            n_edges,
            edge,
            copy_curve,
            swap,
            tol,
            start,
        } => {
            // the file's B-rep in SmBrep::GetEdges order; the target is edge `edge`'s
            // curve (its own: owned by the edge, so skipped; or a copy: no owner)
            let brep = file_brep(file)?;
            let edges = solidcore::topology::brep::p1::edges(&brep, None).map_err(|e| harness(&e))?;
            if edges.len() != *n_edges {
                return Err(harness(&format!("file has {} edges, case says {n_edges}", edges.len())));
            }
            let e = *edges
                .get(*edge)
                .ok_or_else(|| harness(&format!("edge {edge} of {n_edges}")))?;
            let curve = solidcore::topology::edge::curve(&brep, e)
                .map_err(|e| harness(&e))?
                .ok_or_else(|| harness(&"edge without curve"))?
                .clone();
            let v1 = solidcore::topology::edge::vertex(&brep, e)
                .map_err(|e| harness(&e))?
                .ok_or_else(|| harness(&"edge without vertex"))?;
            let v2 = solidcore::topology::edge::other_vertex(&brep, e, Some(v1))
                .map_err(|e| harness(&e))?
                .ok_or_else(|| harness(&"edge without other vertex"))?;
            let (start_vertex, end_vertex) = if *swap { (v2, v1) } else { (v1, v2) };
            let query = CoincidentEdgeQuery {
                start_vertex,
                end_vertex,
                curve: &curve,
                curve_owner: (!*copy_curve).then_some(e),
                interval: solidcore::topology::edge::interval(&brep, e).map_err(|e| harness(&e))?,
                tolerance: *tol,
            };
            match tools::find_coincident_edge(k, &brep, &query, &edges, *start) {
                Ok(Some(m)) => Ok(coincident_out(
                    true,
                    m.same_orientation,
                    index_i64(m.index)?,
                    m.max_deviation,
                )),
                Ok(None) => Ok(coincident_out(false, false, 0, -BIG_DOUBLE)),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        Inputs::Edge { file, pnt } => match tools::find_edge(
            &solidcore::kernel::Kernel::default(),
            &file_brep(file)?,
            p3(*pnt),
        ) {
            Ok(Some(m)) => Ok(status_only(0)
                .int("index", index_i64(m.index)?)
                .scalar("param", m.parameter)),
            Ok(None) => Ok(status_only(SM_ERR)),
            Err(e) => Ok(rust_err(&e)),
        },
        Inputs::Face { file, pnt } => {
            let mut brep = file_brep(file)?;
            match tools::find_face(
                &solidcore::kernel::Kernel::default(),
                &mut brep,
                solidcore::topology::BrepId::from_index(0),
                p3(*pnt),
                &solidcore::topology::brep_cache::arena_identity,
                &|_| None,
            ) {
                // the shim reports the face's index in `GetFaces` (the face list
                // order, not the arena order)
                Ok(Some(f)) => {
                    let faces = solidcore::topology::brep::p1::faces(&brep, None)
                        .map_err(|e| harness(&e))?;
                    let index = faces.iter().position(|&g| g == f).map_or(-1, |i| {
                        i64::try_from(i).unwrap_or(-1)
                    });
                    Ok(status_only(0).int("face", index))
                }
                Ok(None) => Ok(status_only(SM_ERR)),
                Err(e) => Ok(rust_err(&e)),
            }
        }
    }
}

/// The file's B-rep on the Rust side (`SmBrep::ReadFromFile` in the shim), imported
/// through the `topology.brep_p1` export so its edge and face lists are in upstream's
/// list order (`FindEdge` / `FindFace` report indices into them).
fn file_brep(file: &str) -> Result<Brep, HarnessError> {
    crate::units::topology_brep_p1::import_file(file, false)
}

/// Upstream `SM_ERR`.
const SM_ERR: i32 = 1001;

// ---- generation ---------------------------------------------------------------

fn tolerance(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 0.0,
        1 => 1.0e-12,
        2 => 1.0e-6,
        3 => 1.0e-3,
        4 => 0.1,
        5 => 1.0,
        6 => 1.0e3,
        _ => 10f64.powi(rng.range_i64(-10, 2) as i32) * rng.uniform(0.5, 2.0),
    }
}

/// A point near `base` at a distance on the scale of `tol` (or exactly on it).
fn near(rng: &mut Rng, base: [f64; 3], tol: f64) -> [f64; 3] {
    let s = match rng.below(5) {
        0 => 0.0,
        1 => tol * rng.uniform(0.0, 1.0),
        2 => tol * rng.uniform(0.9, 1.1),
        3 => 1.0e-13,
        _ => tol * rng.uniform(1.0, 5.0),
    };
    let d = rng.point3(-1.0, 1.0);
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-300);
    [
        base[0] + s * d[0] / n,
        base[1] + s * d[1] / n,
        base[2] + s * d[2] / n,
    ]
}

fn gen_points(rng: &mut Rng) -> Inputs {
    let tol = tolerance(rng);
    let n = 1 + rng.below(12) as usize;
    let scale = if rng.chance(0.3) { 1.0e3 } else { 10.0 };
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(n + 4);
    for _ in 0..n {
        let p = match pts.last() {
            Some(&prev) if rng.chance(0.4) => near(rng, prev, tol.max(1e-9)),
            Some(&prev) if rng.chance(0.1) => prev,
            _ => rng.point3(-scale, scale),
        };
        pts.push(p);
    }
    let target = rng.below(pts.len() as u64) as usize;
    if rng.chance(0.5) {
        // A neighbour (or two: loops touching at a vertex) near the target.
        let base = pts.get(target).copied().unwrap_or([0.0; 3]);
        for _ in 0..=rng.below(2) {
            let at = rng.below(pts.len() as u64 + 1) as usize;
            pts.insert(at, near(rng, base, tol.max(1e-9)));
        }
        let new_target = pts.iter().position(|p| *p == base).unwrap_or(0);
        return Inputs::Point {
            pts,
            target: new_target,
            tol,
        };
    }
    Inputs::Point { pts, target, tol }
}

fn gen_vertices(rng: &mut Rng) -> Inputs {
    let tol = tolerance(rng);
    let pnt = rng.point3(-10.0, 10.0);
    // Mostly the empty list (the only list the Rust side can search before
    // topology.vertex lands); the rest exercise the C++ path and are deferred.
    let n = if rng.chance(0.5) {
        0
    } else {
        1 + rng.below(6) as usize
    };
    let vertices = (0..n)
        .map(|_| {
            let r = rng.uniform(0.0, 1.0);
            let p = near(rng, pnt, r);
            [p[0], p[1], p[2], tolerance(rng)]
        })
        .collect();
    Inputs::Vertex { vertices, pnt, tol }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.tools"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = if rng.chance(0.8) {
            gen_points(rng)
        } else {
            gen_vertices(rng)
        };
        serde_json::to_value(i).ok()
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
        let k = &solidcore::Kernel::default();
        rust(k, &case.parse()?)
    }
}
