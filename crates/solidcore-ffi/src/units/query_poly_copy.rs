//! Safe wrapper for unit `query.poly_copy`: `SmApiPolyBrepCopy` (the `SmPolyBrep` copy
//! constructor) on a poly B-rep built from plain data (block layout in
//! `harness/cpp/shim/query_poly_copy.cpp`).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_query_poly_copy_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Largest region count accepted.
pub const MAX_REGIONS: usize = 8;
/// Largest shell count of a region.
pub const MAX_SHELLS: usize = 8;
/// Largest vertex count accepted.
pub const MAX_VERTICES: usize = 4096;
/// Largest face count accepted.
pub const MAX_FACES: usize = 4096;
/// Largest loop count of a face.
pub const MAX_LOOPS: usize = 8;
/// Largest vertex count of a loop.
pub const MAX_LOOP_VERTICES: usize = 256;
/// Largest radial ring.
pub const MAX_RING: usize = 16;
/// Largest aux data and attribute counts.
pub const MAX_EXTRAS: usize = 64;

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("poly_copy: {what}"))
}

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let x = *self.v.get(self.at).ok_or_else(|| bad("short block"))?;
        self.at += 1;
        if !x.is_finite() || x.abs() > 1e12 {
            return Err(bad("non-finite or huge value"));
        }
        Ok(x)
    }
    fn int(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let x = self.num()?;
        if x.fract() != 0.0 || x < lo as f64 || x > hi as f64 {
            return Err(bad("bad count or index"));
        }
        Ok(x as usize)
    }
    fn index(&mut self, n: usize) -> Result<usize, FfiError> {
        if n == 0 {
            return Err(bad("index into an empty list"));
        }
        self.int(0, n - 1)
    }
}

/// Checks a block: every count in range, every index valid, open loops (mode 2) with at
/// least two vertices, each edge in at most one radial ring (the destructor walks the
/// rings), attribute ids, values and behaviours in range.
fn check(block: &[f64]) -> Result<(), FfiError> {
    let mut rd = Rd { v: block, at: 0 };
    rd.num()?; // tolerance
    let nr = rd.int(0, MAX_REGIONS)?;
    let mut n_shells = 0usize;
    for _ in 0..nr {
        n_shells += rd.int(0, MAX_SHELLS)?;
    }
    let nv = rd.int(0, MAX_VERTICES)?;
    for _ in 0..nv * 4 {
        rd.num()?;
    }
    let nf = rd.int(0, MAX_FACES)?;
    let mut n_edges = 0usize;
    for _ in 0..nf {
        rd.index(n_shells)?;
        rd.num()?; // tolerance
        rd.int(0, 1)?; // has normal
        for _ in 0..3 {
            rd.num()?;
        }
        let nl = rd.int(0, MAX_LOOPS)?;
        for _ in 0..nl {
            let mode = rd.int(0, 2)?;
            rd.num()?; // edge tolerance
            let k = rd.int(if mode == 2 { 2 } else { 1 }, MAX_LOOP_VERTICES)?;
            for _ in 0..k {
                rd.index(nv)?;
            }
            n_edges += if mode == 2 { k - 1 } else { k };
        }
    }
    let ng = rd.int(0, n_edges)?;
    let mut used = vec![false; n_edges];
    for _ in 0..ng {
        let k = rd.int(1, MAX_RING)?;
        for _ in 0..k {
            let e = rd.index(n_edges)?;
            let slot = used.get_mut(e).ok_or_else(|| bad("edge index"))?;
            if *slot {
                return Err(bad("edge in two radial rings"));
            }
            *slot = true;
        }
    }
    let na = rd.int(0, MAX_EXTRAS)?;
    for _ in 0..na * 5 {
        rd.num()?;
    }
    let nea = rd.int(0, n_edges)?;
    for _ in 0..nea {
        rd.index(n_edges)?;
        rd.index(na)?;
    }
    let nat = rd.int(0, MAX_EXTRAS)?;
    for _ in 0..nat {
        rd.int(0, 1_000_000)?; // id
        let v = rd.num()?;
        if v.fract() != 0.0 || v.abs() > 1e9 {
            return Err(bad("attribute value"));
        }
        rd.int(0, 4)?; // behaviour
    }
    if rd.at != block.len() {
        return Err(bad("trailing values"));
    }
    Ok(())
}

/// Copies the poly B-rep `block` describes with `SmApiPolyBrepCopy` and returns the
/// shim's dump of the copy.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the shim cannot take safely (see `check`);
/// the shim's status otherwise.
pub fn eval(block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(block)?;
    call_with_buffer(8192, |out, len| {
        // SAFETY: the validated block outlives the call; output and length pointers come
        // from call_with_buffer.
        unsafe {
            sys::sc_query_poly_copy_eval(
                block.as_ptr(),
                block.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
