//! The tangent field of a surface boundary: cross-boundary tangents interpolated
//! between a start and an end vector and projected onto the tangent plane of a
//! surface along a parameter-space curve (`SmTangentField`, a curve on a surface).
//!
//! Upstream: `source/SMLib/src/SmTangentField.cpp`, `source/SMLib/inc/SmTangentField.h`.
//!
//! | Rust                                        | Upstream                                     |
//! |---------------------------------------------|----------------------------------------------|
//! | [`TangentField`]                            | `SmTangentField`                             |
//! | [`TangentDirType`]                          | `SmTangentDirType`                           |
//! | [`TangentField::new`]                       | `SmTangentField::SmTangentField` (data constructor) |
//! | `Drop` (implicit)                           | `SmTangentField::~SmTangentField`            |
//! | [`TangentField::approx_eq`]                 | `SmTangentField::operator==`                 |
//! | [`TangentField::create`], [`CurveEnd`]      | `SmTangentField::Create`                     |
//! | [`TangentField::evaluate_point`]            | `SmTangentField::EvaluatePoint`              |
//! | [`TangentField::evaluate`]                  | `SmTangentField::Evaluate`                   |
//! | [`read_from_db`], [`TangentFieldRecord`]    | `SmTangentField::ReadFromDB`                 |
//! | [`is_kind_of`], [`TangentField::is_kind_of`] | `SmTangentField::IsKindOf`                  |
//! | [`TYPE`], [`TYPE_STRING`], [`CLASS_STRING`] | `GetType`, `GetTypeString`, `GetClassString` (`SM_COMMON`) |
//!
//! Deviations:
//! - **Base class.** `SmTangentField` derives from `SmCrvOnSurf` (unit
//!   `curve.crv_on_surf`, not ported). The members it needs are kept here: the UV
//!   curve, the surface and the UV domain. Like the constructor (owner flag `0`, no
//!   context), the tangent field references both the UV curve and the surface without
//!   owning them, so they are borrowed (`&'a`). The surface type is a parameter `S`:
//!   `SmSurface` is not ported, and every surface call is
//!   [`SolidError::Unported`]. Three trivial members of the base are co-ported
//!   (`// CO-PORT(curve.crv_on_surf)`): `SetAll` for a given UV domain (it only stores
//!   the references; the context and owner bookkeeping is not modelled),
//!   `GetNaturalInterval` (the UV curve's interval), `IsKindOf`, and `operator==`
//!   (same UV curve, same surface, same UV domain).
//! - **Unported callees.** Without a UV domain the constructor asks
//!   `SmSurface::GetNaturalUVDomain` (`surface.surface_p2`), so [`TangentField::create`]
//!   (which passes none) always ends in [`SolidError::Unported`] after its checks.
//!   Comparing two different surfaces needs `SmSurface::operator==`
//!   (`surface.surface_p1`); evaluating inside the interval needs
//!   `SmSurface::EvaluateNormal` (`surface.surface_p7`). Reading the `SmCrvOnSurf`
//!   part of a database record needs `SmCrvOnSurf::ReadFromDB` (`curve.crv_on_surf`).
//! - **Arguments.** Out-params become return values; the `SmContext` of `Create` and
//!   `ReadFromDB`, `ReadFromDB`'s type and the `NotUsed` arguments of `Evaluate`
//!   (`bFromLeft`, `bNonZeroTangents`) are dropped. `Create`'s base UV curve is typed
//!   `SmBSplineCurve` upstream; here it is any [`Curve`]. Equality compares two tangent
//!   fields (upstream's `SmCurve::operator==` type and dimension check is always true
//!   between them).
//! - **`ReadFromDB`** returns the tangent-field members it read ([`TangentFieldRecord`])
//!   instead of an object; the `SmCrvOnSurf` part is unported. Upstream ignores the
//!   status of that base read; a read error of the members themselves is returned as
//!   upstream (`SER`). The empty constructor (`SmTangentField()`, for I/O) has no
//!   counterpart.

use crate::curve::curve::p2::{self as curve_base, Curve, CRV_ON_SURF_TYPE, TANGENT_FIELD_TYPE};
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::math::{EFF_ZERO, PI};
use crate::topology::database_io::{DatabaseReader, FileType};

