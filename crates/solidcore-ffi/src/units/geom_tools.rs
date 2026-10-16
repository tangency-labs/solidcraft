//! FFI wrappers for unit `geom.tools`.
//!
//! Shim: `harness/cpp/shim/geom_tools.cpp`.
//! Upstream: `source/SMLib/src/SmTools.cpp`.
//!
//! | Rust                      | shim                                   | upstream                                 |
//! |---------------------------|----------------------------------------|------------------------------------------|
//! | [`find_point`]            | `sc_geom_tools_find_point`             | `SmTools::FindFirstPointWithinTolToPnt`  |
//! | [`find_vertex`]           | `sc_geom_tools_find_vertex`            | `SmTools::FindFirstVertexWithinTolToPnt` |
//! | [`find_coincident_edge`]  | `sc_geom_tools_find_coincident_edge`   | `SmTools::FindCoincidentEdge`            |
//! | [`find_edge`]             | `sc_geom_tools_find_edge`              | `SmTools::FindEdge`                      |
//! | [`find_face`]             | `sc_geom_tools_find_face`              | `SmTools::FindFace`                      |

use std::ffi::CString;

use crate::FfiError;

mod sys {
    use std::ffi::c_char;

    extern "C" {
        pub fn sc_geom_tools_find_point(
            pts: *const f64,
            n: i64,
            target: i64,
            tol: f64,
            out_ints: *mut i64,
            out_dbls: *mut f64,
        ) -> i32;
        pub fn sc_geom_tools_find_vertex(
            pts: *const f64,
            tols: *const f64,
            n: i64,
            pnt: *const f64,
            tol: f64,
            out_ints: *mut i64,
            out_dbls: *mut f64,
        ) -> i32;
        pub fn sc_geom_tools_find_coincident_edge(
            path: *const c_char,
            edge: i64,
            copy_curve: i32,
            swap: i32,
            tol: f64,
            start: i64,
            out_ints: *mut i64,
            out_dbls: *mut f64,
        ) -> i32;
        pub fn sc_geom_tools_find_edge(
            path: *const c_char,
            pnt: *const f64,
            out_ints: *mut i64,
            out_dbls: *mut f64,
        ) -> i32;
        pub fn sc_geom_tools_find_face(
            path: *const c_char,
            pnt: *const f64,
            out_ints: *mut i64,
        ) -> i32;
    }
}

/// Upstream status (`0` = `SM_SUCCESS`) plus outputs. Statuses below `0` are shim
/// errors and are returned as [`FfiError::Status`] instead.
#[derive(Debug, Clone, PartialEq)]
pub struct Call<T> {
    /// Upstream `SmStatus` (`0` for `SM_SUCCESS`).
    pub status: i32,
    /// The outputs.
    pub out: T,
}

/// `([found, index], [closest, second closest])` of [`find_point`].
pub type PointOut = ([i64; 2], [f64; 2]);

fn shim_status(st: i32) -> Result<i32, FfiError> {
    if st < 0 {
        Err(FfiError::Status(st))
    } else {
        Ok(st)
    }
}

fn len_i64(n: usize, what: &str) -> Result<i64, FfiError> {
    i64::try_from(n).map_err(|_| FfiError::InvalidArgument(format!("{what} too long")))
}

fn c_path(path: &str) -> Result<CString, FfiError> {
    CString::new(path).map_err(|_| FfiError::InvalidArgument("path".into()))
}

/// `FindFirstPointWithinTolToPnt(points, target, tol)`: `([found, index], [closest, second])`.
pub fn find_point(
    points: &[[f64; 3]],
    target: usize,
    tol: f64,
) -> Result<Call<PointOut>, FfiError> {
    if target >= points.len() {
        return Err(FfiError::InvalidArgument(format!(
            "target {target} of {} points",
            points.len()
        )));
    }
    let flat: Vec<f64> = points.iter().flatten().copied().collect();
    let n = len_i64(points.len(), "points")?;
    let t = len_i64(target, "target")?;
    let mut ints = [0i64; 2];
    let mut dbls = [0f64; 2];
    // SAFETY: `flat` holds 3*n readable doubles, 0 <= target < n; the output arrays hold
    // 2 writable values each, as the shim writes.
    let st = unsafe {
        sys::sc_geom_tools_find_point(
            flat.as_ptr(),
            n,
            t,
            tol,
            ints.as_mut_ptr(),
            dbls.as_mut_ptr(),
        )
    };
    Ok(Call {
        status: shim_status(st)?,
        out: (ints, dbls),
    })
}

