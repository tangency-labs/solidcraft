//! Checked FFI for `topology.topology_traverser` on real upstream B-reps.
//! Fixture blocks and raw graph exports use [`super::topology_vertexuse`].

use super::topology_vertexuse::{self as fixture, Op};
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_topology_traverser_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            start: i64,
            target: i64,
            mask: i32,
            want: i32,
            pre: i32,
            prefill: i32,
            repeat: i32,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// A traversal request. Opcodes: faces, wires, face uses, closure, shell,
/// replace face-use shells, remove shell, inner-shell parity (0 through 7).
/// `want` selects optional outputs; `pre` marks seed/first edge/vertex/faceuse/vertexuse.
#[derive(Debug, Clone, Copy)]
pub struct Request {
    /// Operation code, 0 through 7.
    pub op: i32,
    /// Seed index; -1 for a null face/wire seed.
    pub start: i64,
    /// Target shell index for operations 4 through 6.
    pub target: i64,
    /// One real mark's bit (1, 2, 4, 8 or 16).
    pub mask: i32,
    /// Optional output bits, 0 through 3.
    pub want: i32,
    /// Initial mark bits, 0 through 31.
    pub pre: i32,
    /// Seed output arrays with their first existing object.
    pub prefill: bool,
    /// Repeat once or twice with persistent marks and output arrays.
    pub repeat: i32,
}

/// Validate the fixture block and selectors, then call upstream traversal.
/// Returns an error before FFI for invalid blocks, flags or selectors.
pub fn eval(path: &str, input: &[f64], r: Request) -> Result<Vec<f64>, FfiError> {
    let invalid = || FfiError::InvalidArgument("topology traversal request".into());
    if !(0..=7).contains(&r.op)
        || ![1, 2, 4, 8, 16].contains(&r.mask)
        || !(0..=3).contains(&r.want)
        || !(0..=31).contains(&r.pre)
        || !(1..=2).contains(&r.repeat)
    {
        return Err(invalid());
    }
    let export = export(path, input)?;
    if let Some(c) = fixture::counts(&export.graph) {
        let kind = match r.op {
            0 => 3,
            1 => 7,
            2 | 3 | 5 => 2,
            4 => 9,
            6 => 1,
            _ => 0,
        };
        if r.op != 7
            && (r.start < 0 || r.start as usize >= *c.get(kind).ok_or_else(invalid)?)
            && !(r.start == -1 && r.op <= 1)
        {
            return Err(invalid());
        }
        if ((4..=6).contains(&r.op) || r.op == 7) && (r.target < 0 || r.target as usize >= c[1]) {
            return Err(invalid());
        }
        // Removing a shell into itself invalidates upstream owning lists.
        if r.op == 6 && r.start == r.target {
            return Err(invalid());
        }
    }
    let path = std::ffi::CString::new(path).map_err(|_| invalid())?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: fixture::eval validated the complete block, all selectors and flags
        // are checked above, and the output pointers reference caller-owned buffers.
        unsafe {
            sys::sc_topology_topology_traverser_eval(
                r.op,
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                r.start,
                r.target,
                r.mask,
                r.want,
                r.pre,
                i32::from(r.prefill),
                r.repeat,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// A complete raw input graph, plus each vertex's native vertex-use list order.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// The existing topology.vertexuse graph encoding.
    pub graph: Vec<f64>,
    /// Vertex-use indices for each vertex, in the raw owning-list order.
    pub vertexuses: Vec<Vec<u32>>,
}

/// Validate fixture construction, then export all connectivity, including links
/// reachable only through vertex use lists. No algorithm result is exported.
pub fn export(path: &str, input: &[f64]) -> Result<Fixture, FfiError> {
    // Reuse the checked reader for the full construction block before C++ reads it.
    fixture::eval(Op::Export, path, input)?;
    let invalid = || FfiError::InvalidArgument("invalid traversal graph export".into());
    let path = std::ffi::CString::new(path).map_err(|_| invalid())?;
    let raw = call_with_buffer(1024, |buf, len| {
        // SAFETY: fixture::eval validated the complete input; op 8 takes no selectors.
        // Output points into the caller's buffers and all objects die inside the shim.
        unsafe {
            sys::sc_topology_topology_traverser_eval(
                8,
                path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                0,
                0,
                1,
                0,
                0,
                0,
                1,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    if raw.first() == Some(&-1.0) {
        return Ok(Fixture {
            graph: raw,
            vertexuses: Vec::new(),
        });
    }
    let len = *raw.first().ok_or_else(invalid)? as usize;
    let end = 1usize.checked_add(len).ok_or_else(invalid)?;
    let graph = raw.get(1..end).ok_or_else(invalid)?.to_vec();
    let counts = fixture::counts(&graph).ok_or_else(invalid)?;
    let mut at = end;
    let mut vertexuses = Vec::new();
    for _ in 0..counts[9] {
        let count = *raw.get(at).ok_or_else(invalid)? as usize;
        at = at.checked_add(1).ok_or_else(invalid)?;
        let next = at.checked_add(count).ok_or_else(invalid)?;
        let list = raw
            .get(at..next)
            .ok_or_else(invalid)?
            .iter()
            .map(|&x| {
                if x < 0.0 || x as usize >= counts[8] {
                    return Err(invalid());
                }
                Ok(x as u32)
            })
            .collect::<Result<Vec<_>, _>>()?;
        vertexuses.push(list);
        at = next;
    }
    if at != raw.len() {
        return Err(invalid());
    }
    Ok(Fixture { graph, vertexuses })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_network_and_ffi_guards() -> Result<(), FfiError> {
        let input = [3.0, 0.0, 0.0];
        let full = export("", &input)?;
        let counts = fixture::counts(&full.graph)
            .ok_or_else(|| FfiError::InvalidArgument("failed network".into()))?;
        assert_eq!(full.vertexuses.len(), counts[9]);
        assert_eq!(
            full.vertexuses.iter().map(Vec::len).sum::<usize>(),
            counts[8]
        );
        assert!(full.vertexuses.iter().any(|list| list.len() > 1));
        let request = Request {
            op: 1,
            start: 0,
            target: 0,
            mask: 2,
            want: 3,
            pre: 0,
            prefill: false,
            repeat: 1,
        };
        assert!(eval("", &input, request).is_ok());
        assert!(export("", &[]).is_err());
        for invalid in [
            Request {
                start: i64::MAX,
                ..request
            },
            Request {
                start: -2,
                ..request
            },
            Request { mask: 0, ..request },
            Request {
                op: 4,
                target: i64::MAX,
                ..request
            },
            Request {
                op: 6,
                start: 0,
                target: 0,
                ..request
            },
            Request {
                repeat: 0,
                ..request
            },
        ] {
            assert!(matches!(
                eval("", &input, invalid),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        Ok(())
    }
}
