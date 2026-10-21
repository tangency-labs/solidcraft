//! Safe wrapper of the `topology.brep_p4` shim (`harness/cpp/shim/topology_brep_p4.cpp`):
//! `SmBrep::DeleteEdge`, `DeleteVertex`, `ShrinkGeometry`, `StitchFaces` and `~SmBrep`
//! on a B-rep built by the shared fixture.
//!
//! The fixture block is validated by the `topology.topology_traverser` export (its
//! `walk(brep, g, true)` order names the target edge, vertex and face); the target and
//! keep indices are range-checked against that export's counts before the C++ runs.
//! Output layout: see the shim header.

use super::topology_topology_traverser::{self as traverser, Fixture};
use super::topology_vertexuse::counts;
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p4_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            editing: i32,
            target: i64,
            keep: i64,
            tol: f64,
            flag: i32,
            damage: i32,
            damage_index: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The operation run by [`eval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `DeleteEdge(edges[target], keep face's surface or NULL)`.
    DeleteEdge,
    /// `DeleteVertex(vertices[target])`.
    DeleteVertex,
    /// `ShrinkGeometry()`.
    ShrinkGeometry,
    /// `StitchFaces(tol, .., keep face or all, flag)`.
    StitchFaces,
    /// `delete brep`.
    Destroy,
    /// No operation: the records of the fixture B-rep as built (the Rust side checks
    /// its imported model against them).
    Check,
}

/// A hand-made defect of the fixture B-rep, applied after it is built and before the
/// operation (the fixtures build whole B-reps only: every face has a surface and every
/// shell sits in a region). [`eval`] answers [`NULL_DEREFERENCE`] for the damaged inputs
/// upstream dereferences `NULL` on, and runs every other one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    /// The face with this index (walk order) loses its surface: `DeleteEdge` of a
    /// manifold edge whose two faces have different surfaces, one of them missing,
    /// evaluates a normal on the missing one (`SmBrep.cpp:5829-5843`). Only with
    /// [`Op::DeleteEdge`] and [`Op::Check`].
    NoSurface(i64),
    /// The shell with this index (walk order) loses its region pointer: `DeleteEdge` of a
    /// wire edge and `DeleteVertex` of a shell vertex call `pShell->GetRegion()->PostInsert`
    /// / `Remove` when they make or remove a shell (`SmBrep.cpp:6827-6949`, `:7967`). Only
    /// with [`Op::DeleteEdge`], [`Op::DeleteVertex`] and [`Op::Check`].
    NoRegion(i64),
    /// The vertex with this index (walk order, a loop vertex with an even number of vertex
    /// uses) loses its last vertex use from its list (the loop use keeps it), leaving an odd
    /// number: `DeleteVertex` of a loop vertex without edges takes its vertex uses in pairs
    /// and reads `sVertexuses[ii + 1]` of the last one, an unset slot of the 16-slot stack
    /// buffer, then dereferences it (`SmBrep.cpp:7937-7942`). Only with [`Op::DeleteVertex`]
    /// and [`Op::Check`].
    LostVertexuse(i64),
}

/// The first value of the output when [`eval`] refused a damaged input because upstream
/// dereferences `NULL` on it (nothing was run).
pub const NULL_DEREFERENCE: f64 = -2.0;

/// The request of [`eval`].
#[derive(Debug, Clone, Copy)]
pub struct Request {
    /// A defect to put into the B-rep first.
    pub damage: Option<Damage>,
    /// The operation.
    pub op: Op,
    /// `m_bEditingEnabled` before the operation.
    pub editing: bool,
    /// The target edge or vertex index (walk order).
    pub target: i64,
    /// The face index whose surface to keep / to stitch, or `-1`.
    pub keep: i64,
    /// `dXSectTol3d` of `StitchFaces`.
    pub tol: f64,
    /// `bAllowSpineEdges` of `StitchFaces`.
    pub flag: bool,
}

fn invalid(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("topology.brep_p4: {what}"))
}

/// Exports the fixture B-rep (validating its block), as the Rust side imports it.
pub fn export(path: &str, input: &[f64]) -> Result<Fixture, FfiError> {
    traverser::export(path, input)
}

/// Builds the fixture B-rep and runs the request on it.
pub fn eval(path: &str, input: &[f64], r: Request) -> Result<Vec<f64>, FfiError> {
    let fixture = export(path, input)?;
    let Some(c) = counts(&fixture.graph) else {
        // the fixture does not build: the shim reports it without running anything
        return Ok(vec![-1.0]);
    };
    match r.op {
        Op::DeleteEdge if r.target < 0 || r.target as u64 >= c[7] as u64 => {
            return Err(invalid("edge index"));
        }
        Op::DeleteVertex if r.target < 0 || r.target as u64 >= c[9] as u64 => {
            return Err(invalid("vertex index"));
        }
        _ => {}
    }
    if r.keep < -1 || (r.keep >= 0 && r.keep as u64 >= c[3] as u64) {
        return Err(invalid("face index"));
    }
    if !r.tol.is_finite() {
        return Err(invalid("tolerance"));
    }
    let (damage, damage_index) = match r.damage {
        None => (0, 0),
        Some(Damage::NoSurface(i)) => {
            if !matches!(r.op, Op::DeleteEdge | Op::Check) {
                return Err(invalid(
                    "a surfaceless face is only probed for delete_edge and check",
                ));
            }
            if i < 0 || i as u64 >= c[3] as u64 {
                return Err(invalid("damaged face index"));
            }
            (1, i)
        }
        Some(Damage::NoRegion(i)) => {
            if !matches!(r.op, Op::DeleteEdge | Op::DeleteVertex | Op::Check) {
                return Err(invalid(
                    "a shell without a region is only probed for delete_edge, delete_vertex and check",
                ));
            }
            if i < 0 || i as u64 >= c[1] as u64 {
                return Err(invalid("damaged shell index"));
            }
            (2, i)
        }
        Some(Damage::LostVertexuse(i)) => {
            if !matches!(r.op, Op::DeleteVertex | Op::Check) {
                return Err(invalid(
                    "a lost vertex use is only probed for delete_vertex and check",
                ));
            }
            if i < 0 || i as u64 >= c[9] as u64 {
                return Err(invalid("damaged vertex index"));
            }
            (3, i)
        }
    };
    let op = match r.op {
        Op::DeleteEdge => 0,
        Op::DeleteVertex => 1,
        Op::ShrinkGeometry => 2,
        Op::StitchFaces => 3,
        Op::Destroy => 4,
        Op::Check => 5,
    };
    let cpath = CString::new(path).map_err(|_| invalid("path"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: the block was validated by the traverser export above, the indices are
        // in range of the B-rep it builds, and all buffers are owned here.
        unsafe {
            sys::sc_topology_brep_p4_eval(
                op,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                i32::from(r.editing),
                r.target,
                r.keep,
                r.tol,
                i32::from(r.flag),
                damage,
                damage_index,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
