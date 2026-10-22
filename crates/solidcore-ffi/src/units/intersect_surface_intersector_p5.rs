//! FFI wrappers for unit `intersect.surface_intersector_p5`.
//!
//! Shim: `harness/cpp/shim/intersect_surface_intersector_p5.cpp`
//! (`sc_intersect_surface_intersector_p5_run`). Upstream:
//! `source/SMLib/src/SmSurfaceIntersector.cpp` (`DoIntersection`, `GrazingBoxProcess`).
//!
//! [`run`] builds surfaces A and B (planes, B-splines or curve-bounded surfaces over
//! either; B may be A itself), the intersector `SmAdvSurfaceIntersector(A, dom1, B,
//! dom2)` with `SetUseSurfaceEdges(edges)`, and calls `DoIntersection(ctx, tol, ang,
//! ...)` with every output array. With [`Case::trees`], the decomposition tree of each
//! surface cache (`smsurf_GetSurfaceCache`; one for B = A) is replaced by the given
//! hand-built tree for the call.
//!
//! The record is `status`, with [`Case::trees`] the intersector state after the call
//! (`closest_value closest_uv1[2] closest_uv2[2] separate_gauss_maps trace_direction
//! ang_tol approx_tol solver_tol_3d solver_operation curve_closed curve_points
//! start_points through_points`, the list members as counts), then for status `0`:
//! `count` and per curve `3d uv1 uv2 type deviation`, where `3d` is a curve and each UV curve is `has [curve]` (`0` for a
//! `NULL` entry). Curves are written as `type dim knot_type form has [n p m cpts(x y z
//! w) knots] ivl[2]`.
//!
//! [`run`] accepts only finite inputs of magnitude at most [`MAX_COORD`], planes whose
//! domain has positive extent (UB-68), well-formed clamped B-splines and trees of the shape of the `intersect.adv_surface_intersector`
//! wrapper (at most [`MAX_NODES`] nodes, [`MAX_POLAR`] polar box vectors per node, the
//! top node first, children after their parent, leaves are patches with a pseudo box of
//! independent basis vectors, patches and heads have a polar box of non-zero vectors).
//! With trees, a surface upstream makes no cache for (a curve-bounded surface without
//! boundary curves) is a shim error ([`FfiError::Status`]): there is no tree to replace.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

pub use crate::units::intersect_adv_surface_intersector::NodeIn;
pub use crate::units::intersect_surface_intersector_p4::SurfaceIn;

mod sys {
    extern "C" {
        pub fn sc_intersect_surface_intersector_p5_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest input magnitude.
pub const MAX_COORD: f64 = 1e9;
/// Most nodes per tree.
pub const MAX_NODES: usize = 64;
/// Most polar box vectors per node.
pub const MAX_POLAR: usize = 8;
/// `NL_NOW`: the weight of a non-rational control point.
pub const NOW: f64 = -f64::MAX;

/// A case (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// Surface A.
    pub a: SurfaceIn,
    /// Surface B, `None` for A itself.
    pub b: Option<SurfaceIn>,
    /// Domain of A.
    pub dom1: [f64; 4],
    /// Domain of B.
    pub dom2: [f64; 4],
    /// `SetUseSurfaceEdges`.
    pub edges: [bool; 2],
    /// `pOptApproxTol3d` (`None` for `NULL`; the raw value, not its absolute value).
    pub tol: Option<f64>,
    /// `pdOptAngTolRad` (`None` for `NULL`).
    pub ang: Option<f64>,
    /// Hand-built trees of A and (unless B is A) B, replacing the cache trees.
    pub trees: Option<(Vec<NodeIn>, Option<Vec<NodeIn>>)>,
}

fn finite(x: f64) -> Result<f64, FfiError> {
    if x.is_finite() && x.abs() <= MAX_COORD {
        Ok(x)
    } else {
        Err(bad("value not finite or too large"))
    }
}

/// Clamped, non-decreasing knots with interior multiplicity at most `p`.
fn check_knots(knots: &[f64], ncp: usize, p: usize) -> Result<(), FfiError> {
    if knots.len() != ncp + p + 1 {
        return Err(bad("knot count"));
    }
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots decreasing"));
    }
    let (Some(&k0), Some(&km)) = (knots.first(), knots.last()) else {
        return Err(bad("knot count"));
    };
    if km <= k0
        || knots.iter().take(p + 1).any(|&k| k != k0)
        || knots.iter().rev().take(p + 1).any(|&k| k != km)
    {
        return Err(bad("knots not clamped"));
    }
    let mut run = 0usize;
    let mut last = f64::NAN;
    for &k in knots.iter().filter(|&&k| k != k0 && k != km) {
        run = if k == last { run + 1 } else { 1 };
        last = k;
        if run > p {
            return Err(bad("interior knot multiplicity"));
        }
    }
    Ok(())
}

