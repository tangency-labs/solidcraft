//! Checked calls to the `SmMerge` B-rep constructor, `MergeEdge`, `MakeFaceuse`,
//! `MergeAddedFace` and `MergeAddedTopology`. Shim:
//! `harness/cpp/shim/boolean_merge_p2.cpp`. Upstream: `source/SMLib/src/SmMerge.cpp`.
//!
//! Operands are the construction blocks of [`super::boolean_merge_p5`] (shared
//! topology blocks, `[4, x, y, z]` shell vertex, `[5]` empty B-rep); entity indices
//! refer to its exported graph (native owning-list order). Output is
//! `integer_count integers... values...`:
//!
//! | request | integers | values |
//! |---|---|---|
//! | [`Request::Construct`] | `0` | tolerance, angle tolerance, the five option flags, `m_bManifoldBoolean`, `m_bIntersectionLoopClosed`, tag id |
//! | [`Request::MergeEdge`] | status, made edge (`-1`), both snapshots | both snapshots |
//! | [`Request::MakeFaceuse`] | status, old region, new region, new-face count, faces, both snapshots | both snapshots |
//! | [`Request::MergeAddedTopology`], [`Request::MergeAddedFace`] | status, edges stitched, primary snapshot | the four gaps, primary snapshot |
//!
//! Guards. These inputs crash or are undefined upstream:
//! - a computed tolerance (`dApproxTol3d < SM_EFF_ZERO`) without other B-rep
//!   (`pOtherBrep->GetMinEdgeBBoxSize`; UB-167, segfaults the reference build);
//! - `MakeFaceuse` of a null face use (`pFaceuseToMakeInBrep->GetMate()`);
//! - a vertex listed twice in a `MergeAddedTopology` list, once `StitchVertexPairs`
//!   glues one copy and deletes it: the other entry dangles (UB-176; segfaults the
//!   reference build).
//!
//! Deliberate over-refusals (harness choices, not upstream crashes):
//! - any face, edge or vertex listed twice in a `MergeAddedTopology` list, glued or
//!   not (UB-176 needs a repeated vertex that is actually glued);
//! - `MergeAddedTopology` / `MergeAddedFace` with an other B-rep: the shim builds the
//!   merge they run in with a null other B-rep, as `SmApiStitch` does;
//! - mates that are not one-to-one per kind (they would make the fixture's `Relate`
//!   fail, not the operation under test).
//!
//! Non-finite tolerances and angles run (probed on every fixture kind).

use super::boolean_merge_p5::{export, Operand};
use super::topology_vertexuse as shared;
use crate::{call_with_buffer, FfiError};
use std::collections::BTreeSet;
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_boolean_merge_p2_eval(
            op: i32,
            path_a: *const std::ffi::c_char,
            a: *const f64,
            na: i64,
            path_b: *const std::ffi::c_char,
            b: *const f64,
            nb: i64,
            params: *const f64,
            np: i64,
            ints: *const i64,
            ni: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;

/// The fixture's intersector state: for each other-operand edge, vertex and face, its
/// primary mate's index or `-1`.
#[derive(Debug, Clone, Copy)]
pub struct Mates<'a> {
    /// Other edge -> primary edge.
    pub edges: &'a [i64],
    /// Other vertex -> primary vertex.
    pub vertices: &'a [i64],
    /// Other face -> primary face.
    pub faces: &'a [i64],
}

/// The operation to run after constructing the merge.
#[derive(Debug, Clone, Copy)]
pub enum Request<'a> {
    /// Only the constructor (`SmMerge(ctx, pBrep, pOtherBrep, tol, ang, pOpt)`).
    Construct,
    /// `MergeEdge(edge use of the other operand or -1, primary region or -1)`.
    MergeEdge {
        /// Fixture mates.
        mates: Mates<'a>,
        /// Other-operand edge use index, `-1` for null.
        edgeuse: i64,
        /// Primary region index, `-1` for null.
        region: i64,
    },
    /// `MakeFaceuse(face use, ..., primary region or -1, new-face list or null)`.
    MakeFaceuse {
        /// Fixture mates.
        mates: Mates<'a>,
        /// Other-operand face use index.
        faceuse: i64,
        /// Primary region index, `-1` for null.
        region: i64,
        /// Pass a new-face list.
        want_new_faces: bool,
    },
    /// `MergeAddedTopology(faces, edges, vertices, ..., flags)`; `None` lists are null.
    MergeAddedTopology {
        /// Primary face indices.
        faces: Option<&'a [i64]>,
        /// Primary edge indices.
        edges: Option<&'a [i64]>,
        /// Primary vertex indices.
        vertices: Option<&'a [i64]>,
        /// The trailing flags in the definition's order: making manifold solid, fast
        /// edge compare, region nesting, ignore problems, `bDoGluing`.
        flags: [bool; 5],
    },
    /// `MergeAddedFace(primary face)`.
    MergeAddedFace {
        /// Primary face index.
        face: i64,
    },
}

