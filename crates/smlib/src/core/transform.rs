//! Copies and affine transforms on solidcore: the C API's `smlib_brep_copy` and
//! `smlib_brep_transform`, ported from the fork's `capi/src/smlib_c.cpp`.
//!
//! As there, a transform works on a copy of its body (`SmApiBrepCopy`), which
//! `SmBrep::Transform(double[4][4], bScaleTol)` (solidcore's `transform_matrix`, solidcore#1570)
//! then moves in place, tolerances scaling with the model. The matrix is the capi's: rows 0-2 the
//! images of x, y and z, row 3 the translation (row-vector convention, `p' = p·A + t`); the
//! fourth column is not read.

use super::Body;
use super::blend::{on_copy, status};
use crate::{Error, Result};
use solidcore::topology::brep::mirror::transform_matrix;

/// `smlib_brep_copy`: `SmApiBrepCopy`.
pub(crate) fn copy(body: &Body) -> Result<Body> {
    on_copy("SmApiBrepCopy", body, |_, b| Ok(Ok(b)))
}

/// `smlib_brep_transform`: `SmBrep::Transform(matrix, TRUE)` on a copy of `body`. Every entry of
/// the matrix must be finite (`SMLIB_ERR_ARG`).
pub(crate) fn transform(body: &Body, matrix: &[[f64; 4]; 4]) -> Result<Body> {
    if !matrix.iter().flatten().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    on_copy("SmBrep::Transform", body, |k, mut b| {
        Ok(match transform_matrix(k, &mut b, matrix, true) {
            Ok(()) => Ok(b),
            Err(e) => Err(status(e)),
        })
    })
}