/// All [`NOW`] or all in `(0, MAX_COORD]`.
fn check_weights<'a>(mut ws: impl Iterator<Item = &'a [f64; 4]>) -> Result<(), FfiError> {
    let Some(first) = ws.next() else {
        return Err(bad("no control points"));
    };
    let rational = first[3] != NOW;
    let ok = |w: f64| {
        if rational {
            w > 0.0 && w <= MAX_COORD
        } else {
            w == NOW
        }
    };
    if !ok(first[3]) || ws.any(|c| !ok(c[3])) {
        return Err(bad("weights"));
    }
    Ok(())
}

/// Checks the tree shape rules of the module docs.
fn check_tree(t: &[NodeIn]) -> Result<(), FfiError> {
    let n = t.len();
    if n == 0 || n > MAX_NODES {
        return Err(bad("tree size"));
    }
    let idx = |i: i64| usize::try_from(i).ok().filter(|&k| k < n);
    for (k, nd) in t.iter().enumerate() {
        if !matches!(nd.aux, 3 | 4 | 6) {
            return Err(bad("aux type"));
        }
        match (k, idx(nd.parent)) {
            (0, None) if nd.parent == -1 => {}
            (0, _) => return Err(bad("top node parent")),
            (_, Some(p)) if p < k => {
                let pn = t.get(p).ok_or_else(|| bad("parent"))?;
                if pn.c1 != k as i64 && pn.c2 != k as i64 {
                    return Err(bad("parent does not point back"));
                }
            }
            _ => return Err(bad("parent")),
        }
        match (nd.c1, nd.c2) {
            (-1, -1) => {
                if nd.aux != 3 {
                    return Err(bad("leaf is not a patch"));
                }
            }
            (a, b) => {
                let (Some(a), Some(b)) = (idx(a), idx(b)) else {
                    return Err(bad("child index"));
                };
                if a <= k || b <= k || a == b {
                    return Err(bad("child order"));
                }
                let back = |c: usize| t.get(c).is_some_and(|cn| cn.parent == k as i64);
                if !back(a) || !back(b) {
                    return Err(bad("child does not point back"));
                }
            }
        }
        if nd.polar.len() > MAX_POLAR || (nd.polar.is_empty() && nd.aux != 6) {
            return Err(bad("polar box"));
        }
        if nd.polar.iter().any(|v| v.iter().all(|&x| x == 0.0)) {
            return Err(bad("zero polar box vector"));
        }
        match (&nd.pseudo, nd.aux) {
            (Some(([a, b, c], _)), 3) => {
                // well clear of SetBasis' linear independence test (|det| < 1e-12)
                let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]);
                if det.is_nan() || det.abs() < 1e-6 {
                    return Err(bad("pseudo box basis not independent"));
                }
            }
            (None, 3) => return Err(bad("patch without pseudo box")),
            (Some(_), _) => return Err(bad("pseudo box on a non-patch")),
            (None, _) => {}
        }
    }
    Ok(())
}

struct Enc {
    v: Vec<f64>,
}

impl Enc {
    fn num(&mut self, x: f64) -> Result<(), FfiError> {
        self.v.push(finite(x)?);
        Ok(())
    }
    fn nums(&mut self, xs: &[f64]) -> Result<(), FfiError> {
        xs.iter().try_for_each(|&x| self.num(x))
    }
    fn int(&mut self, x: usize) {
        self.v.push(x as f64);
    }
    fn sint(&mut self, x: i64) {
        self.v.push(x as f64);
    }
    fn flag(&mut self, b: bool) {
        self.v.push(if b { 1.0 } else { 0.0 });
    }
    fn cpt(&mut self, c: &[f64; 4]) -> Result<(), FfiError> {
        self.nums(&c[..3])?;
        self.v.push(c[3]);
        Ok(())
    }