fn invalid() -> FfiError {
    FfiError::InvalidArgument("boolean.merge_p2 fixture or request".into())
}

/// Entity counts of an operand's graph (`[regions, shells, faceuses, faces,
/// loopuses, loops, edgeuses, edges, vertexuses, vertices]`).
///
/// # Errors
///
/// An invalid construction block.
pub fn counts(o: Operand<'_>) -> Result<[usize; 10], FfiError> {
    shared::counts(&export(o)?.graph).ok_or_else(invalid)
}

/// Check that `mates` maps `others` entities one-to-one into `primaries` (or `-1`).
fn one_to_one(mates: &[i64], primaries: usize, others: usize) -> Result<(), FfiError> {
    let mut seen = BTreeSet::new();
    if mates.len() != others
        || mates
            .iter()
            .any(|&m| m < -1 || (m >= 0 && (m as usize >= primaries || !seen.insert(m))))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Check the fixture mates of a request against both operands' counts.
///
/// # Errors
///
/// Mates of the wrong length, out of range or not one-to-one.
pub fn validate_mates(m: Mates<'_>, a: [usize; 10], b: [usize; 10]) -> Result<(), FfiError> {
    one_to_one(m.edges, a[7], b[7])?;
    one_to_one(m.vertices, a[9], b[9])?;
    one_to_one(m.faces, a[3], b[3])
}

fn push_mates(ints: &mut Vec<i64>, m: Mates<'_>) {
    for list in [m.edges, m.vertices, m.faces] {
        ints.push(list.len() as i64);
        ints.extend(list);
    }
}

fn index_ok(i: i64, n: usize, null_ok: bool) -> bool {
    (null_ok && i == -1) || (i >= 0 && (i as usize) < n)
}

fn push_list(ints: &mut Vec<i64>, list: Option<&[i64]>, n: usize) -> Result<(), FfiError> {
    let Some(list) = list else {
        ints.push(0);
        return Ok(());
    };
    let mut seen = BTreeSet::new();
    if list
        .iter()
        .any(|&i| !index_ok(i, n, false) || !seen.insert(i))
    {
        return Err(invalid());
    }
    ints.extend([1, list.len() as i64]);
    ints.extend(list);
    Ok(())
}

/// Run the request on real upstream objects after checking both construction
/// programs, the tolerances, the options mask (`None`: null; bits 1, 2, 4, 8, 16 in
/// `SmMergeOptions` argument order) and every index (module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a refused input; the shim's status.
pub fn eval(
    a: Operand<'_>,
    b: Option<Operand<'_>>,
    approx_tol_3d: f64,
    ang_tol_rad: f64,
    options: Option<u8>,
    r: Request<'_>,
) -> Result<Vec<f64>, FfiError> {
    if options.is_some_and(|m| m > 31) {
        return Err(invalid());
    }
    let ac = counts(a)?;
    let bc = b.map(counts).transpose()?;
    let computed = approx_tol_3d < EFF_ZERO;
    if bc.is_none() && computed {
        return Err(invalid());
    }
    let mut ints = vec![options.map_or(-1, i64::from)];
    let op = match r {
        Request::Construct => 0,
        Request::MergeEdge {
            mates,
            edgeuse,
            region,
        } => {
            let bc = bc.ok_or_else(invalid)?;
            validate_mates(mates, ac, bc)?;
            if !index_ok(edgeuse, bc[6], true) || !index_ok(region, ac[0], true) {
                return Err(invalid());
            }
            push_mates(&mut ints, mates);
            ints.extend([edgeuse, region]);
            1
        }
        Request::MakeFaceuse {
            mates,
            faceuse,
            region,
            want_new_faces,
        } => {
            let bc = bc.ok_or_else(invalid)?;
            validate_mates(mates, ac, bc)?;
            if !index_ok(faceuse, bc[2], false) || !index_ok(region, ac[0], true) {
                return Err(invalid());
            }
            push_mates(&mut ints, mates);
            ints.extend([faceuse, region, i64::from(want_new_faces)]);
            2
        }
        Request::MergeAddedTopology { .. } | Request::MergeAddedFace { .. } => {
            if bc.is_some() || computed {
                return Err(invalid());
            }
            match r {
                Request::MergeAddedTopology {
                    faces,
                    edges,
                    vertices,
                    flags,
                } => {
                    push_list(&mut ints, faces, ac[3])?;
                    push_list(&mut ints, edges, ac[7])?;
                    push_list(&mut ints, vertices, ac[9])?;
                    ints.extend(flags.map(i64::from));
                    3
                }
                Request::MergeAddedFace { face } => {
                    if !index_ok(face, ac[3], false) {
                        return Err(invalid());
                    }
                    ints.push(face);
                    4
                }
                _ => return Err(invalid()),
            }
        }
    };
    let path_a = CString::new(a.path).map_err(|_| invalid())?;
    let path_b = CString::new(b.map_or("", |b| b.path)).map_err(|_| invalid())?;
    let (pb, nb) = b.map_or((std::ptr::null(), 0), |b| {
        (b.block.as_ptr(), b.block.len() as i64)
    });
    let params = [approx_tol_3d, ang_tol_rad];
    call_with_buffer(1024, |buf, len| {
        // SAFETY: both construction programs were validated by `counts` (the p5
        // export), every index and list above; the pointers reference live borrowed
        // arrays (b may be null with nb == 0) and caller-owned output.
        unsafe {
            sys::sc_boolean_merge_p2_eval(
                op,
                path_a.as_ptr(),
                a.block.as_ptr(),
                a.block.len() as i64,
                path_b.as_ptr(),
                pb,
                nb,
                params.as_ptr(),
                params.len() as i64,
                ints.as_ptr(),
                ints.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: Operand<'static> = Operand {
        path: "",
        block: &[5.0],
    };
    const NETWORK: Operand<'static> = Operand {
        path: "",
        block: &[3.0, 0.0, 0.0],
    };

    #[test]
    fn computed_tolerance_needs_the_other_brep() {
        assert!(eval(EMPTY, None, 0.0, 0.0, None, Request::Construct).is_err());
        assert!(eval(EMPTY, None, -1.0, 0.0, None, Request::Construct).is_err());
        assert!(eval(EMPTY, None, EFF_ZERO, 0.0, None, Request::Construct).is_ok());
        assert!(eval(EMPTY, Some(EMPTY), 0.0, 0.0, Some(31), Request::Construct).is_ok());
        assert!(eval(EMPTY, None, 1.0, 0.0, Some(32), Request::Construct).is_err());
    }

    #[test]
    fn mates_and_indices_are_checked() {
        let c = counts(NETWORK).unwrap();
        let edges = vec![-1; c[7]];
        let vertices = vec![-1; c[9]];
        let mates = Mates {
            edges: &edges,
            vertices: &vertices,
            faces: &[],
        };
        let ok = Request::MergeEdge {
            mates,
            edgeuse: -1,
            region: -1,
        };
        assert!(eval(NETWORK, Some(NETWORK), 1e-3, 0.0, None, ok).is_ok());
        let dup = vec![0; c[7]];
        for r in [
            Request::MergeEdge {
                mates: Mates {
                    edges: &dup,
                    ..mates
                },
                edgeuse: -1,
                region: -1,
            },
            Request::MergeEdge {
                mates,
                edgeuse: c[6] as i64,
                region: -1,
            },
            Request::MakeFaceuse {
                mates,
                faceuse: -1,
                region: -1,
                want_new_faces: true,
            },
        ] {
            assert!(eval(NETWORK, Some(NETWORK), 1e-3, 0.0, None, r).is_err());
        }
        assert!(eval(NETWORK, None, 1e-3, 0.0, None, ok).is_err());
    }

    #[test]
    fn added_topology_is_merged_within_one_brep() {
        let r = Request::MergeAddedTopology {
            faces: None,
            edges: Some(&[0]),
            vertices: Some(&[]),
            flags: [false, true, true, true, true],
        };
        assert!(eval(NETWORK, None, 1e-3, 0.0, None, r).is_ok());
        assert!(eval(NETWORK, Some(NETWORK), 1e-3, 0.0, None, r).is_err());
        // Non-finite tolerances run; a computed tolerance has no other B-rep.
        assert!(eval(NETWORK, None, f64::NAN, 0.0, None, r).is_ok());
        assert!(eval(NETWORK, None, 1e-3, f64::INFINITY, None, r).is_ok());
        assert!(eval(NETWORK, None, 0.0, 0.0, None, r).is_err());
        let twice = Request::MergeAddedTopology {
            faces: None,
            edges: Some(&[0, 0]),
            vertices: None,
            flags: [false; 5],
        };
        assert!(eval(NETWORK, None, 1e-3, 0.0, None, twice).is_err());
        assert!(eval(
            NETWORK,
            None,
            1e-3,
            0.0,
            None,
            Request::MergeAddedFace { face: 0 }
        )
        .is_err());
    }

    #[test]
    fn ub_177_duplicate_glued_vertex_is_refused() {
        let square = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/TestFiles/pt_TestFiles/plane_square.smb");
        let a = Operand {
            path: square.to_str().unwrap(),
            block: &[0.0, 0.0, 0.0, 0.0],
        };
        let r = |vertices: &'static [i64]| Request::MergeAddedTopology {
            faces: Some(&[]),
            edges: Some(&[]),
            vertices: Some(vertices),
            flags: [false, true, true, true, true],
        };
        assert!(eval(a, None, 10.0, 0.0, None, r(&[0, 0])).is_err());
        assert!(eval(a, None, 10.0, 0.0, None, r(&[0])).is_ok());
    }
}