/// `SmTangentField_TYPE` (`GetType`).
pub const TYPE: i64 = TANGENT_FIELD_TYPE;
/// `SmTangentField::GetTypeString`.
pub const TYPE_STRING: &str = "SmTangentField_TYPE";
/// `SmTangentField::GetClassString`.
pub const CLASS_STRING: &str = "SmTangentField";

const OP_CREATE: &str = "curve::tangent_field::TangentField::create";
const OP_EVALUATE: &str = "curve::tangent_field::TangentField::evaluate";

/// `SmTangentField::IsKindOf`: `SmTangentField_TYPE` or a base type
/// (`SmCrvOnSurf`, `SmCurve`, ...).
#[must_use]
pub const fn is_kind_of(t: i64) -> bool {
    // CO-PORT(curve.crv_on_surf): SmCrvOnSurf::IsKindOf
    t == TANGENT_FIELD_TYPE || t == CRV_ON_SURF_TYPE || curve_base::is_kind_of(t)
}

/// How the tangents vary along the field (`SmTangentDirType`). Upstream only
/// supports [`TangentDirType::Interpolate`]; the stored type is not used otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TangentDirType {
    /// `SM_CONSTANT_DIR` (`0`).
    Constant,
    /// `SM_INTERPOLATE_DIR` (`1`).
    Interpolate,
    /// `SM_PERPENDICULAR_DIR` (`2`).
    Perpendicular,
}

impl TangentDirType {
    /// The upstream enum value.
    #[must_use]
    pub const fn code(self) -> u64 {
        match self {
            Self::Constant => 0,
            Self::Interpolate => 1,
            Self::Perpendicular => 2,
        }
    }
}

/// Orientation of an end curve's tangent (`SmOrientType`, `SM_OT_SAME` /
/// `SM_OT_OPPOSITE`) and where it is taken, for [`TangentField::create`].
#[derive(Debug, Clone, Copy)]
pub struct CurveEnd<'c> {
    /// The 3D curve meeting the base curve at this end.
    pub curve: &'c Curve,
    /// Parameter of the mated point on `curve`.
    pub param: f64,
    /// `true` (`SM_OT_OPPOSITE`): the field value is minus the curve's tangent.
    pub opposite: bool,
}

/// A tangent field (`SmTangentField`): the curve on `surface` traced by `uv_curve`,
/// whose value at a parameter is the cosine-blended interpolation of the start and
/// end tangents projected onto the surface's tangent plane.
#[derive(Debug)]
pub struct TangentField<'a, S> {
    /// `m_pUVCurve`: the parameter-space curve (not owned).
    uv_curve: &'a Curve,
    /// `m_pSurface`: the base surface (not owned).
    surface: &'a S,
    /// `m_vUVDomain`: the domain of interest on the surface.
    uv_domain: Extent2d,
    /// `m_eType`.
    dir_type: TangentDirType,
    /// `m_vStartTangentField`: the cross tangent at the start point.
    start_tangent: Vec3,
    /// `m_vEndTangentField`: the cross tangent at the end point.
    end_tangent: Vec3,
}

impl<S> Clone for TangentField<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for TangentField<'_, S> {}

impl<'a, S> TangentField<'a, S> {
    /// The data constructor: the field of `start_tangent` .. `end_tangent` along
    /// `uv_curve` on `surface`, restricted to `uv_domain` (`None` = the surface's
    /// natural UV domain).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] when `uv_domain` is `None`
    /// (`SmSurface::GetNaturalUVDomain`).
    pub fn new(
        uv_curve: &'a Curve,
        surface: &'a S,
        dir_type: TangentDirType,
        start_tangent: Vec3,
        end_tangent: Vec3,
        uv_domain: Option<Extent2d>,
    ) -> Result<Self> {
        // CO-PORT(curve.crv_on_surf): SmCrvOnSurf::SetAll (owner flag 0, no context)
        // set m_vUVDomain
        let uv_domain = match uv_domain {
            Some(d) => d,
            None => surface_natural_uv_domain(surface)?,
        };
        Ok(Self {
            uv_curve,
            surface,
            uv_domain,
            dir_type,
            start_tangent,
            end_tangent,
        })
    }

