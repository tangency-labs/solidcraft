//! NURBS surface evaluation and geometry: basis function derivatives, points,
//! derivatives, unit normals (also at poles), bounding boxes, affine maps,
//! reversal, reparametrisation, type recognition (plane, sphere, torus, cylinder,
//! cone, revolution, ruled, extrusion), smooth closure and the elliptic-ended cone.
//!
//! Upstream: `source/SMLib/src/NL_SrfNurb.cpp`, `source/SMLib/inc/NL_Nurbsdef.h`
//! (`NL_BASISDERIVATIVES`; the inline helpers of `source/SMLib/inc/NLI_Math.h`).
//!
//! | Rust                                         | Upstream                              |
//! |----------------------------------------------|---------------------------------------|
//! | [`BasisDerivs`], [`BasisDerivs::get`]        | `NL_BASISDERIVATIVES`, `NL_BASISDERIVATIVES::operator[]` |
//! | `compute_basis_functions` (private)          | `N_ComputeBasisFunctions`             |
//! | `compute_derivatives` (private)              | `N_ComputeDerivatives`, `N_ComputeDerivativesImpl`, `N_SwapRealPointers` |
//! | [`Surface::basis_derivs`]                    | `N_SrfBasisDerivs`                    |
//! | [`Surface::eval`]                            | `N_SrfEvalPt`, `N_SrfEvalPtImpl`      |
//! | `blend_spec`, `to_euclid_spec` (private)     | `NI_VectorBlendCPt_specialized`, `NI_CPtToPtEuclid_specialized` |
//! | `derivative_matrix` (private)                | `N_ComputeDerivativeMatrix`, `N_ComputeDerivativeMatrixSpec` |
//! | [`Surface::derivs`]                          | `N_SrfDerivs`                         |
//! | [`Surface::first_derivs_into`]               | `N_SrfDerivs` with `udr = vdr = 1`    |
//! | [`Surface::bbox`]                            | `N_SrfGetBBox`                        |
//! | [`Surface::transform`]                       | `N_SrfTransform`                      |
//! | [`Surface::min_max_weight_pos_vectors`], [`MinMaxWeightPos`] | `N_SrfMinMaxWeightPosVectors` |
//! | [`Surface::eval_pt_deriv_normal_fast`], [`PtDerivNormal`] | `N_SrfEvalPtPtDerivNormalFast` |
//! | [`Surface::translate`]                       | `N_SrfTranslate`                      |
//! | [`Surface::rotate_at_pt`]                    | `N_SrfRotateAtPt`                     |
//! | [`Surface::reverse`] / [`Surface::reversed`] | `N_SrfReverse` (in place / into a new surface; knots by `NI_ReverseKnots`) |
//! | [`Surface::surface_type`], [`SurfaceType`], [`SurfaceKind`] | `N_SrfType`            |
//! | [`Surface::reparam`]                         | `N_SrfReparam`                        |
//! | [`Surface::is_closed_smooth`], [`Surface::is_closed_smooth_flag`], `_with_fixes` forms | `N_SrfIsClosedSmooth` |
//! | [`Surface::eval_pt_deriv_normal_pole`]       | `N_SrfEvalPtPtDerivNormalPole`        |
//! | [`Surface::eval_pt_normal_at_pole`]          | `N_SrfEvalPtNormalAtPole`             |
//! | [`cone_elliptic_ends`], [`EllipticCone`]     | `N_ConeEllipticEnds`                  |
//!
//! Deviations:
//! - Out-params become return values; errors are [`SolidError::Nlib`] values with the
//!   upstream `NL_*_ERR` number instead of a flag plus the global `NL_ERROR` record.
//!   The `NL_STACKS` arguments and the memory error paths have no counterpart. A
//!   routine that fails returns only the error, even where upstream has already
//!   written some outputs.
//! - `NL_BASISDERIVATIVES` (a fixed `[NL_MAXDER+1]^2 [NL_MAXDEG+1]^2` array) becomes
//!   [`BasisDerivs`], sized `[udr+1][vdr+1][p+1][q+1]`; the derivative matrix of
//!   [`Surface::derivs`] is a `Vec` of `udr + 1` rows of `vdr + 1` points. Entries
//!   upstream leaves unwritten (the lower half with `mfl`) are zero.
//!   [`Surface::first_derivs_into`] writes the first-derivative matrix into the
//!   caller's 2x2 array instead, and reads a non-rational basis derivative as the
//!   product of the two univariate tables (the value `N_SrfBasisDerivs` stores).
//! - Every array access is bounds checked: degrees above `NL_MAXDEG` (upstream's
//!   scratch array size) and nets or knot vectors that do not match the degrees are
//!   [`SolidError::InvalidInput`] where upstream would index outside its arrays.
//!   The fixed scratch arrays are `TINY_NP` (4) wide when both degrees are below 4,
//!   `SMALL_NP` (8) wide when both are below 8 and `NL_MAXDEG + 1` otherwise (every index used is at most the degree, so the
//!   values and errors are the same; this only avoids zeroing large stack arrays).
//! - `N_SrfIsClosedSmooth`: a degree whose span index `+ 1` overflows (near
//!   `isize::MAX`, upstream's signed overflow) is `InvalidInput`, as an index
//!   outside the knot array.
//! - `N_SrfReverse` keeps both forms: [`Surface::reverse`] in place,
//!   [`Surface::reversed`] into a new surface sized as upstream sizes a NULL output.
//! - `N_SrfType` points its curve outputs at its scratch curves, so a curve output
//!   set by a branch that later fails shows the scratch curve's final content; the
//!   port keeps that ([`SurfaceType::cur_a`] is the final scratch curve whenever
//!   upstream has set `curA`). The other outputs written by failed branches are kept
//!   as well. `N_ProjectPtLine` results that miss the axis (upstream then reads a stale
//!   point) are `InvalidInput`.
//! - `N_SrfIsClosedSmooth` and `N_SrfReverse` take [`Direction`]s, so flags other
//!   than `NL_UDIR` / `NL_VDIR` / `NL_UVDIR` cannot be expressed; the level `n` is a
//!   `usize` (`n < 0` cannot be expressed), missing tolerances are `InvalidInput`.
//!   `N_SrfIsClosedSmooth` returns its error flag after a failed `N_VectorsAngle`
//!   when no later evaluation clears it; the port returns that error.
//! - `N_SrfIsClosedSmooth` with
//!   [`UpstreamFixes::closed_smooth_pole_tangent_limit`] (opt-in, WR-725, #2521): at a
//!   station where the derivative across the seam vanishes (a pole on the seam) the
//!   G1 test takes its limit, the mixed derivative, instead of the angle of two
//!   vanishing vectors; the stations are then evaluated with the mixed derivative
//!   (the lower orders are the same bits).
//! - `NL_MTOL` is [`MODEL_TOL`] and `NL_LTOL` is [`LINE_TOL`]
//!   ([`Surface::is_closed_smooth`] passes its G0 tolerance to
//!   [`Surface::is_closed_tol`] instead of overriding the global).
//! - `N_ConeEllipticEnds` returns a bare error flag (no `NL_*_ERR` number) when an
//!   end plane is parallel to a ruling or an iso-curve has unexpected sizes; the port
//!   returns [`SolidError::Degenerate`] then. A failing `N_IsectLinePlane` (upstream
//!   then reads an unset flag) propagates its error.

// Branches keep upstream's `if( x NEQ NL_NOZ ) ... else ...` order for traceability.
#![allow(clippy::if_not_else)]

use crate::error::{NlibCode, Result, SolidError};
use crate::geom::a_object::reverse_knots;
use crate::kernel::UpstreamFixes;
use crate::math::Vec3;
use crate::nurbs::crv_basic::{curves_are_coincident, ConicType, Curve};
use crate::nurbs::crv_nurb::{line_from_pt_and_vector, CurveType};
use crate::nurbs::geom::{
    dist_pt_inf_line, dist_pt_line_seg, dist_pt_pt, dist_signed_pt_plane, isect_line_plane,
    net_bbox, normalize, project_pt_line, rotation_matrix_about_axis, vectors_angle,
    LinePlaneIsect, LineSeg, MinMaxBox, Plane, Rectangle, LINE_TOL,
};
use crate::nurbs::index::{get, get2, oob, set, set2, sx, ux};
use crate::nurbs::knots::{KnotVector, Side, MAX_DEGREE};
use crate::nurbs::math::{
    combine4_pts, float_op_is_bad, pascal_entry, CPoint, FloatOp, RealMatrix, MAX_DERIVATIVE,
    MODEL_TOL, NOW, NOZ,
};
use crate::nurbs::srf_basic::{Direction, Surface};
use crate::nurbs::srf_common::{cyl_cone, CylCone};
use crate::nurbs::srf_tool::p1::extract_iso_crv;

/// `NL_MAXDEG + 1`: size of upstream's fixed scratch rows.
const NP: usize = MAX_DEGREE + 1;

/// Scratch width used for degrees below it (performance only: every scratch index
/// the evaluators touch is at most the degree, so width `SMALL_NP` gives the same
/// values and errors as `NP` while zeroing far less stack per call).
const SMALL_NP: usize = 8;

/// [`SMALL_NP`] for degrees up to cubic.
const TINY_NP: usize = 4;

/// `NL_BASISFUNCTIONS` (`N = NP`, or `SMALL_NP` / `TINY_NP` for small degrees).
type BasisFunctions<const N: usize> = [[f64; N]; N];

/// `NL_DERIVATIVES` (`N = NP`, or `SMALL_NP` / `TINY_NP` for small degrees).
type Derivatives<const N: usize> = [[f64; N]; MAX_DERIVATIVE + 1];

fn invalid(operation: &'static str, reason: &'static str) -> SolidError {
    SolidError::InvalidInput { operation, reason }
}

/// Entry `[i][j]` of a square scratch table, `None` outside it.
fn cell_mut<const M: usize>(m: &mut [[f64; M]; M], i: usize, j: usize) -> Option<&mut f64> {
    m.get_mut(i)?.get_mut(j)
}

/// Derivatives of the non-vanishing (rational) bivariate basis functions at a
/// parameter (`NL_BASISDERIVATIVES` filled by `N_SrfBasisDerivs`).
#[derive(Debug, Clone, PartialEq)]
pub struct BasisDerivs {
    /// Knot span index of `u` (`usp`).
    pub usp: usize,
    /// Knot span index of `v` (`vsp`).
    pub vsp: usize,
    udr: usize,
    vdr: usize,
    p: usize,
    q: usize,
    /// `BD[k][l][i][j]`, flattened.
    values: Vec<f64>,
}

impl BasisDerivs {
    fn zeros(udr: usize, vdr: usize, p: usize, q: usize, op: &'static str) -> Result<Self> {
        let len = (udr + 1)
            .checked_mul(vdr + 1)
            .and_then(|x| x.checked_mul(p + 1))
            .and_then(|x| x.checked_mul(q + 1))
            .ok_or_else(|| oob(op))?;
        Ok(Self {
            usp: 0,
            vsp: 0,
            udr,
            vdr,
            p,
            q,
            values: vec![0.0; len],
        })
    }

    /// Highest derivative orders `(udr, vdr)` held.
    #[must_use]
    pub fn orders(&self) -> (usize, usize) {
        (self.udr, self.vdr)
    }

    /// Degrees `(p, q)` of the surface the derivatives belong to.
    #[must_use]
    pub fn degrees(&self) -> (usize, usize) {
        (self.p, self.q)
    }

    #[inline]
    fn index(&self, k: isize, l: isize, i: isize, j: isize) -> Option<usize> {
        self.index_u(
            usize::try_from(k).ok()?,
            usize::try_from(l).ok()?,
            usize::try_from(i).ok()?,
            usize::try_from(j).ok()?,
        )
    }

    /// [`BasisDerivs::index`] with unsigned indices (each bound checked).
    #[inline]
    fn index_u(&self, k: usize, l: usize, i: usize, j: usize) -> Option<usize> {
        if k > self.udr || l > self.vdr || i > self.p || j > self.q {
            return None;
        }
        Some(((k * (self.vdr + 1) + l) * (self.p + 1) + i) * (self.q + 1) + j)
    }

    /// The contiguous `j` row, with all outer indices and the whole slice checked.
    #[inline]
    fn row_u(&self, k: usize, l: usize, i: usize, op: &'static str) -> Result<&[f64]> {
        let start = self.index_u(k, l, i, 0).ok_or_else(|| oob(op))?;
        let len = self.q.checked_add(1).ok_or_else(|| oob(op))?;
        let end = start.checked_add(len).ok_or_else(|| oob(op))?;
        self.values.get(start..end).ok_or_else(|| oob(op))
    }

    /// `BD[k][l][i][j] = v` with unsigned indices; `oob` outside `[udr][vdr][p][q]`.
    #[inline]
    fn put_u(
        &mut self,
        k: usize,
        l: usize,
        i: usize,
        j: usize,
        v: f64,
        op: &'static str,
    ) -> Result<()> {
        let x = self.index_u(k, l, i, j).ok_or_else(|| oob(op))?;
        *self.values.get_mut(x).ok_or_else(|| oob(op))? = v;
        Ok(())
    }

    /// `BD[k][l][i][j]`: the `(k, l)`-th derivative of the basis function
    /// `N[usp-p+i][vsp-q+j]`; `None` outside `[udr][vdr][p][q]`.
    #[must_use]
    pub fn get(&self, k: usize, l: usize, i: usize, j: usize) -> Option<f64> {
        let idx = self.index(
            isize::try_from(k).ok()?,
            isize::try_from(l).ok()?,
            isize::try_from(i).ok()?,
            isize::try_from(j).ok()?,
        )?;
        self.values.get(idx).copied()
    }
}

/// Smallest / largest weight and position vector magnitude of the control points
/// ([`Surface::min_max_weight_pos_vectors`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinMaxWeightPos {
    /// Smallest weight ([`NOW`] for a non-rational surface).
    pub wmin: f64,
    /// Largest weight ([`NOW`] for a non-rational surface).
    pub wmax: f64,
    /// Smallest magnitude of a Euclidean control point.
    pub pmin: f64,
    /// Largest magnitude of a Euclidean control point.
    pub pmax: f64,
}

/// Point, unit partial derivatives and unit normal of a surface
/// ([`Surface::eval_pt_deriv_normal_fast`] and the pole evaluators).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtDerivNormal {
    /// Point on the surface.
    pub p: Vec3,
    /// Unit u derivative (zero at a pole in u).
    pub su: Vec3,
    /// Unit v derivative (zero at a pole in v).
    pub sv: Vec3,
    /// Unit normal.
    pub n: Vec3,
}

/// Surface type found by [`Surface::surface_type`] (`NL_NPLANE` .. `NL_NFREEFORM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    /// `NL_NPLANE` (3): plane (bilinear surface).
    Plane,
    /// `NL_NSPHERE` (4): sphere or spherical patch.
    Sphere,
    /// `NL_NTORUS` (5): torus or toroidal patch.
    Torus,
    /// `NL_NCYLINDER` (6): right circular cylinder or patch.
    Cylinder,
    /// `NL_NCONE` (7): right circular cone or patch.
    Cone,
    /// `NL_NREVOLUTION` (8): surface of revolution.
    Revolution,
    /// `NL_NRULED` (9): ruled surface.
    Ruled,
    /// `NL_NEXTRUSION` (10): extruded surface.
    Extrusion,
    /// `NL_NFREEFORM` (11): free-form surface.
    FreeForm,
}

