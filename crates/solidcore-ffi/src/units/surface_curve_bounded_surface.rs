//! FFI wrappers for unit `surface.curve_bounded_surface`.
//!
//! Shim: `harness/cpp/shim/surface_curve_bounded_surface.cpp`
//! (`sc_surface_curve_bounded_surface_eval`). Upstream:
//! `source/SMLib/src/SmCurveBoundedSurface.cpp`, `source/SMLib/inc/SmCurveBoundedSurface.h`.
//!
//! The input starts with `ctor`: `0` (static ops) or `1` (a curve bounded surface `B`).
//! `B` is the basis `oob form S` (an `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`), `implicit`,
//! `n3d` then `n3d` curves `C`, `nuv` then `nuv` curves `C`. `S` is a surface `n m p q r s
//! cpts(x y z w) u v` (control points with the v index fastest), `C` a curve `dim n p m
//! cpts(x y z w) knots` (`SmBSplineCurve(gw_CURVE*, dim, FALSE)`). The op's parameters follow:
//!
//! | [`Op`]          | upstream (through `SmSurface*`)               | parameters                        |
//! |-----------------|-----------------------------------------------|-----------------------------------|
//! | `State`         | - (the members)                               | -                                 |
//! | `Eval`          | `Evaluate`                                    | `u v hu hv u_left v_left upper nzt` |
//! | `EvalPt`        | `EvaluatePoint`                               | `u v`                             |
//! | `Queries`       | domains, degrees, counts, `IsBilinear`, type, `IsKindOf`, type and class strings | `k` then `k` types |
//! | `Defaults`      | `GetKnots`, `IsParallelToVector`, `GetMaxAnalyticDomain`, `Copy`, `CopyAndAddAnalytics`, `SplitAt(0.5, U)`, `Reverse(U)`, `SwapUV`, then the state | `x y z` |
//! | `EqSelf`        | `*s == *s`                                    | -                                 |
//! | `EqOther`       | `*s == *other`                                | `B`                               |
//! | `Normal`        | `EvaluateNormal`                              | `u v u_left v_left`               |
//! | `Simple`, `Step`| `EvaluateSimple`, `EvaluateSTEP`              | `u v hu hv u_left v_left upper`   |
//! | `Singular`      | `EvaluateNormalAtSingularity`                 | `u v tol`                         |
//! | `Geometric`     | `EvaluateGeometric`                           | `u v u_left v_left`               |
//! | `NormalSection` | `EvaluateNormalSection`                       | `u v u_left v_left x y z`         |
//! | `StepPt`        | `EvaluateSTEPPoint`                           | `u v`                             |
//! | `SetContext`    | `SetContext`, then the state                  | -                                 |
//! | `ReadDb`        | `SmSurface::ReadFromDB` (ASCII)               | `type version nb bytes[nb]`       |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). The state is `type`, and for a curve bounded surface `implicit`,
//! the basis (`1 has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`), `n3d` curves and
//! `nuv` curves, each `type dim a b`, five `status x y z` (points at `a + (b - a) * i / 4`),
//! then `has_nurb [n p m cpts knots]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed
//! clamped surfaces and curves, bounded counts, finite values, parameters inside the basis
//! domain, `EqOther` only for a surface with at least as many UV as 3D boundaries
//! (`operator==` indexes the UV array with the 3D count). `ReadDb` streams are only built by [`cbs_record`] from validated data
//! ([`read_db`]): upstream reads an ASCII stream it cannot parse into uninitialised locals,
//! so arbitrary bytes are not accepted.

