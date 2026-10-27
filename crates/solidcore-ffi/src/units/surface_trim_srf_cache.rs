//! FFI wrappers for unit `surface.trim_srf_cache`.
//!
//! Shim: `harness/cpp/shim/surface_trim_srf_cache.cpp` (`sc_surface_trim_srf_cache_eval`).
//! Upstream: `source/SMLib/src/SmTrimSrfCache.cpp`, `source/SMLib/inc/SmTrimSrfCache.h`.
//!
//! Every op starts with a B-spline surface block `S` = `oob form n m p q r s cpts(x y z w) u
//! v` (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, control points with the v index fastest).
//! `D` is a domain `umin vmin umax vmax`, `C` a 2D curve `dim(2) n p m cpts(x y NOZ w) knots`,
//! `T` a subdivision tree `n` then `n` x `parent c1 c2 aux split marked class D` (node 0 the
//! top; `-1` = none). The op's parameters follow:
//!
//! | [`Op`]        | upstream                                     | parameters                    |
//! |---------------|----------------------------------------------|-------------------------------|
//! | `New`         | constructor, `GetType`, `IsKindOf`, state    | `S ch ang ar max min ratio k type..` |
//! | `PointTest`   | `PointTest` without face (testing off or face modified) | `S enabled modified u v has_tol tol` |
//! | `ImplantEu`   | `ImplantEdgeuse` on a hand-built tree        | `S D factor T init D k` then `k` x `C subdiv node has_owner owner_tol` |
//! | `ImplantPoly` | `ImplantPolyEdge`, then `ImplantEdgeuse` / `SubdivideNode` | `S D factor T S k` then `k` x `start(x y z) end(x y z) tolerance node`, `remap m` then `m` x `C subdiv node` |
//! | `Face`        | `BuildTree`, `PointTest`, `PointClassify` on a face's cache | `S build k` then `k` x `u v has_tol tol` |
//!
//! Outputs (first entry the status `0` / `SmStatus`):
//! `New`: `type kinds.. modified boundary point_test have_t containment D factor ch ang ar max min
//! ratio no_tree`. `PointTest`: `ok object_type` (`-1` = none). `ImplantEu`: one status per call
//! up to the first failing one, the face extent `D`, the node count, then for every node of the
//! tree (the input nodes, then the children subdivisions made, in allocation order) `marked
//! class k eu..` (edge use indices = call indices). `Face`: (with `build`) `build_status boundary point_test have_t
//! containment D node_count`, then per point `test_status ok object_type classify_status class
//! object_type`. `ImplantPoly`: polygon-call statuses, edge-use-call statuses, face extent,
//! node count, then per node `parent c1 c2 D split marked class eu_count eu.. poly_count poly..`.
//! Polygon handles are input indices; parent polygon lists and mixed edge-use lists are retained
//! as upstream. `remap` exercises Rust handle mapping without changing native input handles.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed clamped
//! surfaces and curves, domains inside the surface domain, valid enumerators, bounded counts,
//! finite values, well-formed subdivision trees (one root, child pairs pointing back, every node
//! a subdivision node), and `PointTest` without a face only where it does not classify (testing
//! off or face modified; upstream dereferences the null face otherwise).

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 24;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 5;
/// Largest ordinary coordinate magnitude accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e6;
// ImplantEu alone admits large finite UV data to expose Bezier-decomposition
// roundoff. This is a fixture envelope, not a native defined-behavior threshold.
const MAX_IMPLANT_UV_COORD: f64 = 1.0e11;
/// Largest number of subdivision tree nodes.
pub const MAX_NODES: usize = 256;
/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The constructor and the type queries.
    New,
    /// `PointTest` of a cache without face.
    PointTest,
    /// `ImplantEdgeuse`.
    ImplantEu,
    /// Polygon edges followed by edge-use-driven subdivision, with full node lists.
    ImplantPoly,
    /// The cache of a face: `BuildTree`, `PointTest`, `PointClassify`.
    Face,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::New => 0,
            Op::PointTest => 1,
            Op::ImplantEu => 2,
            Op::Face => 3,
            Op::ImplantPoly => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_trim_srf_cache_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > 1.0e6 {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn coordinate(&mut self) -> Result<f64, FfiError> {
        self.0.finite()
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.coordinate()).collect()
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    /// An index in `-1..max` (`-1` = none).
    fn opt_index(&mut self, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < -1.0 || v >= max as f64 {
            return Err(bad(format!("bad index {v} (below {max})")));
        }
        Ok(v as i64)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn domain(&mut self) -> Result<[f64; 4], FfiError> {
        let d = [
            self.coordinate()?,
            self.coordinate()?,
            self.coordinate()?,
            self.coordinate()?,
        ];
        if d[0] > d[2] || d[1] > d[3] {
            return Err(bad("domain not ordered"));
        }
        Ok(d)
    }

    /// A domain inside `outer`; with `strict` of positive size in both directions.
    fn domain_in(&mut self, outer: &[f64; 4], strict: bool) -> Result<[f64; 4], FfiError> {
        let d = self.domain()?;
        let o = outer;
        if d[0] < o[0] || d[1] < o[1] || d[2] > o[2] || d[3] > o[3] {
            return Err(bad("domain outside the surface domain"));
        }
        if strict && (d[2] - d[0] < 1e-6 * (o[2] - o[0]) || d[3] - d[1] < 1e-6 * (o[3] - o[1])) {
            return Err(bad("domain too small"));
        }
        Ok(d)
    }
}

/// Checks control points sharing their rational marker; 2D points carry `NOZ`.
fn check_points(rd: &mut Reader<'_>, k: usize, two_d: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        rd.coordinate()?;
        rd.coordinate()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if two_d != (z == NOZ) {
            return Err(bad(format!(
                "{what}: dimension does not match the z marker"
            )));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let zz = if two_d { 0.0 } else { z };
        if !zz.is_finite() || zz.abs() > MAX_COORD {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// A B-spline surface block `oob form n m p q r s cpts u v`; returns its domain.
fn bspline_block(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok([u0, v0, u1, v1])
}

/// A 2D curve `dim n p m cpts knots`.
fn curve_2d(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(3)? != 2 {
        return Err(bad("curve: dimension must be 2"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, true, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve").map(|_| ())
}

/// A subdivision tree; returns its node count.
fn tree(rd: &mut Reader<'_>, srf: &[f64; 4]) -> Result<usize, FfiError> {
    let n = rd.index(MAX_NODES)?;
    if n == 0 {
        return Err(bad("tree: no nodes"));
    }
    let mut links = Vec::with_capacity(n);
    for _ in 0..n {
        let parent = rd.opt_index(n as i64)?;
        let c1 = rd.opt_index(n as i64)?;
        let c2 = rd.opt_index(n as i64)?;
        let aux = rd.index(6)?;
        if !matches!(aux, 3 | 4 | 6) {
            return Err(bad("tree: aux type must be a surface subdivision type"));
        }
        rd.index(2)?;
        rd.flag()?;
        rd.index(3)?;
        rd.domain_in(srf, false)?;
        links.push((parent, c1, c2));
    }
    // one root (node 0), child pairs pointing back, every node reached once from the root
    let mut seen = vec![false; n];
    let mut stack = vec![0usize];
    if links.first().map(|l| l.0) != Some(-1) {
        return Err(bad("tree: node 0 must be the root"));
    }
    while let Some(k) = stack.pop() {
        let Some(s) = seen.get_mut(k) else {
            return Err(bad("tree: bad node"));
        };
        if *s {
            return Err(bad("tree: node reached twice"));
        }
        *s = true;
        let (_, c1, c2) = links.get(k).copied().ok_or_else(|| bad("tree: bad node"))?;
        match (c1, c2) {
            (-1, -1) => {}
            (a, b) if a >= 0 && b >= 0 && a != b => {
                for c in [a as usize, b as usize] {
                    if links.get(c).map(|l| l.0) != Some(k as i64) {
                        return Err(bad("tree: child does not point back"));
                    }
                    stack.push(c);
                }
            }
            _ => return Err(bad("tree: need two children or none")),
        }
    }
    if seen.iter().any(|s| !s) {
        return Err(bad("tree: unreachable node"));
    }
    Ok(n)
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let srf = bspline_block(rd)?;
    match op {
        Op::New => {
            for _ in 0..6 {
                rd.finite()?;
            }
            let k = rd.index(16)?;
            for _ in 0..k {
                let t = rd.num()?;
                if t.fract() != 0.0 || !(0.0..=1.0e6).contains(&t) {
                    return Err(bad("bad type code"));
                }
            }
        }
        Op::PointTest => {
            let enabled = rd.flag()?;
            let modified = rd.flag()?;
            if enabled && !modified {
                return Err(bad(
                    "point test without face would classify against a null face",
                ));
            }
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::ImplantEu => {
            rd.domain_in(&srf, true)?;
            let f = rd.finite()?;
            if f < 1.0 {
                return Err(bad("subdivision factor below 1"));
            }
            let n = tree(rd, &srf)?;
            rd.flag()?;
            // Face extent: unordered (negative-area) bounds are valid input; upstream
            // resets them to the curve box, so only finiteness is checked here.
            rd.finites(4)?;
            let k = rd.index(8)?;
            for _ in 0..k {
                curve_2d(rd)?;
                rd.flag()?;
                rd.opt_index(n as i64)?;
                rd.flag()?;
                if rd.finite()? < 0.0 {
                    return Err(bad("negative edge owner tolerance"));
                }
            }
        }
        Op::ImplantPoly => {
            rd.domain_in(&srf, true)?;
            let factor = rd.finite()?;
            if !(1.0..=32.0).contains(&factor) {
                return Err(bad("polygon subdivision factor outside 1..=32"));
            }
            let n = tree(rd, &srf)?;
            bspline_block(rd)?;
            let count = rd.index(8)?;
            for _ in 0..count {
                rd.finites(6)?;
                // SmZoneTol3d accepts zero and stores abs(value) for negatives.
                rd.finite()?;
                rd.index(n - 1)?;
            }
            rd.flag()?;
            let count = rd.index(8)?;
            for _ in 0..count {
                curve_2d(rd)?;
                rd.flag()?;
                rd.opt_index(n as i64)?;
            }
        }
        Op::Face => {
            rd.flag()?;
            let k = rd.index(16)?;
            for _ in 0..k {
                rd.finite()?;
                rd.finite()?;
                rd.flag()?;
                let t = rd.finite()?;
                if t <= 0.0 {
                    return Err(bad("tolerance must be positive"));
                }
            }
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op == Op::ImplantEu {
        rd.0 = crate::reader::Coords::new(input, MAX_IMPLANT_UV_COORD);
    }
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces and curves, domains
        // inside the surface domain, well-formed subdivision trees, every index, count and
        // enumerator the shim passes to upstream); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_trim_srf_cache_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
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

    fn surface() -> Vec<f64> {
        let mut input = vec![0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0];
        for (x, y) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
            input.extend([x, y, 0.0, NOW]);
        }
        input.extend([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        input
    }

    #[test]
    fn rejects_truncation_and_trailing_data_before_cpp() {
        let mut input = surface();
        input.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 1.0, 15120.0]);
        assert!(validate(Op::New, &input).is_ok());
        for end in 0..input.len() {
            assert!(validate(Op::New, &input[..end]).is_err(), "prefix {end}");
        }
        input.push(0.0);
        assert!(validate(Op::New, &input).is_err());
    }

    #[test]
    fn rejects_null_face_classification_before_cpp() {
        let mut input = surface();
        input.extend([1.0, 0.0, 0.5, 0.5, 1.0, 0.001]);
        assert!(validate(Op::PointTest, &input).is_err());
        input[33] = 1.0; // face modified: upstream bypasses classification
        assert!(validate(Op::PointTest, &input).is_ok());
    }

    #[test]
    fn rejects_invalid_surface_counts_and_nonfinite_coordinates() {
        let mut input = surface();
        input.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.001, 0.0]);
        for bad_count in [-1.0, 1.5, f64::INFINITY, 1e30] {
            let mut bad = input.clone();
            bad[2] = bad_count;
            assert!(validate(Op::New, &bad).is_err());
        }
        input[8] = f64::NAN;
        assert!(validate(Op::New, &input).is_err());
    }

    #[test]
    fn validates_polygon_context_and_rejects_truncation_before_cpp() {
        let mut input = surface();
        input.extend([0.0, 0.0, 1.0, 1.0, 4.0]); // domain and factor
        input.extend([1.0, -1.0, -1.0, -1.0, 3.0, 2.0, 0.0, 0.0]);
        input.extend([0.0, 0.0, 1.0, 1.0]); // root domain
        input.extend(surface()); // polygon's original face surface
        input.extend([1.0, 0.1, 0.2, 0.0, 0.9, 0.8, 0.0, 0.001, 0.0]);
        input.extend([0.0, 0.0]); // no mapping, no edge-use calls
        assert!(validate(Op::ImplantPoly, &input).is_ok());
        for end in 0..input.len() {
            assert!(
                validate(Op::ImplantPoly, &input[..end]).is_err(),
                "prefix {end}"
            );
        }
        let node = input.len() - 3;
        input[node] = 1.0; // one past the final node
        assert!(validate(Op::ImplantPoly, &input).is_err());
        input[node] = 0.0;
        input[node - 1] = 0.0;
        assert!(validate(Op::ImplantPoly, &input).is_ok());
        input[node - 1] = -0.001;
        assert!(validate(Op::ImplantPoly, &input).is_ok());
        input[node - 1] = f64::NAN;
        assert!(validate(Op::ImplantPoly, &input).is_err());
    }
}
