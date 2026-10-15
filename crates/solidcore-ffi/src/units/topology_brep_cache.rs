//! Checked FFI for `SmBrepCache` (`harness/cpp/shim/topology_brep_cache.cpp`).
//!
//! Protocol: exact/build flags, tolerance, vertex count with xyzt records,
//! polygon count with vertex-index lists, resize-vertex count with xyzt records,
//! resize-edge count with start/end xyz and edge/start/end tolerances, and query
//! count with min/max xyz boxes. Exact sources have no polygon vertices/faces.
//! Output is an integer count, the exact integer stream, then floating-point boxes.
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_topology_brep_cache_eval(
            input: *const f64,
            n: i64,
            output: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn invalid() -> FfiError {
    FfiError::InvalidArgument("invalid B-rep cache description".into())
}

/// Runs a checked cache description. Counts are bounded to 4096, coordinates
/// to 1e100, polygon loops closed with at least 3 vertices. Tolerances retain
/// native `fabs` / `SM_USE_DEFAULT` handling, including negative inputs.
/// These limits bound the test wrapper, not the kernel's supported geometry.
pub fn eval(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    if input.iter().any(|v| !v.is_finite() || v.abs() > 1e100) {
        return Err(invalid());
    }
    let mut r = Reader { input, pos: 0 };
    let exact = r.flag()?;
    r.flag()?;
    r.num()?;
    let nv = r.count()?;
    if exact && nv != 0 {
        return Err(invalid());
    }
    r.vertices(nv)?;
    let nf = r.count()?;
    if exact && nf != 0 {
        return Err(invalid());
    }
    let mut total = 0usize;
    for _ in 0..nf {
        let count = r.count()?;
        total = total.checked_add(count).ok_or_else(invalid)?;
        if count < 3 || total > 4096 {
            return Err(invalid());
        }
        for _ in 0..count {
            if r.count()? >= nv {
                return Err(invalid());
            }
        }
    }
    let resize = r.count()?;
    r.vertices(resize)?;
    let ne = r.count()?;
    for _ in 0..ne {
        let a = [r.num()?, r.num()?, r.num()?];
        let b = [r.num()?, r.num()?, r.num()?];
        // Keep the squared-length test: the native line constructor also rejects
        // tiny nonzero segments whose squared length underflows to zero.
        let length_sq = a.iter().zip(b).map(|(a, b)| (a - b) * (a - b)).sum::<f64>();
        if length_sq == 0.0 {
            return Err(invalid());
        }
        for _ in 0..3 {
            r.num()?;
        }
    }
    let nq = r.count()?;
    for _ in 0..nq {
        for _ in 0..6 {
            r.num()?;
        }
    }
    if r.pos != input.len() {
        return Err(invalid());
    }
    call_with_buffer(1024, |out, len| {
        // SAFETY: the parser checked all counts, indices and complete input length;
        // out and len are writable for the capacities passed to the shim.
        unsafe {
            sys::sc_topology_brep_cache_eval(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

struct Reader<'a> {
    input: &'a [f64],
    pos: usize,
}
impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self.input.get(self.pos).copied().ok_or_else(invalid)?;
        self.pos += 1;
        Ok(v)
    }
    fn count(&mut self) -> Result<usize, FfiError> {
        let n = self.num()?;
        if !(0.0..=4096.0).contains(&n) || n.fract() != 0.0 {
            return Err(invalid());
        }
        Ok(n as usize)
    }
    fn flag(&mut self) -> Result<bool, FfiError> {
        match self.num()? {
            0.0 => Ok(false),
            1.0 => Ok(true),
            _ => Err(invalid()),
        }
    }
    fn vertices(&mut self, n: usize) -> Result<(), FfiError> {
        for _ in 0..n {
            for _ in 0..3 {
                self.num()?;
            }
            self.num()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_malformed_before_native_code() {
        for input in [
            vec![],
            vec![0.0; 6],
            vec![2.0; 7],
            vec![0.0, 1.0, 0.0, 1e99, 0.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0, 0.0, 1.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            vec![1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ] {
            assert!(matches!(eval(&input), Err(FfiError::InvalidArgument(_))));
        }
    }
}