    /// The convenience constructor: the start and end tangents are those of
    /// `start` and `end` (negated when `opposite`), whose points must lie within
    /// `approx_tol_3d` of the ends of `base_3d_curve` (only used for this check).
    /// The field runs along `base_uv_curve` on `surface`, interpolated.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when an end point does not match; those of the curve evaluations;
    /// [`SolidError::Unported`] from the constructor (no UV domain is given).
    pub fn create(
        approx_tol_3d: f64,
        base_3d_curve: &Curve,
        base_uv_curve: &'a Curve,
        surface: &'a S,
        start: CurveEnd<'_>,
        end: CurveEnd<'_>,
    ) -> Result<Self> {
        // StartCurve position and tangent
        let pv = start.curve.evaluate(start.param, 1, true, true)?;
        let mut start_tangent = at(&pv, 1);
        if start.opposite {
            start_tangent = -start_tangent;
        }

        // Base3DCurve start position
        let ivl = base_3d_curve.natural_interval();
        let start_pnt = base_3d_curve.evaluate_point(ivl.evaluate(0.0))?;

        // check state: Start points must match
        let dist = at(&pv, 0).distance(start_pnt);
        if dist > approx_tol_3d {
            // Ends don't match
            return Err(SolidError::sm_err(OP_CREATE));
        }

        // EndCurve position and tangent
        let pv = end.curve.evaluate(end.param, 1, true, true)?;
        let mut end_tangent = at(&pv, 1);
        if end.opposite {
            end_tangent = -end_tangent;
        }

        // base3DCurve end position
        let end_pnt = base_3d_curve.evaluate_point(ivl.evaluate(1.0))?;

        // check state: End points must match
        let dist = at(&pv, 0).distance(end_pnt);
        if dist > approx_tol_3d {
            // Ends don't match
            return Err(SolidError::sm_err(OP_CREATE));
        }

        // allocate SmTangentField
        Self::new(
            base_uv_curve,
            surface,
            TangentDirType::Interpolate, // for now, only value supported
            start_tangent,
            end_tangent,
            None,
        )
    }

