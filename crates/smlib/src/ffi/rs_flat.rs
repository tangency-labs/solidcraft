//! The flat-array calls on the `solidcore` backend: a [`FlatHandle`] is the arrays themselves.
use super::rs::{body, state};
use super::{ERR_ARG, Handle, NOT_BUILT};
use crate::flat::MAX_LEN;
use crate::import::Arrays;

/// One B-rep's flat arrays.
#[derive(Default)]
pub struct FlatHandle(Arrays);

impl FlatHandle {
    pub(super) fn arrays(&self) -> &Arrays {
        &self.0
    }
}

/// Array ids of `smlib.h`: int64 1..=35, double 101..=165.
fn i64_id(id: i32) -> bool {
    (1..=35).contains(&id)
}
fn f64_id(id: i32) -> bool {
    (101..=165).contains(&id)
}

pub fn new() -> Result<FlatHandle, i32> {
    Ok(FlatHandle::default())
}
pub fn export(b: &Handle) -> Result<FlatHandle, i32> {
    let s = state();
    super::rs_export::arrays(&s.kernel, &body(b)).map(FlatHandle)
}
pub fn import(f: &FlatHandle) -> Result<Handle, i32> {
    super::rs_import::import(f)
}
#[allow(dead_code)]
fn import_unported(_: &FlatHandle) -> Result<Handle, i32> {
    Err(NOT_BUILT)
}
pub fn get_i64(f: &FlatHandle, id: i32) -> Result<Vec<i64>, i32> {
    if !i64_id(id) {
        return Err(ERR_ARG);
    }
    Ok(f.0.ints.get(&id).cloned().unwrap_or_default())
}
pub fn get_f64(f: &FlatHandle, id: i32) -> Result<Vec<f64>, i32> {
    if !f64_id(id) {
        return Err(ERR_ARG);
    }
    Ok(f.0.reals.get(&id).cloned().unwrap_or_default())
}
pub fn set_i64(f: &mut FlatHandle, id: i32, v: &[i64]) -> i32 {
    if !i64_id(id) || v.len() > MAX_LEN {
        return ERR_ARG;
    }
    f.0.ints.insert(id, v.to_vec());
    super::OK
}
pub fn set_f64(f: &mut FlatHandle, id: i32, v: &[f64]) -> i32 {
    if !f64_id(id) || v.len() > MAX_LEN {
        return ERR_ARG;
    }
    f.0.reals.insert(id, v.to_vec());
    super::OK
}
pub fn is_manifold_solid(b: &Handle) -> Result<bool, i32> {
    let _kernel = state();
    // unconfirmed: the C shim adds nothing to SMLib's own check.
    solidcore::topology::brep::p2::is_manifold_solid(&body(b)).map_err(|e| super::rs::status(&e))
}