impl SurfaceKind {
    /// The upstream flag value.
    #[must_use]
    pub const fn flag(self) -> i32 {
        match self {
            SurfaceKind::Plane => 3,
            SurfaceKind::Sphere => 4,
            SurfaceKind::Torus => 5,
            SurfaceKind::Cylinder => 6,
            SurfaceKind::Cone => 7,
            SurfaceKind::Revolution => 8,
            SurfaceKind::Ruled => 9,
            SurfaceKind::Extrusion => 10,
            SurfaceKind::FreeForm => 11,
        }
    }
}

/// The outputs of [`Surface::surface_type`] (`N_SrfType`); the meaning of each field
/// depends on [`SurfaceType::kind`] (see the upstream description).
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceType {
    /// Surface type (`stp`).
    pub kind: SurfaceKind,
    /// Direction of construction (`dir`); `None` is `NL_NONE`.
    pub dir: Option<Direction>,
    /// Generator curve (`curA`): profile, first boundary or extruded curve.
    pub cur_a: Option<Curve>,
    /// Second boundary of a ruled surface (`curB`).
    pub cur_b: Option<Curve>,
    /// `AA`: plane corner, axis point, bottom circle centre, extrusion vector.
    pub aa: Vec3,
    /// `BB`: plane corner or axis direction.
    pub bb: Vec3,
    /// `CC`: plane corner or profile plane normal.
    pub cc: Vec3,
    /// `DD`: plane corner or generating circle centre.
    pub dd: Vec3,
    /// `XX`: local x axis.
    pub xx: Vec3,
    /// `YY`: local y axis.
    pub yy: Vec3,
    /// `r1`: radius or extrusion distance.
    pub r1: f64,
    /// `r2`: second radius or distance.
    pub r2: f64,
    /// `ac`: sweep angle of the generating circle.
    pub ac: f64,
    /// `al`: angle of rotation.
    pub al: f64,
    /// `h`: cone / cylinder height.
    pub h: f64,
}

/// Inputs of [`cone_elliptic_ends`] (`N_ConeEllipticEnds`); angles in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EllipticCone {
    /// Front radius before the tilt.
    pub radius_front: f64,
    /// Rear radius before the tilt.
    pub radius_rear: f64,
    /// Distance in z between the end planes' origins.
    pub height: f64,
    /// Front tilt about y.
    pub tilt_y_front: f64,
    /// Rear tilt about y.
    pub tilt_y_rear: f64,
    /// Front tilt about x.
    pub tilt_x_front: f64,
    /// Rear tilt about x.
    pub tilt_x_rear: f64,
}

// ---- basis functions --------------------------------------------------------------

/// `N_ComputeBasisFunctions`: the triangular table `ndu` of algorithm A2.3.
#[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
fn compute_basis_functions<const N: usize>(
    p: isize,
    u: f64,
    uk: &[f64],
    ku: isize,
    ndu: &mut BasisFunctions<N>,
    op: &'static str,
) -> Result<()> {
    // `left` and `right` are the rows of `a`, as upstream.
    let mut a = [[0.0_f64; N]; 2];
    set2(ndu, 0, 0, 1.0, op)?;
    for j in 1..p + 1 {
        set2(&mut a, 0, j, u - get(uk, ku + 1 - j, op)?, op)?;
        set2(&mut a, 1, j, get(uk, ku + j, op)? - u, op)?;
        let mut saved = 0.0;
        for r in 0..j {
            let right = get2(&a, 1, r + 1, op)?;
            let left = get2(&a, 0, j - r, op)?;
            set2(ndu, j, r, right + left, op)?;
            let temp = get2(ndu, r, j - 1, op)? / get2(ndu, j, r, op)?;
            set2(ndu, r, j, saved + right * temp, op)?;
            saved = left * temp;
        }
        set2(ndu, j, j, saved, op)?;
    }
    Ok(())
}

/// `N_ComputeDerivatives` / `N_ComputeDerivativesImpl`: rows `1..=mder` of `du`
/// (before the factor `p!/(p-k)!`). `N_SwapRealPointers` is the swap of `s1`, `s2`.
#[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
fn compute_derivatives<const N: usize>(
    p: isize,
    mder: isize,
    ndu: &BasisFunctions<N>,
    du: &mut Derivatives<N>,
    op: &'static str,
) -> Result<()> {
    let mut a = [[0.0_f64; N]; 2];
    for r in 0..p + 1 {
        let mut s1: isize = 0;
        let mut s2: isize = 1;
        set2(&mut a, s1, 0, 1.0, op)?;
        for k in 1..mder + 1 {
            let mut dd = 0.0;
            let rk = r - k;
            let pk = p - k;
            if r >= k {
                let v = get2(&a, s1, 0, op)? / get2(ndu, pk + 1, rk, op)?;
                set2(&mut a, s2, 0, v, op)?;
                dd = v * get2(ndu, rk, pk, op)?;
            }
            let j1 = if rk >= -1 { 1 } else { -rk };
            let j2 = if r - 1 <= pk { k - 1 } else { p - r };
            for j in j1..j2 + 1 {
                let v = (get2(&a, s1, j, op)? - get2(&a, s1, j - 1, op)?)
                    / get2(ndu, pk + 1, rk + j, op)?;
                set2(&mut a, s2, j, v, op)?;
                dd += v * get2(ndu, rk + j, pk, op)?;
            }
            if r <= pk {
                let v = -get2(&a, s1, k - 1, op)? / get2(ndu, pk + 1, r, op)?;
                set2(&mut a, s2, k, v, op)?;
                dd += v * get2(ndu, r, pk, op)?;
            }
            set2(du, k, r, dd, op)?;
            core::mem::swap(&mut s1, &mut s2);
        }
    }
    Ok(())
}

/// One direction of `N_SrfBasisDerivs`: the span index, with `D[k][j]`, `k <= dr`,
/// written into `du` (the caller's zeroed array, as upstream's stack array; filled in
/// place so the array is not moved out through the `Result`).
#[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
#[allow(clippy::too_many_arguments)] // upstream's arguments plus the output array
fn univariate_derivs<const N: usize>(
    uk: &[f64],
    m: usize,
    pu: usize,
    t: f64,
    side: Side,
    dr: isize,
    du: &mut Derivatives<N>,
    op: &'static str,
) -> Result<isize> {
    let p = sx(pu, op)?;
    let mut ndu: BasisFunctions<N> = [[0.0; N]; N];

    // Compute all non-vanishing basis functions.
    let knt = KnotVector::new(uk, m)?;
    let ku = sx(knt.find_span(pu, t, side)?, op)?;

    // Get maximum derivative index and set zero derivatives.
    let mder = p.min(dr);
    for k in p + 1..dr + 1 {
        for j in 0..p + 1 {
            set2(du, k, j, 0.0, op)?;
        }
    }

    // Compute the basis functions.
    compute_basis_functions(p, t, uk, ku, &mut ndu, op)?;

    // Load the basis functions.
    for j in 0..p + 1 {
        set2(du, 0, j, get2(&ndu, j, p, op)?, op)?;
    }

    // Compute derivatives.
    compute_derivatives(p, mder, &ndu, du, op)?;

    // Multiply through by the correct factors.
    let mut r = p;
    for k in 1..mder + 1 {
        for j in 0..p + 1 {
            let v = get2(du, k, j, op)? * r as f64;
            set2(du, k, j, v, op)?;
        }
        r *= p - k;
    }
    Ok(ku)
}

/// [`univariate_derivs`] for `dr = 1`: the span index, with the basis functions and
/// their first derivatives (`du[0]`, `du[1]`) for degree `pu < N`. The same arithmetic in
/// the same order (`compute_basis_functions`, then `compute_derivatives` at `k = 1`, then
/// the factor `p`), on unchecked-free stack tables: every index is below `N`, which the
/// caller has checked.
fn univariate_first<const N: usize>(
    uk: &[f64],
    m: usize,
    pu: usize,
    t: f64,
    side: Side,
    op: &'static str,
) -> Result<(usize, [f64; N], [f64; N])> {
    if pu >= N {
        return Err(oob(op));
    }
    let knt = KnotVector::new(uk, m)?;
    let ku = knt.find_span(pu, t, side)?;
    let at = |i: usize| uk.get(i).copied();
    let g = |a: &[[f64; N]; N], i: usize, j: usize| {
        a.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
    };
    let put = |a: &mut [[f64; N]; N], i: usize, j: usize, v: f64| {
        if let Some(x) = a.get_mut(i).and_then(|r| r.get_mut(j)) {
            *x = v;
        }
    };
    // `left[j] = t - U[ku + 1 - j]`, `right[j] = U[ku + j] - t` (rows of upstream's `a`).
    let mut left = [0.0_f64; N];
    let mut right = [0.0_f64; N];
    let mut ndu = [[0.0_f64; N]; N];
    put(&mut ndu, 0, 0, 1.0);
    for j in 1..=pu {
        let lo = (ku + 1)
            .checked_sub(j)
            .and_then(at)
            .ok_or_else(|| oob(op))?;
        let hi = at(ku + j).ok_or_else(|| oob(op))?;
        if let (Some(l), Some(r)) = (left.get_mut(j), right.get_mut(j)) {
            *l = t - lo;
            *r = hi - t;
        }
        let mut saved = 0.0;
        for r in 0..j {
            let rt = right.get(r + 1).copied().unwrap_or(0.0);
            let lf = left.get(j - r).copied().unwrap_or(0.0);
            put(&mut ndu, j, r, rt + lf);
            let temp = g(&ndu, r, j - 1) / g(&ndu, j, r);
            put(&mut ndu, r, j, saved + rt * temp);
            saved = lf * temp;
        }
        put(&mut ndu, j, j, saved);
    }
    let mut d0 = [0.0_f64; N];
    let mut d1 = [0.0_f64; N];
    for (j, x) in d0.iter_mut().enumerate().take(pu + 1) {
        *x = g(&ndu, j, pu);
    }
    if pu >= 1 {
        // `compute_derivatives` at k = 1: `a[s1][0] = 1`, so `a[s2][0] = 1 / ndu[p][r-1]`
        // and `a[s2][1] = -1 / ndu[p][r]`; then the factor `p`.
        let p = pu - 1;
        for (r, x) in d1.iter_mut().enumerate().take(pu + 1) {
            let mut d = 0.0;
            if r >= 1 {
                let v = 1.0 / g(&ndu, p + 1, r - 1);
                d = v * g(&ndu, r - 1, p);
            }
            if r <= p {
                let v = -1.0 / g(&ndu, p + 1, r);
                d += v * g(&ndu, r, p);
            }
            *x = d * pu as f64;
        }
    }
    Ok((ku, d0, d1))
}

// ---- specialised point helpers ----------------------------------------------------

/// `NI_VectorBlendCPt_specialized`: `b += alpha * a` with `z` / `w` dropped by the
/// surface-wide flags.
fn blend_spec(noz: bool, now: bool, alpha: f64, a: CPoint, b: &mut CPoint) {
    b.x += alpha * a.x;
    b.y += alpha * a.y;
    if !noz {
        b.z += alpha * a.z;
    } else {
        b.z = NOZ;
    }
    if !now {
        b.w += alpha * a.w;
    } else {
        b.w = NOW;
    }
}

/// `NI_CPtToPtEuclid_specialized`.
fn to_euclid_spec(noz: bool, now: bool, pw: CPoint) -> Vec3 {
    if !now {
        let reciprocal_w = 1.0 / pw.w;
        let z = if !noz { pw.z * reciprocal_w } else { 0.0 };
        Vec3::new(pw.x * reciprocal_w, pw.y * reciprocal_w, z)
    } else {
        let z = if !noz { pw.z } else { 0.0 };
        Vec3::new(pw.x, pw.y, z)
    }
}

/// `N_ComputeDerivativeMatrix` / `N_ComputeDerivativeMatrixSpec` into the caller's
/// matrix `sd` (rows `0..=udr` of at least `vdr + 1` points, zeroed); `N` is the
/// scratch width (above both degrees). `alpha_row(k, l, i)` supplies the checked
/// scalar accessor for the basis derivative `BD[k][l][i][j]`.
#[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
#[allow(clippy::too_many_arguments)] // upstream's arguments plus the output matrix
fn derivative_matrix<const N: usize, R: AsMut<[Vec3]>, A: Fn(usize) -> Result<f64>>(
    sur: &Surface,
    no_w: bool,
    no_z: bool,
    upper: bool,
    p: isize,
    q: isize,
    udr: isize,
    vdr: isize,
    l2: isize,
    l3: isize,
    alpha_row: impl Fn(usize, usize, usize) -> Result<A>,
    sd: &mut [R],
    op: &'static str,
) -> Result<()> {
    let mut pwl = [[Vec3::ZERO; N]; N];
    for i in 0..p + 1 {
        for j in 0..q + 1 {
            let pw = sur.cpt(ux(i + l2, op)?, ux(j + l3, op)?)?;
            set2(&mut pwl, i, j, to_euclid_spec(no_z, no_w, pw), op)?;
        }
    }

    // (`p`, `q` are below `N`, so the scratch rows hold every `i <= p`, `j <= q`)
    let (pu, qu) = (ux(p, op)?, ux(q, op)?);
    for k in 0..udr + 1 {
        let lmax = if upper { udr - k } else { vdr };
        let ku = ux(k, op)?;
        for l in 0..lmax + 1 {
            let lu = ux(l, op)?;
            let mut s = Vec3::ZERO;
            for i in 0..pu + 1 {
                let row = pwl.get(i).ok_or_else(|| oob(op))?;
                let alpha = alpha_row(ku, lu, i)?;
                for j in 0..qu + 1 {
                    let pt = row.get(j).copied().ok_or_else(|| oob(op))?;
                    let alpha = alpha(j)?;
                    s.x += alpha * pt.x; // upstream: alpha * P.x + sd.x
                    s.y += alpha * pt.y; // upstream: alpha * P.y + sd.y
                    if !no_z {
                        s.z += alpha * pt.z; // upstream: alpha * P.z + sd.z
                    }
                }
            }
            let row = sd.get_mut(ux(k, op)?).ok_or_else(|| oob(op))?;
            set(row.as_mut(), l, s, op)?;
        }
    }
    Ok(())
}

/// A surface's control points divided by their weights, with the weights, row by row
/// ([`Surface::euclid_net`]).
#[derive(Debug, Clone, PartialEq)]
pub struct EuclidNet {
    points: Vec<Vec<Vec3>>,
    weights: Vec<Vec<f64>>,
}

impl Surface {
    /// Derivatives of all non-vanishing (rational) basis functions at `(u, v)`
    /// (`N_SrfBasisDerivs`): `BD[k][l][i][j]` is the `(k, l)`-th derivative of
    /// `R[usp-p+i][vsp-q+j]`, for `k <= udr`, `l <= vdr` (`l <= udr - k` when `upper`
    /// and `udr == vdr`). Parameters outside the domain evaluate the end spans.
    ///
    /// Errors with [`NlibCode::MaxDerivative`] when `udr` or `vdr` exceeds
    /// `NL_MAXDER`, with [`NlibCode::Convergence`] when the rational denominator is
    /// not invertible, and with the errors of
    /// [`KnotVector::find_span`].
    pub fn basis_derivs(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        udr: usize,
        vdr: usize,
    ) -> Result<BasisDerivs> {
        const OP: &str = "nurbs::srf_nurb::Surface::basis_derivs";
        let (pu, qu) = self.degrees();

        // Check order of derivatives.
        if udr > MAX_DERIVATIVE || vdr > MAX_DERIVATIVE {
            return Err(SolidError::nlib(OP, NlibCode::MaxDerivative));
        }
        if pu > MAX_DEGREE || qu > MAX_DEGREE {
            return Err(invalid(
                OP,
                "degree above NL_MAXDEG (upstream scratch array size)",
            ));
        }
        if pu < TINY_NP && qu < TINY_NP {
            self.basis_derivs_n::<TINY_NP>(u, v, ufl, vfl, upper, udr, vdr)
        } else if pu < SMALL_NP && qu < SMALL_NP {
            self.basis_derivs_n::<SMALL_NP>(u, v, ufl, vfl, upper, udr, vdr)
        } else {
            self.basis_derivs_n::<NP>(u, v, ufl, vfl, upper, udr, vdr)
        }
    }

