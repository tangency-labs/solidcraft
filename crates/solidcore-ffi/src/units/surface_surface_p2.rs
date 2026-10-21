//! FFI wrappers for unit `surface.surface_p2`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p2.cpp` (`sc_surface_surface_p2_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! The input starts with a surface block `ctor`: `0` (static ops), `1` (`SmSurface()`, the
//! base class) or `2` (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)` from `oob form n m p q r s
//! cpts(x y z w) u v`, control points with the v index fastest). A curve `C` is `dim n p m
//! cpts(x y z w) knots` (`SmBSplineCurve(gw_CURVE*, dim, FALSE)`). `D` is a domain `umin vmin
//! umax vmax`, `I` an interval `a b`, `V` a vector `x y z`, `B` a box (two corners `V V`). The
//! op's parameters follow:
//!
//! | [`Op`]           | upstream                         | parameters                                   |
//! |------------------|----------------------------------|----------------------------------------------|
//! | `Eval`           | `Evaluate`                       | `u v hu hv u_left v_left upper nzt`          |
//! | `EvalPt`         | `EvaluatePoint`                  | `u v`                                        |
//! | `Queries`        | domains, degrees, counts, `IsBilinear`, type, `IsKindOf` | -                    |
//! | `Knots`          | `GetKnots`                       | `dir has_ivl I`                              |
//! | `Parallel`       | `IsParallelToVector`             | `V`                                          |
//! | `Reverse`        | `Reverse`                        | `dir`                                        |
//! | `Reparam`        | `Reparameterize`                 | `D`                                          |
//! | `Swap`           | `SwapUV`                         | -                                            |
//! | `Transform`      | `Transform`                      | `origin z x has_scale scale` (`V` each)      |
//! | `Trim`           | `TrimWithDomain`                 | `D`                                          |
//! | `Split`          | `SplitAt`                        | `t dir`                                      |
//! | `StepInv`        | `STEPInversion`                  | `D V tol has_guess gu gv`                    |
//! | `Extend`         | `CreateExtendedSurface`          | `dist cont`                                  |
//! | `ExtendDir`      | `CreateExtendedSurface` (dir)    | `dir dist cont`                              |
//! | `AdjustStep`     | `AdjustSTEPUVDomain`             | `D`                                          |
//! | `MaxAnal`        | `GetMaxAnalyticDomain`           | -                                            |
//! | `Copy`, `CopyAnal` | `Copy`, `CopyAndAddAnalytics`  | -                                            |
//! | `DropFast`       | `DropPointFast`                  | `D V`                                        |
//! | `ConvertUv`      | `ConvertUVFromSTEPToNURBS` / `ConvertUVFromNURBSToSTEP` | `u v`                 |
//! | `SEval`          | `SmSurfaceEval`                  | `ctor u v V k` then `k` x `kind a b c`       |
//! | `Gssi`           | `GlobalSurfaceIntersect`         | `D` surface (`oob form` + data) `D` `has_tol tol has_move V` |
//! | `Lift2`, `Lift3` | `smsurf_Lift*Derivative`         | 7 / 12 `V`                                   |
//! | `ScaleStep`      | `smsurf_ScaleSTEPAngularDerivatives` | `hu hv u_ang v_ang` `(hu+1)(hv+1)` `V`   |
//! | `Geom`           | `smsurf_EvaluateGeometric`       | 9 `V` (row major `[i][j]`)                   |
//! | `PointType`      | `smsurf_ComputeSurfacePointType` | `V V V want_uu want_vv side_uu`              |
//! | `DropVecs`       | `smsurf_DropVectors`             | `V V k` `k` `V`                              |
//! | `ParamDelta`     | `sm_ConvertToParameterDelta`     | `V V`                                        |
//! | `Merge`          | `sm_MergeDoubleArrays`           | `n a[n] m b[m]`                              |
//! | `MapUv`          | `s_MapUV`                        | `u v orient du dv`                           |
//! | `IsEven`         | `s_IsEven`                       | `i`                                          |
//! | `Breaks`         | `sm_SetUpBreaks`                 | `I n b[n]`                                   |
//! | `Limits`         | `GetDerivativeLimit`, `GetNormalLimit` | `V`                                    |
//! | `DerivAssert`    | `SmDerivSurfDefinition`          | `samples tt sdd edd has_tref stl ets sa ea V ct bet sr ers has_cref flip with_list` |
//! | `Cross`          | `CreateCrossSectionCurves`       | `n flags` `n` `V`, `k` `k` `V`, then per set flag bit `k` `k` `V` |
//! | `CrossSkin`      | ... then `SkinDerivSurface`      | as `Cross`, then `np params C has_r2 [C] tol` |
//! | `Dcf`            | `SmDropCurveFail`                | see the shim                                 |
//! | `CcLocal`        | `sm_CCLocalSolve`                | `C I k0 k1 C I k0 k1 op tol`                 |
//! | `IsectSplit`     | `sm_IntersectAndSplit`           | `C I V V n s[n]`                             |
//! | `IsectCurveSplit`| `sm_IntersectCurveAndSplit`      | `C I V V I zero n s[n]`                      |
//! | `Lsi`            | `SmLSIGlobalSolver::BranchMayContainAnswers` | `B V V has_ivl I ray tol`        |
//! | `CsPerpen`       | `sm_CsArePerpenDisjoint`         | `aux B aux B`                                |
//! | `Plane3d`        | `CreatePlaneWith3DCurves`        | `k` `k` `C` `tol`                            |
//! | `ReadDb`         | `SmSurface::ReadFromDB`          | `type file_type version nb bytes[nb]`        |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). [`eval`] accepts only what upstream handles without undefined
//! behaviour: well-formed clamped surfaces and curves, valid enumerators, bounded counts,
//! finite values, intervals inside the curves' ranges, non-zero line vectors, and the op
//! combinations upstream does not dereference null pointers in (`PointType` asks for the `v`
//! side only with the `u` side; `CsPerpen` with a Bezier surface node only when the cone
//! between the boxes is wider than 90 degrees; `Breaks` needs a break; `SEval` and `Gssi`
//! need a B-spline).

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 128;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e6;
/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Eval,
    EvalPt,
    Queries,
    Knots,
    Parallel,
    Reverse,
    Reparam,
    Swap,
    Transform,
    Trim,
    Split,
    StepInv,
    Extend,
    ExtendDir,
    AdjustStep,
    MaxAnal,
    Copy,
    CopyAnal,
    DropFast,
    ConvertUv,
    SEval,
    Gssi,
    Lift2,
    Lift3,
    ScaleStep,
    Geom,
    PointType,
    DropVecs,
    ParamDelta,
    Merge,
    MapUv,
    IsEven,
    Breaks,
    Limits,
    DerivAssert,
    Cross,
    CrossSkin,
    Dcf,
    CcLocal,
    IsectSplit,
    IsectCurveSplit,
    Lsi,
    CsPerpen,
    Plane3d,
    ReadDb,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        #[allow(clippy::enum_glob_use)]
        use Op::*;
        match self {
            Eval => 0,
            EvalPt => 1,
            Queries => 2,
            Knots => 3,
            Parallel => 4,
            Reverse => 5,
            Reparam => 6,
            Swap => 7,
            Transform => 8,
            Trim => 9,
            Split => 10,
            StepInv => 11,
            Extend => 12,
            ExtendDir => 13,
            AdjustStep => 14,
            MaxAnal => 15,
            Copy => 16,
            CopyAnal => 17,
            DropFast => 18,
            ConvertUv => 19,
            SEval => 20,
            Gssi => 21,
            Lift2 => 40,
            Lift3 => 41,
            ScaleStep => 42,
            Geom => 43,
            PointType => 44,
            DropVecs => 45,
            ParamDelta => 46,
            Merge => 47,
            MapUv => 48,
            IsEven => 49,
            Breaks => 50,
            Limits => 51,
            DerivAssert => 52,
            Cross => 53,
            CrossSkin => 54,
            Dcf => 55,
            CcLocal => 56,
            IsectSplit => 57,
            IsectCurveSplit => 58,
            Lsi => 59,
            CsPerpen => 60,
            Plane3d => 61,
            ReadDb => 62,
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
        pub fn sc_surface_surface_p2_eval(
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

    fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }

    fn ordered_pair(&mut self, what: &str) -> Result<(f64, f64), FfiError> {
        let a = self.finite()?;
        let b = self.finite()?;
        if a > b {
            return Err(bad(format!("{what}: not ordered")));
        }
        Ok((a, b))
    }

    fn domain(&mut self) -> Result<[f64; 4], FfiError> {
        let d = [
            self.finite()?,
            self.finite()?,
            self.finite()?,
            self.finite()?,
        ];
        if d[0] > d[2] || d[1] > d[3] {
            return Err(bad("domain not ordered"));
        }
        Ok(d)
    }
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

/// A B-spline surface block `oob form surface`.
fn bspline_block(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
    rd.flag()?;
    rd.index(11)?;
    surface(rd)
}

/// A curve `dim n p m cpts knots` (dimension 2 or 3); returns its parameter range.
fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
    check_knots(&knots, p, "curve")
}

