//! A rational patch on a sphere stored with its definition (opt-in, not upstream): the
//! sphere's centre and radius. Every point of its net is on that sphere, so its offset
//! is exact: the net scaled about the centre onto the concentric sphere.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/inc/SmBallPatch.h`,
//! `source/SMLib/src/SmBallPatch.cpp`, `SmSurfTypes.h`, `SmSurface::ReadFromDB`'s
//! dispatch and `SmDatabaseIO::IsValidType` (tangency-labs/smlib@f8e6709, smlib#79 step
//! 4c). The blend corner's Ball ([`crate::blend::blend_corner::ball`]) makes one under
//! [`crate::kernel::UpstreamFixes::fillet_blend_corner`] (WR-697), and only with that
//! flag does the file reader read one.
//!
//! | Rust                                  | Fork                                         |
//! |---------------------------------------|----------------------------------------------|
//! | [`BallPatch`]                         | `SmBallPatch`                                |
//! | [`BallPatch::create`]                 | `SmBallPatch::Create`                        |
//! | [`BallPatch::net`]                    | the `SmBSplineSurface` base                  |
//! | [`BallPatch::centre`], [`BallPatch::radius`] | `GetCentre`, `GetRadius`              |
//! | [`BallPatch::approx_eq`]              | `operator==`                                 |
//! | [`BallPatch::copy`]                   | `Copy`                                       |
//! | `normal_side` (private)               | `NormalSide`                                 |
//! | [`BallPatch::offset_collapses`]       | `OffsetCollapses`                            |
//! | [`BallPatch::create_offset_surface`]  | `CreateOffsetSurface`                        |
//! | [`BallPatch::transform`]              | `Transform`                                  |
//! | [`BallPatch::read_from_db`]           | `ReadFromDB` (the writer: `topology::geometry_write`) |
//! | [`BallPatch::is_kind_of`]             | `IsKindOf`                                   |
//! | [`BallPatch::is_on_sphere`], `binomial` (private) | `IsOnSphere`, `sm_Binomial` (efb8007; no SMLib caller) |
//! | [`BALL_PATCH_TYPE`]                   | `SmBallPatch_TYPE`                           |
//!
//! Deviations: out-params are return values. `SmDatabaseIO::IsValidType` is a debug-build
//! assertion only (not ported for any type). The fork's b439d73 notes that the shell's
//! exact offset of a Ball patch is later put on a tangent-extended B-spline
//! (`GetCreateExtendedSurface`), which no longer carries the sphere: so here too. No
//! recursion: the reader reads its net as a plain B-spline record, inside the surface
//! dispatcher's nesting count.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::p2::ControlPointForm;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use crate::nurbs::knots::Side;
use crate::surface::surface::Surface;
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};

const OP_CREATE: &str = "surface::ball_patch::BallPatch::create";
const OP_OFFSET: &str = "surface::ball_patch::BallPatch::create_offset_surface";
const OP_TRANSFORM: &str = "surface::ball_patch::BallPatch::transform";
const OP_ON_SPHERE: &str = "surface::ball_patch::BallPatch::is_on_sphere";
const OP_READ: &str = "surface::ball_patch::BallPatch::read_from_db";

/// `SURF_BASE_TYPE` (`SmTypes.h`).
const SURF_BASE_TYPE: i64 = 15000;

/// `SmBallPatch_TYPE` (`SURF_BASE_TYPE + 42`, the fork's `SmSurfTypes.h`).
pub const BALL_PATCH_TYPE: i64 = SURF_BASE_TYPE + 42;

/// `SmBallPatch`: a rational B-spline patch every point of which is on the sphere of
/// centre `C` and radius `R` (its net is exact, not an approximation); `(C, R)` is its
/// definition, recorded where it is made. Its offset by `d` along its normal is the same
/// patch scaled about `C` by `(R + s d) / R` (`s = +1` where its normal points away from
/// `C`, `-1` towards it), its weights unchanged; it collapses when `R + s d` is not
/// positive. A rigid or uniformly scaled transform moves `(C, R)` with the net.
#[derive(Debug, Clone)]
pub struct BallPatch {
    /// The net (the `SmBSplineSurface` base).
    pub(crate) net: BSplineSurface,
    /// `m_sCentre`.
    centre: Point3,
    /// `m_dRadius`.
    radius: f64,
}

