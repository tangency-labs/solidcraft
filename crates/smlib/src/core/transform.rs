//! Copies and affine transforms on solidcore: the C API's `smlib_brep_copy` and
//! `smlib_brep_transform`, ported from the fork's `capi/src/smlib_c.cpp`, through solidcore-ops'
//! own copy and transform ([`Body::copy`], [`Body::transform`]).
//!
//! As there, a transform works on a copy of its body (`SmApiBrepCopy`), which
//! `SmBrep::Transform(double[4][4], bScaleTol)` then moves in place, tolerances scaling with the
//! model. The matrix is the capi's: rows 0-2 the images of x, y and z, row 3 the translation
//! (row-vector convention, `p' = p·A + t`); the fourth column is not read, so it is handed on
//! as `(0, 0, 0, 1)`.

use super::Body;
use super::call::ops_as;
use crate::{Error, Result};

/// `smlib_brep_copy`: `SmApiBrepCopy`.
pub(crate) fn copy(body: &Body) -> Result<Body> {
    ops_as("SmApiBrepCopy", &[("brep", body)], super::blend::refused, || body.copy())
}

/// `smlib_brep_transform`: `SmBrep::Transform(matrix, TRUE)` on a copy of `body`. Every entry of
/// the matrix must be finite (`SMLIB_ERR_ARG`); so is a matrix that flattens the body
/// (solidcore-ops' regularity check: its smallest scale proved above 1e-9 of its largest).
pub(crate) fn transform(body: &Body, matrix: &[[f64; 4]; 4]) -> Result<Body> {
    if !matrix.iter().flatten().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let mut affine = *matrix;
    for (row, last) in affine.iter_mut().zip([0.0, 0.0, 0.0, 1.0]) {
        row[3] = last;
    }
    ops_as("SmBrep::Transform", &[("brep", body)], super::blend::refused, || {
        let moved = body.copy()?;
        moved.transform(&affine)?;
        Ok(moved)
    })
}