use std::fmt::Write as _;

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 32;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 7;
/// Largest number of boundary curves per array accepted by [`eval`].
pub const MAX_CURVES: usize = 6;
/// Largest coordinate magnitude accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e6;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `SmBSplineSurface_TYPE`.
pub const B_SPLINE_SURFACE_TYPE: i64 = 15002;
/// `SmBSplineCurve_TYPE`.
pub const B_SPLINE_CURVE_TYPE: i64 = 14033;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    State,
    Eval,
    EvalPt,
    Queries,
    Defaults,
    EqSelf,
    EqOther,
    Normal,
    Simple,
    Singular,
    Geometric,
    NormalSection,
    Step,
    StepPt,
    SetContext,
    ReadDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        #[allow(clippy::enum_glob_use)]
        use Op::*;
        match self {
            State => 0,
            Eval => 1,
            EvalPt => 2,
            Queries => 3,
            Defaults => 4,
            EqSelf => 5,
            EqOther => 6,
            Normal => 7,
            Simple => 8,
            Singular => 9,
            Geometric => 10,
            NormalSection => 11,
            Step => 12,
            StepPt => 13,
            SetContext => 14,
            ReadDb => 40,
        }
    }

    /// Whether the op is static (no surface).
    #[must_use]
    pub fn is_static(self) -> bool {
        self.code() >= 40
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_curve_bounded_surface_eval(
            op: i32,
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

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
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

    /// A parameter point inside `dom` (`umin vmin umax vmax`).
    fn uv_in(&mut self, dom: [f64; 4]) -> Result<(), FfiError> {
        let u = self.finite()?;
        let v = self.finite()?;
        if u < dom[0] || u > dom[2] || v < dom[1] || v > dom[3] {
            return Err(bad("parameter outside the basis domain"));
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
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
}

/// Checks 3D control points sharing their rational marker.
fn check_points(rd: &mut Reader<'_>, k: usize, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// A surface `n m p q r s cpts u v`; returns its parameter domain.
fn surface(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
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
    check_points(rd, (n + 1) * (m + 1), "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok([u0, v0, u1, v1])
}

/// A curve `dim n p m cpts knots` (dimension 2 or 3).
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")?;
    Ok(())
}

/// `n` then `n` curves; returns `n`.
fn curves(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let n = rd.index(MAX_CURVES)?;
    for _ in 0..n {
        curve(rd)?;
    }
    Ok(n)
}

/// A curve bounded surface block `B`; returns the basis domain and the numbers of 3D
/// and UV boundaries.
fn cbs_block(rd: &mut Reader<'_>) -> Result<([f64; 4], usize, usize), FfiError> {
    rd.flag()?;
    rd.index(11)?;
    let dom = surface(rd)?;
    rd.flag()?;
    let n3d = curves(rd)?;
    let nuv = curves(rd)?;
    Ok((dom, n3d, nuv))
}

/// `u v hu hv u_left v_left upper`: orders up to 4, equal when `upper`.
fn derivs(rd: &mut Reader<'_>, dom: [f64; 4]) -> Result<(), FfiError> {
    rd.uv_in(dom)?;
    let hu = rd.index(4)?;
    let hv = rd.index(4)?;
    rd.flag()?;
    rd.flag()?;
    let upper = rd.flag()?;
    if upper && hu != hv {
        return Err(bad("upper half needs equal orders"));
    }
    Ok(())
}

fn validate_surface_op(
    op: Op,
    rd: &mut Reader<'_>,
    (dom, n3d, nuv): ([f64; 4], usize, usize),
) -> Result<(), FfiError> {
    #[allow(clippy::enum_glob_use)]
    use Op::*;
    match op {
        State | EqSelf | SetContext => {}
        Eval => {
            derivs(rd, dom)?;
            rd.flag()?;
        }
        EvalPt | StepPt => rd.uv_in(dom)?,
        Queries => {
            let k = rd.index(16)?;
            for _ in 0..k {
                rd.index(1 << 20)?;
            }
        }
        Defaults => {
            rd.vec3()?;
        }
        EqOther => {
            // operator== indexes the UV boundaries up to the number of 3D boundaries
            if n3d > nuv {
                return Err(bad(
                    "eq: more 3D than UV boundaries (reads past the UV array)",
                ));
            }
            cbs_block(rd)?;
        }
        Normal | Geometric => {
            rd.uv_in(dom)?;
            rd.flag()?;
            rd.flag()?;
        }
        Simple | Step => derivs(rd, dom)?,
        Singular => {
            rd.uv_in(dom)?;
            let tol = rd.finite()?;
            if tol <= 0.0 {
                return Err(bad("singular: need a positive tolerance"));
            }
        }
        NormalSection => {
            rd.uv_in(dom)?;
            rd.flag()?;
            rd.flag()?;
            let v = rd.vec3()?;
            if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
                return Err(bad("normal section: zero direction"));
            }
        }
        ReadDb => return Err(bad("static op")),
    }
    Ok(())
}

/// Checks `input` for `op` as [`eval`] does (generators use it to redraw inputs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    if op == Op::ReadDb {
        return Err(bad("ReadDb streams go through read_db"));
    }
    let mut rd = Reader::new(input);
    let ctor = rd.index(1)?;
    if ctor != 1 {
        return Err(bad("surface ops need a curve bounded surface (ctor 1)"));
    }
    let block = cbs_block(&mut rd)?;
    validate_surface_op(op, &mut rd, block)?;
    rd.done()
}

fn run(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(65536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated by the caller: well-formed clamped surfaces and
        // curves, bounded counts and indices, parameters inside the basis domain, or an
        // ASCII record built by `cbs_record` from such data); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_curve_bounded_surface_eval(
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

/// Runs surface op `op` on `input` (layouts in the module docs); returns the shim's
/// output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    run(op, input)
}

/// An inline B-spline record for [`cbs_record`]: control points `[x, y, z, w]` (`w` =
/// `NL_NOW` when non-rational) and knots.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordCurve {
    /// Dimension (2 or 3): the header's and the record's.
    pub dim: u64,
    /// Degree.
    pub p: usize,
    /// Control points as stored.
    pub cpts: Vec<[f64; 4]>,
    /// Knots.
    pub knots: Vec<f64>,
}

/// An inline B-spline surface record for [`cbs_record`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecordSurface {
    /// Degrees `(p, q)`.
    pub pq: (usize, usize),
    /// Control net as stored, rows in u.
    pub net: Vec<Vec<[f64; 4]>>,
    /// u knots.
    pub u: Vec<f64>,
    /// v knots.
    pub v: Vec<f64>,
}