/// `sm_Binomial` (the fork's efb8007): `n` choose `k` as a double, by its product.
#[allow(clippy::cast_precision_loss)] // degrees are small
fn binomial(n: usize, k: usize) -> f64 {
    let mut d = 1.0;
    for i in 1..=k {
        d = d * (n - k + i) as f64 / i as f64;
    }
    d
}

/// `true` when `x > 0` (`false` for NaN), the fork's `!( x > 0.0 )` refusals negated.
fn positive(x: f64) -> bool {
    x.partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater)
}

impl BallPatch {
    /// `SmBallPatch::Create`: the patch `net` (copied) on the sphere of centre `centre`
    /// and radius `radius`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] (`SM_ERR_INVALID_INPUT`, named) for a radius not
    /// finite and positive or a centre not finite, and for a net not rational or with a
    /// weight not positive; those of the net's control points.
    pub fn create(net: &BSplineSurface, centre: Point3, radius: f64) -> Result<Self> {
        if !radius.is_finite()
            || !positive(radius)
            || !centre.x.is_finite()
            || !centre.y.is_finite()
            || !centre.z.is_finite()
        {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "SmBallPatch: a sphere's centre is finite and its radius finite and positive",
            ));
        }
        let cpn = net.control_point_net()?;
        if !net.is_rational() || cpn.weights.len() != cpn.points.len() {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "SmBallPatch: a patch on a sphere is rational",
            ));
        }
        if cpn.weights.iter().any(|&w| !positive(w)) {
            return Err(SolidError::invalid_input(
                OP_CREATE,
                "SmBallPatch: a patch on a sphere has positive weights",
            ));
        }
        Ok(Self {
            net: net.copy(),
            centre,
            radius,
        })
    }

    /// `IsOnSphere` (the fork's efb8007): whether every point of `net` is on the sphere of
    /// centre `centre` and radius `radius`, decided on the net, not at sample points. The
    /// net is cut into Bezier pieces (every interior knot raised to the degree); over each,
    /// the Bernstein coefficients (degrees `2p`, `2q`) of the homogeneous identity
    /// `|X - C W|^2 - R^2 W^2` must all be zero within `rel_tol` of
    /// `((R + |C|) max W)^2`. `false` for a net that is not rational or a radius that is
    /// not positive. The fork's SMLib has no caller (only its capi's import does).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a cut net whose control net is not a whole number of Bezier pieces
    /// (or a degree 0); those of the knots, the knot insertion and the control net.
    pub fn is_on_sphere(
        net: &BSplineSurface,
        centre: Point3,
        radius: f64,
        rel_tol: f64,
    ) -> Result<bool> {
        let err = || SolidError::sm_err(OP_ON_SPHERE);
        if !net.is_rational() || !positive(radius) {
            return Ok(false);
        }
        // the net cut into Bezier pieces: every interior knot to the degree's multiplicity
        let mut cut = net.copy();
        let degrees = [cut.degree(SurfParamType::U), cut.degree(SurfParamType::V)];
        for (dir, &deg) in [SurfParamType::U, SurfParamType::V].iter().zip(&degrees) {
            let (knots, mults) = cut.knots(*dir, None)?;
            let mut add = Vec::new();
            let interior = knots.len().saturating_sub(1);
            for k in 1..interior {
                let (Some(&knot), Some(&mult)) = (knots.get(k), mults.get(k)) else {
                    return Err(err());
                };
                for _ in mult..deg {
                    add.push(knot);
                }
            }
            if !add.is_empty() {
                cut.insert_knots(*dir, &add)?;
            }
        }
        let cpn = cut.control_point_net()?;
        let (lu, lv) = (cpn.u_count, cpn.v_count);
        let [p, q] = degrees;
        let whole = |n: usize, d: usize| n.checked_sub(1).is_some_and(|m| m % d == 0);
        if p == 0
            || q == 0
            || !whole(lu, p)
            || !whole(lv, q)
            || Some(cpn.weights.len()) != lu.checked_mul(lv)
        {
            return Err(err());
        }
        let mut w_max = 0.0_f64;
        for &w in &cpn.weights {
            let aw = w.abs();
            w_max = if w_max > aw { w_max } else { aw };
        }
        let scale = (radius + centre.distance(Point3::new(0.0, 0.0, 0.0))) * w_max;
        let tol = rel_tol * scale * scale;
        let at = |k: usize| -> Result<(Point3, f64)> {
            match (cpn.points.get(k), cpn.weights.get(k)) {
                (Some(&pt), Some(&w)) => Ok((pt, w)),
                _ => Err(err()),
            }
        };
        // per piece: the homogeneous coordinates' Bernstein coefficients H = w (P - C), W = w
        let mut bu = 0;
        while bu + 1 < lu {
            let mut bv = 0;
            while bv + 1 < lv {
                // F = |H|^2 - R^2 W^2, of degree (2p, 2q)
                for i in 0..=2 * p {
                    for j in 0..=2 * q {
                        let mut f = 0.0;
                        for a in 0..=p {
                            if a > i || i - a > p {
                                continue;
                            }
                            for b in 0..=q {
                                if b > j || j - b > q {
                                    continue;
                                }
                                let k1 = (bu + a) * lv + (bv + b);
                                let k2 = (bu + i - a) * lv + (bv + j - b);
                                let (p1, w1) = at(k1)?;
                                let (p2, w2) = at(k2)?;
                                let h1 = (p1 - centre) * w1;
                                let h2 = (p2 - centre) * w2;
                                let c = binomial(p, a)
                                    * binomial(p, i - a)
                                    * binomial(q, b)
                                    * binomial(q, j - b);
                                f += c * (h1.dot(h2) - radius * radius * w1 * w2);
                            }
                        }
                        f /= binomial(2 * p, i) * binomial(2 * q, j);
                        if f.abs() > tol {
                            return Ok(false);
                        }
                    }
                }
                bv += q;
            }
            bu += p;
        }
        Ok(true)
    }

    /// The net (the `SmBSplineSurface` base): every evaluation of the patch is its net's.
    #[must_use]
    pub const fn net(&self) -> &BSplineSurface {
        &self.net
    }

    /// The net, mutably (the `SmBSplineSurface` base's inherited edits: they keep the
    /// sphere, as the fork's do).
    pub fn net_mut(&mut self) -> &mut BSplineSurface {
        &mut self.net
    }

    /// `GetCentre`.
    #[must_use]
    pub const fn centre(&self) -> Point3 {
        self.centre
    }

    /// `GetRadius`.
    #[must_use]
    pub const fn radius(&self) -> f64 {
        self.radius
    }

    /// `operator==` past `SmBSplineSurface::operator==` (the net's): the same centre and
    /// radius, compared exactly.
    #[must_use]
    pub fn approx_eq(&self, other: &Self) -> bool {
        self.net.approx_eq(&other.net)
            && self.radius == other.radius
            && self.centre.x == other.centre.x
            && self.centre.y == other.centre.y
            && self.centre.z == other.centre.z
    }

    /// `Copy`: a copy of the net with the same definition.
    #[must_use]
    pub fn copy(&self) -> Self {
        Self {
            net: self.net.copy(),
            centre: self.centre,
            radius: self.radius,
        }
    }

    /// An exact copy whose net shares nothing with this one's (an evaluation copy).
    #[must_use]
    pub(crate) fn clone_exact(&self) -> Self {
        Self {
            net: self.net.clone_exact(),
            centre: self.centre,
            radius: self.radius,
        }
    }

    /// `SmBallPatch_TYPE`.
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        BALL_PATCH_TYPE
    }

    /// `"SmBallPatch_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmBallPatch_TYPE"
    }

    /// `"SmBallPatch"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        "SmBallPatch"
    }

    /// `IsKindOf`: this type, or the net's (`SmBSplineSurface` and its bases).
    ///
    /// # Errors
    ///
    /// Those of the net's `IsKindOf`.
    pub fn is_kind_of(&self, t: i64) -> Result<bool> {
        if t == BALL_PATCH_TYPE {
            return Ok(true);
        }
        self.net.is_kind_of(t)
    }

    /// `NormalSide`: which way the patch's normal points at the middle of its domain,
    /// `+1` away from the centre, `-1` towards it (`SmBSplineSurface::EvaluatePoint` and
    /// `EvaluateNormal` from the left).
    fn normal_side(&self, k: &Kernel) -> Result<f64> {
        let mid = self.net.natural_uv_domain().evaluate(0.5, 0.5);
        let net = Surface::from(self.net.clone());
        let at = net.evaluate_point(k, mid)?;
        let normal = net.evaluate_normal(k, mid, Side::Left, Side::Left)?;
        Ok(if normal.dot(at - self.centre) >= 0.0 {
            1.0
        } else {
            -1.0
        })
    }

    /// `OffsetCollapses`: whether the offset by `signed_offset_distance` leaves a radius
    /// `R + s d` of `tol` or less; `false` where the normal's side cannot be evaluated.
    #[must_use]
    pub fn offset_collapses(&self, k: &Kernel, signed_offset_distance: f64, tol: f64) -> bool {
        let Ok(side) = self.normal_side(k) else {
            return false;
        };
        self.radius + side * signed_offset_distance <= tol
    }

    /// `CreateOffsetSurface`: the exact offset, the patch scaled about its centre onto the
    /// concentric sphere of radius `R + s d`, its weights unchanged (the approximation
    /// tolerance is not used).
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] where the offset passes the sphere's centre
    /// ([`Self::offset_collapses`]); `SM_ERR` for a control net of the wrong size; those
    /// of the evaluations, the control points and [`Self::create`].
    pub fn create_offset_surface(&self, k: &Kernel, signed_offset_distance: f64) -> Result<Self> {
        let side = self.normal_side(k)?;
        let new_radius = self.radius + side * signed_offset_distance;
        if !positive(new_radius) {
            return Err(SolidError::invalid_input(
                OP_OFFSET,
                "SmBallPatch: the offset passes the sphere's centre (SmSurface::OffsetCollapses)",
            ));
        }
        let scale = new_radius / self.radius;
        // the net's B-spline part, its control points scaled about the centre
        let mut net = self.net.copy();
        let cpn = self.net.control_point_net()?;
        let (u_count, v_count) = (cpn.u_count, cpn.v_count);
        let count = u_count
            .checked_mul(v_count)
            .ok_or_else(|| SolidError::sm_err(OP_OFFSET))?;
        if cpn.points.len() != count || cpn.weights.len() != cpn.points.len() {
            return Err(SolidError::sm_err(OP_OFFSET));
        }
        for ii in 0..u_count {
            for jj in 0..v_count {
                let at = ii * v_count + jj;
                let (Some(&p), Some(&w)) = (cpn.points.get(at), cpn.weights.get(at)) else {
                    return Err(SolidError::sm_err(OP_OFFSET));
                };
                let scaled = self.centre + (p - self.centre) * scale;
                net.set_control_point(ControlPointForm::EuclideanRational, ii, jj, scaled, w)?;
            }
        }
        Self::create(&net, self.centre, new_radius)
    }

    /// `Transform`: a rigid motion, with at most a uniform scale; the centre moves as the
    /// net's points (a point curve at it, transformed alike) and the radius scales.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a scale that is not uniform and positive; those of the point curve's
    /// and the net's transforms.
    pub fn transform(&mut self, rotate_n_move: &Axis2Placement, scale: Option<Vec3>) -> Result<()> {
        let mut factor = 1.0;
        if let Some(sc) = scale {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-refusing test
            if !are_same(sc.x, sc.y) || !are_same(sc.x, sc.z) || !(sc.x > EFF_ZERO) {
                return Err(SolidError::sm_err(OP_TRANSFORM));
            }
            factor = sc.x;
        }
        // the centre as the net's points move: a point curve at it, transformed alike
        let mut centre = BSplineCurve::create_point_curve(self.centre)?;
        centre.transform(rotate_n_move, scale)?;
        self.centre = centre.evaluate_point(centre.natural_interval().min())?;
        self.radius *= factor;
        self.net.transform(rotate_n_move, scale)
    }

    /// `ReadFromDB`: the definition (the centre and the radius), then the net's
    /// `SmBSplineSurface` record, made a patch by [`Self::create`].
    ///
    /// # Errors
    ///
    /// `SM_ERR` for an ASCII definition that could not be read; those of the binary
    /// reads, the net's reader and [`Self::create`] (a bad definition is refused).
    pub fn read_from_db(db: &mut DatabaseReader, version: u64) -> Result<Self> {
        let mut values = [0.0_f64; 4];
        if db.file_type() == FileType::Ascii {
            let st = db
                .in_stream_mut()
                .ok_or_else(|| SolidError::null_pointer(OP_READ))?;
            let mut read = true;
            for v in &mut values {
                match st.extract_double() {
                    Some(x) => *v = x,
                    None => read = false,
                }
            }
            db.go_to_next_line();
            if !read || db.in_stream().is_none_or(InputStream::fail) {
                // "its definition could not be read"
                return Err(SolidError::sm_err(OP_READ));
            }
        } else {
            let v = db.read_doubles(4)?;
            for (x, &y) in values.iter_mut().zip(&v) {
                *x = y;
            }
        }
        let net = BSplineSurface::read_from_db(db, version)?;
        let [x, y, z, radius] = values;
        Self::create(&net, Point3::new(x, y, z), radius)
    }
}
