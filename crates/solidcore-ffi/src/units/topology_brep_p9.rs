//! Native `topology.brep_p9` entrypoints over validated real B-rep fixtures.
//!
//! The export call builds exactly the same initial graph as the operation call.
//! For insertion it adds two isolated interior loop vertices as fixture setup.
//! Outputs are status, record length, traversal-index record, raw graph export.
use super::topology_vertexuse::{self as fixture, Op};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;
use std::io::Read;

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p9_eval(
            op: i32,
            export_only: i32,
            selector: i32,
            variant: i32,
            editing: i32,
            same: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            parameter: f64,
            point: *const f64,
            start_tol: f64,
            end_tol: f64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// One native operation and its fixture construction program.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// 0 infinite region, 1 orient sheet, 2 shell vertex, 3 vertex loop,
    /// 4 split edge, 5 insert edge in face, 6/7 point-class merge into edge/vertex,
    /// 8 insertion projection prefix (without topology editing).
    pub op: u8,
    /// Entity selector, resolved modulo the actual count.
    pub selector: u8,
    /// Insertion refusal/default-region selector (0..=5).
    pub variant: u8,
    /// Caller editing state.
    pub editing: bool,
    /// Same/opposite orientation.
    pub same: bool,
    /// Upstream file path, empty for procedural fixtures.
    pub path: &'a str,
    /// Existing `topology_vertexuse` fixture program.
    pub block: &'a [f64],
    /// Split parameter in the curve's actual domain.
    pub parameter: f64,
    /// Point for vertex creation.
    pub point: [f64; 3],
    /// Raw endpoint tolerances, converted by native `SmZoneTol3d` before assignment.
    pub tolerances: [f64; 2],
}

/// Export the initial graph (with its prepared VU indices) or run the operation.
/// Refuses only invalid layouts and missing required fixture entities before native
/// dereferences. Nonfinite split parameters, points and tolerances (`SetPoint` and
/// `SetTolerance(x, FALSE)` only store them) and safe NULL refusal branches remain
/// callable. Part-container streams (UB-1)
/// are rejected before the shared native fixture validator reads them.
pub fn eval(request: Request<'_>, export_only: bool) -> Result<Vec<f64>, FfiError> {
    let bad = || FfiError::InvalidArgument("brep_p9 request or fixture".into());
    if request.op > 8 || request.variant > 5 {
        return Err(bad());
    }
    if request.block.first() == Some(&0.0) {
        // UB-1: ReadFromFile expects a B-rep stream, not a part container. The
        // latter's summary header crashes SmTopologyData::ReadFromDB. Check it
        // before fixture::eval, which itself enters the native reader.
        let file = std::fs::File::open(request.path).map_err(|_| bad())?;
        let mut prefix = Vec::new();
        file.take(256).read_to_end(&mut prefix).map_err(|_| bad())?;
        if prefix.trim_ascii_start().starts_with(b"//[Output Summary]") {
            return Err(bad());
        }
    }
    let raw = fixture::eval(Op::Export, request.path, request.block)?;
    let counts = fixture::counts(&raw).ok_or_else(bad)?;
    let required = match request.op {
        2 => counts[0] > 0,
        3 | 8 => counts[3] > 0,
        4 | 6 | 7 => counts[7] > 0,
        5 if request.variant == 0 && request.editing => counts[3] > 0,
        _ => true,
    };
    if !required {
        return Err(bad());
    }
    let path = CString::new(request.path).map_err(|_| bad())?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: fixture::eval validates the entire construction program and
        // path; scalar ranges and required entities are checked above. Native
        // additionally checks the selected edge's curve and endpoints. Buffers
        // and C strings stay live; all native allocations stay inside the shim.
        unsafe {
            sys::sc_topology_brep_p9_eval(
                i32::from(request.op),
                i32::from(export_only),
                i32::from(request.selector),
                i32::from(request.variant),
                i32::from(request.editing),
                i32::from(request.same),
                path.as_ptr(),
                request.block.as_ptr(),
                request.block.len() as i64,
                request.parameter,
                request.point.as_ptr(),
                request.tolerances[0],
                request.tolerances[1],
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_container_is_rejected_before_native_fixture_loading() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../upstream/TestFiles/pt_TestFiles/SmallBox.smp");
        let request = Request {
            op: 4,
            selector: 0,
            variant: 0,
            editing: true,
            same: true,
            path: path.to_str().unwrap(),
            block: &[0.0, 0.0, 0.0, 0.0],
            parameter: 0.5,
            point: [0.0; 3],
            tolerances: [0.0; 2],
        };
        for export_only in [false, true] {
            assert!(matches!(
                eval(request, export_only),
                Err(FfiError::InvalidArgument(_))
            ));
        }
    }
}