/// `FindFirstVertexWithinTolToPnt` over free vertices `(point, tolerance)`:
/// `([found, index], distance)`.
pub fn find_vertex(
    vertices: &[([f64; 3], f64)],
    pnt: [f64; 3],
    tol: f64,
) -> Result<Call<([i64; 2], f64)>, FfiError> {
    let flat: Vec<f64> = vertices.iter().flat_map(|(p, _)| *p).collect();
    let tols: Vec<f64> = vertices.iter().map(|(_, t)| *t).collect();
    let n = len_i64(vertices.len(), "vertices")?;
    let mut ints = [0i64; 2];
    let mut d = [0f64; 1];
    // SAFETY: `flat` holds 3*n and `tols` n readable doubles, `pnt` 3; outputs hold 2 and 1
    // writable values.
    let st = unsafe {
        sys::sc_geom_tools_find_vertex(
            flat.as_ptr(),
            tols.as_ptr(),
            n,
            pnt.as_ptr(),
            tol,
            ints.as_mut_ptr(),
            d.as_mut_ptr(),
        )
    };
    Ok(Call {
        status: shim_status(st)?,
        out: (ints, d[0]),
    })
}

/// `FindCoincidentEdge` on the B-rep in the ASCII `.smb` file `path`, with the curve of
/// edge `edge` (copied when `copy_curve`) between its vertices (swapped when `swap`),
/// searching from `start`: `([n_edges, found, same_orient, index], max_deviation)`.
pub fn find_coincident_edge(
    path: &str,
    edge: usize,
    copy_curve: bool,
    swap: bool,
    tol: f64,
    start: usize,
) -> Result<Call<([i64; 4], f64)>, FfiError> {
    let p = c_path(path)?;
    let e = len_i64(edge, "edge")?;
    let s = len_i64(start, "start")?;
    let mut ints = [0i64; 4];
    let mut d = [0f64; 1];
    // SAFETY: `p` is a NUL-terminated string; outputs hold 4 and 1 writable values. The shim
    // checks `edge` against the edge count before using it.
    let st = unsafe {
        sys::sc_geom_tools_find_coincident_edge(
            p.as_ptr(),
            e,
            i32::from(copy_curve),
            i32::from(swap),
            tol,
            s,
            ints.as_mut_ptr(),
            d.as_mut_ptr(),
        )
    };
    Ok(Call {
        status: shim_status(st)?,
        out: (ints, d[0]),
    })
}

/// `FindEdge` on the B-rep in `path`: `([n_edges, index], param)`.
pub fn find_edge(path: &str, pnt: [f64; 3]) -> Result<Call<([i64; 2], f64)>, FfiError> {
    let p = c_path(path)?;
    let mut ints = [0i64; 2];
    let mut d = [0f64; 1];
    // SAFETY: `p` is NUL-terminated, `pnt` holds 3 doubles; outputs hold 2 and 1 values.
    let st = unsafe {
        sys::sc_geom_tools_find_edge(p.as_ptr(), pnt.as_ptr(), ints.as_mut_ptr(), d.as_mut_ptr())
    };
    Ok(Call {
        status: shim_status(st)?,
        out: (ints, d[0]),
    })
}

/// `FindFace` on the B-rep in `path`: `[n_faces, face index or -1]`.
pub fn find_face(path: &str, pnt: [f64; 3]) -> Result<Call<[i64; 2]>, FfiError> {
    let p = c_path(path)?;
    let mut ints = [0i64; 2];
    // SAFETY: `p` is NUL-terminated, `pnt` holds 3 doubles; the output holds 2 values.
    let st = unsafe { sys::sc_geom_tools_find_face(p.as_ptr(), pnt.as_ptr(), ints.as_mut_ptr()) };
    Ok(Call {
        status: shim_status(st)?,
        out: ints,
    })
}