    fn surface(&mut self, s: &SurfaceIn) -> Result<(), FfiError> {
        match s {
            SurfaceIn::Plane {
                o,
                x,
                y,
                scale,
                domain,
            } => {
                // UB-68: a plane with a degenerate or inverted domain has no NURB, and
                // its surface cache hands the NULL NURB to `sm_CreateBezierPatchBlock`
                // (`SmSurfaceCache.cpp:4800`, SIGSEGV)
                if !(domain[2] > domain[0] && domain[3] > domain[1]) {
                    return Err(bad("plane domain must have positive extent"));
                }
                self.int(0);
                self.nums(o)?;
                self.nums(x)?;
                self.nums(y)?;
                self.nums(scale)?;
                self.nums(domain)
            }
            SurfaceIn::BSpline { p, q, net, u, v } => {
                let nu = net.len();
                let nv = net.first().map_or(0, Vec::len);
                if !(1..=8).contains(p) || !(1..=8).contains(q) {
                    return Err(bad("surface degree"));
                }
                if !(2..=17).contains(&nu) || !(2..=17).contains(&nv) || *p >= nu || *q >= nv {
                    return Err(bad("surface size"));
                }
                if net.iter().any(|r| r.len() != nv) {
                    return Err(bad("ragged net"));
                }
                check_knots(u, nu, *p)?;
                check_knots(v, nv, *q)?;
                check_weights(net.iter().flatten())?;
                self.int(1);
                self.int(*p);
                self.int(*q);
                self.int(nu);
                self.int(nv);
                for c in net.iter().flatten() {
                    self.cpt(c)?;
                }
                self.nums(u)?;
                self.nums(v)
            }
            SurfaceIn::CurveBounded(basis) => {
                if matches!(**basis, SurfaceIn::CurveBounded(_)) {
                    return Err(bad("nested curve-bounded surface"));
                }
                self.int(3);
                self.surface(basis)
            }
        }
    }

    fn tree(&mut self, t: &[NodeIn]) -> Result<(), FfiError> {
        check_tree(t)?;
        self.int(t.len());
        for nd in t {
            self.sint(nd.parent);
            self.sint(nd.c1);
            self.sint(nd.c2);
            self.int(usize::from(nd.aux));
            self.nums(&nd.bbox)?;
            self.nums(&nd.dom)?;
            self.int(nd.polar.len());
            for v in &nd.polar {
                self.nums(v)?;
            }
            if let Some((basis, ivls)) = &nd.pseudo {
                for v in basis {
                    self.nums(v)?;
                }
                for i in ivls {
                    self.nums(i)?;
                }
            }
        }
        Ok(())
    }
}

/// Encodes `case` for the shim.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected case.
pub fn encode(case: &Case) -> Result<Vec<f64>, FfiError> {
    let mut e = Enc { v: Vec::new() };
    e.surface(&case.a)?;
    match &case.b {
        None => e.int(2),
        Some(b) => e.surface(b)?,
    }
    e.nums(&case.dom1)?;
    e.nums(&case.dom2)?;
    e.flag(case.edges[0]);
    e.flag(case.edges[1]);
    e.flag(case.tol.is_some());
    e.num(case.tol.unwrap_or(0.0))?;
    e.flag(case.ang.is_some());
    e.num(case.ang.unwrap_or(0.0))?;
    match &case.trees {
        None => e.flag(false),
        Some((ta, tb)) => {
            e.flag(true);
            e.tree(ta)?;
            match (&case.b, tb) {
                (None, None) => {}
                (Some(_), Some(tb)) => e.tree(tb)?,
                _ => return Err(bad("one tree per distinct surface")),
            }
        }
    }
    Ok(e.v)
}

/// Runs `case` (see the module docs) and returns the record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for rejected inputs; [`FfiError::Status`] for a shim
/// failure.
pub fn run(case: &Case) -> Result<Vec<f64>, FfiError> {
    let input = encode(case)?;
    let n_in = i64::try_from(input.len()).map_err(|_| bad("case too long"))?;
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_intersect_surface_intersector_p5_run(
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