    /// The parameter-space curve (`GetUVCurve`).
    #[must_use]
    pub const fn uv_curve(&self) -> &'a Curve {
        self.uv_curve
    }

    /// The base surface (`GetBaseSurface`).
    #[must_use]
    pub const fn surface(&self) -> &'a S {
        self.surface
    }

    /// The domain of interest on the surface (`GetUVDomain`).
    #[must_use]
    pub const fn uv_domain(&self) -> Extent2d {
        self.uv_domain
    }

    /// The interpolation type (`m_eType`).
    #[must_use]
    pub const fn dir_type(&self) -> TangentDirType {
        self.dir_type
    }

    /// The stored start tangent (`m_vStartTangentField`).
    #[must_use]
    pub const fn start_tangent(&self) -> Vec3 {
        self.start_tangent
    }

    /// The stored end tangent (`m_vEndTangentField`).
    #[must_use]
    pub const fn end_tangent(&self) -> Vec3 {
        self.end_tangent
    }

    /// `SmTangentField_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        TYPE
    }

    /// `"SmTangentField_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        TYPE_STRING
    }

    /// `"SmTangentField"` (`GetClassString`).
    #[must_use]
    pub const fn class_string(&self) -> &'static str {
        CLASS_STRING
    }

    /// Whether the field is of type `t` or derives from it (`IsKindOf`).
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        is_kind_of(t)
    }

    /// The natural parameter interval: the UV curve's (`GetNaturalInterval`).
    #[must_use]
    pub fn natural_interval(&self) -> crate::geom::extent1d::Extent1d {
        // CO-PORT(curve.crv_on_surf): SmCrvOnSurf::GetNaturalInterval
        self.uv_curve.natural_interval()
    }

    /// `operator==`: the same UV curve (object or value), the same surface object
    /// (or equal surfaces), the same UV domain, then the same type and end tangents
    /// (upstream's tolerant vector equality).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] when the surfaces are different objects
    /// (`SmSurface::operator==`).
    pub fn approx_eq(&self, other: &Self) -> Result<bool> {
        // low work
        if core::ptr::eq(self, other) {
            return Ok(true);
        }

        // first check the base
        // CO-PORT(curve.crv_on_surf): SmCrvOnSurf::operator==
        // (SmCurve::operator==: same type, both of dimension 3)
        let mut rtn = true;
        rtn &=
            core::ptr::eq(self.uv_curve, other.uv_curve) || self.uv_curve.approx_eq(other.uv_curve);
        rtn &=
            core::ptr::eq(self.surface, other.surface) || surface_eq(self.surface, other.surface)?;
        rtn &= self.uv_domain.are_same(&other.uv_domain);

        if rtn {
            // check equivalence of these objects
            rtn = self.dir_type == other.dir_type
                && self.start_tangent.approx_eq(other.start_tangent)
                && self.end_tangent.approx_eq(other.end_tangent);
        }

        // all done
        Ok(rtn)
    }

    /// The field value at `t` (`EvaluatePoint`): the stored start (end) tangent
    /// within `1e-8` of the start (end) of the interval; elsewhere the tangents
    /// blended by `T = 0.5 cos(pi s) + 0.5` (`s` the normalised parameter, so the
    /// blend has zero slope at both ends) as `T start + (1 - T) end`, projected onto
    /// the tangent plane of the surface at `uv_curve(t)`.
    ///
    /// # Errors
    ///
    /// Those of the UV curve's evaluation; [`SolidError::Unported`] inside the
    /// interval (`SmSurface::EvaluateNormal`).
    pub fn evaluate_point(&self, t: f64) -> Result<Point3> {
        // locals
        let param_tol = 1.0e-8;
        let ivl = self.natural_interval();

        // Start Param Value - return stored StartTangent
        if t < ivl.min() + param_tol + EFF_ZERO {
            return Ok(self.start_tangent);
        }

        // End Param Value - return stored EndTangent
        if t > ivl.max() - param_tol - EFF_ZERO {
            return Ok(self.end_tangent);
        }

        // linear normalized parameter
        let mut dt = (t - ivl.min()) / ivl.length();

        // Cosine smoothed normalized parameter - dT/dParameter at 0 and 1 = 0
        // This yields a vanishing twist vector at the ends of the tangent interpolation.
        // note: dT = 1.0 at minParam and dT = 0.0 at maxParam
        dt *= PI;
        dt = 0.5 * dt.cos() + 0.5;

        // sTangentField(dParam) value is an interpolated value of the stored endTangentField values
        let tangent_field = self.start_tangent * dt + self.end_tangent * (1.0 - dt);

        // Get NormalPlane to m_pSurface at the m_pUVCurve(dParameter) point
        let pnt = self.uv_curve.evaluate_point(t)?;
        let uv = Point2::new(pnt.x, pnt.y);

        let unit_normal = surface_evaluate_normal(self.surface, uv)?;

        // return value = sTangentField projection onto the tangent plane
        Ok(tangent_field.project_to_plane(unit_normal))
    }

    /// The field value at `t` and, for `num_derivs == 1`, its derivative by a forward
    /// difference of step `1e-8` (`Evaluate`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for `num_derivs > 1`; those of [`TangentField::evaluate_point`].
    pub fn evaluate(&self, t: f64, num_derivs: usize) -> Result<Vec<Vec3>> {
        if num_derivs == 0 {
            return Ok(vec![self.evaluate_point(t)?]);
        }

        // get 1st derivative using finite differences
        if num_derivs == 1 {
            // Currently just use divided diff.
            let p0 = self.evaluate_point(t)?;
            let delta_t = 1.0e-8;
            let param1 = t + delta_t;
            let tangent1 = self.evaluate_point(param1)?;
            let diff = tangent1 - p0;
            return Ok(vec![p0, diff / delta_t]);
        }
        // Can not go higher than 1st derivative
        Err(SolidError::sm_err(OP_EVALUATE))
    }
}

/// The members of a tangent field read by [`read_from_db`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TangentFieldRecord {
    /// The raw `SmTangentDirType` value (upstream casts whatever was stored).
    pub dir_type: u64,
    /// `m_vStartTangentField`.
    pub start_tangent: Vec3,
    /// `m_vEndTangentField`.
    pub end_tangent: Vec3,
}

