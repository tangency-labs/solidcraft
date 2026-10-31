//! Checked native `SmBrep::{CopyEdges, MakeFaceTopology, TestClosureSplitRegion}`.
//! Uses the topology.vertexuse fixture block and the traverser's raw input export;
//! [`make_faces`] appends its `MakeFaceTopology` calls to the block and returns the
//! post-operation graph. [`eval`] appends the same tagged graph record after ops 0
//! and 1 (the split source B-rep, or the copy destination B-rep).

use super::topology_topology_traverser::{self as graph, Fixture};
use super::topology_vertexuse::counts;
use crate::{call_with_buffer, FfiError};

/// One request. `op`: closure=0, copy=1, edgeless face=2 (face construction on real
/// loops is [`make_faces`], the shim's op 3). Flag bits are
/// infinite-region testing / calculate trims (1), optional copy list (2), null
/// surface (4), opposite orientation (8), and radial verification (16).
#[derive(Debug, Clone)]
pub struct Request {
    /// Operation, 0..=2.
    pub op: i32,
    /// Existing face-use index for closure testing.
    pub seed: i64,
    /// Edge indices in copy order (duplicates are valid).
    pub indices: Vec<i64>,
    /// Repeat with the same topology/context once or twice.
    pub repeat: i32,
    /// Mask of candidate marks for one held lock (1..=31), or zero for no lock.
    pub lock_mask: i32,
    /// Operation flag bits, 0..=31.
    pub flags: i32,
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p6_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            seed: i64,
            indices: *const i64,
            count: i64,
            repeat: i32,
            lock_mask: i32,
            flags: i32,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Validate construction and all operation-dependent selectors before native code.
pub fn export(path: &str, input: &[f64]) -> Result<Fixture, FfiError> {
    graph::export(path, input)
}

/// Execute a checked native request. `[-1, status]` means fixture construction
/// failed, `[-2]` means a trimmed curve constructor returned null; both happen
/// before the operation. Other statuses/exceptions are operation/harness errors.
/// Closure outputs and face-use shell links use -1 for null, -3 for a non-null
/// object outside the original graph, and nonnegative original-graph indices.
/// These new-object paths are currently deferred in the differential runner.
///
/// # Record contract
///
/// Op 2 returns only its fixed prefix (status, outputs, mark deltas and locks, the
/// face-use shells and the source/destination counts). Ops 0 and 1 return their fixed
/// fields followed by the whole post-operation graph as `tag value` pairs (`0` an
/// integer, `1` a double), in `topology.brep_p10`'s traversal order:
///
/// - op 0 exports the source B-rep after `TestClosureSplitRegion`;
/// - op 1 exports the destination B-rep after `CopyEdges`: its infinite region and
///   wire shells, each copied edge's interval, tolerance, edge uses and copied curve
///   (type, dimension, natural interval, five evaluations), and each vertex's point,
///   tolerance and uses.
///
/// The graph is serialized by the shim only for the C++ side of a comparison; the
/// Rust runner walks its own independently built destination, so no native output or
/// process state reaches the Rust side.
pub fn eval(path: &str, input: &[f64], request: &Request) -> Result<Vec<f64>, FfiError> {
    let invalid = || FfiError::InvalidArgument("brep_p6 request".into());
    if !(0..=2).contains(&request.op)
        || !(1..=2).contains(&request.repeat)
        || !(0..=31).contains(&request.lock_mask)
        || !(0..=31).contains(&request.flags)
        || u32::try_from(request.indices.len()).is_err()
        || (request.op != 1 && !request.indices.is_empty())
    {
        return Err(invalid());
    }
    let fixture = export(path, input)?;
    if let Some(c) = counts(&fixture.graph) {
        if request.op == 0 && (request.seed < 0 || request.seed as usize >= c[2]) {
            return Err(invalid());
        }
        if request
            .indices
            .iter()
            .any(|i| *i < 0 || *i as usize >= c[7])
        {
            return Err(invalid());
        }
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid())?;
    call_with_buffer(512, |buffer, len| {
        // SAFETY: the shared fixture parser validated every byte of input; all
        // indices, flags and repeat bounds were checked above. Arrays and output
        // buffers remain live for the call; native objects never cross FFI.
        unsafe {
            sys::sc_topology_brep_p6_eval(
                request.op,
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                request.seed,
                request.indices.as_ptr(),
                request.indices.len() as i64,
                request.repeat,
                request.lock_mask,
                request.flags,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Most `MakeFaceTopology` calls of one op-3 request.
pub const MAX_FACES: usize = 16;
/// Most loops, edge uses, loop vertices or UV curves of one call.
pub const MAX_FACE_ITEMS: usize = 256;

/// One `MakeFaceTopology` call of op 3. Indices refer to the fixture export's
/// traversal order (regions, edge uses, vertices); blocks use the shared fixture
/// encodings of [`super::topology_vertexuse`].
#[derive(Debug, Clone, Default)]
pub struct FaceSpec {
    /// The surface block, `None` for a NULL surface.
    pub surface: Option<Vec<f64>>,
    /// Region index, or `-1` for NULL (the infinite region).
    pub region: i64,
    /// Bit 0 `bCalculateTrims`, bit 1 `bVerifyRadialSectors`, bit 2 `SM_OT_OPPOSITE`.
    pub flags: i32,
    /// `crEdgeLoops`.
    pub loops: Vec<i64>,
    /// `crEdgeuses` (edge use indices).
    pub edgeuses: Vec<i64>,
    /// `crLoopVertices` (vertex indices).
    pub vertices: Vec<i64>,
    /// `cpOptUVCurves`: curve blocks (`None` entries are NULL); `None` for a NULL array.
    pub uv_curves: Option<Vec<Option<Vec<f64>>>>,
}

/// Run `MakeFaceTopology` once per spec, in order, on the fixture of `block`, stopping
/// after the first call that does not succeed. Output: `[-1, status]` for a failed
/// fixture, `[-2]` when a call would dereference null in C++ (see the shim), else the
/// number of calls made, per call `status face shell region` (post-operation graph
/// indices), five `mark delta, locked` pairs, then the post-operation graph export as
/// `tag value` pairs (`0` integer, `1` double).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed block or spec (indices past the
/// fixture's counts, too many items, bad flags) before any native call.
pub fn make_faces(path: &str, block: &[f64], faces: &[FaceSpec]) -> Result<Vec<f64>, FfiError> {
    let invalid = |what: &str| FfiError::InvalidArgument(format!("brep_p6 make_faces: {what}"));
    let end = super::topology_vertexuse::brep_block_at(block, path)?;
    if end != block.len() {
        return Err(invalid("trailing block data"));
    }
    let fixture = export(path, block)?;
    let Some(c) = counts(&fixture.graph) else {
        return Ok(fixture.graph);
    };
    if faces.len() > MAX_FACES {
        return Err(invalid("too many faces"));
    }
    let item = |v: i64, count: usize| v >= 0 && (v as u64) < count as u64;
    let mut input = block.to_vec();
    input.push(faces.len() as f64);
    for f in faces {
        if !(0..=7).contains(&f.flags)
            || (f.region != -1 && !item(f.region, c[0]))
            || f.loops.len() > MAX_FACE_ITEMS
            || f.edgeuses.len() > MAX_FACE_ITEMS
            || f.vertices.len() > MAX_FACE_ITEMS
            || f.loops.iter().any(|&n| !(0..=MAX_FACE_ITEMS as i64).contains(&n))
            || f.edgeuses.iter().any(|&e| !item(e, c[6]))
            || f.vertices.iter().any(|&v| !item(v, c[9]))
        {
            return Err(invalid("selector, count or flag out of range"));
        }
        match &f.surface {
            None => input.push(0.0),
            Some(surface) => {
                let (_, used) = super::topology_vertexuse::surface_at(surface, 0)?;
                if used != surface.len() {
                    return Err(invalid("trailing surface data"));
                }
                input.push(1.0);
                input.extend(surface);
            }
        }
        input.push(f.region as f64);
        input.push(f64::from(f.flags));
        for list in [&f.loops, &f.edgeuses, &f.vertices] {
            input.push(list.len() as f64);
            input.extend(list.iter().map(|&x| x as f64));
        }
        match &f.uv_curves {
            None => input.push(-1.0),
            Some(curves) => {
                if curves.len() > MAX_FACE_ITEMS {
                    return Err(invalid("too many UV curves"));
                }
                input.push(curves.len() as f64);
                for curve in curves {
                    match curve {
                        None => input.push(0.0),
                        Some(curve) => {
                            let (_, used) = super::topology_vertexuse::curve_at(curve, 0)?;
                            if used != curve.len() {
                                return Err(invalid("trailing curve data"));
                            }
                            input.push(1.0);
                            input.extend(curve);
                        }
                    }
                }
            }
        }
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid("path"))?;
    call_with_buffer(1 << 16, |buffer, len| {
        // SAFETY: the block and every spec were validated above against the fixture's
        // counts; the shim reads exactly this layout. Buffers stay live for the call
        // and native objects never cross FFI.
        unsafe {
            sys::sc_topology_brep_p6_eval(
                3,
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                0,
                std::ptr::null(),
                0,
                1,
                0,
                0,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_selectors_flags_and_counts_before_native_operation() {
        let base = Request {
            op: 1,
            seed: 0,
            indices: vec![],
            repeat: 1,
            lock_mask: 0,
            flags: 2,
        };
        for request in [
            Request {
                indices: vec![-1],
                ..base.clone()
            },
            Request {
                indices: vec![i64::MAX],
                ..base.clone()
            },
            Request {
                op: 0,
                indices: vec![0],
                ..base.clone()
            },
            Request {
                op: 0,
                seed: -1,
                ..base.clone()
            },
            Request {
                op: 0,
                seed: i64::MAX,
                ..base.clone()
            },
            Request {
                repeat: 0,
                ..base.clone()
            },
            Request {
                repeat: 3,
                ..base.clone()
            },
            Request {
                lock_mask: 32,
                ..base.clone()
            },
            Request { flags: 32, ..base },
        ] {
            assert!(
                matches!(
                    eval("", &[3.0, 0.0, 0.0], &request),
                    Err(FfiError::InvalidArgument(_))
                ),
                "{request:?}"
            );
        }
    }

    #[test]
    fn make_faces_rejects_bad_specs_and_builds_a_square() {
        // a kind-5 square: four shell vertices and four line edges
        let mut block = vec![5.0, 4.0];
        for p in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]] {
            block.extend(p);
        }
        block.push(4.0);
        for (a, b) in [(0.0, 1.0), (1.0, 2.0), (2.0, 3.0), (3.0, 0.0)] {
            block.extend([a, b, 0.0]);
        }
        block.extend([0.0, 0.0]);
        let plane = vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let edgeless = FaceSpec {
            surface: Some(plane.clone()),
            region: -1,
            ..FaceSpec::default()
        };
        for bad in [
            FaceSpec {
                region: 99,
                ..edgeless.clone()
            },
            FaceSpec {
                flags: 8,
                ..edgeless.clone()
            },
            FaceSpec {
                edgeuses: vec![-1],
                ..edgeless.clone()
            },
            FaceSpec {
                vertices: vec![99],
                ..edgeless.clone()
            },
            FaceSpec {
                loops: vec![-1],
                ..edgeless.clone()
            },
            FaceSpec {
                surface: Some(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
                ..edgeless.clone()
            },
            FaceSpec {
                uv_curves: Some(vec![Some(vec![2.0])]),
                ..edgeless.clone()
            },
        ] {
            assert!(
                matches!(
                    make_faces("", &block, std::slice::from_ref(&bad)),
                    Err(FfiError::InvalidArgument(_))
                ),
                "{bad:?}"
            );
        }
        let out = make_faces("", &block, &[edgeless]).unwrap();
        // one call, SM_SUCCESS, a face and its new shell, no region
        assert_eq!(out.get(..2), Some(&[1.0, 0.0][..]));
        assert!(out.get(2).is_some_and(|f| *f >= 0.0));
        assert!(out.get(3).is_some_and(|s| *s >= 0.0));
        assert_eq!(out.get(4), Some(&-1.0));
    }

    #[test]
    fn accepts_longer_valid_copy_lists_and_duplicate_edges() {
        // Native SmNewMarkAndLock accepts OR-ed candidate bits as well as a
        // single mark. All these masks select the first free mark here.
        for lock_mask in [0, 3, 7, 31] {
            let request = Request {
                op: 1,
                seed: 0,
                indices: vec![0; 65],
                repeat: 1,
                lock_mask,
                flags: 2,
            };
            let input = [3.0, 0.0, 0.0];
            let c = counts(&export("", &input).unwrap().graph).unwrap();
            // op-1 fixed prefix: status, copied count, five mark pairs, face-use
            // shells, then seven counts (the last three are destination regions,
            // infinite index and void). The tagged destination graph follows it.
            let prefix = 1 + 1 + 5 * (2 + c[2]) + c[2] + 7;
            let result = eval("", &input, &request).unwrap();
            assert_eq!(result.first(), Some(&0.0));
            assert_eq!(result.get(1), Some(&65.0));
            assert_eq!(result.get(3), Some(&f64::from(lock_mask != 0)));
            assert_eq!(
                result.get(prefix - 3..prefix),
                Some([1.0, 0.0, 1.0].as_slice())
            );
            assert!(result.len() > prefix, "destination graph follows the prefix");
            assert_eq!(result.get(prefix), Some(&1.0)); // the graph starts with a double
        }
    }

    #[test]
    fn native_split_error_keeps_nonnull_outputs_without_original_graph_ids() {
        // The tiny plane is edgeless in SMLib and reaches the split branch.
        // New shell/region pointers cannot resolve in the pre-operation graph.
        let input = [
            2.0, 1.0, 2e-9, -3e-9, 5e-9, 0.0, 0.0, 1.0, -1e-9, -2e-9, 3e-9, 4e-9, 0.0, 0.0,
        ];
        let request = Request {
            op: 0,
            seed: 1,
            indices: vec![],
            repeat: 2,
            lock_mask: 0,
            flags: 1,
        };
        let result = eval("", &input, &request).unwrap();
        assert!(result.first().is_some_and(|status| *status > 0.0));
        assert_eq!(result.get(1..3), Some([-3.0, -3.0].as_slice()));
    }

    #[test]
    fn open_sheet_reaches_traversal_and_releases_its_mark_lock() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/topology.brep_p6/open_box.smb"
        );
        let input = [0.0, 0.0, 0.0, 0.0];
        let fixture = export(path, &input).unwrap();
        let c = counts(&fixture.graph).unwrap();
        assert_eq!(c[0..4], [1, 1, 10, 5]);
        for seed in [0, 5] {
            for repeat in [1, 2] {
                let result = eval(
                    path,
                    &input,
                    &Request {
                        op: 0,
                        seed,
                        indices: vec![],
                        repeat,
                        lock_mask: 0,
                        flags: 1,
                    },
                )
                .unwrap();
                assert_eq!(result[0..3], [0.0, -1.0, -1.0]);
                // This would be zero for either pre-traversal early return.
                assert_eq!(result[3], 4.0 * f64::from(repeat));
                assert_eq!(result[4], 0.0); // operation lock released
                assert!(result[5..5 + c[2]].contains(&1.0));
            }
        }
    }
}
