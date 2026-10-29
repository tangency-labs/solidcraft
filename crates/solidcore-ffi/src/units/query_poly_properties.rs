//! Safe wrapper for unit `query.poly_properties`: `SmApiPolyBrepIsManifoldSolid`,
//! `SmApiPolyBrepComputeVolume`, `SmApiPolyBrepComputeMassProperties`,
//! `SmPolyBrep::ComputeProperties` and `SmPolyFace::ComputeProperties` on a poly B-rep
//! built from plain data (block layout in `harness/cpp/shim/query_poly_properties.cpp`).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_query_poly_properties_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

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

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("poly_properties: {what}"))
}

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let x = *self.v.get(self.at).ok_or_else(|| bad("short block"))?;
        self.at += 1;
        if !x.is_finite() {
            return Err(bad("non-finite value"));
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
    fn coord(&mut self) -> Result<f64, FfiError> {
        let x = self.num()?;
        if x.abs() > 1e12 {
            return Err(bad("huge coordinate"));
        }
        Ok(x)
    }
}

/// Checks a block: every count in range, every index valid, every loop closed with at
/// least two vertices, every face with a loop (upstream reads its first edge), each edge
/// in at most one radial ring (the destructor walks the rings).
fn check(block: &[f64]) -> Result<(), FfiError> {
    let mut rd = Rd { v: block, at: 0 };
    rd.int(0, 4)?; // op
    rd.int(0, 4)?; // which
    for _ in 0..5 {
        rd.num()?; // origin, density, brep tolerance
    }
    let nv = rd.int(0, MAX_VERTICES)?;
    for _ in 0..nv {
        rd.coord()?;
        rd.coord()?;
        rd.coord()?;
        rd.num()?;
    }
    let nf = rd.int(0, MAX_FACES)?;
    let mut n_edges = 0usize;
    for _ in 0..nf {
        rd.num()?; // tolerance
        rd.int(0, 1)?; // has normal
        for _ in 0..3 {
            rd.num()?;
        }
        let nl = rd.int(1, MAX_LOOPS)?;
        for _ in 0..nl {
            rd.num()?; // edge tolerance
            let k = rd.int(2, MAX_LOOP_VERTICES)?;
            if nv == 0 {
                return Err(bad("loop without vertices"));
            }
            for _ in 0..k {
                rd.int(0, nv - 1)?;
            }
            n_edges += k;
        }
    }
    let ng = rd.int(0, n_edges)?;
    let mut used = vec![false; n_edges];
    for _ in 0..ng {
        let k = rd.int(1, MAX_RING)?;
        if n_edges == 0 {
            return Err(bad("ring without edges"));
        }
        for _ in 0..k {
            let e = rd.int(0, n_edges - 1)?;
            let slot = used.get_mut(e).ok_or_else(|| bad("edge index"))?;
            if *slot {
                return Err(bad("edge in two radial rings"));
            }
            *slot = true;
        }
    }
    if rd.at != block.len() {
        return Err(bad("trailing values"));
    }
    Ok(())
}

/// Runs the operation `block` describes and returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the shim cannot take safely (see `check`);
/// the shim's status otherwise.
pub fn eval(block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(block)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: the validated block outlives the call; output and length pointers come
        // from call_with_buffer.
        unsafe {
            sys::sc_query_poly_properties_eval(
                block.as_ptr(),
                block.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