/// `SmTangentField::ReadFromDB`: reads the tangent-field members (type, start and
/// end tangents) from `db`, then the `SmCrvOnSurf` part (image dimension `dim`,
/// database version `version`). ASCII values that fail to parse keep their previous
/// value (`0`, [`Vec3::UNDEF`]; upstream leaves them uninitialised).
///
/// # Errors
///
/// Those of the binary reads; [`SolidError::Unported`] for the `SmCrvOnSurf` part.
pub fn read_from_db(db: &mut DatabaseReader, dim: u64, version: u64) -> Result<TangentFieldRecord> {
    let mut rec = TangentFieldRecord {
        dir_type: 0,
        start_tangent: Vec3::UNDEF,
        end_tangent: Vec3::UNDEF,
    };
    if db.file_type() == FileType::Ascii {
        if let Some(s) = db.in_stream_mut() {
            if let Some(v) = s.extract_ulong() {
                rec.dir_type = v;
            }
        }
        db.go_to_next_line();
        for v in [&mut rec.start_tangent, &mut rec.end_tangent] {
            if let Some(s) = db.in_stream_mut() {
                for coord in [&mut v.x, &mut v.y, &mut v.z] {
                    if let Some(x) = s.extract_double() {
                        *coord = x;
                    }
                }
            }
            db.go_to_next_line();
        }
    } else {
        rec.dir_type = db.read_ulong()?;

        rec.start_tangent.x = db.read_double()?;
        rec.start_tangent.y = db.read_double()?;
        rec.start_tangent.z = db.read_double()?;

        rec.end_tangent.x = db.read_double()?;
        rec.end_tangent.y = db.read_double()?;
        rec.end_tangent.z = db.read_double()?;
    }

    // read the base type
    read_crv_on_surf(db, dim, version)?;

    // all done
    Ok(rec)
}

// ---- unported callees -------------------------------------------------------------

/// `SmCrvOnSurf::ReadFromDB` into the tangent field.
fn read_crv_on_surf(_db: &mut DatabaseReader, _dim: u64, _version: u64) -> Result<()> {
    // UNPORTED(curve.crv_on_surf): SmCrvOnSurf::ReadFromDB
    Err(SolidError::Unported {
        upstream: "SmCrvOnSurf::ReadFromDB",
        unit: "curve.crv_on_surf",
    })
}

/// `m_pSurface->GetNaturalUVDomain()`.
fn surface_natural_uv_domain<S>(_surface: &S) -> Result<Extent2d> {
    // UNPORTED(surface.surface_p2): SmSurface::GetNaturalUVDomain
    Err(SolidError::Unported {
        upstream: "SmSurface::GetNaturalUVDomain",
        unit: "surface.surface_p2",
    })
}

/// `*m_pSurface == *rOther.m_pSurface`.
fn surface_eq<S>(_a: &S, _b: &S) -> Result<bool> {
    // UNPORTED(surface.surface_p1): SmSurface::operator==
    Err(SolidError::Unported {
        upstream: "SmSurface::operator==",
        unit: "surface.surface_p1",
    })
}

/// `m_pSurface->EvaluateNormal(uv, TRUE, TRUE, normal)`.
fn surface_evaluate_normal<S>(_surface: &S, _uv: Point2) -> Result<Vec3> {
    // UNPORTED(surface.surface_p7): SmSurface::EvaluateNormal
    Err(SolidError::Unported {
        upstream: "SmSurface::EvaluateNormal",
        unit: "surface.surface_p7",
    })
}

