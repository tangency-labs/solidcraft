//! Checked calls to `SmMerge::ProcessWiresAndShellVertices`.
//! Shared topology construction blocks come from `topology.vertexuse`; additional
//! blocks are `[4, x, y, z]` (isolated shell vertex) and `[5]` (empty B-rep).
//! The shim can export the complete input graph before invoking the unit.

use super::topology_topology_traverser::Fixture;
use super::topology_vertexuse::{self as shared, Op};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;
use std::io::Read;

mod sys {
    extern "C" {
        pub fn sc_boolean_merge_p5_eval(
            mode: i32,
            path_a: *const std::ffi::c_char,
            a: *const f64,
            na: i64,
            path_b: *const std::ffi::c_char,
            b: *const f64,
            nb: i64,
            operation: i32,
            mark_a: i32,
            mark_b: i32,
            marked_a: *const i64,
            nma: i64,
            marked_b: *const i64,
            nmb: i64,
            mates: *const i64,
            nm: i64,
            prefill: i32,
            repeat: i32,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// One validated topology construction program and its optional fixture path.
#[derive(Debug, Clone, Copy)]
pub struct Operand<'a> {
    /// Path for the shared file construction block; empty for procedural blocks.
    pub path: &'a str,
    /// Construction block (see module documentation).
    pub block: &'a [f64],
}

/// Operation and established intersection state. Mark flags list all edges, then
/// all vertices, in the exported graph order. Mate entries correspond to other
/// edges, with a primary edge index or -1 for no mate.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// `SmBooleanOperationType`, 0 through 10.
    pub operation: i32,
    /// Two real mark bits (equal bits share one lock), each of 1, 2, 4, 8, 16.
    pub marks: [i32; 2],
    /// Initial primary edge and vertex marks.
    pub primary_marked: &'a [i64],
    /// Initial other edge and vertex marks.
    pub other_marked: &'a [i64],
    /// Other edge -> primary edge matches, or -1.
    pub mates: &'a [i64],
    /// Prefill the output with the first primary edge to check its reset.
    pub prefill: bool,
    /// Repeat once or twice without advancing the marks.
    pub repeat: i32,
}

fn invalid() -> FfiError {
    FfiError::InvalidArgument("boolean.merge_p5 fixture or request".into())
}

fn validate(o: Operand<'_>) -> Result<CString, FfiError> {
    let path = CString::new(o.path).map_err(|_| invalid())?;
    match o.block.first().copied() {
        Some(4.0) => {
            if o.block.len() != 4 || o.block.iter().any(|v| !v.is_finite() || v.abs() > 1.0e300) {
                return Err(invalid());
            }
        }
        Some(5.0) => {
            if o.block.len() != 1 {
                return Err(invalid());
            }
        }
        _ => {
            if o.block.first() == Some(&0.0) {
                // UB-1: part containers such as SmallBox.smp are NOT B-rep
                // streams. ReadFromFile misreads their header and dereferences
                // null in SmTopologyData::ReadFromDB. Reject this wrong format
                // before even the shared fixture validator invokes C++.
                let file = std::fs::File::open(o.path).map_err(|_| invalid())?;
                let mut prefix = Vec::new();
                file.take(256)
                    .read_to_end(&mut prefix)
                    .map_err(|_| invalid())?;
                if prefix.trim_ascii_start().starts_with(b"//[Output Summary]") {
                    return Err(invalid());
                }
            }
            shared::eval(Op::Export, o.path, o.block)?;
        }
    }
    Ok(path)
}

/// Export a graph BEFORE running the operation. The result contains raw topology
/// and geometry only; it does not import any merge decisions into Rust.
pub fn export(o: Operand<'_>) -> Result<Fixture, FfiError> {
    let path = validate(o)?;
    let raw = call_with_buffer(1024, |buf, len| {
        // SAFETY: validate checked the full construction block and CString. Mode
        // -1 does not read the second operand or request arrays. Buffers are owned.
        unsafe {
            sys::sc_boolean_merge_p5_eval(
                -1,
                path.as_ptr(),
                o.block.as_ptr(),
                o.block.len() as i64,
                path.as_ptr(),
                o.block.as_ptr(),
                o.block.len() as i64,
                0,
                1,
                2,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                0,
                1,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let n = *raw.first().ok_or_else(invalid)? as usize;
    let end = n.checked_add(1).ok_or_else(invalid)?;
    let graph = raw.get(1..end).ok_or_else(invalid)?.to_vec();
    let counts = shared::counts(&graph).ok_or_else(invalid)?;
    let mut at = end;
    let mut vertexuses = Vec::new();
    for _ in 0..counts[9] {
        let n = *raw.get(at).ok_or_else(invalid)? as usize;
        at = at.checked_add(1).ok_or_else(invalid)?;
        let end = at.checked_add(n).ok_or_else(invalid)?;
        let list = raw
            .get(at..end)
            .ok_or_else(invalid)?
            .iter()
            .map(|&id| {
                if id < 0.0 || id as usize >= counts[8] {
                    return Err(invalid());
                }
                Ok(id as u32)
            })
            .collect::<Result<Vec<_>, _>>()?;
        vertexuses.push(list);
        at = end;
    }
    if at != raw.len() {
        return Err(invalid());
    }
    Ok(Fixture { graph, vertexuses })
}

/// Validate the one-to-one edge relation established by fixture `Relate` calls.
/// A primary edge cannot mate with two other edges; -1 entries remain unrelated.
/// Both differential engines use this before setting up the intersector state.
pub fn validate_mates(
    mates: &[i64],
    primary_edges: usize,
    other_edges: usize,
) -> Result<(), FfiError> {
    let mut seen = std::collections::BTreeSet::new();
    if mates.len() != other_edges
        || mates
            .iter()
            .any(|&m| m < -1 || (m >= 0 && (m as usize >= primary_edges || !seen.insert(m))))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Execute the real upstream unit after checking programs, array lengths, marks
/// and mate indices. Invalid requests never reach upstream pointer dereferences.
/// Output is `integer_count integers... geometry_values...`.
pub fn eval(a: Operand<'_>, b: Operand<'_>, r: Request<'_>) -> Result<Vec<f64>, FfiError> {
    if !(0..=10).contains(&r.operation)
        || !(1..=2).contains(&r.repeat)
        || r.marks.iter().any(|m| ![1, 2, 4, 8, 16].contains(m))
        || r.primary_marked
            .iter()
            .chain(r.other_marked)
            .any(|m| ![0, 1].contains(m))
    {
        return Err(invalid());
    }
    let aa = export(a)?;
    let bb = export(b)?;
    let ac = shared::counts(&aa.graph).ok_or_else(invalid)?;
    let bc = shared::counts(&bb.graph).ok_or_else(invalid)?;
    if r.primary_marked.len() != ac[7] + ac[9] || r.other_marked.len() != bc[7] + bc[9] {
        return Err(invalid());
    }
    validate_mates(r.mates, ac[7], bc[7])?;
    let pa = validate(a)?;
    let pb = validate(b)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: both complete programs, flags, mark locks, lengths and mate
        // indices are checked above. The shim rejects unmated marked wires only
        // for operations that append the mate to the unrepresentable null list.
        // All pointers reference live borrowed arrays or caller-owned output.
        unsafe {
            sys::sc_boolean_merge_p5_eval(
                0,
                pa.as_ptr(),
                a.block.as_ptr(),
                a.block.len() as i64,
                pb.as_ptr(),
                b.block.as_ptr(),
                b.block.len() as i64,
                r.operation,
                r.marks[0],
                r.marks[1],
                r.primary_marked.as_ptr(),
                r.primary_marked.len() as i64,
                r.other_marked.as_ptr(),
                r.other_marked.len() as i64,
                r.mates.as_ptr(),
                r.mates.len() as i64,
                i32::from(r.prefill),
                r.repeat,
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

    #[test]
    fn relations_are_one_to_one_with_unrelated_edges_allowed() {
        assert!(validate_mates(&[0, 0, 1, 2, 3], 5, 5).is_err());
        assert!(validate_mates(&[4, 3, 2, 1, 0], 5, 5).is_ok());
        assert!(validate_mates(&[-1, -1, 0, -1, 1], 2, 5).is_ok());
    }

    #[test]
    fn unmated_marked_wires_are_guarded_only_for_keep_operations() {
        let network = Operand {
            path: "",
            block: &[3.0, 0.0, 0.0],
        };
        let f = export(network).unwrap();
        let counts = shared::counts(&f.graph).unwrap();
        let flags = vec![1; counts[7] + counts[9]];
        let mates = vec![-1; counts[7]];
        for operation in 0..=10 {
            let result = eval(
                network,
                network,
                Request {
                    operation,
                    marks: [1, 2],
                    primary_marked: &flags,
                    other_marked: &flags,
                    mates: &mates,
                    prefill: true,
                    repeat: 2,
                },
            );
            assert_eq!(
                result.is_err(),
                matches!(operation, 1 | 2 | 5 | 6),
                "{operation}"
            );
        }
    }

    #[test]
    fn reject_invalid_programs_and_requests() {
        let o = Operand {
            path: "",
            block: &[5.0],
        };
        let request = Request {
            operation: 1,
            marks: [1, 2],
            primary_marked: &[],
            other_marked: &[],
            mates: &[],
            prefill: true,
            repeat: 1,
        };
        assert!(eval(o, o, request).is_ok());
        for block in [
            &[][..],
            &[4.0][..],
            &[4.0, 0.0, 0.0, f64::NAN][..],
            &[5.0, 0.0][..],
        ] {
            assert!(export(Operand { block, ..o }).is_err());
        }
        for r in [
            Request {
                operation: 11,
                ..request
            },
            Request {
                marks: [1, 31],
                ..request
            },
            Request {
                marks: [0, 2],
                ..request
            },
            Request {
                repeat: 0,
                ..request
            },
            Request {
                mates: &[i64::MAX],
                ..request
            },
            Request {
                primary_marked: &[2],
                ..request
            },
        ] {
            assert!(eval(o, o, r).is_err());
        }
        assert!(eval(
            o,
            o,
            Request {
                marks: [1, 1],
                ..request
            }
        )
        .is_ok());
        let part = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/TestFiles/pt_TestFiles/SmallBox.smp");
        assert!(matches!(
            export(Operand {
                path: part.to_str().unwrap(),
                block: &[0.0, 0.0, 0.0, 0.0],
            }),
            Err(FfiError::InvalidArgument(_))
        ));
    }
}