/// An interval inside `(a, b)` of a curve.
fn interval_in(rd: &mut Reader<'_>, (a, b): (f64, f64)) -> Result<(), FfiError> {
    let (x, y) = rd.ordered_pair("interval")?;
    if x < a || y > b || x >= y {
        return Err(bad("interval outside the curve's range"));
    }
    Ok(())
}

/// `n` then `n` values.
fn values(rd: &mut Reader<'_>, max: usize) -> Result<usize, FfiError> {
    let n = rd.index(max)?;
    rd.finites(n)?;
    Ok(n)
}

/// `n` then `n` vectors.
fn vectors(rd: &mut Reader<'_>, max: usize) -> Result<usize, FfiError> {
    let n = rd.index(max)?;
    for _ in 0..n {
        rd.vec3()?;
    }
    Ok(n)
}

/// `ComputeConeBoundTo`'s half angle exceeds 90 degrees (the boxes overlap or are close):
/// conservative check that upstream returns before touching the Bezier patch.
fn cone_wider_than_90(s: [f64; 6], c: [f64; 6]) -> bool {
    // the grown other box contains the centre of this box: the cone is the full sphere
    let half = [
        (s[3] - s[0]).abs() / 2.0,
        (s[4] - s[1]).abs() / 2.0,
        (s[5] - s[2]).abs() / 2.0,
    ];
    let lo = [c[0].min(c[3]), c[1].min(c[4]), c[2].min(c[5])];
    let hi = [c[0].max(c[3]), c[1].max(c[4]), c[2].max(c[5])];
    let ctr = [
        (s[0] + s[3]) / 2.0,
        (s[1] + s[4]) / 2.0,
        (s[2] + s[5]) / 2.0,
    ];
    ctr.iter()
        .zip(lo.iter().zip(hi.iter()))
        .zip(half.iter())
        .all(|((c, (l, h)), r)| *c >= l - r && *c <= h + r)
}

