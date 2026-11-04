//! A rational patch on a sphere stored with its definition (not upstream): the
//! sphere's centre and radius. Every point of its net is on that sphere, so its offset
//! is exact: the net scaled about the centre onto the concentric sphere.
//!
//! Not in the pinned upstream: the SolidCraft fork's `source/SMLib/inc/SmBallPatch.h`,
//! `source/SMLib/src/SmBallPatch.cpp`, `SmSurfTypes.h`, `SmSurface::ReadFromDB`'s
//! dispatch and `SmDatabaseIO::IsValidType` (tangency-labs/smlib@f8e6709, smlib#79 step
//! 4c). The blend corner's Ball ([`crate::blend::blend_corner::ball`]) makes one (WR-697),
//! and the file reader reads one.
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
//! | [`BallPatch::is_on_sphere`]           | `IsOnSphere` (efb8007; no SMLib caller), by [`crate::eval::sphere`] |
//! | [`BallPatch::is_curve_on_sphere`]     | not in the fork: `IsOnSphere`'s certificate on a curve (#2357) |
//! | [`BallPatch::move_centre`]            | `Transform`'s centre                         |
//! | [`BallPatch::create_certified`]       | the capi import's ball patch (`smlib_flat.cpp` `FlatToBrepData`, bc814a5, smlib#102) |
//! | [`ON_SPHERE_REL_TOL`]                 | `kBallPatchRelTol` (`smlib_flat.cpp`, bc814a5) |
//! | [`BALL_PATCH_TYPE`]                   | `SmBallPatch_TYPE`                           |
//!
//! Deviations: out-params are return values. `IsOnSphere`'s tolerance is not the fork's
//! `rel_tol ((R + |C|) max W)^2`, which grows with the centre's distance from the origin
//! and admits a net millimetres off a sphere far from it: here it is `rel_tol (R max W)^2`
//! plus a forward error bound of computing each coefficient, the one certificate of
//! [`crate::eval::sphere`] (#2357). `SmDatabaseIO::IsValidType` is a debug-build
//! assertion only (not ported for any type). The fork's b439d73 notes that the shell's
//! exact offset of a Ball patch is later put on a tangent-extended B-spline
//! (`GetCreateExtendedSurface`), which no longer carries the sphere: so here too, unless
//! `offset_extends_ball_patches_as_themselves` extends it as a ball patch (WR-728). No
//! recursion: the reader reads its net as a plain B-spline record, inside the surface
//! dispatcher's nesting count.

use crate::error::{Result, SolidError};
use crate::eval;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::tol::are_same;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::p2::ControlPointForm;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p2::EditNotify;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::surface::surface::Surface;
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};

const OP_CREATE: &str = "surface::ball_patch::BallPatch::create";
const OP_OFFSET: &str = "surface::ball_patch::BallPatch::create_offset_surface";
const OP_TRANSFORM: &str = "surface::ball_patch::BallPatch::transform";
const OP_ON_SPHERE: &str = "surface::ball_patch::BallPatch::is_on_sphere";
const OP_CURVE_ON_SPHERE: &str = "surface::ball_patch::BallPatch::is_curve_on_sphere";
const OP_CERTIFIED: &str = "surface::ball_patch::BallPatch::create_certified";
const OP_READ: &str = "surface::ball_patch::BallPatch::read_from_db";

/// `SURF_BASE_TYPE` (`SmTypes.h`).
const SURF_BASE_TYPE: i64 = 15000;

/// `SmBallPatch_TYPE` (`SURF_BASE_TYPE + 42`, the fork's `SmSurfTypes.h`).
pub const BALL_PATCH_TYPE: i64 = SURF_BASE_TYPE + 42;

/// `kBallPatchRelTol` (the fork's capi `smlib_flat.cpp`, bc814a5): a net is on its
/// definition's sphere when `|X - C W|^2 - R^2 W^2` is zero to this much of the sphere's
/// size squared, `(R max W)^2`, past the rounding of computing it
/// ([`BallPatch::is_on_sphere`]).
pub const ON_SPHERE_REL_TOL: f64 = eval::sphere::ON_SPHERE_REL_TOL;

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