/// What [`cbs_record`] writes: an `SmCurveBoundedSurface::WriteToDB` ASCII record (stream
/// version <= 32 layout of the B-spline records).
#[derive(Debug, Clone, PartialEq)]
pub struct CbsRecord {
    /// The implicit-outer flag written.
    pub implicit: bool,
    /// The basis surface's type header: `SmBSplineSurface_TYPE` or a type that is not a
    /// surface (upstream fails without reading on).
    pub basis_type: i64,
    /// The basis surface record.
    pub basis: RecordSurface,
    /// The 3D boundaries (written with `SmBSplineCurve_TYPE` headers).
    pub curves_3d: Vec<RecordCurve>,
    /// The type written in the UV boundaries' headers (upstream reads them with the
    /// last 3D type whatever this says).
    pub uv_header_type: i64,
    /// The UV boundaries.
    pub curves_uv: Vec<RecordCurve>,
}

fn rational(pts: &[[f64; 4]]) -> bool {
    pts.first().is_some_and(|p| p[3] != NOW)
}

fn push_point(t: &mut String, p: &[f64; 4], dim: u64, rat: bool) {
    let k = if dim == 2 { 2 } else { 3 };
    let mut first = true;
    for c in p
        .iter()
        .take(k)
        .chain(p.iter().skip(3).take(usize::from(rat)))
    {
        if !first {
            t.push(' ');
        }
        first = false;
        t.push_str(&c.to_string());
    }
    t.push('\n');
}

fn push_curve(t: &mut String, ty: i64, c: &RecordCurve) {
    let rat = rational(&c.cpts);
    let _ = write!(t, "Object Type {ty}\nCurve Dim {}\n", c.dim);
    let _ = write!(
        t,
        "{}\n{}\n{}\n{}\n",
        c.cpts.len().saturating_sub(1),
        c.p,
        u8::from(rat),
        c.dim
    );
    for p in &c.cpts {
        push_point(t, p, c.dim, rat);
    }
    for k in &c.knots {
        let _ = writeln!(t, "{k}");
    }
}

