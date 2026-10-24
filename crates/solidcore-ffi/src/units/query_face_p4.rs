//! FFI wrappers for unit `query.face_p4`.
//!
//! Shim: `harness/cpp/shim/query_face_p4.cpp` (`sc_query_face_p4_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp` (`SmFace::ClassifyLoops`).
//!
//! Every op starts with a B-rep block (the shim builds a fresh `SmBrep` per call and
//! deletes it after):
//!
//! | block                                           | B-rep                                     |
//! |-------------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`             | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)` (the `surface` block of [`super::query_face_p2`]) |
//! | `4 copies face validate_only init_gaps`         | `MakeFaceWithCurves` of `face`, `copies` times |
//! | `5 any_surface validate_only init_gaps`         | `CreateFaceInRegionFromSurface` over the natural domain |
//!
//! `any_surface` is a `surface` block or `kind scale a b c swap`: kind `10` `SmCylinder`
//! (radius `scale*a`), `11` `SmSphere` (radius `scale*a`), `12` `SmTorus` (radii `scale*a`,
//! `scale*b`), `13` `SmCone` (bottom radius `scale*a`, semi-angle `b` degrees), made by
//! `CreateCanonical` at the default placement and `SwapUV`'d when `swap`; an unbounded
//! natural direction is clamped to `[0, scale*c]`. `face` is `any_surface n_loops
//! count* n_curves (type a b c d orient)* n_pts (a b)*`: per loop its curve count; curve
//! `type` `0` a 2D line between the fractions `(a, b)` and `(c, d)` of the face domain,
//! `1` a counter-clockwise circle centred at fractions `(a, b)` with radius `c` times the
//! smaller domain side; `orient` `1` `SM_OT_SAME`, `2` `SM_OT_OPPOSITE`; points at
//! fractions `(a, b)` make single vertex loops.
//!
//! Then the case block `face props_mode loops_mode`: the face (modulo the face count),
//! the face props (`0` fresh `SmFaceProps(face, index)`, `1` as `SetProps_Stage3` builds
//! them before its `ClassifyLoops` call), the target loops (`0` none: `ClassifyLoops`
//! takes the face's, `1` the base list: `SetProps_Stage3`'s closed loops for props mode
//! `1`, else the face's loops, `2` the base list reversed, `3` without its last loop, `4`
//! with its first loop replaced by a loop of the next face, which an empty list gets).
//!
//! | [`Op`]     | outputs (after a leading `0`; `-1 status` when the B-rep could not be built) |
//! |------------|---------|
//! | `Export`   | the graph of [`super::topology_face_p2`], the case block `rejected [face props loops]`, the case block's length |
//! | `Classify` | `rejected [status loops classifiable not_yet n_io (loop type orient degen area containment)* start stop n_no_area idx* tree scratch_size]` |
//!
//! `props` is `stage face_index closed_u closed_v poles not_yet n_lp (cross_u wind_u
//! cross_v wind_v)* class_u class_v done_u done_v zone_tol no_vertex_poles
//! bad_missing_seam n_pp (x y z)*` (`n_pp` pole points); a classification is `0` (none) or `1
//! n (start_class start_vertex end_class end_vertex)*` (the vertex's export index for a
//! vertex classification, else `-1`); a touch list `n (loop dir loop_index)*` (`loop`: the
//! loop's export index, `-1` none, `-2` not a loop); `loops` `n idx*`; `tree` `n (pole
//! loop io partner_pole partner_loop partner_io child sibling)*` in pre-order.
//!
//! **Rejected** (`rejected` `1`, nothing else is run): inputs `ClassifyLoops` reads
//! outside its arrays with — fresh props on a closed surface (it reads the empty loop
//! props), a seam touch list of odd length, connected to something other than a loop or
//! with a loop index outside the target loops, an empty seam classification of a loop
//! crossing the seam, more target loops than loop props on a closed surface — and a
//! B-rep without faces or a next face without loops (loops mode `4`).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal, closed
//! loops of non-degenerate lines or lone circles inside the face domain, finite inputs.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Largest number of loops, curves per loop and vertex loop points of a face.
pub const MAX_LIST: usize = 8;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Classify = 1,
}