/// Entry `i` of an evaluation, or `UNDEF` (an unwritten `SmVector3d`).
fn at(v: &[Vec3], i: usize) -> Vec3 {
    v.get(i).copied().unwrap_or(Vec3::UNDEF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::hermite_curve::HermiteCurve;

    fn hermite(p1: [f64; 3], d1: [f64; 3], p2: [f64; 3], d2: [f64; 3], dim: u64) -> Curve {
        Curve::from(HermiteCurve::new(
            Vec3::from_array(p1),
            Vec3::from_array(d1),
            Vec3::from_array(p2),
            Vec3::from_array(d2),
            dim,
        ))
    }

    fn domain() -> Extent2d {
        let mut d = Extent2d::new();
        d.set_min_max(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0))
            .unwrap();
        d
    }

    #[test]
    fn type_queries() {
        assert!(is_kind_of(TYPE));
        assert!(is_kind_of(CRV_ON_SURF_TYPE));
        assert!(is_kind_of(curve_base::TYPE));
        assert!(!is_kind_of(curve_base::BSPLINE_CURVE_TYPE));
        assert_eq!(TYPE_STRING, "SmTangentField_TYPE");
        assert_eq!(CLASS_STRING, "SmTangentField");
        assert_eq!(TangentDirType::Interpolate.code(), 1);
    }

    #[test]
    fn ends_return_stored_tangents() {
        let uv = hermite(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            2,
        );
        let srf = ();
        let s = Vec3::new(0.0, 1.0, 0.0);
        let e = Vec3::new(0.0, 0.0, 1.0);
        let tf = TangentField::new(&uv, &srf, TangentDirType::Interpolate, s, e, Some(domain()))
            .unwrap();
        assert_eq!(tf.evaluate_point(0.0).unwrap(), s);
        assert_eq!(tf.evaluate_point(1.0).unwrap(), e);
        let d = tf.evaluate(1.0, 1).unwrap();
        assert_eq!(d[1], Vec3::new(0.0, 0.0, 0.0));
        assert!(tf.evaluate(0.0, 2).is_err());
        assert!(matches!(
            tf.evaluate_point(0.5),
            Err(SolidError::Unported { .. })
        ));
        assert!(matches!(
            TangentField::new(&uv, &srf, TangentDirType::Interpolate, s, e, None),
            Err(SolidError::Unported { .. })
        ));
    }

    #[test]
    fn equality_same_surface() {
        let uv = hermite(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            2,
        );
        let srf = 0_u8;
        let s = Vec3::new(0.0, 1.0, 0.0);
        let e = Vec3::new(0.0, 0.0, 1.0);
        let a = TangentField::new(&uv, &srf, TangentDirType::Interpolate, s, e, Some(domain()))
            .unwrap();
        let b =
            TangentField::new(&uv, &srf, TangentDirType::Constant, s, e, Some(domain())).unwrap();
        assert!(a.approx_eq(&a).unwrap());
        assert!(!a.approx_eq(&b).unwrap());
        let srf2 = 0_u8;
        let c = TangentField::new(
            &uv,
            &srf2,
            TangentDirType::Interpolate,
            s,
            e,
            Some(domain()),
        )
        .unwrap();
        assert!(a.approx_eq(&c).is_err());
    }

    #[test]
    fn create_checks_ends() {
        let base = hermite(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            3,
        );
        let uv = hermite(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            2,
        );
        let sc = hermite(
            [0.0; 3],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            3,
        );
        let ec = hermite(
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            3,
        );
        let srf = ();
        let start = CurveEnd {
            curve: &sc,
            param: 0.0,
            opposite: false,
        };
        let end = CurveEnd {
            curve: &ec,
            param: 0.0,
            opposite: true,
        };
        // matching ends reach the unported constructor
        assert!(matches!(
            TangentField::create(1e-6, &base, &uv, &srf, start, end),
            Err(SolidError::Unported { .. })
        ));
        // a mismatched end is SM_ERR
        let bad = CurveEnd {
            curve: &ec,
            param: 1.0,
            opposite: false,
        };
        assert!(matches!(
            TangentField::create(1e-6, &base, &uv, &srf, start, bad),
            Err(SolidError::Upstream { code: 1001, .. })
        ));
    }

    #[test]
    fn read_binary_members_then_unported() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u64.to_ne_bytes());
        for x in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            bytes.extend_from_slice(&x.to_ne_bytes());
        }
        let mut db = DatabaseReader::new();
        db.open_for_read(bytes.clone(), FileType::Binary);
        assert!(matches!(
            read_from_db(&mut db, 3, 30),
            Err(SolidError::Unported { .. })
        ));
        let mut db = DatabaseReader::new();
        db.open_for_read(bytes.get(..20).unwrap().to_vec(), FileType::Binary);
        assert!(matches!(
            read_from_db(&mut db, 3, 30),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