/// The ASCII record of `r`, laid out as `SmCurveBoundedSurface::WriteToDB` and the
/// B-spline writers do (version <= 32).
#[must_use]
pub fn cbs_record(r: &CbsRecord) -> String {
    let mut t = String::new();
    let _ = writeln!(
        t,
        "{} SmCurveBoundedSurface Number of 3D Boundaries ",
        r.curves_3d.len()
    );
    let _ = writeln!(
        t,
        "{} SmCurveBoundedSurface Number of UV Boundaries ",
        r.curves_uv.len()
    );
    let _ = writeln!(
        t,
        "{} SmCurveBoundedSurface Implicit Outer Flag ",
        u8::from(r.implicit)
    );
    t.push_str(" SmCurveBoundedSurface->BasisSurface \n");
    let _ = writeln!(t, "Object Type {}", r.basis_type);
    let s = &r.basis;
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    let rat = s.net.first().is_some_and(|row| rational(row));
    let _ = write!(t, "{n} {m}\n{} {}\n{}\n", s.pq.0, s.pq.1, u8::from(rat));
    for row in &s.net {
        for p in row {
            push_point(&mut t, p, 3, rat);
        }
    }
    for k in s.u.iter().chain(&s.v) {
        let _ = writeln!(t, "{k}");
    }
    for c in &r.curves_3d {
        push_curve(&mut t, B_SPLINE_CURVE_TYPE, c);
    }
    for c in &r.curves_uv {
        push_curve(&mut t, r.uv_header_type, c);
    }
    t
}

fn check_record_curve(c: &RecordCurve, what: &str) -> Result<(), FfiError> {
    let mut f = vec![
        c.dim as f64,
        c.cpts.len().saturating_sub(1) as f64,
        c.p as f64,
    ];
    f.push(c.knots.len().saturating_sub(1) as f64);
    for p in &c.cpts {
        f.extend_from_slice(p);
    }
    f.extend_from_slice(&c.knots);
    let mut rd = Reader::new(&f);
    curve(&mut rd).map_err(|e| bad(format!("{what}: {e}")))?;
    rd.done()
}

fn check_record(r: &CbsRecord) -> Result<(), FfiError> {
    // the B-spline reader, or a type that is not a surface (SM_ERR without reading):
    // the other surface readers would read the B-spline record as their own layout
    if (15000..=15500).contains(&r.basis_type) && r.basis_type != B_SPLINE_SURFACE_TYPE {
        return Err(bad("read db: basis types other than the B-spline"));
    }
    let s = &r.basis;
    let n = s.net.len().checked_sub(1).ok_or_else(|| bad("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| bad("empty net row"))?;
    if s.net.iter().any(|row| row.len() != cols) {
        return Err(bad("net rows differ in length"));
    }
    let mut f = vec![
        n as f64,
        m as f64,
        s.pq.0 as f64,
        s.pq.1 as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ];
    for row in &s.net {
        for p in row {
            f.extend_from_slice(p);
        }
    }
    f.extend_from_slice(&s.u);
    f.extend_from_slice(&s.v);
    let mut rd = Reader::new(&f);
    surface(&mut rd)?;
    rd.done()?;
    if r.curves_3d.len() > MAX_CURVES || r.curves_uv.len() > MAX_CURVES {
        return Err(bad("too many boundaries"));
    }
    for c in &r.curves_3d {
        check_record_curve(c, "3D boundary")?;
    }
    for c in &r.curves_uv {
        check_record_curve(c, "UV boundary")?;
    }
    if !(1..=1 << 20).contains(&r.uv_header_type) {
        return Err(bad("UV header type out of range"));
    }
    Ok(())
}

/// Checks a [`read_db`] request as [`read_db`] does (generators use it to redraw
/// inputs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a record, type or version the shim must not see.
pub fn validate_read_db(surface_type: i64, version: u64, r: &CbsRecord) -> Result<(), FfiError> {
    check_record(r)?;
    if surface_type != 15045 {
        return Err(bad("read db: only SmCurveBoundedSurface_TYPE"));
    }
    if version > 32 {
        return Err(bad(
            "read db: records are written in the version <= 32 layout",
        ));
    }
    Ok(())
}

/// Reads [`cbs_record`]`(r)` with `SmSurface::ReadFromDB(surface_type, ..)` (ASCII,
/// `version`); returns the shim's output record (status, then the state on success).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a record or type the shim must not see;
/// [`FfiError::Status`] for a shim failure.
pub fn read_db(surface_type: i64, version: u64, r: &CbsRecord) -> Result<Vec<f64>, FfiError> {
    validate_read_db(surface_type, version, r)?;
    let bytes = cbs_record(r).into_bytes();
    let mut input = vec![0.0, surface_type as f64, version as f64, bytes.len() as f64];
    input.extend(bytes.iter().map(|&b| f64::from(b)));
    run(Op::ReadDb, &input)
}