    /// The rational branch of [`Surface::basis_derivs_n`] (`N_SrfBasisDerivs`): from
    /// the spans `ku`, `kv` and the univariate derivative tables `du`, `dv`, gives
    /// `put(k, l, n, m, BD[k][l][n][m])` for every derivative it computes, one basis
    /// function `(n, m)` at a time, in upstream's order.
    #[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
    #[allow(clippy::too_many_arguments)] // the spans, both tables, the orders and the sink
    fn rational_basis_derivs<const N: usize>(
        &self,
        ku: isize,
        kv: isize,
        du: &Derivatives<N>,
        dv: &Derivatives<N>,
        upper_half: bool,
        udr: usize,
        vdr: usize,
        mut put: impl FnMut(usize, usize, usize, usize, f64) -> Result<()>,
    ) -> Result<()> {
        const OP: &str = "nurbs::srf_nurb::Surface::basis_derivs";
        const D: usize = MAX_DERIVATIVE + 1;
        let (pu, qu) = self.degrees();
        let (p, q) = (sx(pu, OP)?, sx(qu, OP)?);
        // (every scratch table below is sized by `N` > both degrees and
        // `MAX_DERIVATIVE`; a call outside them is refused before any work, where the
        // per-access checks this replaces would have refused it on first use)
        if pu >= N || qu >= N || udr > MAX_DERIVATIVE || vdr > MAX_DERIVATIVE {
            return Err(oob(OP));
        }
        let at = |a: &[[f64; D]], i: usize, j: usize| -> Result<f64> {
            a.get(i)
                .and_then(|r| r.get(j))
                .copied()
                .ok_or_else(|| oob(OP))
        };
        let at_n = |a: &Derivatives<N>, i: usize, j: usize| -> Result<f64> {
            a.get(i)
                .and_then(|r| r.get(j))
                .copied()
                .ok_or_else(|| oob(OP))
        };

        // The weights of the (p+1) x (q+1) control points of the span, read once, in
        // the order the denominator loop below first reads them.
        let s1 = ku - p;
        let s2 = kv - q;
        let mut w = [[0.0_f64; N]; N];
        for (i, row) in (0..=p).zip(w.iter_mut()) {
            for (j, wij) in (0..=q).zip(row.iter_mut()) {
                *wij = self.cpt(ux(s1 + i, OP)?, ux(s2 + j, OP)?)?.w;
            }
        }

        // Get derivatives of denominator.
        let mut tu = [0.0_f64; N];
        let mut d = [[0.0_f64; D]; D];
        for l in 0..vdr + 1 {
            let dv_l = dv.get(l).ok_or_else(|| oob(OP))?;
            for (t, w_row) in tu.iter_mut().zip(&w).take(pu + 1) {
                *t = 0.0;
                for (wij, dvlj) in w_row.iter().zip(dv_l).take(qu + 1) {
                    *t += wij * dvlj;
                }
            }
            for (du_k, d_k) in du.iter().zip(d.iter_mut()).take(udr + 1) {
                let dkl = d_k.get_mut(l).ok_or_else(|| oob(OP))?;
                *dkl = 0.0;
                for (t, duki) in tu.iter().zip(du_k).take(pu + 1) {
                    *dkl += t * duki;
                }
            }
        }

        // Compute derivatives of rational basis.
        let d00 = at(&d, 0, 0)?;
        if float_op_is_bad(1.0, d00, FloatOp::Division) {
            return Err(SolidError::nlib(OP, NlibCode::Convergence));
        }
        let inv_w00 = 1.0 / d00;

        // Upstream's static `NL_PascalTri` (the rows `0..=NL_MAXDER` this loop reads).
        // Every product of the recurrence is `tri * d * BD`, evaluated left to right, so
        // its `tri * d` factor depends on the orders only: computed once here instead of
        // once per basis function, with the same rounding.
        let tri = |a: usize, b: usize| -> Result<f64> { pascal_entry(a, b).ok_or_else(|| oob(OP)) };
        let mut tri_k = [[0.0_f64; D]; D]; // [k][i] = tri(k, i)
        let mut c0 = [[0.0_f64; D]; D]; // [l][j] = tri(l, j) * d[0][j]
        let mut c1 = [[0.0_f64; D]; D]; // [k][i] = tri(k, i) * d[i][0]
        let mut c2 = [[[0.0_f64; D]; D]; D]; // [i][l][j] = tri(l, j) * d[i][j]
        let top = udr.max(vdr);
        for a in 0..top + 1 {
            for b in 0..a + 1 {
                let t = tri(a, b)?;
                *cell_mut(&mut tri_k, a, b).ok_or_else(|| oob(OP))? = t;
                *cell_mut(&mut c0, a, b).ok_or_else(|| oob(OP))? = t * at(&d, 0, b)?;
                *cell_mut(&mut c1, a, b).ok_or_else(|| oob(OP))? = t * at(&d, b, 0)?;
                for (i, c2_i) in c2.iter_mut().enumerate().take(udr + 1) {
                    let x = c2_i
                        .get_mut(a)
                        .and_then(|r| r.get_mut(b))
                        .ok_or_else(|| oob(OP))?;
                    *x = t * at(&d, i, b)?;
                }
            }
        }

        // One basis function at a time: its derivatives only read its own lower
        // orders, kept in `own`. Every entry read was written earlier in this loop
        // order for the same function (a lower `k`, or the same `k` and a lower `l`;
        // with the upper half, a lower `k + l`), so `own` is cleared once, not per
        // function.
        let mut own = [[0.0_f64; D]; D];
        for (n, w_row) in w.iter().enumerate().take(pu + 1) {
            for (m, &wnm) in w_row.iter().enumerate().take(qu + 1) {
                for k in 0..udr + 1 {
                    let lmax = if upper_half { udr - k } else { vdr };
                    for l in 0..lmax + 1 {
                        let mut v1 = wnm * at_n(du, k, n)? * at_n(dv, l, m)?;
                        for j in 1..l + 1 {
                            v1 -= at(&c0, l, j)? * at(&own, k, l - j)?;
                        }
                        for i in 1..k + 1 {
                            v1 -= at(&c1, k, i)? * at(&own, k - i, l)?;
                            let c2_i = c2.get(i).ok_or_else(|| oob(OP))?;
                            let mut v2 = 0.0;
                            for j in 1..l + 1 {
                                v2 += at(c2_i, l, j)? * at(&own, k - i, l - j)?;
                            }
                            v1 -= at(&tri_k, k, i)? * v2;
                        }
                        let value = v1 * inv_w00;
                        *own.get_mut(k)
                            .and_then(|r| r.get_mut(l))
                            .ok_or_else(|| oob(OP))? = value;
                        put(k, l, n, m, value)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// [`Surface::basis_derivs`] after its argument checks, with scratch width `N`
    /// (above both degrees).
    #[allow(clippy::range_plus_one)] // exclusive ranges: `RangeInclusive` is measurably slower here
    fn basis_derivs_n<const N: usize>(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        udr: usize,
        vdr: usize,
    ) -> Result<BasisDerivs> {
        const OP: &str = "nurbs::srf_nurb::Surface::basis_derivs";
        let (pu, qu) = self.degrees();
        let (_, _, s1u, s2u) = self.array_sizes();
        let (udri, vdri) = (sx(udr, OP)?, sx(vdr, OP)?);

        // u- then v-direction.
        let mut du: Derivatives<N> = [[0.0; N]; MAX_DERIVATIVE + 1];
        let mut dv: Derivatives<N> = [[0.0; N]; MAX_DERIVATIVE + 1];
        let ku = univariate_derivs::<N>(self.u_knots(), s1u, pu, u, ufl, udri, &mut du, OP)?;
        let kv = univariate_derivs::<N>(self.v_knots(), s2u, qu, v, vfl, vdri, &mut dv, OP)?;

        let mut bd = BasisDerivs::zeros(udr, vdr, pu, qu, OP)?;
        bd.usp = ux(ku, OP)?;
        bd.vsp = ux(kv, OP)?;
        let upper_half = upper && udr == vdr;

        if self.is_rational() {
            let (q1, pq1) = (qu + 1, (pu + 1) * (qu + 1));
            let sk = (vdr + 1) * pq1;
            let values = &mut bd.values;
            self.rational_basis_derivs::<N>(
                ku,
                kv,
                &du,
                &dv,
                upper_half,
                udr,
                vdr,
                |k, l, n, m, x| {
                    *values
                        .get_mut(k * sk + l * pq1 + n * q1 + m)
                        .ok_or_else(|| oob(OP))? = x;
                    Ok(())
                },
            )?;
        } else {
            // Compute derivative matrix.
            let at2u = |a: &[[f64; N]], i: usize, j: usize| -> Result<f64> {
                a.get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .ok_or_else(|| oob(OP))
            };
            for k in 0..udr + 1 {
                let lmax = if upper_half { udr - k } else { vdr };
                for l in 0..lmax + 1 {
                    for i in 0..pu + 1 {
                        for j in 0..qu + 1 {
                            let x = at2u(&du, k, i)? * at2u(&dv, l, j)?;
                            bd.put_u(k, l, i, j, x, OP)?;
                        }
                    }
                }
            }
        }
        Ok(bd)
    }

    /// Point `S(u, v)` (`N_SrfEvalPt`): curve evaluations only, with the dimension and
    /// rationality taken from `Pw[0][0]` for every control point.
    ///
    /// Errors with the errors of [`KnotVector::basis_eval`].
    pub fn eval(&self, u: f64, v: f64, ufl: Side, vfl: Side) -> Result<Vec3> {
        const OP: &str = "nurbs::srf_nurb::Surface::eval";
        let p00 = self.cpt(0, 0)?;
        let pnoz = p00.z == NOZ;
        let pnow = p00.w == NOW;
        let (pu, qu) = self.degrees();
        let (knu, knv) = self.knot_vectors()?;

        // Compute non-vanishing B-splines (on the stack, as upstream's `Nu`/`Nv`).
        let mut nu = [0.0_f64; MAX_DEGREE + 2];
        let mut nv = [0.0_f64; MAX_DEGREE + 2];
        let u_span = knu.basis_eval_into(pu, u, ufl, &mut nu)?;
        let v_span = knv.basis_eval_into(qu, v, vfl, &mut nv)?;

        // Compute the point on the surface.
        let (p, q) = (sx(pu, OP)?, sx(qu, OP)?);
        let mut sw = CPoint::ZERO;
        let tmp1 = sx(u_span, OP)? - p;
        let tmp2 = sx(v_span, OP)? - q;
        for i in 0..=p {
            let mut tw = CPoint::ZERO;
            let row = self.row(ux(tmp1 + i, OP)?)?;
            for j in 0..=q {
                blend_spec(
                    pnoz,
                    pnow,
                    get(&nv, j, OP)?,
                    get(row, tmp2 + j, OP)?,
                    &mut tw,
                );
            }
            blend_spec(pnoz, pnow, get(&nu, i, OP)?, tw, &mut sw);
        }
        Ok(to_euclid_spec(pnoz, pnow, sw))
    }

    /// Derivatives `SD[k][l] = d^(k+l) S / du^k dv^l` for `k <= udr`, `l <= vdr`
    /// (`N_SrfDerivs`); with `upper` and `udr == vdr` only `k + l <= udr` is computed
    /// (the rest stays zero). `SD[0][0]` is the point.
    ///
    /// Errors with the errors of [`Surface::basis_derivs`].
    pub fn derivs(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        udr: usize,
        vdr: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        let (pu, qu) = self.degrees();
        // first derivatives (the common order): the stack evaluation, the same
        // products in the same order, so the same bits
        // (`first_derivs_into_matches_derivs_bit_for_bit`)
        if udr == 1 && vdr == 1 && pu <= MAX_DEGREE && qu <= MAX_DEGREE {
            let mut sd = [[Vec3::ZERO; 2]; 2];
            self.first_derivs_into(u, v, ufl, vfl, upper, &mut sd)?;
            let [d0, d1] = sd;
            return Ok(vec![d0.to_vec(), d1.to_vec()]);
        }
        self.derivs_general(u, v, ufl, vfl, upper, udr, vdr)
    }

    /// [`Surface::derivs`] through the heap [`BasisDerivs`] table, for every order.
    fn derivs_general(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        udr: usize,
        vdr: usize,
    ) -> Result<Vec<Vec<Vec3>>> {
        const OP: &str = "nurbs::srf_nurb::Surface::derivs";
        let (pu, qu) = self.degrees();

        // Compute basis function derivatives.
        let bd = self.basis_derivs(u, v, ufl, vfl, upper, udr, vdr)?;

        // Compute derivatives.
        let (p, q) = (sx(pu, OP)?, sx(qu, OP)?);
        let l2 = sx(bd.usp, OP)? - p;
        let l3 = sx(bd.vsp, OP)? - q;
        let p00 = self.cpt(0, 0)?;
        let no_w = p00.w == NOW;
        let no_z = p00.z == NOZ;
        let mut sd = vec![vec![Vec3::ZERO; vdr + 1]; udr + 1];
        let alpha = |k, l, i| {
            let row = bd.row_u(k, l, i, OP)?;
            Ok(move |j| row.get(j).copied().ok_or_else(|| oob(OP)))
        };
        let (upper, udr, vdr) = (upper && udr == vdr, sx(udr, OP)?, sx(vdr, OP)?);
        if pu < TINY_NP && qu < TINY_NP {
            derivative_matrix::<TINY_NP, _, _>(
                self, no_w, no_z, upper, p, q, udr, vdr, l2, l3, alpha, &mut sd, OP,
            )?;
        } else if pu < SMALL_NP && qu < SMALL_NP {
            derivative_matrix::<SMALL_NP, _, _>(
                self, no_w, no_z, upper, p, q, udr, vdr, l2, l3, alpha, &mut sd, OP,
            )?;
        } else {
            derivative_matrix::<NP, _, _>(
                self, no_w, no_z, upper, p, q, udr, vdr, l2, l3, alpha, &mut sd, OP,
            )?;
        }
        Ok(sd)
    }

    /// [`Surface::derivs`] with `udr = vdr = 1` into the caller's 2x2 block `sd`
    /// (`SD[k][l]`, every entry written; the entry `[1][1]` is zero with `upper`), with
    /// no heap allocation for a polynomial (non-rational) surface: the basis
    /// derivatives `Du[k][i] * Dv[l][j]` are read straight from the two univariate
    /// tables instead of being stored in a [`BasisDerivs`] first (the same products,
    /// so the same bits). A rational surface goes through [`Surface::basis_derivs`].
    ///
    /// Errors as [`Surface::derivs`].
    pub fn first_derivs_into(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        sd: &mut [[Vec3; 2]; 2],
    ) -> Result<()> {
        self.first_derivs_net_into(None, u, v, ufl, vfl, upper, sd)
    }

    /// The Euclidean control points and weights of the net, as
    /// [`Surface::first_derivs_into`] reads them: made once for a surface that is evaluated
    /// many times ([`Surface::first_derivs_net_into`]).
    ///
    /// Errors when the net is malformed.
    pub fn euclid_net(&self) -> Result<EuclidNet> {
        let p00 = self.cpt(0, 0)?;
        let (no_w, no_z) = (p00.w == NOW, p00.z == NOZ);
        let (n, _, _, _) = self.array_sizes();
        let mut points = Vec::with_capacity(n + 1);
        let mut weights = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let row = self.row(i)?;
            points.push(row.iter().map(|c| to_euclid_spec(no_z, no_w, *c)).collect());
            weights.push(row.iter().map(|c| c.w).collect());
        }
        Ok(EuclidNet { points, weights })
    }

    /// [`Surface::first_derivs_into`] reading the control points from `euclid`, this
    /// surface's [`Surface::euclid_net`] when given: the same bits, without dividing every
    /// control point of the span by its weight on each call.
    ///
    /// Errors as [`Surface::first_derivs_into`].
    #[allow(clippy::too_many_arguments)] // the evaluation arguments plus the net
    pub fn first_derivs_net_into(
        &self,
        euclid: Option<&EuclidNet>,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        sd: &mut [[Vec3; 2]; 2],
    ) -> Result<()> {
        const OP: &str = "nurbs::srf_nurb::Surface::derivs";
        const BD_OP: &str = "nurbs::srf_nurb::Surface::basis_derivs";
        let (pu, qu) = self.degrees();
        *sd = [[Vec3::ZERO; 2]; 2];
        if pu > MAX_DEGREE || qu > MAX_DEGREE {
            // (the degree error of `basis_derivs`)
            let full = self.derivs_general(u, v, ufl, vfl, upper, 1, 1)?;
            for (out, row) in sd.iter_mut().zip(&full) {
                for (o, x) in out.iter_mut().zip(row) {
                    *o = *x;
                }
            }
            return Ok(());
        }
        if pu < TINY_NP && qu < TINY_NP {
            self.first_derivs_span::<TINY_NP>(u, v, ufl, vfl, upper, sd, euclid, OP, BD_OP)
        } else if pu < SMALL_NP && qu < SMALL_NP {
            self.first_derivs_span::<SMALL_NP>(u, v, ufl, vfl, upper, sd, euclid, OP, BD_OP)
        } else {
            self.first_derivs_span::<NP>(u, v, ufl, vfl, upper, sd, euclid, OP, BD_OP)
        }
    }

    /// [`Surface::first_derivs_into`] for degrees below `N`, rational or not: the
    /// arithmetic of [`Surface::basis_derivs_n`] (its rational recurrence for orders
    /// `k, l <= 1`, each product and sum in its order) and of `derivative_matrix`, fused
    /// into one pass over the span's control points, so the same bits
    /// (`first_derivs_into_matches_derivs_bit_for_bit`) without the per-entry checked
    /// tables.
    #[allow(clippy::too_many_arguments)] // the evaluation arguments plus both names
    fn first_derivs_span<const N: usize>(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
        upper: bool,
        sd: &mut [[Vec3; 2]; 2],
        euclid: Option<&EuclidNet>,
        op: &'static str,
        bd_op: &'static str,
    ) -> Result<()> {
        let (pu, qu) = self.degrees();
        if pu >= N || qu >= N {
            return Err(oob(op));
        }
        let (_, _, s1u, s2u) = self.array_sizes();
        let (ku, du0, du1) = univariate_first::<N>(self.u_knots(), s1u, pu, u, ufl, bd_op)?;
        let (kv, dv0, dv1) = univariate_first::<N>(self.v_knots(), s2u, qu, v, vfl, bd_op)?;
        let p00 = self.cpt(0, 0)?;
        let no_w = p00.w == NOW;
        let no_z = p00.z == NOZ;
        let first_u = ku.checked_sub(pu).ok_or_else(|| oob(op))?;
        let first_v = kv.checked_sub(qu).ok_or_else(|| oob(op))?;

        // The span's (p+1) x (q+1) control points: weights and Euclidean points.
        let mut w = [[0.0_f64; N]; N];
        let mut pe = [[Vec3::ZERO; N]; N];
        for (i, (w_row, p_row)) in w.iter_mut().zip(pe.iter_mut()).take(pu + 1).enumerate() {
            if let Some(net) = euclid {
                // (made by `to_euclid_spec` on these points: the same values)
                let (points, weights) = net
                    .points
                    .get(first_u + i)
                    .zip(net.weights.get(first_u + i))
                    .and_then(|(p, w)| {
                        Some((
                            p.get(first_v..=first_v + qu)?,
                            w.get(first_v..=first_v + qu)?,
                        ))
                    })
                    .ok_or_else(|| oob(op))?;
                for ((wij, pij), (p, wt)) in w_row
                    .iter_mut()
                    .zip(p_row.iter_mut())
                    .zip(points.iter().zip(weights))
                {
                    *wij = *wt;
                    *pij = *p;
                }
                continue;
            }
            let net_row = self
                .row(first_u + i)?
                .get(first_v..=first_v + qu)
                .ok_or_else(|| oob(op))?;
            for ((wij, pij), c) in w_row.iter_mut().zip(p_row.iter_mut()).zip(net_row) {
                *wij = c.w;
                *pij = to_euclid_spec(no_z, no_w, *c);
            }
        }

        let mut s = [[Vec3::ZERO; 2]; 2];
        let add = |s: &mut Vec3, alpha: f64, pt: Vec3| {
            s.x += alpha * pt.x; // upstream: alpha * P.x + sd.x
            s.y += alpha * pt.y; // upstream: alpha * P.y + sd.y
            if !no_z {
                s.z += alpha * pt.z; // upstream: alpha * P.z + sd.z
            }
        };
        if no_w {
            for (i, p_row) in pe.iter().enumerate().take(pu + 1) {
                let (a0, a1) = (
                    du0.get(i).copied().unwrap_or(0.0),
                    du1.get(i).copied().unwrap_or(0.0),
                );
                for ((pt, b0), b1) in p_row.iter().zip(&dv0).zip(&dv1).take(qu + 1) {
                    let [[s00, s01], [s10, s11]] = &mut s;
                    add(s00, a0 * b0, *pt);
                    add(s01, a0 * b1, *pt);
                    add(s10, a1 * b0, *pt);
                    if !upper {
                        add(s11, a1 * b1, *pt);
                    }
                }
            }
        } else {
            // Derivatives of the denominator, `d[k][l]`.
            let mut d = [[0.0_f64; 2]; 2];
            for (l, dvl) in [&dv0, &dv1].into_iter().enumerate() {
                let mut tu = [0.0_f64; N];
                for (t, w_row) in tu.iter_mut().zip(&w).take(pu + 1) {
                    for (wij, dvlj) in w_row.iter().zip(dvl).take(qu + 1) {
                        *t += wij * dvlj;
                    }
                }
                for (duk, dk) in [&du0, &du1].into_iter().zip(d.iter_mut()) {
                    let mut x = 0.0;
                    for (t, duki) in tu.iter().zip(duk).take(pu + 1) {
                        x += t * duki;
                    }
                    if let Some(slot) = dk.get_mut(l) {
                        *slot = x;
                    }
                }
            }
            let [[d00, d01], [d10, d11]] = d;
            if float_op_is_bad(1.0, d00, FloatOp::Division) {
                return Err(SolidError::nlib(bd_op, NlibCode::Convergence));
            }
            let inv_w00 = 1.0 / d00;
            let t11 = pascal_entry(1, 1).ok_or_else(|| oob(bd_op))?;
            let (c0, c1, c2) = (t11 * d01, t11 * d10, t11 * d11);
            for (i, (w_row, p_row)) in w.iter().zip(&pe).enumerate().take(pu + 1) {
                let (a0, a1) = (
                    du0.get(i).copied().unwrap_or(0.0),
                    du1.get(i).copied().unwrap_or(0.0),
                );
                for (((wnm, pt), b0), b1) in
                    w_row.iter().zip(p_row).zip(&dv0).zip(&dv1).take(qu + 1)
                {
                    // The rational basis function's derivatives, upstream's recurrence.
                    let r00 = (wnm * a0 * b0) * inv_w00;
                    let r01 = (wnm * a0 * b1 - c0 * r00) * inv_w00;
                    let mut v10 = wnm * a1 * b0;
                    v10 -= c1 * r00;
                    v10 -= t11 * 0.0;
                    let r10 = v10 * inv_w00;
                    let [[s00, s01], [s10, s11]] = &mut s;
                    add(s00, r00, *pt);
                    add(s01, r01, *pt);
                    add(s10, r10, *pt);
                    if !upper {
                        let mut v11 = wnm * a1 * b1;
                        v11 -= c0 * r10;
                        v11 -= c1 * r01;
                        let v2 = 0.0 + c2 * r00;
                        v11 -= t11 * v2;
                        add(s11, v11 * inv_w00, *pt);
                    }
                }
            }
        }
        *sd = s;
        Ok(())
    }

    /// Min-max box of the Euclidean control net (`N_SrfGetBBox`).
    pub fn bbox(&self) -> Result<MinMaxBox> {
        let (n, m, _, _) = self.array_sizes();
        // Map control net.
        let ntl = self.enet(0, n, 0, m)?;
        // Get min-max box.
        net_bbox(&ntl)
    }

    /// Applies the 4x4 matrix `rma` to every control point (`N_SrfTransform`).
    ///
    /// Errors with `InvalidInput` (changing nothing) when `rma`'s storage is smaller
    /// than 4x4.
    pub fn transform(&mut self, rma: &RealMatrix) -> Result<()> {
        const OP: &str = "nurbs::srf_nurb::Surface::transform";
        for i in 0..4 {
            for j in 0..4 {
                rma.get(i, j).map_err(|_| oob(OP))?;
            }
        }
        let (n, m, _, _) = self.array_sizes();
        for i in 0..=n {
            for j in 0..=m {
                let pw = self.cpt_mut(i, j)?;
                *pw = pw.transform(rma)?;
            }
        }
        Ok(())
    }

    /// Smallest and largest weight and position vector magnitude of the control
    /// points (`N_SrfMinMaxWeightPosVectors`).
    #[must_use]
    pub fn min_max_weight_pos_vectors(&self) -> MinMaxWeightPos {
        let p00 = self.cpt(0, 0).unwrap_or(CPoint::ZERO);

        // Compute min-max weights.
        let (mut wmin, mut wmax) = (NOW, NOW);
        if self.is_rational() {
            wmin = p00.w;
            wmax = p00.w;
            for row in self.rows() {
                for pw in row {
                    let w = pw.w;
                    if w < wmin {
                        wmin = w;
                    }
                    if w > wmax {
                        wmax = w;
                    }
                }
            }
        }

        // Compute min-max position vector magnitudes.
        let mag = p00.to_euclid().length();
        let (mut pmin, mut pmax) = (mag, mag);
        for row in self.rows() {
            for pw in row {
                let mag = pw.to_euclid().length();
                if mag < pmin {
                    pmin = mag;
                }
                if mag > pmax {
                    pmax = mag;
                }
            }
        }
        MinMaxWeightPos {
            wmin,
            wmax,
            pmin,
            pmax,
        }
    }

    /// Point, unit partial derivatives and unit normal at `(u, v)`
    /// (`N_SrfEvalPtPtDerivNormalFast`). Where a partial derivative is shorter than
    /// `0.001 * NL_MTOL` or the normal shorter than `NL_LTOL`, the pole evaluator
    /// [`Surface::eval_pt_deriv_normal_pole`] is used instead.
    ///
    /// Errors with the errors of [`Surface::derivs`] and with
    /// [`NlibCode::Geometry`] when a vector cannot be normalised.
    pub fn eval_pt_deriv_normal_fast(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
    ) -> Result<PtDerivNormal> {
        const OP: &str = "nurbs::srf_nurb::Surface::eval_pt_deriv_normal_fast";
        // Compute surface derivatives.
        let sd = self.derivs(u, v, ufl, vfl, true, 1, 1)?;

        // Get point and tangents.
        let p = sd_at(&sd, 0, 0, OP)?;
        let su = sd_at(&sd, 1, 0, OP)?;
        let sv = sd_at(&sd, 0, 1, OP)?;

        // Get unit tangents and the normal.
        if su.length() <= 0.001 * MODEL_TOL {
            return self.eval_pt_deriv_normal_pole(u, v, ufl, vfl);
        }
        let (su, _) = normalize(su)?;
        if sv.length() <= 0.001 * MODEL_TOL {
            return self.eval_pt_deriv_normal_pole(u, v, ufl, vfl);
        }
        let (sv, _) = normalize(sv)?;
        let n = su.cross(sv);
        if n.length() <= LINE_TOL {
            return self.eval_pt_deriv_normal_pole(u, v, ufl, vfl);
        }
        let (n, _) = normalize(n)?;
        Ok(PtDerivNormal { p, su, sv, n })
    }

    /// Translates every control point by `t` (`N_SrfTranslate`).
    pub fn translate(&mut self, t: Vec3) {
        let (n, m, _, _) = self.array_sizes();
        for i in 0..=n {
            for j in 0..=m {
                if let Ok(pw) = self.cpt_mut(i, j) {
                    *pw = pw.translate(t);
                }
            }
        }
    }

    /// Rotates the surface by `al` degrees about the axis through `p` along `v`
    /// (`N_SrfRotateAtPt`).
    ///
    /// Errors with the errors of [`rotation_matrix_about_axis`] (nothing changes).
    pub fn rotate_at_pt(&mut self, p: Vec3, v: Vec3, al: f64) -> Result<()> {
        // Get rotation matrix.
        let rma = rotation_matrix_about_axis(p, v, al)?;
        // Get rotated control points.
        let (n, m, _, _) = self.array_sizes();
        for i in 0..=n {
            for j in 0..=m {
                let pw = self.cpt_mut(i, j)?;
                *pw = pw.transform(&rma)?;
            }
        }
        Ok(())
    }

    /// Reverses the surface in place in `dir` (`N_SrfReverse` with `surQ == surP`):
    /// the rows (u) or columns (v) of the net are swapped end for end and that
    /// direction's knots reversed by
    /// [`reverse_knots`] (`NI_ReverseKnots`).
    ///
    /// Errors with [`NlibCode::Call`] for [`Direction::UV`] (nothing changes).
    pub fn reverse(&mut self, dir: Direction) -> Result<()> {
        const OP: &str = "nurbs::srf_nurb::Surface::reverse";
        let (nu, mu, ru, su) = self.array_sizes();
        let (n, m) = (sx(nu, OP)?, sx(mu, OP)?);
        match dir {
            Direction::U => {
                let k = n / 2;
                for j in 0..=m {
                    for i in 0..=k {
                        self.swap_cpts((i, j), (n - i, j), OP)?;
                    }
                }
                // (each pair of the in-place knot reversal is read before it is
                // written, so reversing from a copy gives the same knots)
                let up = self.u_knots().to_vec();
                reverse_knots(&up, ru, self.u_knots_mut())?;
            }
            Direction::V => {
                let k = m / 2;
                for i in 0..=n {
                    for j in 0..=k {
                        self.swap_cpts((i, j), (i, m - j), OP)?;
                    }
                }
                let vp = self.v_knots().to_vec();
                reverse_knots(&vp, su, self.v_knots_mut())?;
            }
            Direction::UV => return Err(SolidError::nlib(OP, NlibCode::Call)),
        }
        Ok(())
    }

    fn swap_cpts(&mut self, a: (isize, isize), b: (isize, isize), op: &'static str) -> Result<()> {
        let (ai, aj) = (ux(a.0, op)?, ux(a.1, op)?);
        let (bi, bj) = (ux(b.0, op)?, ux(b.1, op)?);
        let pa = self.cpt(ai, aj)?;
        let pb = self.cpt(bi, bj)?;
        *self.cpt_mut(ai, aj)? = pb;
        *self.cpt_mut(bi, bj)? = pa;
        Ok(())
    }

    /// The surface reversed in `dir` (`N_SrfReverse` into a NULL `surQ`): net rows
    /// (u) or columns (v) in reverse order, that direction's knots reversed by
    /// [`reverse_knots`] (`NI_ReverseKnots`), the
    /// other's copied.
    ///
    /// Errors with [`NlibCode::Call`] for [`Direction::UV`].
    pub fn reversed(&self, dir: Direction) -> Result<Surface> {
        const OP: &str = "nurbs::srf_nurb::Surface::reversed";
        let idx = self.indices();
        let (n, m, r, s) = (
            sx(idx.n, OP)?,
            sx(idx.m, OP)?,
            sx(idx.r, OP)?,
            sx(idx.s, OP)?,
        );
        let up = self.u_knots();
        let vp = self.v_knots();
        let mut uq = vec![0.0; idx.r + 1];
        let mut vq = vec![0.0; idx.s + 1];
        let mut net = vec![vec![CPoint::ZERO; idx.m + 1]; idx.n + 1];
        let put = |net: &mut Vec<Vec<CPoint>>, i: isize, j: isize, pw: CPoint| -> Result<()> {
            let row = net.get_mut(ux(i, OP)?).ok_or_else(|| oob(OP))?;
            set(row, j, pw, OP)
        };
        let cpt = |i: isize, j: isize| -> Result<CPoint> { self.cpt(ux(i, OP)?, ux(j, OP)?) };

        match dir {
            Direction::U => {
                for j in 0..=m {
                    for i in 0..=n {
                        put(&mut net, i, j, cpt(n - i, j)?)?;
                    }
                }
                reverse_knots(up, idx.r, &mut uq)?;
                for j in 0..=s {
                    set(&mut vq, j, get(vp, j, OP)?, OP)?;
                }
            }
            Direction::V => {
                for i in 0..=n {
                    for j in 0..=m {
                        put(&mut net, i, j, cpt(i, m - j)?)?;
                    }
                }
                for i in 0..=r {
                    set(&mut uq, i, get(up, i, OP)?, OP)?;
                }
                reverse_knots(vp, idx.s, &mut vq)?;
            }
            Direction::UV => return Err(SolidError::nlib(OP, NlibCode::Call)),
        }
        Surface::new(net, idx.p, idx.q, uq, vq)
    }

    /// Rescales the knot vectors to `[us, ue] x [vs, ve]` in place, rescaling only
    /// the directions whose bounds change (`N_SrfReparam`).
    ///
    /// Errors with the errors of [`Surface::reparam_to_interval`].
    pub fn reparam(&mut self, us: f64, ue: f64, vs: f64, ve: f64) -> Result<()> {
        // Make new parameter rectangle.
        let rect = Rectangle::new(us, ue, vs, ve);
        // Determine which directions need scaling and call the utility.
        let b = self.parameter_bounds();
        let du = us != b.ul || ue != b.ur;
        let dv = vs != b.vb || ve != b.vt;
        match (du, dv) {
            (true, true) => self.reparam_to_interval(rect, Direction::UV),
            (true, false) => self.reparam_to_interval(rect, Direction::U),
            (false, true) => self.reparam_to_interval(rect, Direction::V),
            // `N_SrfReparamToInterval` with flag 0 changes nothing.
            (false, false) => Ok(()),
        }
    }

    /// Whether the surface is G`level` smoothly closed (`N_SrfIsClosedSmooth`):
    /// `level = 0` checks G0 closure of the control net within `tols[0]`;
    /// `level = 1` samples each knot span of the boundaries `u = U[0]` / `U[r]`
    /// (u-closure) and `v = V[0]` / `V[s]` (v-closure), requiring points within
    /// `tols[0]` and first partial derivative angles within `tols[1]`. `dflg` picks
    /// the directions checked. Returns the closed direction(s), `None` for none.
    ///
    /// Errors with [`NlibCode::Input`] for `level > 1`, with [`NlibCode::Knot`] when
    /// the knot vector has no next span, with the errors of [`Surface::derivs`], and
    /// with the error of a failed derivative angle when nothing clears it.
    pub fn is_closed_smooth(
        &self,
        level: usize,
        tols: &[f64],
        dflg: Direction,
    ) -> Result<Option<Direction>> {
        self.is_closed_smooth_with_fixes(&UpstreamFixes::default(), level, tols, dflg)
    }

    /// [`Surface::is_closed_smooth`] with `fixes`: with
    /// [`UpstreamFixes::closed_smooth_pole_tangent_limit`], a station where the
    /// derivative across the seam vanishes (a pole on the seam) is tested by its limit
    /// direction (WR-725).
    ///
    /// # Errors
    ///
    /// Those of [`Surface::is_closed_smooth`].
    pub fn is_closed_smooth_with_fixes(
        &self,
        fixes: &UpstreamFixes,
        level: usize,
        tols: &[f64],
        dflg: Direction,
    ) -> Result<Option<Direction>> {
        match self.is_closed_smooth_flag_with_fixes(fixes, level, tols, dflg)? {
            (_, Some(e)) => Err(e),
            (closed, None) => Ok(closed),
        }
    }

    /// [`Surface::is_closed_smooth`] split as upstream returns it: the closed
    /// direction(s) upstream stores in `*gnflg`, and the error of a failed derivative
    /// angle that nothing cleared, which upstream returns after storing the flag.
    ///
    /// # Errors
    ///
    /// Those of [`Surface::is_closed_smooth`] that upstream returns before storing the
    /// flag (bad level, knots, evaluations).
    pub fn is_closed_smooth_flag(
        &self,
        level: usize,
        tols: &[f64],
        dflg: Direction,
    ) -> Result<(Option<Direction>, Option<SolidError>)> {
        self.is_closed_smooth_flag_with_fixes(&UpstreamFixes::default(), level, tols, dflg)
    }

    /// [`Surface::is_closed_smooth_flag`] with `fixes` (see
    /// [`Surface::is_closed_smooth_with_fixes`]).
    ///
    /// # Errors
    ///
    /// Those of [`Surface::is_closed_smooth_flag`].
    pub fn is_closed_smooth_flag_with_fixes(
        &self,
        fixes: &UpstreamFixes,
        level: usize,
        tols: &[f64],
        dflg: Direction,
    ) -> Result<(Option<Direction>, Option<SolidError>)> {
        const OP: &str = "nurbs::srf_nurb::Surface::is_closed_smooth";
        let pole_limit = fixes.closed_smooth_pole_tangent_limit;
        let tol = |i: usize| -> Result<f64> {
            tols.get(i)
                .copied()
                .ok_or_else(|| invalid(OP, "missing tolerance"))
        };

        // Check for bad input data.
        if level > 1 {
            return Err(SolidError::nlib(OP, NlibCode::Input));
        }
        let mut uboo = false;
        let mut vboo = false;
        let do_u = matches!(dflg, Direction::U | Direction::UV);
        let do_v = matches!(dflg, Direction::V | Direction::UV);

        // Handle case n = 0.
        if level == 0 {
            let t0 = tol(0)?;
            if do_u {
                uboo = self.is_closed_tol(Direction::U, t0);
            }
            if do_v {
                vboo = self.is_closed_tol(Direction::V, t0);
            }
            return Ok((flag_of(uboo, vboo), None));
        }

        // Get local notation.
        let (_, _, ru, su) = self.array_sizes();
        let (pu, qu) = self.degrees();
        let (r, s) = (sx(ru, OP)?, sx(su, OP)?);
        let (p, q) = (sx(pu, OP)?, sx(qu, OP)?);
        let uk = self.u_knots();
        let vk = self.v_knots();
        let (t0, t1) = (tol(0)?, tol(1)?);
        let mut pending: Option<SolidError> = None;
        // The mixed derivative is the limit direction at a pole (WR-725): evaluate it
        // too then (`upper` leaves it zero; the lower orders are the same either way).
        let upper = !pole_limit;
        let dom = self.parameter_bounds();

        // Loop through knot spans and check continuity.
        if do_u {
            let mut ii = q; // check u-closure
            loop {
                let dv = (get(vk, succ(ii, OP)?, OP)? - get(vk, ii, OP)?) / q as f64;
                let mut broke = false;
                for jj in 0..=q {
                    let (x, d1, d2) = if jj == q {
                        let v = get(vk, ii + 1, OP)?;
                        let d1 =
                            self.derivs(get(uk, 0, OP)?, v, Side::Left, Side::Right, upper, 1, 1)?;
                        let d2 =
                            self.derivs(get(uk, r, OP)?, v, Side::Right, Side::Right, upper, 1, 1)?;
                        (v, d1, d2)
                    } else {
                        let v = get(vk, ii, OP)? + jj as f64 * dv;
                        let d1 =
                            self.derivs(get(uk, 0, OP)?, v, Side::Left, Side::Left, upper, 1, 1)?;
                        let d2 =
                            self.derivs(get(uk, r, OP)?, v, Side::Right, Side::Left, upper, 1, 1)?;
                        (v, d1, d2)
                    };
                    pending = None;

                    // check G0
                    let dd = dist_pt_pt(sd_at(&d1, 0, 0, OP)?, sd_at(&d2, 0, 0, OP)?);
                    if dd > t0 {
                        broke = true;
                        break;
                    }

                    // check G1
                    let (a, b) = if pole_limit {
                        let end = x == dom.vt;
                        (
                            seam_tangent(&d1, Direction::U, end, OP)?,
                            seam_tangent(&d2, Direction::U, end, OP)?,
                        )
                    } else {
                        (sd_at(&d1, 1, 0, OP)?, sd_at(&d2, 1, 0, OP)?)
                    };
                    match vectors_angle(a, b) {
                        Err(e) => {
                            pending = Some(e);
                            broke = true;
                            break;
                        }
                        Ok(dd) if dd > t1 => {
                            broke = true;
                            break;
                        }
                        Ok(_) => {}
                    }
                }
                if broke {
                    break;
                }
                ii += 1;
                if ii >= s - q {
                    uboo = true;
                    break;
                }
                // go to next span
                while ii < s && get(vk, ii, OP)? == get(vk, ii + 1, OP)? {
                    ii += 1;
                }
                if ii >= s {
                    return Err(SolidError::nlib(OP, NlibCode::Knot));
                }
            }
        }

        if do_v {
            let mut ii = p; // check v-closure
            loop {
                let du = (get(uk, succ(ii, OP)?, OP)? - get(uk, ii, OP)?) / p as f64;
                let mut broke = false;
                for jj in 0..=p {
                    let (x, d1, d2) = if jj == p {
                        let u = get(uk, ii + 1, OP)?;
                        let d1 =
                            self.derivs(u, get(vk, 0, OP)?, Side::Right, Side::Left, upper, 1, 1)?;
                        let d2 =
                            self.derivs(u, get(vk, s, OP)?, Side::Right, Side::Right, upper, 1, 1)?;
                        (u, d1, d2)
                    } else {
                        let u = get(uk, ii, OP)? + jj as f64 * du;
                        let d1 =
                            self.derivs(u, get(vk, 0, OP)?, Side::Left, Side::Left, upper, 1, 1)?;
                        let d2 =
                            self.derivs(u, get(vk, s, OP)?, Side::Left, Side::Right, upper, 1, 1)?;
                        (u, d1, d2)
                    };
                    pending = None;

                    // check G0
                    let dd = dist_pt_pt(sd_at(&d1, 0, 0, OP)?, sd_at(&d2, 0, 0, OP)?);
                    if dd > t0 {
                        broke = true;
                        break;
                    }

                    // check G1
                    let (a, b) = if pole_limit {
                        let end = x == dom.ur;
                        (
                            seam_tangent(&d1, Direction::V, end, OP)?,
                            seam_tangent(&d2, Direction::V, end, OP)?,
                        )
                    } else {
                        (sd_at(&d1, 0, 1, OP)?, sd_at(&d2, 0, 1, OP)?)
                    };
                    match vectors_angle(a, b) {
                        Err(e) => {
                            pending = Some(e);
                            broke = true;
                            break;
                        }
                        Ok(dd) if dd > t1 => {
                            broke = true;
                            break;
                        }
                        Ok(_) => {}
                    }
                }
                if broke {
                    break;
                }
                ii += 1;
                if ii >= r - p {
                    vboo = true;
                    break;
                }
                // go to next span
                while ii < r && get(uk, ii, OP)? == get(uk, ii + 1, OP)? {
                    ii += 1;
                }
                if ii >= r {
                    return Err(SolidError::nlib(OP, NlibCode::Knot));
                }
            }
        }

        // set the output flag (also when a failed angle is returned)
        Ok((flag_of(uboo, vboo), pending))
    }

    /// Point, unit partial derivatives and unit normal where the parameter may be at a
    /// pole (`N_SrfEvalPtPtDerivNormalPole`); same as
    /// [`Surface::eval_pt_normal_at_pole`].
    ///
    /// # Errors
    ///
    /// The errors of [`Surface::eval_pt_normal_at_pole`].
    pub fn eval_pt_deriv_normal_pole(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
    ) -> Result<PtDerivNormal> {
        self.eval_pt_normal_at_pole(u, v, ufl, vfl)
    }

    /// Point, unit partial derivatives and unit normal at a possible pole
    /// (`N_SrfEvalPtNormalAtPole`): a partial derivative shorter than
    /// `0.001 * NL_MTOL` (or not normalisable) is a pole and comes back zero; the
    /// normal then uses the mixed derivative `S_uv` with the other partial
    /// derivative (negated at the domain end).
    ///
    /// Errors with the errors of [`Surface::derivs`] and with
    /// [`NlibCode::Geometry`] when both partials vanish or the normal is too short.
    pub fn eval_pt_normal_at_pole(
        &self,
        u: f64,
        v: f64,
        ufl: Side,
        vfl: Side,
    ) -> Result<PtDerivNormal> {
        const OP: &str = "nurbs::srf_nurb::Surface::eval_pt_normal_at_pole";
        let mut pole = 0;

        // Compute surface derivatives.
        let sd = self.derivs(u, v, ufl, vfl, false, 1, 1)?;

        // Get point and tangents.
        let p = sd_at(&sd, 0, 0, OP)?;
        let mut su = sd_at(&sd, 1, 0, OP)?;
        let mut sv = sd_at(&sd, 0, 1, OP)?;

        // Get unit tangents and the normal.
        let mag = su.length();
        let nsu = normalize(su);
        if mag <= 0.001 * MODEL_TOL || nsu.is_err() {
            pole = 1;
            su = Vec3::ZERO;
        } else if let Ok((x, _)) = nsu {
            su = x;
        }

        let mag = sv.length();
        let nsv = normalize(sv);
        if mag <= 0.001 * MODEL_TOL || nsv.is_err() {
            sv = Vec3::ZERO;
            if pole == 1 {
                return Err(SolidError::nlib(OP, NlibCode::Geometry));
            }
            pole = 2;
        } else if let Ok((x, _)) = nsv {
            sv = x;
        }

        let n = match pole {
            1 => {
                let b = self.parameter_bounds();
                let mut s01 = sd_at(&sd, 0, 1, OP)?;
                if v == b.vt {
                    s01 *= -1.0;
                }
                sd_at(&sd, 1, 1, OP)?.cross(s01)
            }
            2 => {
                let b = self.parameter_bounds();
                let mut s10 = sd_at(&sd, 1, 0, OP)?;
                if u == b.ur {
                    s10 *= -1.0;
                }
                s10.cross(sd_at(&sd, 1, 1, OP)?)
            }
            _ => su.cross(sv),
        };

        if n.length() < 0.001 * MODEL_TOL {
            return Err(SolidError::nlib(OP, NlibCode::Geometry));
        }
        let (n, _) = normalize(n)?;
        Ok(PtDerivNormal { p, su, sv, n })
    }

    /// Recognises the surface type (`N_SrfType`): plane (bilinear patch with
    /// straight boundaries), surface of revolution (sphere, torus, cylinder, cone or
    /// general) in u then v, ruled surface or extrusion in u then v, else free-form.
    /// `tol` bounds the distances, radius differences and angle differences used.
    ///
    /// Errors with the errors of the iso-curve extractions and of the geometric
    /// helpers whose failure upstream turns into an error return (projections,
    /// normalisations, planarity); a failed curve-type test only moves on to the
    /// next candidate, except in the last (v-ruled) test.
    pub fn surface_type(&self, tol: f64) -> Result<SurfaceType> {
        TypeFinder::new(self, tol)?.run()
    }
}

/// `i + 1` on a signed index; `InvalidInput` where it overflows (an index that far
/// out is outside every array upstream would read).
fn succ(i: isize, op: &'static str) -> Result<isize> {
    i.checked_add(1).ok_or_else(|| oob(op))
}

/// Shortest partial derivative that is not a pole: `N_SrfEvalPtNormalAtPole` takes a
/// shorter one as vanishing (`0.001 * NL_MTOL`).
const POLE_DERIV_TOL: f64 = 0.001 * MODEL_TOL;

/// The direction across the seam of `seam` (`U`: the u seam, `S_u`; `V`: `S_v`) from
/// the first derivatives `sd` (with the mixed one) at a station of
/// [`Surface::is_closed_smooth_flag_with_fixes`]. Where it vanishes (shorter than
/// [`POLE_DERIV_TOL`]: a row collapsed to a pole) it is its limit along the seam,
/// the mixed derivative `S_uv`, negated at the end of the domain along the seam
/// (`end`), where the row is approached from below (WR-725; the pole normal of
/// [`Surface::eval_pt_normal_at_pole`] takes the same limit).
fn seam_tangent(sd: &[Vec<Vec3>], seam: Direction, end: bool, op: &'static str) -> Result<Vec3> {
    let across = match seam {
        Direction::V => sd_at(sd, 0, 1, op)?,
        _ => sd_at(sd, 1, 0, op)?,
    };
    if across.length() > POLE_DERIV_TOL {
        return Ok(across);
    }
    let mut mixed = sd_at(sd, 1, 1, op)?;
    if end {
        mixed *= -1.0;
    }
    Ok(mixed)
}

/// `SD[k][l]`.
fn sd_at(sd: &[Vec<Vec3>], k: usize, l: usize, op: &'static str) -> Result<Vec3> {
    sd.get(k)
        .and_then(|row| row.get(l))
        .copied()
        .ok_or_else(|| oob(op))
}

/// `NL_NO` / `NL_UDIR` / `NL_VDIR` / `NL_UVDIR` from the two closure flags.
fn flag_of(uboo: bool, vboo: bool) -> Option<Direction> {
    if uboo && vboo {
        Some(Direction::UV)
    } else if uboo {
        Some(Direction::U)
    } else if vboo {
        Some(Direction::V)
    } else {
        None
    }
}

// ---- N_SrfType --------------------------------------------------------------------

const OP_TYPE: &str = "nurbs::srf_nurb::Surface::surface_type";

/// Control flow of `N_SrfType`'s labelled sections.
enum Step {
    /// `NL_OUT` without an error: return the outputs.
    Exit,
    /// `goto` the next label.
    Next,
}

/// State of `N_SrfType`: the outputs (written as upstream writes them) and the
/// scratch curves `curP` / `curS` (`curA` points at `curP` once set).
struct TypeFinder<'a> {
    sur: &'a Surface,
    tol: f64,
    n: isize,
    m: isize,
    p: isize,
    q: isize,
    r: isize,
    s: isize,
    out: SurfaceType,
    cur_p: Option<Curve>,
    cur_s: Option<Curve>,
    cur_a_set: bool,
}

impl<'a> TypeFinder<'a> {
    fn new(sur: &'a Surface, tol: f64) -> Result<Self> {
        let (n, m, r, s) = sur.array_sizes();
        let (p, q) = sur.degrees();
        Ok(Self {
            sur,
            tol,
            n: sx(n, OP_TYPE)?,
            m: sx(m, OP_TYPE)?,
            p: sx(p, OP_TYPE)?,
            q: sx(q, OP_TYPE)?,
            r: sx(r, OP_TYPE)?,
            s: sx(s, OP_TYPE)?,
            out: SurfaceType {
                kind: SurfaceKind::FreeForm,
                dir: None,
                cur_a: None,
                cur_b: None,
                aa: Vec3::ZERO,
                bb: Vec3::ZERO,
                cc: Vec3::ZERO,
                dd: Vec3::ZERO,
                xx: Vec3::ZERO,
                yy: Vec3::ZERO,
                r1: 0.0,
                r2: 0.0,
                ac: 0.0,
                al: 0.0,
                h: 0.0,
            },
            cur_p: None,
            cur_s: None,
            cur_a_set: false,
        })
    }

    fn run(mut self) -> Result<SurfaceType> {
        let mut step = self.plane()?;
        if matches!(step, Step::Next) {
            step = self.revolution(Direction::U)?;
        }
        if matches!(step, Step::Next) {
            step = self.revolution(Direction::V)?;
        }
        if matches!(step, Step::Next) {
            step = self.ruled(Direction::U)?;
        }
        if matches!(step, Step::Next) {
            self.ruled(Direction::V)?;
        }
        let mut out = self.out;
        if self.cur_a_set {
            out.cur_a = self.cur_p;
        }
        Ok(out)
    }

    fn epoint(&self, i: isize, j: isize) -> Result<Vec3> {
        Ok(self.sur.cpt(ux(i, OP_TYPE)?, ux(j, OP_TYPE)?)?.to_euclid())
    }

    fn uk(&self, i: isize) -> Result<f64> {
        get(self.sur.u_knots(), i, OP_TYPE)
    }

    fn vk(&self, j: isize) -> Result<f64> {
        get(self.sur.v_knots(), j, OP_TYPE)
    }

    /// Check if the surface is a plane.
    fn plane(&mut self) -> Result<Step> {
        let (n, m, tol) = (self.n, self.m, self.tol);

        // Fit plane to the four corners.
        let p1 = self.epoint(0, 0)?;
        let p2 = self.epoint(n, 0)?;
        let p3 = self.epoint(n, m)?;
        let p4 = self.epoint(0, m)?;

        let v1 = p2 - p1;
        let v2 = p3 - p2;
        let v3 = p4 - p3;
        let v4 = p1 - p4;

        let mut n1 = v4.cross(v1);
        let f1 = n1.length();
        let mut n2 = v1.cross(v2);
        let f2 = n2.length();
        let mut n3 = v2.cross(v3);
        let f3 = n3.length();
        let mut n4 = v3.cross(v4);
        let f4 = n4.length();

        let mut max = f1;
        let mut a = n1;
        if f2 > max {
            max = f2;
            a = n2;
        }
        if f3 > max {
            max = f3;
            a = n3;
        }
        if f4 > max {
            max = f4;
            a = n4;
        }
        if max < MODEL_TOL {
            return Ok(Step::Next);
        }

        if a.dot(n1) < 0.0 {
            n1 = -n1;
        }
        if a.dot(n2) < 0.0 {
            n2 = -n2;
        }
        if a.dot(n3) < 0.0 {
            n3 = -n3;
        }
        if a.dot(n4) < 0.0 {
            n4 = -n4;
        }

        let pa = combine4_pts(0.25, p1, 0.25, p2, 0.25, p3, 0.25, p4);
        let na = combine4_pts(0.25, n1, 0.25, n2, 0.25, n3, 0.25, n4);
        let (na, _) = normalize(na)?;

        let f4 = -pa.dot(na);
        let (f1, f2, f3) = (na.x, na.y, na.z);

        // Check flatness.
        for i in 0..=n {
            for j in 0..=m {
                let pa = self.epoint(i, j)?;
                let d = dist_signed_pt_plane(f1, f2, f3, f4, pa);
                if d.abs() > tol {
                    return Ok(Step::Next);
                }
            }
        }

        // Check straightness of boundaries.
        for i in 1..n {
            let pa = self.epoint(i, 0)?;
            if dist_pt_line_seg(pa, p1, p2)? > tol {
                return Ok(Step::Next);
            }
            let pa = self.epoint(i, m)?;
            if dist_pt_line_seg(pa, p4, p3)? > tol {
                return Ok(Step::Next);
            }
        }
        for j in 1..m {
            let pa = self.epoint(0, j)?;
            if dist_pt_line_seg(pa, p1, p4)? > tol {
                return Ok(Step::Next);
            }
            let pa = self.epoint(n, j)?;
            if dist_pt_line_seg(pa, p2, p3)? > tol {
                return Ok(Step::Next);
            }
        }

        // Output defining entities.
        self.out.aa = p1;
        self.out.bb = p2;
        self.out.cc = p3;
        self.out.dd = p4;
        self.out.kind = SurfaceKind::Plane;
        Ok(Step::Exit)
    }

    /// Check for a surface of revolution whose circles are the iso-curves in
    /// `iso` (`U`: revolution in the u-direction, sampled over the v knots).
    fn revolution(&mut self, iso: Direction) -> Result<Step> {
        let tol = self.tol;
        let (deg, last) = if iso == Direction::U {
            (self.q, self.s)
        } else {
            (self.p, self.r)
        };
        let knot = |me: &Self, i: isize| {
            if iso == Direction::U {
                me.vk(i)
            } else {
                me.uk(i)
            }
        };

        let mut pa = Vec3::ZERO;
        let mut na = Vec3::ZERO;
        let mut ps: Vec<Vec3> = Vec::new();
        let mut ns: Vec<Vec3> = Vec::new();
        let mut angles: Vec<f64> = Vec::new();

        let mut vl = knot(self, deg)?;
        let mut j = deg + 1;
        let mut aa = 0.0;
        let kh = 2 * deg;

        while j < last {
            let kl = isize::from(j != deg + 1);
            while j < last && knot(self, j)? == knot(self, j + 1)? {
                j += 1;
            }
            let vr = knot(self, j)?;
            let vi = (vr - vl) / kh as f64;

            for k in kl..=kh {
                let v = if k == kh { vr } else { vl + k as f64 * vi };
                let cur = extract_iso_crv(self.sur, v, iso)?;
                let degen = cur.is_degen();
                let ty = if degen { None } else { Some(cur.get_type(tol)) };
                self.cur_s = Some(cur);
                let Some(ty) = ty else {
                    continue;
                };
                let Ok(CurveType::Circle {
                    center,
                    normal,
                    angle,
                    ..
                }) = ty
                else {
                    return Ok(Step::Next);
                };
                ps.push(center);
                ns.push(normal);
                angles.push(angle);
                pa += center;
                na += normal;
                aa += angle;
            }
            vl = vr;
            j += 1;
        }

        let f = 1.0 / ps.len() as f64;
        let aa = f * aa;
        let pa = pa * f;
        let na = na * f;
        if na.length() < tol {
            return Ok(Step::Next);
        }

        let lsg = LineSeg::new(pa, na, false);
        for ((pk, nk), ak) in ps.iter().zip(&ns).zip(&angles) {
            if dist_pt_inf_line(lsg, *pk)? > tol {
                return Ok(Step::Next);
            }
            if (*nk - na).length() > tol {
                return Ok(Step::Next);
            }
            if (aa - ak).abs() > tol {
                return Ok(Step::Next);
            }
        }

        // Output revolution defining data.
        let (profile_t, profile_dir) = if iso == Direction::U {
            (self.uk(0)?, Direction::V)
        } else {
            (self.vk(0)?, Direction::U)
        };
        let cur_p = extract_iso_crv(self.sur, profile_t, profile_dir)?;
        let ty = cur_p.get_type(tol);
        // `N_CrvIsPlanar` of the free-form branch (no side effects, so evaluated here).
        let planar = matches!(ty, Ok(CurveType::FreeForm)).then(|| cur_p.is_planar(tol));
        self.cur_p = Some(cur_p);
        self.cur_a_set = true;
        self.out.al = aa;
        self.out.aa = pa;
        self.out.bb = na;

        // Determine sub-type.
        let Ok(ty) = ty else {
            return Ok(Step::Next);
        };
        // Line: cylinder or cone, linear in the profile direction.
        // Circle: sphere or torus. Otherwise: general revolution.
        let (line_dir, rev_dir) = if iso == Direction::U {
            (Direction::V, Direction::U)
        } else {
            (Direction::U, Direction::V)
        };
        match ty {
            CurveType::Line { start: p3, dir: n3 } => {
                let p4 = p3 + n3;
                let p1 = project_hit(lsg, p3)?;
                let p2 = project_hit(lsg, p4)?;
                let f1 = dist_pt_pt(p1, p3);
                let f2 = dist_pt_pt(p2, p4);
                self.out.kind = if (f1 - f2).abs() < tol {
                    SurfaceKind::Cylinder
                } else {
                    SurfaceKind::Cone
                };
                let v1 = if f1 > f2 { p3 - p1 } else { p4 - p2 };
                self.out.h = dist_pt_pt(p1, p2);
                let cc = na.cross(v1);
                self.out.cc = cc;
                self.out.yy = cc;
                self.out.aa = p1;
                self.out.xx = v1;
                self.out.cc = normalize(self.out.cc)?.0;
                self.out.xx = normalize(self.out.xx)?.0;
                self.out.yy = normalize(self.out.yy)?.0;
                self.cur_p = Some(line_from_pt_and_vector(p3, n3)?);
                self.out.r1 = f1;
                self.out.r2 = f2;
                self.out.dir = Some(line_dir);
            }
            CurveType::Circle {
                center: p3,
                normal: n3,
                x_axis: v1,
                y_axis: v2,
                radius: f1,
                angle: f2,
            } => {
                self.out.dd = p3;
                self.out.cc = n3;
                self.out.xx = v1;
                self.out.yy = v2;
                let d = dist_pt_inf_line(lsg, p3)?;
                self.out.kind = if d < tol {
                    SurfaceKind::Sphere
                } else {
                    SurfaceKind::Torus
                };
                self.out.r1 = f1;
                self.out.r2 = d;
                self.out.ac = f2;
                self.out.dir = Some(rev_dir);
            }
            CurveType::FreeForm => {
                let planarity = planar.ok_or_else(|| oob(OP_TYPE))??;
                if planarity.flat {
                    if let Some((_, nn)) = planarity.plane {
                        self.out.cc = nn;
                    }
                }
                self.out.dir = Some(rev_dir);
                self.out.kind = SurfaceKind::Revolution;
            }
        }
        Ok(Step::Exit)
    }

    /// Check for a ruled surface whose rulings are the iso-curves in `iso` (`U`:
    /// ruled in the u-direction, sampled over the v knots). The v-direction test is
    /// upstream's last: its failures end the routine.
    fn ruled(&mut self, iso: Direction) -> Result<Step> {
        let tol = self.tol;
        let last_test = iso == Direction::V;
        let (deg, last) = if iso == Direction::U {
            (self.q, self.s)
        } else {
            (self.p, self.r)
        };
        let knot = |me: &Self, i: isize| {
            if iso == Direction::U {
                me.vk(i)
            } else {
                me.uk(i)
            }
        };

        let mut vl = knot(self, deg)?;
        let mut j = deg + 1;
        let mut cnt: usize = 0;
        let kh = 2 * deg;
        let mut vv = Vec3::ZERO;

        while j < last {
            let kl = isize::from(j != deg + 1);
            while j < last && knot(self, j)? == knot(self, j + 1)? {
                j += 1;
            }
            let vr = knot(self, j)?;
            let vi = (vr - vl) / kh as f64;

            for k in kl..=kh {
                let v = if k == kh { vr } else { vl + k as f64 * vi };
                let cur = extract_iso_crv(self.sur, v, iso)?;
                let ty = cur.get_type(tol);
                self.cur_p = Some(cur);
                match ty {
                    Ok(CurveType::Line { dir, .. }) => {
                        vv += dir;
                        cnt += 1;
                    }
                    // Upstream's last test ends the routine on any failure.
                    Err(e) => return if last_test { Err(e) } else { Ok(Step::Next) },
                    Ok(_) => return Ok(if last_test { Step::Exit } else { Step::Next }),
                }
            }
            vl = vr;
            j += 1;
        }

        // Output ruled surface defining data.
        let (ta, tb, bdir) = if iso == Direction::U {
            (self.uk(0)?, self.uk(self.r)?, Direction::V)
        } else {
            (self.vk(0)?, self.vk(self.s)?, Direction::U)
        };
        let cur_p = extract_iso_crv(self.sur, ta, bdir)?;
        self.cur_p = Some(cur_p.clone());
        let mut cur_s = extract_iso_crv(self.sur, tb, bdir)?;

        self.out.dir = Some(iso);
        self.cur_a_set = true;

        let f = 1.0 / cnt as f64;
        let vv = vv * f;

        let mut cur_t = cur_p;
        cur_t.translate(vv);
        if curves_are_coincident(&mut cur_t, &mut cur_s, tol)? {
            self.out.aa = vv;
            self.out.r1 = vv.length();
            self.out.kind = SurfaceKind::Extrusion;
        } else {
            self.out.kind = SurfaceKind::Ruled;
            self.out.cur_b = Some(cur_s.clone());
        }
        self.cur_s = Some(cur_s);
        Ok(Step::Exit)
    }
}

/// `N_ProjectPtLine` onto the (unbounded) axis; `InvalidInput` when it misses
/// (upstream then reads a stale point).
fn project_hit(lsg: LineSeg, p: Vec3) -> Result<Vec3> {
    match project_pt_line(lsg, p)? {
        LinePlaneIsect::Hit { p: q, .. } => Ok(q),
        LinePlaneIsect::Parallel | LinePlaneIsect::OutOfBounds { .. } => Err(invalid(
            OP_TYPE,
            "projection misses the axis (upstream reads a stale point)",
        )),
    }
}

// ---- N_ConeEllipticEnds -----------------------------------------------------------

/// A ruled surface (generalised cone) between two elliptic ends
/// (`N_ConeEllipticEnds`): a cone of radii `radius_front` (at `z = 0`) and
/// `radius_rear` (at `z = height`) about the z axis, ruled in v, whose end circles
/// are replaced by their intersections with the end planes tilted by the given
/// angles (degrees) about x and y.
///
/// # Errors
///
/// The errors of [`cyl_cone`] and [`extract_iso_crv`], [`NlibCode::Geometry`] for a
/// zero-length ruling, and [`SolidError::Degenerate`] (upstream's bare error flag)
/// when an end plane is parallel to a ruling.
pub fn cone_elliptic_ends(c: &EllipticCone) -> Result<Surface> {
    const OP: &str = "nurbs::srf_nurb::cone_elliptic_ends";
    let bare = || SolidError::Degenerate {
        operation: OP,
        entity: None,
    };
    let zero_angle = 1.0e-8;
    let my_pi = core::f64::consts::PI;
    let deg_to_rad = my_pi / 180.0;
    let x_axis = Vec3::new(1.0, 0.0, 0.0);
    let y_axis = Vec3::new(0.0, 1.0, 0.0);

    // Set the z-axis location of the front and rear planes.
    let front_origin = Vec3::new(0.0, 0.0, 0.0);
    let rear_origin = Vec3::new(0.0, 0.0, c.height);

    // Convert these to radians.
    let theta_y_front = c.tilt_y_front * deg_to_rad;
    let theta_x_front = c.tilt_x_front * deg_to_rad;
    let theta_y_rear = c.tilt_y_rear * deg_to_rad;
    let theta_x_rear = c.tilt_x_rear * deg_to_rad;

    // Construct a cone from the two radii and the length.
    let cc = CylCone {
        center: front_origin,
        x: x_axis,
        y: y_axis,
        rb: c.radius_front,
        rt: c.radius_rear,
        start: 0.0,
        end: 360.0,
        h: c.height,
    };
    let mut sur = cyl_cone(&cc, ConicType::Quadratic, Direction::V)?;
    let idx = sur.indices();
    let nv = sx(idx.m, OP)?;

    // Extract the circle from each end; the sizes must match the cone's.
    let ellip_front = extract_iso_crv(&sur, 0.0, Direction::V)?;
    let ellip_rear = extract_iso_crv(&sur, 1.0, Direction::V)?;
    if ellip_front.n() != idx.m || ellip_front.degree() != idx.q || ellip_front.m() != idx.s {
        return Err(bare());
    }
    if ellip_rear.n() != idx.m || ellip_rear.degree() != idx.q || ellip_rear.m() != idx.s {
        return Err(bare());
    }
    let mut pw_front = ellip_front.cpts().to_vec();
    let mut pw_rear = ellip_rear.cpts().to_vec();
    let cpt = |sur: &Surface, i: usize, j: isize| -> Result<CPoint> { sur.cpt(i, ux(j, OP)?) };

    // Front plane: X*tan(theta_X_front) + Y*tan(theta_Y_front) - Z = 0.
    let tan_y_front = theta_y_front.tan();
    let tan_x_front = theta_x_front.tan();
    if c.tilt_y_front.abs() < zero_angle && c.tilt_x_front.abs() < zero_angle {
        // There is no tilt at the front.
        for ii in 0..=nv {
            set(&mut pw_front, ii, cpt(&sur, 0, ii)?, OP)?;
        }
    } else {
        let denom = (tan_y_front * tan_y_front + tan_x_front * tan_x_front + 1.0).sqrt();
        let front_normal = Vec3::new(-tan_x_front / denom, -tan_y_front / denom, 1.0 / denom);
        let f_pln = Plane::new(front_origin, front_normal);
        for ii in 0..nv {
            let p0 = cpt(&sur, 0, ii)?.to_euclid();
            let p1 = cpt(&sur, 1, ii)?.to_euclid();
            let (dp, _) = normalize(p0 - p1)?;
            let lnseg = LineSeg::new(p0, dp, false);
            let LinePlaneIsect::Hit { p: qq, .. } = isect_line_plane(lnseg, f_pln)? else {
                return Err(bare());
            };
            let w0 = cpt(&sur, 0, ii)?.w;
            set(
                &mut pw_front,
                ii,
                CPoint::new(w0 * qq.x, w0 * qq.y, w0 * qq.z, w0),
                OP,
            )?;
        }
        let first = get(&pw_front, 0, OP)?;
        set(&mut pw_front, nv, first, OP)?;
    }

    // Rear plane: X*tan(theta_X_rear) + Y*tan(theta_Y_rear) - Z + L = 0.
    let tan_y_rear = theta_y_rear.tan();
    let tan_x_rear = theta_x_rear.tan();
    if c.tilt_y_rear.abs() < zero_angle && c.tilt_x_rear.abs() < zero_angle {
        // There is no tilt at the rear.
        for ii in 0..=nv {
            set(&mut pw_rear, ii, cpt(&sur, 1, ii)?, OP)?;
        }
    } else {
        let denom = (tan_y_rear * tan_y_rear + tan_x_rear * tan_x_rear + 1.0).sqrt();
        let rear_normal = Vec3::new(-tan_x_rear / denom, -tan_y_rear / denom, 1.0 / denom);
        let r_pln = Plane::new(rear_origin, rear_normal);
        for ii in 0..nv {
            let p0 = cpt(&sur, 0, ii)?.to_euclid();
            let p1 = cpt(&sur, 1, ii)?.to_euclid();
            let (dp, _) = normalize(p1 - p0)?;
            let lnseg = LineSeg::new(p1, dp, false);
            let LinePlaneIsect::Hit { p: qq, .. } = isect_line_plane(lnseg, r_pln)? else {
                return Err(bare());
            };
            let w1 = cpt(&sur, 1, ii)?.w;
            set(
                &mut pw_rear,
                ii,
                CPoint::new(w1 * qq.x, w1 * qq.y, w1 * qq.z, w1),
                OP,
            )?;
        }
        let first = get(&pw_rear, 0, OP)?;
        set(&mut pw_rear, nv, first, OP)?;
    }

    // Reset the coefficients of the cone to those of the ellipse at each end.
    for ic in 0..=nv {
        let j = ux(ic, OP)?;
        *sur.cpt_mut(0, j)? = get(&pw_front, ic, OP)?;
        *sur.cpt_mut(1, j)? = get(&pw_rear, ic, OP)?;
    }
    Ok(sur)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bilinear() -> Surface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(1.0, 0.0, 0.0, NOW),
                CPoint::new(1.0, 1.0, 1.0, NOW),
            ],
        ];
        Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    fn flat() -> Surface {
        let net = vec![
            vec![
                CPoint::new(0.0, 0.0, 0.0, NOW),
                CPoint::new(0.0, 1.0, 0.0, NOW),
            ],
            vec![
                CPoint::new(2.0, 0.0, 0.0, NOW),
                CPoint::new(2.0, 1.0, 0.0, NOW),
            ],
        ];
        Surface::new(
            net,
            1,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    #[test]
    fn eval_and_derivs_bilinear() {
        let s = bilinear();
        let p = s.eval(0.5, 0.5, Side::Left, Side::Left).unwrap();
        assert_eq!(p, Vec3::new(0.5, 0.5, 0.25));
        let sd = s
            .derivs(0.5, 0.5, Side::Left, Side::Left, false, 1, 1)
            .unwrap();
        assert_eq!(sd[0][0], p);
        assert_eq!(sd[1][0], Vec3::new(1.0, 0.0, 0.5));
        assert_eq!(sd[0][1], Vec3::new(0.0, 1.0, 0.5));
        assert_eq!(sd[1][1], Vec3::new(0.0, 0.0, 1.0));
        let bd = s
            .basis_derivs(0.5, 0.5, Side::Left, Side::Left, false, 1, 1)
            .unwrap();
        assert_eq!(bd.get(0, 0, 0, 0), Some(0.25));
        assert_eq!(bd.get(2, 0, 0, 0), None);
        assert!(s
            .basis_derivs(0.5, 0.5, Side::Left, Side::Left, false, 5, 1)
            .is_err());
    }

    /// A clamped surface of degrees `(p, q)` over `(n+1) x (m+1)` points with one
    /// interior knot run in each direction; weights `1 + 0.1 * ..` when `rational`.
    fn spline(p: usize, q: usize, n: usize, m: usize, rational: bool) -> Surface {
        let knots = |deg: usize, last: usize| -> Vec<f64> {
            let inner = last - deg;
            let mut k = vec![0.0; deg + 1];
            k.extend((1..=inner).map(|i| i as f64 / (inner + 1) as f64));
            k.extend(vec![1.0; deg + 1]);
            k
        };
        let net = (0..=n)
            .map(|i| {
                (0..=m)
                    .map(|j| {
                        let (x, y) = (i as f64, j as f64);
                        let z = (x * 0.7 + y * 1.3).sin();
                        let w = if rational {
                            1.0 + 0.1 * ((i + 2 * j) % 3) as f64
                        } else {
                            NOW
                        };
                        let f = if rational { w } else { 1.0 };
                        CPoint::new(x * f, y * f, z * f, w)
                    })
                    .collect()
            })
            .collect();
        Surface::new(net, p, q, knots(p, n), knots(q, m)).unwrap()
    }

    #[test]
    fn first_derivs_into_matches_derivs_bit_for_bit() {
        let surfaces = [
            spline(3, 1, 6, 4, false),
            spline(1, 1, 3, 3, false),
            spline(3, 2, 5, 5, true),
            spline(8, 1, 9, 2, false),
            spline(5, 2, 7, 4, true),
            spline(4, 3, 6, 5, false),
            bilinear(),
        ];
        let params = [-0.2, 0.0, 0.25, 1.0 / 3.0, 0.5, 0.75, 1.0, 1.3];
        let sides = [Side::Left, Side::Right];
        for s in &surfaces {
            for &u in &params {
                for &v in &params {
                    for &ufl in &sides {
                        for &vfl in &sides {
                            for upper in [false, true] {
                                // against the general (BasisDerivs) path, which `derivs`
                                // no longer takes for first derivatives
                                let full = s.derivs_general(u, v, ufl, vfl, upper, 1, 1);
                                let mut block = [[Vec3::new(9.0, 9.0, 9.0); 2]; 2];
                                let r = s.first_derivs_into(u, v, ufl, vfl, upper, &mut block);
                                let net = s.euclid_net().unwrap();
                                let mut by_net = [[Vec3::new(7.0, 7.0, 7.0); 2]; 2];
                                let rn = s.first_derivs_net_into(
                                    Some(&net),
                                    u,
                                    v,
                                    ufl,
                                    vfl,
                                    upper,
                                    &mut by_net,
                                );
                                assert_eq!(r.is_ok(), rn.is_ok());
                                if r.is_ok() {
                                    for (a, b) in
                                        block.iter().flatten().zip(by_net.iter().flatten())
                                    {
                                        assert_eq!(
                                            [a.x.to_bits(), a.y.to_bits(), a.z.to_bits()],
                                            [b.x.to_bits(), b.y.to_bits(), b.z.to_bits()]
                                        );
                                    }
                                }
                                match full {
                                    Ok(full) => {
                                        assert!(r.is_ok());
                                        for k in 0..2 {
                                            for l in 0..2 {
                                                let (a, b) = (full[k][l], block[k][l]);
                                                assert_eq!(
                                                    [a.x.to_bits(), a.y.to_bits(), a.z.to_bits()],
                                                    [b.x.to_bits(), b.y.to_bits(), b.z.to_bits()],
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => assert_eq!(r.err(), Some(e)),
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn normal_and_type_of_plane() {
        let s = flat();
        let r = s
            .eval_pt_deriv_normal_fast(0.3, 0.6, Side::Left, Side::Left)
            .unwrap();
        assert_eq!(r.n, Vec3::new(0.0, 0.0, 1.0));
        let t = s.surface_type(1e-6).unwrap();
        assert_eq!(t.kind, SurfaceKind::Plane);
        assert_eq!(t.cc, Vec3::new(2.0, 1.0, 0.0));
    }

    #[test]
    fn reverse_forms_agree() {
        let mut s = bilinear();
        let q = s.reversed(Direction::U).unwrap();
        s.reverse(Direction::U).unwrap();
        assert_eq!(s, q);
        assert!(s.reverse(Direction::UV).is_err());
        let b = s.bbox().unwrap();
        assert_eq!((b.xl, b.xr, b.zf), (0.0, 1.0, 1.0));
    }

    #[test]
    fn closed_smooth_and_cone() {
        let s = flat();
        assert_eq!(
            s.is_closed_smooth(1, &[1e-6, 1.0], Direction::UV).unwrap(),
            None
        );
        assert!(s.is_closed_smooth(2, &[1e-6, 1.0], Direction::UV).is_err());
        let c = EllipticCone {
            radius_front: 1.0,
            radius_rear: 2.0,
            height: 3.0,
            tilt_y_front: 10.0,
            tilt_y_rear: 0.0,
            tilt_x_front: 5.0,
            tilt_x_rear: -7.0,
        };
        let cone = cone_elliptic_ends(&c).unwrap();
        assert_eq!(cone.degrees(), (1, 2));
        let closed = cone.is_closed_smooth(0, &[1e-9], Direction::V).unwrap();
        assert_eq!(closed, Some(Direction::V));
    }

    #[test]
    fn closed_smooth_absurd_degree_is_an_error() {
        // fuzz (nlib.srf_nurb fuzz01067): p = q = i64::MAX overflowed `ii + 1`.
        let f = flat();
        let s = Surface::new(
            f.cpts(),
            i64::MAX as usize,
            i64::MAX as usize,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        assert!(s.is_closed_smooth(1, &[1e-6, 1.0], Direction::UV).is_err());
    }
    /// `nlib.srf_nurb` case `gen0652` (tols [0.001, 1.0]): rows `[wx, wy, wz, w]`.
    // The weights are the generator's 1/sqrt(2), as the case has them.
    #[allow(clippy::approx_constant)]
    const GEN0652: [[[f64; 4]; 5]; 9] = [
        [
            [
                -1.703002701750313,
                0.7735448152843192,
                1.4722476397469413,
                1.0,
            ],
            [
                -0.41389769497987433,
                0.7880282835525899,
                1.5942309418007503,
                0.7071067811865476,
            ],
            [
                0.2618622986480523,
                0.8260514062216804,
                1.169912816597166,
                1.0,
            ],
            [
                0.7842269092021439,
                0.3801848183434774,
                0.06027563022506944,
                0.7071067811865476,
            ],
            [
                -0.008598637103206852,
                0.19676705546035284,
                -0.6970927659576331,
                1.0,
            ],
        ],
        [
            [
                -1.204204758786658,
                0.5469787844392374,
                1.0410362896509515,
                0.7071067811865476,
            ],
            [
                -0.34392190291123537,
                1.223938038897442,
                0.9099951779012391,
                0.5000000000000001,
            ],
            [
                0.11268328259677719,
                1.5269880415085346,
                0.519949866494209,
                0.7071067811865476,
            ],
            [
                0.5032801294123179,
                0.9355491589854588,
                -0.1746750249510484,
                0.5000000000000001,
            ],
            [
                -0.0060801546046398165,
                0.13913531923012498,
                -0.4929190219247293,
                0.7071067811865476,
            ],
        ],
        [
            [
                -1.703002701750313,
                0.7735448152843192,
                1.4722476397469413,
                1.0,
            ],
            [
                -1.2766860833010154,
                1.4898602749997385,
                0.7337328701322507,
                0.7071067811865476,
            ],
            [
                -0.9583047415737298,
                1.8185917270335559,
                -0.04701522675232139,
                1.0,
            ],
            [
                -0.07856147911899733,
                1.082016809790626,
                -0.80022244144343,
                0.7071067811865476,
            ],
            [
                -0.008598637103206852,
                0.19676705546035284,
                -0.6970927659576331,
                1.0,
            ],
        ],
        [
            [
                -1.204204758786658,
                0.5469787844392374,
                1.0410362896509515,
                0.7071067811865476,
            ],
            [
                -1.4615848709860477,
                0.8830425680480978,
                0.127659798198727,
                0.5000000000000001,
            ],
            [
                -1.46793084501679,
                1.04488904328183,
                -0.5864394378053883,
                0.7071067811865476,
            ],
            [
                -0.6143828386624944,
                0.5946536881361144,
                -0.9570104046535606,
                0.5000000000000001,
            ],
            [
                -0.0060801546046398165,
                0.13913531923012498,
                -0.4929190219247293,
                0.7071067811865476,
            ],
        ],
        [
            [
                -1.703002701750313,
                0.7735448152843192,
                1.4722476397469413,
                1.0,
            ],
            [
                -1.9945118225934413,
                0.3059292853258852,
                0.4878416375011528,
                0.7071067811865476,
            ],
            [
                -1.9734636375015722,
                0.1442604645229919,
                -0.3947579428078579,
                1.0,
            ],
            [
                -0.7963872184114235,
                -0.10191417988322726,
                -1.046113674074528,
                0.7071067811865476,
            ],
            [
                -0.008598637103206852,
                0.19676705546035284,
                -0.6970927659576331,
                1.0,
            ],
        ],
        [
            [
                -1.204204758786658,
                0.5469787844392374,
                1.0410362896509515,
                0.7071067811865476,
            ],
            [
                -1.3590807988390778,
                -0.4503932236131225,
                0.5622524618457024,
                0.5000000000000001,
            ],
            [
                -1.322968195988075,
                -0.8408739378391721,
                0.02816740123201294,
                0.7071067811865476,
            ],
            [
                -0.5118787665155248,
                -0.7387821035251056,
                -0.522417741006585,
                0.5000000000000001,
            ],
            [
                -0.0060801546046398165,
                0.13913531923012498,
                -0.4929190219247293,
                0.7071067811865476,
            ],
        ],
        [
            [
                -1.703002701750313,
                0.7735448152843192,
                1.4722476397469413,
                1.0,
            ],
            [
                -1.1317234342723004,
                -0.39590270612126355,
                1.348339709169652,
                0.7071067811865476,
            ],
            [
                -0.7532965972797901,
                -0.8482798562888838,
                0.8221701005416294,
                1.0,
            ],
            [
                0.06640116990971762,
                -0.803746171330376,
                -0.18561560240602865,
                0.7071067811865476,
            ],
            [
                -0.008598637103206852,
                0.19676705546035284,
                -0.6970927659576331,
                1.0,
            ],
        ],
        [
            [
                -1.204204758786658,
                0.5469787844392374,
                1.0410362896509515,
                0.7071067811865476,
            ],
            [
                -0.2414178307642656,
                -0.1094977527637783,
                1.3445878415482146,
                0.5000000000000001,
            ],
            [
                0.2576459316254921,
                -0.3587749396124676,
                1.1345567055316106,
                0.7071067811865476,
            ],
            [
                0.6057842015592876,
                -0.39788663267576163,
                0.25991763869592716,
                0.5000000000000001,
            ],
            [
                -0.0060801546046398165,
                0.13913531923012498,
                -0.4929190219247293,
                0.7071067811865476,
            ],
        ],
        [
            [
                -1.703002701750313,
                0.7735448152843192,
                1.4722476397469413,
                1.0,
            ],
            [
                -0.41389769497987433,
                0.7880282835525899,
                1.5942309418007503,
                0.7071067811865476,
            ],
            [
                0.2618622986480523,
                0.8260514062216804,
                1.169912816597166,
                1.0,
            ],
            [
                0.7842269092021439,
                0.3801848183434774,
                0.06027563022506944,
                0.7071067811865476,
            ],
            [
                -0.008598637103206852,
                0.19676705546035284,
                -0.6970927659576331,
                1.0,
            ],
        ],
    ];
    /// `nlib.srf_nurb` case `gen1542` (tols [0.001, 0.5]): rows `[wx, wy, wz, w]`.
    // The weights are the generator's 1/sqrt(2), as the case has them.
    #[allow(clippy::approx_constant)]
    const GEN1542: [[[f64; 4]; 5]; 9] = [
        [
            [
                1.077244833001874,
                -1.5395190829686578,
                0.9352518981944178,
                1.0,
            ],
            [
                1.7337094283962637,
                -0.3885814088404935,
                1.4707584596208803,
                0.7071067811865476,
            ],
            [
                1.2858539145941201,
                1.1290816531093006,
                2.02837710590835,
                1.0,
            ],
            [
                0.08476261685327637,
                1.9853439956943004,
                1.397799953161796,
                0.7071067811865476,
            ],
            [
                -1.2547181115140909,
                1.8177184201864842,
                0.8320729888694953,
                1.0,
            ],
        ],
        [
            [
                0.7617271264137951,
                -1.088604383333233,
                0.6613229593308635,
                0.7071067811865476,
            ],
            [
                0.7434958877813607,
                -0.5838437568478878,
                1.8865797084223954,
                0.5000000000000001,
            ],
            [
                0.22698856229753112,
                0.3612829430262547,
                2.6315473569192878,
                0.7071067811865476,
            ],
            [
                -0.422485584476622,
                1.0947749947296834,
                1.834990253759934,
                0.5000000000000001,
            ],
            [
                -0.8872196851291925,
                1.2853210212015613,
                0.5883644528717789,
                0.7071067811865476,
            ],
        ],
        [
            [
                1.077244833001874,
                -1.5395190829686578,
                0.9352518981944178,
                1.0,
            ],
            [
                0.07947966608655625,
                -1.525702733733882,
                1.8585911098588126,
                0.7071067811865476,
            ],
            [
                -1.0535802505454896,
                -0.4790507466185918,
                2.576855299805935,
                1.0,
            ],
            [
                -1.569467145456431,
                0.8482226708009123,
                1.7856326033997283,
                0.7071067811865476,
            ],
            [
                -1.2547181115140909,
                1.8177184201864842,
                0.8320729888694953,
                1.0,
            ],
        ],
        [
            [
                0.7617271264137951,
                -1.088604383333233,
                0.6613229593308635,
                0.7071067811865476,
            ],
            [
                -0.6310946660688678,
                -1.5738257413482755,
                0.7418650460460011,
                0.5000000000000001,
            ],
            [
                -1.716976041667406,
                -1.0387630059592243,
                1.012676356339253,
                0.7071067811865476,
            ],
            [
                -1.7970761383268505,
                0.10479301022929591,
                0.6902755913835399,
                0.5000000000000001,
            ],
            [
                -0.8872196851291925,
                1.2853210212015613,
                0.5883644528717789,
                0.7071067811865476,
            ],
        ],
        [
            [
                1.077244833001874,
                -1.5395190829686578,
                0.9352518981944178,
                1.0,
            ],
            [
                -0.21025517556867354,
                -1.7886273578259726,
                -0.1481125409591538,
                0.7071067811865476,
            ],
            [
                -1.4633271931063367,
                -0.8508823158914741,
                -0.26105221884443697,
                1.0,
            ],
            [
                -1.8592019871116612,
                0.5852980467088217,
                -0.22107104741823824,
                0.7071067811865476,
            ],
            [
                -1.2547181115140909,
                1.8177184201864842,
                0.8320729888694953,
                1.0,
            ],
        ],
        [
            [
                0.7617271264137951,
                -1.088604383333233,
                0.6613229593308635,
                0.7071067811865476,
            ],
            [
                0.3337489452205133,
                -0.9556753261207703,
                -0.9513278102279767,
                0.5000000000000001,
            ],
            [
                -0.3524811210129288,
                -0.16456630515792675,
                -1.3818599447166455,
                0.7071067811865476,
            ],
            [
                -0.8322325270374694,
                0.722943425456801,
                -1.002917264890438,
                0.5000000000000001,
            ],
            [
                -0.8872196851291925,
                1.2853210212015613,
                0.5883644528717789,
                0.7071067811865476,
            ],
        ],
        [
            [
                1.077244833001874,
                -1.5395190829686578,
                0.9352518981944178,
                1.0,
            ],
            [
                1.4439745867410338,
                -0.6515060329325844,
                -0.5359451911970862,
                0.7071067811865476,
            ],
            [
                0.8761069720332727,
                0.7572500838364181,
                -0.8095304127420219,
                1.0,
            ],
            [
                -0.20497222480195365,
                1.7224193716022098,
                -0.6089036976561706,
                0.7071067811865476,
            ],
            [
                -1.2547181115140909,
                1.8177184201864842,
                0.8320729888694953,
                1.0,
            ],
        ],
        [
            [
                0.7617271264137951,
                -1.088604383333233,
                0.6613229593308635,
                0.7071067811865476,
            ],
            [
                1.7083394990707421,
                0.03430665837961711,
                0.19338685214841647,
                0.5000000000000001,
            ],
            [
                1.5914834829520086,
                1.2354796438275524,
                0.2370110558633886,
                0.7071067811865476,
            ],
            [
                0.5423580268127594,
                1.7129254099571884,
                0.14179739748595524,
                0.5000000000000001,
            ],
            [
                -0.8872196851291925,
                1.2853210212015613,
                0.5883644528717789,
                0.7071067811865476,
            ],
        ],
        [
            [
                1.077244833001874,
                -1.5395190829686578,
                0.9352518981944178,
                1.0,
            ],
            [
                1.7337094283962637,
                -0.3885814088404935,
                1.4707584596208803,
                0.7071067811865476,
            ],
            [
                1.2858539145941201,
                1.1290816531093006,
                2.02837710590835,
                1.0,
            ],
            [
                0.08476261685327637,
                1.9853439956943004,
                1.397799953161796,
                0.7071067811865476,
            ],
            [
                -1.2547181115140909,
                1.8177184201864842,
                0.8320729888694953,
                1.0,
            ],
        ],
    ];

    /// The gen0652 / gen1542 nets of `nlib.srf_nurb`: rational biquadratic spheres whose
    /// first and last u rows are the seam and whose v = 0 and v = 1 columns are poles.
    fn sphere(net: &[[[f64; 4]; 5]; 9]) -> Surface {
        let net = net
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                    .collect()
            })
            .collect();
        let u = vec![
            0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
        ];
        let v = vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
        Surface::new(net, 2, 2, u, v).unwrap()
    }

    fn pole_fix() -> UpstreamFixes {
        UpstreamFixes {
            closed_smooth_pole_tangent_limit: true,
            ..UpstreamFixes::default()
        }
    }

    /// #2521 (WR-725): at the poles `S_u` is exactly zero on both sides of the seam,
    /// so upstream fails the angle with `NL_GEO_ERR`; by its limit `S_uv` the seam is
    /// smooth there as everywhere else.
    #[test]
    fn closed_smooth_pole_of_sphere_is_smooth() {
        for (net, tols) in [(&GEN0652, [0.001, 1.0]), (&GEN1542, [0.001, 0.5])] {
            let s = sphere(net);
            let d = s
                .derivs(0.0, 0.0, Side::Left, Side::Left, true, 1, 1)
                .unwrap();
            assert_eq!(d[1][0], Vec3::ZERO);
            let upstream = s.is_closed_smooth(1, &tols, Direction::U);
            assert!(matches!(
                upstream,
                Err(SolidError::Nlib {
                    code: NlibCode::Geometry,
                    ..
                })
            ));
            let fixed = s.is_closed_smooth_with_fixes(&pole_fix(), 1, &tols, Direction::U);
            assert_eq!(fixed.unwrap(), Some(Direction::U));
            let flag = s.is_closed_smooth_flag_with_fixes(&pole_fix(), 1, &tols, Direction::UV);
            assert_eq!(flag.unwrap(), (Some(Direction::U), None));
        }
    }

    /// #2521 (WR-725): the gen1542 sphere turned about an axis leaves `S_u` at the pole
    /// as rounding noise (1.8e-15, opposite on the two sides), so upstream measures 180
    /// degrees and reports the seam not smooth; by its limit it is smooth.
    #[test]
    fn closed_smooth_pole_noise_does_not_decide() {
        let mut s = sphere(&GEN1542);
        s.rotate_at_pt(
            Vec3::new(0.8023059287187584, -0.8849950821048629, 1.3585098475056792),
            Vec3::new(0.6152973741302618, 0.6277831403618859, 0.47675724437681416),
            -158.12925138258618,
        )
        .unwrap();
        let tols = [0.001, 0.5];
        let d1 = s
            .derivs(0.0, 0.0, Side::Left, Side::Left, true, 1, 1)
            .unwrap();
        let d2 = s
            .derivs(1.0, 0.0, Side::Right, Side::Left, true, 1, 1)
            .unwrap();
        let (a, b) = (d1[1][0], d2[1][0]);
        assert!(a != Vec3::ZERO && a.length() < 1e-14 && b.length() < 1e-14);
        assert!(vectors_angle(a, b).unwrap() > 90.0);
        assert_eq!(s.is_closed_smooth(1, &tols, Direction::U).unwrap(), None);
        let fixed = s.is_closed_smooth_with_fixes(&pole_fix(), 1, &tols, Direction::U);
        assert_eq!(fixed.unwrap(), Some(Direction::U));
    }
}