mod sys {
    extern "C" {
        pub fn sc_query_face_p4_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!("{what}: interior multiplicity above the degree")));
        }
    }
    Ok((a, b))
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z].iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a surface block, `None` for a plane.
type Domain = Option<[f64; 4]>;

/// A surface block; returns its domain.
fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    let kind = rd.index(1)?;
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok(None);
    }
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
    Ok(Some([u0, v0, u1, v1]))
}


/// An `any_surface` block (kinds 4 and 5): a B-spline `surface` block (not a plane,
/// whose natural domain is unbounded) or an analytic surface. Returns its periodic
/// parameter directions `[u, v]` (none for a B-spline).
fn any_surface(rd: &mut Reader<'_>) -> Result<[bool; 2], FfiError> {
    let kind = rd.input.get(rd.at).copied().ok_or_else(|| bad("input too short"))?;
    if kind == 0.0 {
        return surface(rd).map(|_| [false, false]);
    }
    rd.at += 1;
    if !(10.0..=13.0).contains(&kind) || kind.fract() != 0.0 {
        return Err(bad("surface kind: 0 or 10..13"));
    }
    let scale = rd.finite()?;
    let a = rd.finite()?;
    let b = rd.finite()?;
    let c = rd.finite()?;
    let swap = rd.flag()?;
    if !(1e-3..=1e3).contains(&scale) || !(0.1..=10.0).contains(&a) {
        return Err(bad("analytic surface: scale in [1e-3, 1e3], a in [0.1, 10]"));
    }
    let ok = match kind as i64 {
        12 => (0.01 * a..=0.9 * a).contains(&b),
        13 => (5.0..=60.0).contains(&b) && (0.1..=0.5 * a / b.to_radians().tan()).contains(&c),
        10 => (0.1..=100.0).contains(&c),
        _ => true,
    };
    if !ok {
        return Err(bad(
            "torus: minor radius in [0.01, 0.9] of the major; cone: semi-angle in [5, 60], \
             height in [0.1, half the apex height]; cylinder: height in [0.1, 100]",
        ));
    }
    Ok(if kind == 12.0 {
        [true, true]
    } else {
        [!swap, swap]
    })
}

/// A fraction of the face domain.
fn frac(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if !(0.0..=1.0).contains(&v) {
        return Err(bad("domain fraction outside [0, 1]"));
    }
    Ok(v)
}