fn box6(rd: &mut Reader<'_>) -> Result<[f64; 6], FfiError> {
    let a = rd.vec3()?;
    let b = rd.vec3()?;
    Ok([a[0], a[1], a[2], b[0], b[1], b[2]])
}

fn validate_surface_op(op: Op, rd: &mut Reader<'_>, dom: Option<[f64; 4]>) -> Result<(), FfiError> {
    #[allow(clippy::enum_glob_use)]
    use Op::*;
    let is_base = dom.is_none();
    match op {
        Eval => {
            rd.finites(2)?;
            let hu = rd.index(4)?;
            let hv = rd.index(4)?;
            rd.flag()?;
            rd.flag()?;
            let upper = rd.flag()?;
            rd.flag()?;
            if upper && hu != hv {
                return Err(bad("eval: upper half needs equal orders"));
            }
        }
        EvalPt | ConvertUv => {
            rd.finites(2)?;
        }
        Queries | Swap | MaxAnal | Copy | CopyAnal => {}
        Knots => {
            rd.index(1)?;
            rd.flag()?;
            rd.ordered_pair("knot interval")?;
        }
        Parallel => {
            rd.vec3()?;
        }
        Reverse => {
            rd.index(1)?;
        }
        Reparam | Trim | AdjustStep => {
            let d = rd.domain()?;
            if d[2] - d[0] < 1e-6 || d[3] - d[1] < 1e-6 {
                return Err(bad("domain too small"));
            }
        }
        Transform => {
            rd.vec3()?;
            let z = rd.nonzero3()?;
            let x = rd.nonzero3()?;
            let cross = [
                z[1] * x[2] - z[2] * x[1],
                z[2] * x[0] - z[0] * x[2],
                z[0] * x[1] - z[1] * x[0],
            ];
            if cross.iter().map(|c| c * c).sum::<f64>() < 1e-6 {
                return Err(bad("transform: axes parallel"));
            }
            rd.flag()?;
            let s = rd.vec3()?;
            if s.iter().any(|&c| c.abs() < 1e-3) {
                return Err(bad("transform: scale too small"));
            }
        }
        Split => {
            let t = rd.finite()?;
            let dir = rd.index(1)?;
            if let Some(d) = dom {
                let (a, b) = if dir == 0 { (d[0], d[2]) } else { (d[1], d[3]) };
                if t <= a || t >= b {
                    return Err(bad("split: parameter not inside the domain"));
                }
            }
        }
        StepInv => {
            rd.domain()?;
            rd.vec3()?;
            rd.positive("step inversion tol")?;
            rd.flag()?;
            rd.finites(2)?;
        }
        Extend => {
            rd.positive("extension distance")?;
            rd.index(13)?;
        }
        ExtendDir => {
            rd.index(8)?;
            rd.positive("extension distance")?;
            rd.index(13)?;
        }
        DropFast => {
            rd.domain()?;
            rd.vec3()?;
        }
        SEval => {
            if is_base {
                return Err(bad("surface evaluator needs a B-spline"));
            }
            rd.index(2)?;
            rd.finites(2)?;
            rd.vec3()?;
            let k = rd.index(32)?;
            for _ in 0..k {
                rd.index(3)?;
                rd.finites(3)?;
            }
        }
        Gssi => {
            if is_base {
                return Err(bad("intersection needs a B-spline"));
            }
            rd.domain()?;
            bspline_block(rd)?;
            rd.domain()?;
            let has_tol = rd.flag()?;
            let tol = rd.finite()?;
            if has_tol && tol <= 0.0 {
                return Err(bad("intersection tolerance: need a positive value"));
            }
            rd.flag()?;
            rd.vec3()?;
        }
        _ => return Err(bad("not a surface op")),
    }
    Ok(())
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    #[allow(clippy::enum_glob_use)]
    use Op::*;
    match op {
        Lift2 => {
            for _ in 0..7 {
                rd.vec3()?;
            }
        }
        Lift3 => {
            for _ in 0..12 {
                rd.vec3()?;
            }
        }
        ScaleStep => {
            let hu = rd.index(4)?;
            let hv = rd.index(4)?;
            rd.flag()?;
            rd.flag()?;
            for _ in 0..(hu + 1) * (hv + 1) {
                rd.vec3()?;
            }
        }
        Geom => {
            for _ in 0..9 {
                rd.vec3()?;
            }
        }
        PointType => {
            for _ in 0..3 {
                rd.vec3()?;
            }
            let uu = rd.flag()?;
            let vv = rd.flag()?;
            rd.index(2)?;
            if vv && !uu {
                return Err(bad("point type: v side without u side (null dereference)"));
            }
        }
        DropVecs => {
            rd.vec3()?;
            rd.vec3()?;
            vectors(rd, 16)?;
        }
        ParamDelta => {
            rd.vec3()?;
            rd.vec3()?;
        }
        Merge => {
            values(rd, 64)?;
            values(rd, 64)?;
        }
        MapUv => {
            rd.finites(2)?;
            rd.index(2)?;
            rd.finites(2)?;
        }
        IsEven => {
            let v = rd.finite()?;
            if v.fract() != 0.0 || v.abs() > 1e6 {
                return Err(bad("is_even: not a small integer"));
            }
        }
        Breaks => {
            rd.ordered_pair("break interval")?;
            if values(rd, 64)? == 0 {
                return Err(bad("breaks: empty array"));
            }
        }
        Limits => {
            rd.vec3()?;
        }
        DerivAssert => {
            rd.index(1000)?;
            rd.index(8)?;
            rd.finites(2)?;
            rd.flag()?;
            rd.finites(4)?;
            rd.vec3()?;
            rd.index(4)?;
            rd.index(3)?;
            rd.finites(2)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Cross | CrossSkin => {
            vectors_cross(rd)?;
            if op == CrossSkin {
                values(rd, 64)?;
                curve(rd)?;
                if rd.flag()? {
                    curve(rd)?;
                }
                let t = rd.finite()?;
                if t < 0.0 {
                    return Err(bad("skin: negative tolerance"));
                }
            }
        }
        Dcf => {
            rd.index(5000)?;
            rd.index(3)?;
            rd.index(24)?;
            rd.flag()?;
            rd.domain()?;
            rd.ordered_pair("interval")?;
            rd.finites(3)?;
            for _ in 0..5 {
                rd.index(2)?;
            }
            rd.finite()?;
            rd.index(2)?;
            rd.index(2)?;
            rd.index(9)?;
            rd.index(15)?;
            rd.index(15)?;
            vectors(rd, 8)?;
            vectors(rd, 8)?;
            vectors(rd, 16)?;
            values(rd, 16)?;
            let k = rd.index(4)?;
            for _ in 0..k {
                curve(rd)?;
            }
        }
        CcLocal => {
            let r1 = curve(rd)?;
            interval_in(rd, r1)?;
            rd.index(13)?;
            rd.index(13)?;
            let r2 = curve(rd)?;
            interval_in(rd, r2)?;
            rd.index(13)?;
            rd.index(13)?;
            let sop = rd.index(30)?;
            // SM_SO_MINIMIZE (3) or SM_SO_MAXIMIZE (4): the operations sm_CCLocalSolve asserts
            if sop != 3 && sop != 4 {
                return Err(bad("cc local solve: minimize or maximize only"));
            }
            rd.positive("tolerance")?;
        }
        IsectSplit => {
            let r = curve(rd)?;
            interval_in(rd, r)?;
            rd.vec3()?;
            rd.nonzero3()?;
            values(rd, 32)?;
        }
        IsectCurveSplit => {
            let r = curve(rd)?;
            interval_in(rd, r)?;
            rd.vec3()?;
            rd.nonzero3()?;
            let (a, b) = rd.ordered_pair("line interval")?;
            if b - a < 1e-6 {
                return Err(bad("line interval too small"));
            }
            rd.positive("scaled zero")?;
            values(rd, 32)?;
        }
        Lsi => {
            box6(rd)?;
            rd.vec3()?;
            rd.nonzero3()?;
            rd.flag()?;
            rd.ordered_pair("line interval")?;
            rd.flag()?;
            let t = rd.finite()?;
            if t < 0.0 {
                return Err(bad("negative tolerance"));
            }
        }
        CsPerpen => {
            let ca = rd.index(8)?;
            let cb = box6(rd)?;
            let sa = rd.index(8)?;
            let sb = box6(rd)?;
            let _ = ca;
            // SM_AD_BEZIER_SURFACE (3) / _HEAD (4) reach the patch's polar box unless the
            // cone between the boxes is wider than 90 degrees
            if (sa == 3 || sa == 4) && !cone_wider_than_90(sb, cb) {
                return Err(bad("cs perpen: Bezier surface node without patch data"));
            }
        }
        Plane3d => {
            let k = rd.index(8)?;
            for _ in 0..k {
                curve(rd)?;
            }
            rd.positive("tolerance")?;
        }
        ReadDb => {
            let t = rd.num()?;
            // the B-spline reader and types that are not surfaces (SM_ERR); the other
            // surface readers would read this stream as their own layout
            if t.fract() != 0.0 || !(0.0..=1e6).contains(&t) {
                return Err(bad("read db: bad type"));
            }
            let t = t as i64;
            if (15000..=15500).contains(&t) && t != 15002 {
                return Err(bad("read db: surface types other than the B-spline"));
            }
            rd.index(1)?;
            rd.index(100)?;
            let nb = rd.index(1 << 20)?;
            for _ in 0..nb {
                rd.index(255)?;
            }
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

/// The `Cross` arrays: `n flags` `n` `V`, `k` `k` `V`, then per set flag bit `k` `k` `V`.
fn vectors_cross(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(32)?;
    let flags = rd.index(63)?;
    for _ in 0..n {
        rd.vec3()?;
    }
    vectors(rd, 32)?;
    for b in 0..6 {
        if flags & (1 << b) != 0 {
            vectors(rd, 32)?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` as [`eval`] does (generators use it to redraw inputs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let ctor = rd.index(2)?;
    if (ctor == 0) != op.is_static() {
        return Err(bad("static ops need ctor 0, surface ops a surface"));
    }
    if ctor == 0 {
        validate_static(op, &mut rd)?;
    } else {
        let dom = if ctor == 2 {
            Some(bspline_block(&mut rd)?)
        } else {
            None
        };
        validate_surface_op(op, &mut rd, dom)?;
    }
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
        // reads for `op` (validated above: well-formed clamped surfaces and curves, every
        // index, count and flag the shim passes to upstream, the combinations upstream
        // handles without reading outside its arrays or through null pointers); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_p2_eval(
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