/// A point's coordinates.
const fn xyz(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// A NURBS surface's form for the evaluator: its knots and homogeneous control points as
/// stored (weight 1 where it is not rational); `None` without NURB data.
pub(crate) fn surface_form(net: &BSplineSurface) -> Option<eval::Surface> {
    let n = net.gw_nurb()?;
    let w = |c: &CPoint| if c.w == NOW { 1.0 } else { c.w };
    Some(eval::Surface::RationalSpline {
        u_knots: n.u_knots().to_vec(),
        v_knots: n.v_knots().to_vec(),
        homogeneous: n
            .rows()
            .map(|r| r.iter().map(|c| [c.x, c.y, c.z, w(c)]).collect())
            .collect(),
    })
}

/// A NURBS curve's form for the evaluator, as [`surface_form`] (z 0 for a 2D curve).
pub(crate) fn curve_form(c: &BSplineCurve) -> Option<eval::Curve> {
    let n = c.nurb.as_ref()?;
    Some(eval::Curve::RationalSpline {
        knots: n.knots().to_vec(),
        homogeneous: n
            .cpts()
            .iter()
            .map(|p| {
                let w = if p.w == NOW { 1.0 } else { p.w };
                [p.x, p.y, if p.z == NOZ { 0.0 } else { p.z }, w]
            })
            .collect(),
    })
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

    /// A patch from data that carries a net and its definition (the fork's capi import of
    /// an `SMLIB_FLAT_SURFACE_BALL_PATCH` face, `FlatToBrepData`, bc814a5): `net` is taken
    /// as the patch on the sphere of centre `centre` and radius `radius` only when
    /// [`Self::is_on_sphere`] certifies it at [`ON_SPHERE_REL_TOL`]; then [`Self::create`].
    /// An importer calls this, never [`Self::create`], for a definition it did not make.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] (`SM_ERR_INVALID_INPUT`, as the import sets it for every
    /// failure) for a net not certified on the sphere, and for any error of the certificate
    /// or of [`Self::create`].
    pub fn create_certified(net: &BSplineSurface, centre: Point3, radius: f64) -> Result<Self> {
        let refused = |reason: &'static str| SolidError::invalid_input(OP_CERTIFIED, reason);
        match Self::is_on_sphere(net, centre, radius, ON_SPHERE_REL_TOL) {
            Ok(true) => {}
            Ok(false) => {
                return Err(refused(
                    "SmBallPatch: the net is not on its definition's sphere",
                ))
            }
            Err(_) => return Err(refused("SmBallPatch: the net's sphere certificate failed")),
        }
        Self::create(net, centre, radius)
            .map_err(|_| refused("SmBallPatch: the definition is not a ball patch's"))
    }

    /// `IsOnSphere` (the fork's efb8007): whether every point of `net` is on the sphere of
    /// centre `centre` and radius `radius`, decided on the net, not at sample points, by the
    /// one certificate [`eval::sphere::surface_on_sphere`] (its tolerance `rel_tol (R max W)^2`
    /// past the rounding of computing each coefficient, not the fork's). `false` for a net the
    /// certificate refuses: off the sphere, not finite, or not rational; a radius that is not
    /// finite and positive.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a net without NURB data.
    pub fn is_on_sphere(
        net: &BSplineSurface,
        centre: Point3,
        radius: f64,
        rel_tol: f64,
    ) -> Result<bool> {
        if !net.is_rational() {
            return Ok(false);
        }
        let form = surface_form(net).ok_or_else(|| SolidError::sm_err(OP_ON_SPHERE))?;
        Ok(eval::sphere::surface_on_sphere(&form, xyz(centre), radius, rel_tol).is_ok())
    }

    /// Whether every point of `curve` is on the sphere of centre `centre` and radius
    /// `radius`: [`Self::is_on_sphere`]'s certificate on a curve (not in the fork; the Ball's
    /// loop, #2357), [`eval::sphere::curve_on_sphere`].
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a curve without NURB data.
    pub fn is_curve_on_sphere(
        curve: &BSplineCurve,
        centre: Point3,
        radius: f64,
        rel_tol: f64,
    ) -> Result<bool> {
        let form = curve_form(curve).ok_or_else(|| SolidError::sm_err(OP_CURVE_ON_SPHERE))?;
        Ok(eval::sphere::curve_on_sphere(&form, xyz(centre), radius, rel_tol).is_ok())
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
        self.transform_notifying(rotate_n_move, scale, &mut |_| Ok(()))
    }

    /// [`BallPatch::transform`] with the edit notifications of the net's transform
    /// ([`BSplineSurface::transform_notifying`]) sent through `notify`.
    ///
    /// # Errors
    ///
    /// Those of [`BallPatch::transform`] and of `notify`.
    pub(crate) fn transform_notifying(
        &mut self,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
        notify: &mut EditNotify<'_>,
    ) -> Result<()> {
        let mut factor = 1.0;
        if let Some(sc) = scale {
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // the fork's NaN-refusing test
            if !are_same(sc.x, sc.y) || !are_same(sc.x, sc.z) || !(sc.x > EFF_ZERO) {
                return Err(SolidError::sm_err(OP_TRANSFORM));
            }
            factor = sc.x;
        }
        self.centre = Self::move_centre(self.centre, rotate_n_move, scale)?;
        self.radius *= factor;
        self.net.transform_notifying(rotate_n_move, scale, notify)
    }

    /// `Transform`'s centre: `centre` moved as a net's points move (a point curve at it,
    /// transformed alike), so a definition that keeps the sphere moves it to the same bits
    /// as the patch's own.
    ///
    /// # Errors
    ///
    /// Those of the point curve's transform.
    pub fn move_centre(
        centre: Point3,
        rotate_n_move: &Axis2Placement,
        scale: Option<Vec3>,
    ) -> Result<Point3> {
        let mut c = BSplineCurve::create_point_curve(centre)?;
        c.transform(rotate_n_move, scale)?;
        c.evaluate_point(c.natural_interval().min())
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