/// A `MakeFaceWithCurves` face block: closed loops of lines or a lone circle.
fn face_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let periodic = any_surface(rd)?;
    let n_loops = rd.index(MAX_LIST)?;
    if n_loops == 0 {
        return Err(bad("face: no loop"));
    }
    let mut counts = Vec::new();
    for _ in 0..n_loops {
        let c = rd.index(MAX_LIST)?;
        if c == 0 {
            return Err(bad("face: empty loop"));
        }
        counts.push(c);
    }
    let n_curves = rd.index(MAX_LIST * MAX_LIST)?;
    if n_curves != counts.iter().sum::<usize>() {
        return Err(bad("face: curve count differs from the loops' total"));
    }
    for &count in &counts {
        // (start, end) of each curve in loop order
        let mut ends: Vec<([f64; 2], [f64; 2])> = Vec::new();
        let mut circle = false;
        for _ in 0..count {
            let ty = rd.index(1)?;
            let a = frac(rd)?;
            let b = frac(rd)?;
            let c = rd.finite()?;
            let d = rd.finite()?;
            let orient = rd.index(2)?;
            if orient == 0 {
                return Err(bad("curve orientation: 1 or 2"));
            }
            if ty == 1 {
                circle = true;
                if !(1e-3..=0.5).contains(&c) || a < c || a > 1.0 - c || b < c || b > 1.0 - c {
                    return Err(bad("circle: radius in [1e-3, 0.5] and inside the domain"));
                }
            } else {
                if !(0.0..=1.0).contains(&c) || !(0.0..=1.0).contains(&d) {
                    return Err(bad("line: end outside the domain"));
                }
                if (a - c).abs() < 1e-3 && (b - d).abs() < 1e-3 {
                    return Err(bad("line: degenerate"));
                }
                let (s, e) = if orient == 2 { ([c, d], [a, b]) } else { ([a, b], [c, d]) };
                ends.push((s, e));
            }
        }
        if circle {
            if count != 1 {
                return Err(bad("circle: only as a loop of its own"));
            }
            continue;
        }
        // consecutive ends meet, or wrap around a periodic direction (0 and 1)
        let meets = |p: [f64; 2], q: [f64; 2]| {
            (0..2).all(|i| {
                let (a, b) = (p.get(i), q.get(i));
                a == b
                    || (periodic.get(i) == Some(&true)
                        && matches!((a, b), (Some(&x), Some(&y)) if (x - y).abs() == 1.0))
            })
        };
        let closed = ends
            .iter()
            .zip(ends.iter().cycle().skip(1))
            .all(|(p, q)| meets(p.1, q.0));
        if !closed {
            return Err(bad("face: loop not closed"));
        }
    }
    let n_pts = rd.index(MAX_LIST)?;
    for _ in 0..n_pts {
        frac(rd)?;
        frac(rd)?;
    }
    Ok(())
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(5)? {
        0 => {
            let p = Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return Err(bad("B-rep file: need an existing .smb file"));
            }
            rd.flag()?;
        }
        2 => {
            let dom = surface(rd)?;
            let d = rd.finites(4)?;
            let (Some(&a), Some(&b2), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3))
            else {
                return Err(bad("domain too short"));
            };
            if a >= c || b2 >= e {
                return Err(bad("face domain not ordered"));
            }
            match dom {
                Some([u0, v0, u1, v1]) => {
                    if a < u0 || c > u1 || b2 < v0 || e > v1 {
                        return Err(bad("face domain outside the surface's"));
                    }
                }
                None => {
                    if d.iter().any(|x| x.abs() > 1e4) {
                        return Err(bad("plane face domain too large"));
                    }
                }
            }
        }
        4 => {
            let copies = rd.index(2)?;
            if copies == 0 {
                return Err(bad("face copies: 1 or 2"));
            }
            face_block(rd)?;
        }
        5 => {
            any_surface(rd)?;
        }
        _ => return Err(bad("B-rep kind: 0, 2, 4 or 5")),
    }
    rd.flag()?;
    rd.flag()?;
    Ok(())
}

/// Checks the B-rep block at the start of `input` as [`eval`] does (shared with unit
/// `query.face_p3`, whose shim builds B-reps with this unit's builders); returns the
/// block's length.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the shim would not handle safely.
pub fn check_brep_block(input: &[f64], path: &str) -> Result<usize, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    Ok(rd.at)
}

/// Runs `op` on `input` (a B-rep block and the case block); `path` is the `.smb` file of
/// a kind `0` block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim would not handle safely (module
/// docs); [`FfiError::Status`] for a shim failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    rd.index(1 << 20)?;
    rd.index(1)?;
    rd.index(4)?;
    rd.done()?;
    let c_path = std::ffi::CString::new(path).map_err(|_| bad("path contains NUL"))?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: input is valid for input.len() reads, c_path is a NUL-terminated
        // string, buf is valid for buf.len() writes and len for one write.
        unsafe {
            sys::sc_query_face_p4_eval(
                op as i32,
                c_path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Splits an `Export` output into the graph (for [`super::topology_face_p2`]'s importer)
/// and the case block; `None` for a build failure or a malformed output.
#[must_use]
pub fn split_export(out: &[f64]) -> Option<(&[f64], &[f64])> {
    if out.first() != Some(&0.0) {
        return None;
    }
    let (&len, rest) = out.split_last()?;
    if len.fract() != 0.0 || len < 1.0 {
        return None;
    }
    let at = rest.len().checked_sub(len as usize)?;
    Some((rest.get(..at)?, rest.get(at..)?))
}
