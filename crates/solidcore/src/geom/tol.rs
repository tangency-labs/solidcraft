//! Tolerance policy (`SmTol`): typed tolerance values, model-size based default
//! tolerances, zone / intersection / approximation tolerances, scaled zeros, simple
//! tolerance predicates, and the pointer-pair container `SmRelation`.
//!
//! Upstream: `source/SMLib/inc/SmTol.h`, `source/SMLib/src/SmTol.cpp`,
//! `source/SMLib/inc/SmRelation.h`.
//!
//! | Rust                                   | Upstream                                       |
//! |----------------------------------------|------------------------------------------------|
//! | [`tol3d`]                              | `SmTol3d(double)`, `SmScaledZero(double)`, `SmTol2d(double)`, `SmTol1d(double)`, every `operator=(double)` of `SM_TOL_CALLS`, `SmTol3d::operator=(const SmTol3d)` |
//! | [`zone_tol3d`]                         | `SmZoneTol3d(double)`                          |
//! | [`xsect_tol3d`]                        | `SmXSectTol3d(double)`                         |
//! | [`approx_tol3d`]                       | `SmApproxTol3d(double)`                        |
//! | [`tol_mul`]                            | `operator*=(SmTol3d&, double)`                 |
//! | (plain `f64`)                          | `operator double&`, `operator const double&` of every tolerance class |
//! | [`SmallTopoType`]                      | `SmSmallTopoType`                              |
//! | [`size_estimate_to_zone_tol_3d`]       | `SmTol::SizeEstimateToZoneTol3d`               |
//! | [`in_tol`]                             | `SmTol::InTol(double, const SmTol3d&)`         |
//! | [`is_zero`], [`is_zero_squared`], [`is_zero_to_tol`] | `SM_IS_ZERO`, `SM_IS_ZERO_SQUARED`, `SM_IS_ZERO_TO_TOL` (`SmMath.h`) |
//! | [`are_same`], [`are_same_to_tol`]      | `SM_ARE_SAME`, `SM_ARE_SAME_TO_TOL` (`SmMath.h`) |
//! | [`calc_face_zone_tol_3d`]              | `SmTol::CalcFaceZoneTol3d(const SmContext*)`   |
//! | [`calc_edge_zone_tol_3d`]              | `SmTol::CalcEdgeZoneTol3d(double, double, SmZoneTol3d*, const SmContext*)` |
//! | [`calc_vertex_zone_tol_3d`]            | `SmTol::CalcVertexZoneTol3d(double, double, SmZoneTol3d*, const SmContext*)` |
//! | [`eff_zero`], [`eff_zero_sq`], [`eff_zero_param`], [`eff_zero_ang_rad`] | `SmTol::GetEffZero`, `GetEffZeroSq`, `GetEffZeroParam`, `GetEffZeroAngRad` |
//! | [`xsect_tol_3d_from_zones`]            | `SmTol::GetXSectTol3d(const SmZoneTol3d&, const SmZoneTol3d&)` |
//! | [`ang_tol_rad`]                        | `SmTol::GetAngTolRad()`                        |
//! | [`model_size_estimate`]                | `SmTol::GetModelSizeEstimate()`                |
//! | [`context_model_size_estimate`]        | `SmTol::GetModelSizeEstimate(const SmContext&)` |
//! | [`set_context_model_size_estimate`]    | `SmTol::SetModelSizeEstimate(SmContext&, double)` |
//! | [`are_parallel`]                       | `SmTol::AreParallel`                           |
//! | [`are_perpendicular`]                  | `SmTol::ArePerpendicular`                      |
//! | [`is_zero_speed`]                      | `SmTol::IsZeroSpeed`                           |
//! | [`is_degenerate_vec3`]                 | `SmTol::IsDegenerate(const SmVector3d, SmXSectTol3d*)` |
//! | [`gap_gain`]                           | `SmTol::GetGapGain`                            |
//! | [`zone_tol_3d`]                        | `SmTol::GetZoneTol3d(SmSmallTopoType)` (system) |
//! | [`context_zone_tol_3d`]                | `SmTol::GetZoneTol3d(const SmContext*, SmSmallTopoType)` |
//! | [`approx_tol_3d`]                      | `SmTol::GetApproxTol3d(SmSmallTopoType)`       |
//! | [`context_approx_tol_3d`]              | `SmTol::GetApproxTol3d(const SmContext*, SmSmallTopoType)` |
//! | [`xsect_tol_3d`]                       | `SmTol::GetXSectTol3d(SmSmallTopoType)`        |
//! | [`context_xsect_tol_3d`]               | `SmTol::GetXSectTol3d(const SmContext*, SmSmallTopoType)` |
//! | [`scaled_zero`], [`scaled_zero_sq`], [`scaled_zero_sqrt`] | `SmTol::GetScaledZero()`, `GetScaledZeroSq()`, `GetScaledZeroSqrt()` |
//! | [`ScaledZero`] (`f64`, [`Vec2`], [`Vec3`], [`Extent1d`], `Extent2d`, [`Extent3d`], `PseudoBox`) | `SmTol::GetScaledZero(const double)`, `(const SmVector2d&)`, `(const SmVector3d&)`, `(const SmExtent1d&)`, `(const SmExtent2d&)` (impl in `geom::extent2d`), `(const SmExtent3d&)`, `(const SmPseudoBox&)` (impl in `geom::pseudo_box`) |
//! | [`scaled_zero_of_equations`]           | `SmTol::GetScaledZero(const SmEvalNFunctionsObject&)` |
//! | [`scaled_zero_of_volume`]              | `SmTol::GetScaledZero(const SmVolume&)`        |
//! | [`map_to_1d`]                          | `SmTol::MapTo1d(SmTol3d&, SmVector3d&, const SmExtent1d&)` |
//! | [`scaled_zero_sq_of`], [`scaled_zero_sqrt_of`] | unary `GetScaledZeroSq<cA>`, `GetScaledZeroSqrt<cA>` |
//! | [`scaled_zero2`], [`scaled_zero_sq2`], [`scaled_zero_sqrt2`] | binary `GetScaledZero<cA,cB>`, `...Sq`, `...Sqrt` |
//! | [`scaled_zero3`], [`scaled_zero_sq3`], [`scaled_zero_sqrt3`] | tertiary templates     |
//! | [`scaled_zero4`], [`scaled_zero_sq4`], [`scaled_zero_sqrt4`] | quaternary templates   |
//! | [`Relation`]                           | `SmRelation<TYPE1, TYPE2>`                     |
//! | [`Relation::new`]                      | `SmRelation(const SmContext&, SmRelationCardinalityType)` |
//! | [`Relation::cardinality`]              | `m_eCardinality` (`GetType`/`GetTypeString`/`IsKindOf` are RTTI, see below) |
//! | [`Relation::relate_pair`]              | `RelatePair`                                   |
//! | [`Relation::relate_to_many`]           | `RelateToMany`                                 |
//! | [`Relation::remove_all`]               | `RemoveAll`                                    |
//! | [`Relation::second`]                   | `GetSecond`                                    |
//! | [`Relation::seconds`]                  | `GetSeconds`                                   |
//! | [`Relation::all_firsts`]               | `GetAllFirsts`                                 |
//! | [`Relation::all_seconds`]              | `GetAllSeconds`                                |
//! | [`Cardinality`]                        | `SmRelationCardinalityType`                    |
//!
//! System state: upstream keeps the system tolerances in thread-local statics
//! (`s_ZoneTol3d`, `s_TolGapGain`, `s_ModelSizeEstimate`, `s_LargeSmallSizeRatio`);
//! here they are the [`Tolerances`] of the [`Kernel`] (`zone_tol_3d`, `tol_gap_gain`,
//! `model_size_estimate`, `large_small_size_ratio`). The `const SmContext*` overloads
//! take an `Option<&`[`Context`]`>` (`None` = null pointer: the system value; usually
//! `Some(&k.context)`). Note that the context overloads ignore the small-topology
//! flag, as upstream does.
//!
//! Not here yet (they need types of later units; see the `UNPORTED` markers below):
//! every overload taking an `SmObject`, `SmCurve`, `SmSurface`, `SmVolume`,
//! `SmPseudoBox`, `SmBrep`, `SmBrepData`, a topology
//! entity, a classification / interval object, or `SmEvalNFunctionsObject`.
//!
//! Deviations:
//! - The tolerance classes (`SmTol3d`, `SmZoneTol3d`, ...) are plain `f64` values; their
//!   converting constructors are the functions [`tol3d`], [`zone_tol3d`],
//!   [`xsect_tol3d`] and [`approx_tol3d`], applied exactly where upstream converts a
//!   `double` into a tolerance class (a return statement, a cast, an initialisation),
//!   so that `fabs` and the `SM_USE_DEFAULT` substitution happen at the same places.
//! - Optional pointer arguments become `Option<f64>` holding the pointee's value.
//! - `SmRelation`: upstream stores pairs in `std::unordered_map` / `std::unordered_set`
//!   keyed by pointer, so `GetSeconds`, `GetAllFirsts` and `GetAllSeconds` come out in
//!   an unspecified, address-dependent order. [`Relation`] keeps keys in `BTreeMap`s /
//!   `BTreeSet`s and returns them sorted (see `plan/findings.md`). `RelatePair`'s null
//!   checks have no Rust counterpart; its `SM_ERR` status is
//!   [`SolidError::Upstream`] with code 1001. `GetSecond`'s `SE(SM_ERR)` for an
//!   ambiguous key is a no-op in the release build; [`Relation::second`] returns
//!   `None` there as upstream returns `NULL`. `GetType`, `GetTypeString`, `IsKindOf`
//!   (the `SmObject` run-time type system) are replaced by Rust's static types.
//! - `SM_ASSERT*`, `WARN`, `ERR_MSG` and `SM_DEBUG_CODE` blocks are not ported (they do
//!   not change results in the release build).

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{Result, SolidError};
use crate::geom::context::Context;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::local_solve_nd::EvalNFunctions;
use crate::geom::vector2d::Vec2;
use crate::geom::vector3d::sqrt as sm_sqrt;
use crate::kernel::{Kernel, Tolerances};
use crate::math::{
    max2, max3, max4, rad_to_deg, Vec3, EFF_ZERO, EFF_ZERO_PARAM, EFF_ZERO_RAD, EFF_ZERO_SQ,
    EFF_ZERO_SQRT, PI,
};
use crate::surface::volume::Volume;

// UNPORTED(topology.brep_p1): SmTol::GetModelSizeEstimate(const SmBrep&),
// SmTol::SetModelSizeEstimate(SmBrep&, double), SmTol::GetZoneTol3d(const SmBrep*),
// SmTol::GetApproxTol3d(const SmBrep*), SmTol::CalcFaceZoneTol3d(SmBrep*),
// SmTol::CalcEdgeZoneTol3d(.., SmBrep*, ..), SmTol::CalcVertexZoneTol3d(.., SmBrep*, ..)
// need SmBrep::GetTolerance / SmBrep::SetThisModelSizeEstimate; and
// SmTol::GetScaledZero(const SmBrep&) needs SmBrep::CalculateBoundingBox.
// UNPORTED(topology.vertex), UNPORTED(topology.edge), UNPORTED(topology.face_p2),
// UNPORTED(topology.edgeuse_p1), UNPORTED(topology.brep_p1): SmTol::GetZoneTol3d(const
// SmObject*), SmTol::GetZoneTol3d(const SmBrepData*), SmTol::GetApproxTol3d(const
// SmObject*), SmTol::GetXSectTol3d(const SmObject*, const SmObject*, ..),
// SmTol::GetZoneTol3d_ForXSectResult(const SmObject*, const SmObject*),
// SmTol::IsDegenerate(const SmObject*), SmTol::IsClosed(const SmObject*),
// SmTol::GetScaledZero(const SmObject&) dispatch on IsKindOf over SmTopology (the
// virtual GetTolerance: the base is topology::topology::Topology::tolerance, the
// overrides belong to SmVertex, SmEdge, SmFace, SmEdgeuse), SmSurface/SmCurve
// (GetFace/GetEdge), SmBrep, SmBrepData and SmCurveClassification; those classes have
// no Rust object type yet.
// UNPORTED(curve.curve_p2): SmTol::IsDegenerate(const SmCurve*, SmExtent1d, ..),
// SmTol::MapTo1d(SmTol3d&, double, const SmCurve&), SmTol::MapTo2d(.., double, const
// SmCurve&, const SmSurface&) and SmTol::GetScaledZero(const SmCurve&) need
// SmCurve::IsDegenerate / Evaluate / GetNaturalInterval / ApproximateLength.
// UNPORTED(surface.surface_p1): SmTol::MapTo2d(const SmTol3d&, const SmVector2d&, [const
// SmVector2d&,] const SmSurface&) and SmTol::GetScaledZero(const SmSurface&) need
// SmSurface::Evaluate1stDerivatives / GetNaturalUVDomain / ApproximateSize.
// UNPORTED(intersect.curve_class_p1): SmTol::GetSrcZoneTol3d / GetObjZoneTol3d of
// SmCurveClassification / SmCurveInterval and SmTol::GetScaledZero(const
// SmCurveInterval&) need the classification classes.
// SmTol::GetSrcZoneTol3d / GetObjZoneTol3d / GetXSectTol3d of SmPointClassification
// are intersect::point_class::PointClassification::src_zone_tol_3d / obj_zone_tol_3d /
// xsect_tol_3d.
// UNPORTED(query.poly): SmTol::GetSrcZoneTol3d / GetObjZoneTol3d of
// SmLineSegClassification / SmPolyPointClassification / SmLineSegInterval.
// UNPORTED(blend.fillet_solver_p1): SmTol::GetAngTolRad(const SmFilletSolver*);
// UNPORTED(blend.fillet_corner_p1): the SmFilletCorner branch of GetApproxTol3d(SmObject*);
// the other GetApproxTol3d / GetAngTolRad overloads of algorithm objects (SmHCR,
// SmPolyIntersector, SmSurfaceIntersector, SmSurfaceTracer, SmTopologyIntersector,
// SmOffsetGeometryCreation) belong with those objects' units.
// UNPORTED(topology.vertex): SmTol::GetScaledZero(const SmVertex&) (SmVertex::GetPoint).
// UNPORTED(topology.edge): SmTol::GetScaledZero(const SmEdge&) (SmEdge::GetCurve).
// UNPORTED(topology.face_p2): SmTol::GetScaledZero(const SmFace&) (SmFace::GetSurface).
// UNPORTED(topology.shell): SmTol::GetScaledZero(const SmRegion&) and (const SmShell&)
// (CalculateBoundingBox).
// UNPORTED(topology.vertex), UNPORTED(topology.edge), UNPORTED(topology.face_p2):
// SmTol::UpdateObjectTolerance(SmTopology*, double, double, SmBoolean) needs
// SmVertex/SmEdge/SmFace::SetTolerance, GetMaxGap3d (heal.gap) and SmFace::GetEdges.

/// `SM_USE_DEFAULT`: sentinel meaning "use the current system value".
pub const USE_DEFAULT: f64 = 9876543.21;
/// `SM_UNINIT_TOL`: sentinel of an uninitialised tolerance.
pub const UNINIT_TOL: f64 = 1.23456e-61;
/// `SM_MODEL_SIZE_ESTIMATE`: default model size estimate.
pub const MODEL_SIZE_ESTIMATE: f64 = 50.0;
/// `SM_LARGE_SMALL_SIZE_RATIO`: default large/small topology size ratio.
pub const LARGE_SMALL_SIZE_RATIO: f64 = 1000.0;
/// `SM_ZONE_TOL_3D`: default zone tolerance.
pub const ZONE_TOL_3D: f64 = 1.0e-5;
/// `SM_TOL_GAP_GAIN`: over-size factor above an object's largest gap.
pub const TOL_GAP_GAIN: f64 = 1.1;
/// `SM_ANG_TOL_DEG`: angle tolerance in degrees.
pub const ANG_TOL_DEG: f64 = 0.5;
/// `SM_ANG_TOL_RAD` = `SM_ANG_TOL_DEG * SM_PI / 180.0`.
pub const ANG_TOL_RAD: f64 = ANG_TOL_DEG * PI / 180.0;

/// Upstream status `SM_ERR`.
const SM_ERR: i32 = 1001;

// ---- tolerance values ------------------------------------------------------------

/// `SmTol3d(double)` (and the `SmScaledZero`, `SmTol2d`, `SmTol1d` constructors and
/// every `operator=(double)`): a tolerance is stored as `fabs(d)`.
#[must_use]
pub fn tol3d(d: f64) -> f64 {
    d.abs()
}

/// `SmZoneTol3d(double)`: [`USE_DEFAULT`] becomes the system zone tolerance, then
/// `fabs`.
#[must_use]
pub fn zone_tol3d(k: &Kernel, d: f64) -> f64 {
    tol3d(if d == USE_DEFAULT {
        k.tolerances.zone_tol_3d
    } else {
        d
    })
}

/// `SmXSectTol3d(double)`: [`USE_DEFAULT`] becomes twice the system zone tolerance,
/// then `fabs`.
#[must_use]
pub fn xsect_tol3d(k: &Kernel, d: f64) -> f64 {
    tol3d(if d == USE_DEFAULT {
        k.tolerances.zone_tol_3d * 2.0
    } else {
        d
    })
}

/// `SmApproxTol3d(double)`: [`USE_DEFAULT`] becomes half the system zone tolerance,
/// then `fabs`.
#[must_use]
pub fn approx_tol3d(k: &Kernel, d: f64) -> f64 {
    tol3d(if d == USE_DEFAULT {
        k.tolerances.zone_tol_3d / 2.0
    } else {
        d
    })
}

/// `operator*=(SmTol3d&, double)`: `tol * d` (no `fabs`, as upstream).
#[must_use]
pub fn tol_mul(tol: f64, d: f64) -> f64 {
    tol * d
}

/// Control flags for small-topology tolerances (`SmSmallTopoType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SmallTopoType {
    /// `SM_ST_NORMAL_SIZE`: normal (large) scale unless the object says otherwise.
    #[default]
    NormalSize,
    /// `SM_ST_SMALL_SIZE`: small ("pinhole in a battleship") scale unless the object
    /// says otherwise.
    SmallSize,
    /// `SM_ST_FORCE_NORMAL_SIZE`: always normal scale.
    ForceNormalSize,
    /// `SM_ST_FORCE_SMALL_SIZE`: always small scale.
    ForceSmallSize,
    /// `SM_ST_NO_CHANGE`: no-action value for `SetZoneTol3d`.
    NoChange,
}

impl SmallTopoType {
    /// Upstream enum value (`SM_ST_NORMAL_SIZE` = 0 .. `SM_ST_NO_CHANGE` = 4).
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::NormalSize => 0,
            Self::SmallSize => 1,
            Self::ForceNormalSize => 2,
            Self::ForceSmallSize => 3,
            Self::NoChange => 4,
        }
    }

    /// The flag with upstream value `code`, if any.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::NormalSize),
            1 => Some(Self::SmallSize),
            2 => Some(Self::ForceNormalSize),
            3 => Some(Self::ForceSmallSize),
            4 => Some(Self::NoChange),
            _ => None,
        }
    }

    /// `eIsSmall == SM_ST_SMALL_SIZE || eIsSmall == SM_ST_FORCE_SMALL_SIZE`.
    const fn is_small(self) -> bool {
        matches!(self, Self::SmallSize | Self::ForceSmallSize)
    }
}

// ---- model size and system tolerances ----------------------------------------------

/// Maps a model size estimate to a default zone tolerance
/// (`SmTol::SizeEstimateToZoneTol3d`):
///
/// | size estimate      | zone tolerance |
/// |--------------------|----------------|
/// | `0.00005 - 0.005`  | `1e-7`         |
/// | `0.005 - 0.5`      | `1e-6`         |
/// | `0.5 - 500`        | `1e-5`         |
/// | `500 - 50000`      | `1e-4`         |
/// | `50000 - 5e6`      | `1e-3`         |
///
/// and `10^floor(log10(|size|) / 2) * 1e-5` outside those ranges; [`USE_DEFAULT`]
/// means [`MODEL_SIZE_ESTIMATE`]. (The `SmZoneTol3d` conversions of the result never
/// see `SM_USE_DEFAULT` and only apply `fabs`.)
#[must_use]
pub fn size_estimate_to_zone_tol_3d(size: f64) -> f64 {
    if size == USE_DEFAULT {
        size_estimate_to_zone_tol_3d(MODEL_SIZE_ESTIMATE)
    } else if (0.5..=500.0).contains(&size) {
        1.0e-5
    } else if (500.0..=50000.0).contains(&size) {
        1.0e-4
    } else if (0.005..=0.5).contains(&size) {
        1.0e-6
    } else if (50000.0..=5e6).contains(&size) {
        1.0e-3
    } else if (0.00005..=0.005).contains(&size) {
        1.0e-7
    } else {
        tol3d(10f64.powf((size.abs().log10() / 2.0).floor()) * 1.0e-5)
    }
}

/// The system model size estimate (`SmTol::GetModelSizeEstimate()`).
#[must_use]
pub fn model_size_estimate(k: &Kernel) -> f64 {
    k.tolerances.model_size_estimate
}

/// A context's model size estimate, the system one if it has none
/// (`SmTol::GetModelSizeEstimate(const SmContext&)`).
#[must_use]
pub fn context_model_size_estimate(k: &Kernel, ctx: &Context) -> f64 {
    ctx.this_model_size_estimate(k)
}

/// Sets a context's model size estimate and returns its new zone tolerance
/// (`SmTol::SetModelSizeEstimate(SmContext&, double)`).
pub fn set_context_model_size_estimate(ctx: &mut Context, model_size_estimate: f64) -> f64 {
    ctx.set_this_model_size_estimate(model_size_estimate)
}

/// The gap gain: the over-size factor that keeps an object's zone tolerance a little
/// larger than half its largest gap (`SmTol::GetGapGain`).
#[must_use]
pub fn gap_gain(k: &Kernel) -> f64 {
    k.tolerances.tol_gap_gain
}

/// The system zone tolerance (`SmTol::GetZoneTol3d(SmSmallTopoType)`): multiplied by
/// the large/small size ratio for small topology.
#[must_use]
pub fn zone_tol_3d(k: &Kernel, small: SmallTopoType) -> f64 {
    let t: &Tolerances = &k.tolerances;
    if t.zone_tol_3d == UNINIT_TOL {
        // upstream: smos_AssertErrorMessage(.., "Uninit-Tol", ..)
        zone_tol3d(k, UNINIT_TOL)
    } else if small.is_small() {
        zone_tol3d(k, zone_tol3d(k, t.large_small_size_ratio) * t.zone_tol_3d)
    } else {
        t.zone_tol_3d
    }
}

/// The context zone tolerance (`SmTol::GetZoneTol3d(const SmContext*,
/// SmSmallTopoType)`): the context's own zone tolerance, or the system one for `None`
/// (a null context); `small` is ignored, as upstream.
#[must_use]
pub fn context_zone_tol_3d(k: &Kernel, ctx: Option<&Context>, _small: SmallTopoType) -> f64 {
    let z = k.tolerances.zone_tol_3d;
    if z == UNINIT_TOL {
        zone_tol3d(k, UNINIT_TOL)
    } else {
        ctx.map_or(z, Context::this_zone_tol_3d)
    }
}

/// The system approximation tolerance, half the zone tolerance
/// (`SmTol::GetApproxTol3d(SmSmallTopoType)`).
#[must_use]
pub fn approx_tol_3d(k: &Kernel, small: SmallTopoType) -> f64 {
    approx_tol3d(k, 0.5 * zone_tol_3d(k, small))
}

/// The context approximation tolerance (`SmTol::GetApproxTol3d(const SmContext*,
/// SmSmallTopoType)`; `None` = null context).
#[must_use]
pub fn context_approx_tol_3d(k: &Kernel, ctx: Option<&Context>, small: SmallTopoType) -> f64 {
    approx_tol3d(k, 0.5 * context_zone_tol_3d(k, ctx, small))
}

/// The system intersection tolerance, twice the zone tolerance
/// (`SmTol::GetXSectTol3d(SmSmallTopoType)`).
#[must_use]
pub fn xsect_tol_3d(k: &Kernel, small: SmallTopoType) -> f64 {
    xsect_tol3d(k, 2.0 * zone_tol_3d(k, small))
}

/// The context intersection tolerance (`SmTol::GetXSectTol3d(const SmContext*,
/// SmSmallTopoType)`; `None` = null context).
#[must_use]
pub fn context_xsect_tol_3d(k: &Kernel, ctx: Option<&Context>, small: SmallTopoType) -> f64 {
    xsect_tol3d(k, 2.0 * context_zone_tol_3d(k, ctx, small))
}

/// The intersection tolerance of two objects with zone tolerances `zone1`, `zone2`:
/// their sum (`SmTol::GetXSectTol3d(const SmZoneTol3d&, const SmZoneTol3d&)`).
#[must_use]
pub fn xsect_tol_3d_from_zones(k: &Kernel, zone1: f64, zone2: f64) -> f64 {
    xsect_tol3d(k, zone1 + zone2)
}

/// The system angle tolerance in radians (`SmTol::GetAngTolRad()`, `SM_ANG_TOL_RAD`).
#[must_use]
pub fn ang_tol_rad() -> f64 {
    ANG_TOL_RAD
}

/// `SM_EFF_ZERO` as a scaled zero (`SmTol::GetEffZero`).
#[must_use]
pub fn eff_zero() -> f64 {
    tol3d(EFF_ZERO)
}

/// `SM_EFF_ZERO_SQ` as a scaled zero (`SmTol::GetEffZeroSq`).
#[must_use]
pub fn eff_zero_sq() -> f64 {
    tol3d(EFF_ZERO_SQ)
}

/// `SM_EFF_ZERO_PARAM` as a scaled zero (`SmTol::GetEffZeroParam`): the smallest
/// distance between distinct parameter values.
#[must_use]
pub fn eff_zero_param() -> f64 {
    tol3d(EFF_ZERO_PARAM)
}

/// `SM_EFF_ZERO_RAD` as a scaled zero (`SmTol::GetEffZeroAngRad`).
#[must_use]
pub fn eff_zero_ang_rad() -> f64 {
    tol3d(EFF_ZERO_RAD)
}

// ---- topology zone tolerances from gaps --------------------------------------------

/// A face's zone tolerance: the context zone tolerance
/// (`SmTol::CalcFaceZoneTol3d(const SmContext*)`; `None` = null context).
#[must_use]
pub fn calc_face_zone_tol_3d(k: &Kernel, ctx: Option<&Context>) -> f64 {
    context_zone_tol_3d(k, ctx, SmallTopoType::NormalSize)
}

/// An edge's zone tolerance from its largest edge/edge and edge/face gaps
/// (`SmTol::CalcEdgeZoneTol3d(double, double, SmZoneTol3d*, const SmContext*)`;
/// `None` = null context):
/// `max(def, max_face_zone_tol?, gain * edge_edge_gap / 2, gain * edge_face_gap - def)`
/// where `def` is the context zone tolerance and `gain` the [`gap_gain`].
#[must_use]
pub fn calc_edge_zone_tol_3d(
    k: &Kernel,
    edge_edge_max_gap: f64,
    edge_face_max_gap: f64,
    max_face_zone_tol: Option<f64>,
    ctx: Option<&Context>,
) -> f64 {
    calc_gap_zone_tol(
        k,
        edge_edge_max_gap,
        edge_face_max_gap,
        max_face_zone_tol,
        ctx,
    )
}

/// A vertex's zone tolerance from its largest vertex/edge and vertex/face gaps
/// (`SmTol::CalcVertexZoneTol3d(double, double, SmZoneTol3d*, const SmContext*)`;
/// `None` = null context); same formula as
/// [`calc_edge_zone_tol_3d`].
#[must_use]
pub fn calc_vertex_zone_tol_3d(
    k: &Kernel,
    vertex_edge_max_gap: f64,
    vertex_face_max_gap: f64,
    max_edge_zone_tol: Option<f64>,
    ctx: Option<&Context>,
) -> f64 {
    calc_gap_zone_tol(
        k,
        vertex_edge_max_gap,
        vertex_face_max_gap,
        max_edge_zone_tol,
        ctx,
    )
}

/// Shared body of `CalcEdgeZoneTol3d` / `CalcVertexZoneTol3d`.
fn calc_gap_zone_tol(
    k: &Kernel,
    gap_1d: f64,
    gap_2d: f64,
    max_up: Option<f64>,
    ctx: Option<&Context>,
) -> f64 {
    let def = context_zone_tol_3d(k, ctx, SmallTopoType::NormalSize);
    let gain = gap_gain(k);
    let r = match max_up {
        Some(up) => max4(def, up, gain * gap_1d / 2.0, gain * gap_2d - def),
        None => max3(def, gain * gap_1d / 2.0, gain * gap_2d - def),
    };
    zone_tol3d(k, r)
}

// ---- predicates --------------------------------------------------------------------

/// `|val| <= tol` (`SmTol::InTol(double, const SmTol3d&)`).
#[must_use]
pub fn in_tol(val: f64, tol: f64) -> bool {
    val.abs() <= tol
}

/// `SM_IS_ZERO(a)`: `|a| <=` [`scaled_zero`] (`SmMath.h`).
#[must_use]
pub fn is_zero(a: f64) -> bool {
    in_tol(a, scaled_zero())
}

/// `SM_IS_ZERO_SQUARED(a)`: `|a| <=` [`scaled_zero_sq`] (`SmMath.h`).
#[must_use]
pub fn is_zero_squared(a: f64) -> bool {
    in_tol(a, scaled_zero_sq())
}

/// `SM_IS_ZERO_TO_TOL(a, tol)`: `|a| <= |tol|` (`SmMath.h`).
#[must_use]
pub fn is_zero_to_tol(a: f64, tol: f64) -> bool {
    in_tol(a, tol3d(tol))
}

/// `SM_ARE_SAME(a, b)`: `|a - b| <=` the larger scaled zero of `a` and `b`
/// (`SmMath.h`).
#[must_use]
pub fn are_same(a: f64, b: f64) -> bool {
    in_tol(a - b, scaled_zero2(&a, &b))
}

/// `SM_ARE_SAME_TO_TOL(a, b, tol)`: `|a - b| <= |tol|` (`SmMath.h`).
#[must_use]
pub fn are_same_to_tol(a: f64, b: f64, tol: f64) -> bool {
    in_tol(a - b, tol3d(tol))
}

/// True when `speed` is zero relative to the size `max_pos_dimension`:
/// `|speed| < override` or, without override, `|speed| < SM_EFF_ZERO * (1 + |max|)`
/// (`SmTol::IsZeroSpeed`).
#[must_use]
pub fn is_zero_speed(
    speed: f64,
    max_pos_dimension: f64,
    override_scaled_zero: Option<f64>,
) -> bool {
    speed.abs()
        < match override_scaled_zero {
            Some(z) => z,
            None => EFF_ZERO * (1.0 + max_pos_dimension.abs()),
        }
}

/// True when `v` is shorter than the intersection tolerance (the override, else the
/// system [`xsect_tol_3d`]) (`SmTol::IsDegenerate(const SmVector3d, SmXSectTol3d*)`).
#[must_use]
pub fn is_degenerate_vec3(k: &Kernel, v: Vec3, override_xsect_tol: Option<f64>) -> bool {
    let tol = match override_xsect_tol {
        Some(t) => t,
        None => xsect_tol_3d(k, SmallTopoType::NormalSize),
    };
    v.is_zero(tol)
}

/// True when `v1` and `v2` are parallel within the angle tolerance (the override in
/// radians, else [`ang_tol_rad`]) (`SmTol::AreParallel`).
#[must_use]
pub fn are_parallel(v1: Vec3, v2: Vec3, override_ang_tol_rad: Option<f64>) -> bool {
    let ang = override_ang_tol_rad.unwrap_or_else(ang_tol_rad);
    v1.is_parallel_to(v2, rad_to_deg(ang))
}

/// True when `v1` and `v2` are perpendicular within the angle tolerance (the override
/// in radians, else [`ang_tol_rad`]) (`SmTol::ArePerpendicular`).
#[must_use]
pub fn are_perpendicular(v1: Vec3, v2: Vec3, override_ang_tol_rad: Option<f64>) -> bool {
    let ang = override_ang_tol_rad.unwrap_or_else(ang_tol_rad);
    v1.is_perpendicular_to(v2, rad_to_deg(ang))
}

// ---- parameter-space mapping ------------------------------------------------------

/// Maps the 3D tolerance `tol` to a curve's parameter space at a point with first
/// derivative `deriv`: `tol / |deriv|`, or `natural_interval.length() / 100` where the
/// derivative is (near) zero (`SmTol::MapTo1d(SmTol3d&, SmVector3d&, const
/// SmExtent1d&)`). `tol` is the value of an `SmTol3d` (already `fabs`).
#[must_use]
pub fn map_to_1d(tol: f64, deriv: Vec3, natural_interval: &Extent1d) -> f64 {
    let scaled_zero = scaled_zero();
    let deriv_size = deriv.length();
    tol3d(if deriv_size < scaled_zero {
        natural_interval.length() / 100.0
    } else {
        tol / deriv_size
    })
}

// ---- scaled zeros ------------------------------------------------------------------

/// The default scaled zero, `SM_EFF_ZERO` (`SmTol::GetScaledZero()`).
#[must_use]
pub fn scaled_zero() -> f64 {
    tol3d(EFF_ZERO)
}

/// `SM_EFF_ZERO_SQ` (`SmTol::GetScaledZeroSq()`).
#[must_use]
pub fn scaled_zero_sq() -> f64 {
    tol3d(EFF_ZERO_SQ)
}

/// `SM_EFF_ZERO_SQRT` (`SmTol::GetScaledZeroSqrt()`).
#[must_use]
pub fn scaled_zero_sqrt() -> f64 {
    tol3d(EFF_ZERO_SQRT)
}

/// Values with a size-relative numerical zero (the unary `SmTol::GetScaledZero`
/// overloads): `SM_EFF_ZERO * (1 + size)`.
pub trait ScaledZero {
    /// The scaled zero of `self`.
    fn scaled_zero(&self) -> f64;
}

impl ScaledZero for f64 {
    /// `SmTol::GetScaledZero(const double)`: `SM_EFF_ZERO * (1 + |d|)`.
    fn scaled_zero(&self) -> f64 {
        tol3d(EFF_ZERO * (1.0 + self.abs()))
    }
}

impl ScaledZero for Vec2 {
    /// `SmTol::GetScaledZero(const SmVector2d&)`: `SM_EFF_ZERO * (1 + max |coord|)`.
    fn scaled_zero(&self) -> f64 {
        tol3d(EFF_ZERO * (1.0 + self.max_dimension()))
    }
}

impl ScaledZero for Vec3 {
    /// `SmTol::GetScaledZero(const SmVector3d&)`: `SM_EFF_ZERO * (1 + max |coord|)`.
    fn scaled_zero(&self) -> f64 {
        tol3d(EFF_ZERO * (1.0 + self.max_dimension()))
    }
}

impl ScaledZero for Extent1d {
    /// `SmTol::GetScaledZero(const SmExtent1d&)`: `SM_EFF_ZERO * (1 + max |end|)`.
    fn scaled_zero(&self) -> f64 {
        tol3d(EFF_ZERO * (1.0 + self.max_dimension()))
    }
}

impl ScaledZero for Extent3d {
    /// `SmTol::GetScaledZero(const SmExtent3d&)`: `SM_EFF_ZERO * (1 + max |coord|)`
    /// over both corners.
    fn scaled_zero(&self) -> f64 {
        tol3d(EFF_ZERO * (1.0 + max2(self.min().max_dimension(), self.max().max_dimension())))
    }
}

/// `SmTol::GetScaledZero(const SmEvalNFunctionsObject&)`: the equation set's own
/// scaled zero ([`EvalNFunctions::scaled_zero`]) as a tolerance (`fabs`).
#[must_use]
pub fn scaled_zero_of_equations<E: EvalNFunctions + ?Sized>(equations: &E) -> f64 {
    tol3d(equations.scaled_zero())
}

/// `SmTol::GetScaledZero(const SmVolume&)`: `100 * SM_EFF_ZERO` for an unbounded
/// parameter domain; a bounded one needs the out-space bounding box.
///
/// # Errors
///
/// [`SolidError::Unported`] from the volume, and for a bounded domain
/// (`SmVolume::EvaluateBoundingBox`, unit `surface.volume_p2`).
pub fn scaled_zero_of_volume(volume: &Volume) -> Result<f64> {
    let param_domain = volume.natural_param_domain()?;
    if param_domain.is_bounded() {
        // UNPORTED(surface.volume_p2): SmVolume::EvaluateBoundingBox
        return Err(SolidError::Unported {
            upstream: "SmVolume::EvaluateBoundingBox",
            unit: "surface.volume_p2",
        });
    }
    Ok(tol3d(100.0 * EFF_ZERO))
}

/// Square of the scaled zero of `a` (unary `SmTol::GetScaledZeroSq<cA>`).
#[must_use]
pub fn scaled_zero_sq_of<A: ScaledZero + ?Sized>(a: &A) -> f64 {
    let r = a.scaled_zero();
    tol3d(r * r)
}

/// Square root of the scaled zero of `a` (unary `SmTol::GetScaledZeroSqrt<cA>`).
#[must_use]
pub fn scaled_zero_sqrt_of<A: ScaledZero + ?Sized>(a: &A) -> f64 {
    tol3d(sm_sqrt(a.scaled_zero()))
}

/// Largest scaled zero of two values (binary `SmTol::GetScaledZero<cA, cB>`).
#[must_use]
pub fn scaled_zero2<A: ScaledZero + ?Sized, B: ScaledZero + ?Sized>(a: &A, b: &B) -> f64 {
    max2(a.scaled_zero(), b.scaled_zero())
}

/// Square of [`scaled_zero2`] (binary `SmTol::GetScaledZeroSq<cA, cB>`).
#[must_use]
pub fn scaled_zero_sq2<A: ScaledZero + ?Sized, B: ScaledZero + ?Sized>(a: &A, b: &B) -> f64 {
    let r = scaled_zero2(a, b);
    tol3d(r * r)
}

/// Square root of [`scaled_zero2`] (binary `SmTol::GetScaledZeroSqrt<cA, cB>`).
#[must_use]
pub fn scaled_zero_sqrt2<A: ScaledZero + ?Sized, B: ScaledZero + ?Sized>(a: &A, b: &B) -> f64 {
    tol3d(sm_sqrt(scaled_zero2(a, b)))
}

/// Largest scaled zero of three values (tertiary `SmTol::GetScaledZero<cA, cB, cC>`).
#[must_use]
pub fn scaled_zero3<A, B, C>(a: &A, b: &B, c: &C) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
{
    max3(a.scaled_zero(), b.scaled_zero(), c.scaled_zero())
}

/// Square of [`scaled_zero3`] (tertiary `SmTol::GetScaledZeroSq`).
#[must_use]
pub fn scaled_zero_sq3<A, B, C>(a: &A, b: &B, c: &C) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
{
    let r = scaled_zero3(a, b, c);
    tol3d(r * r)
}

/// Square root of [`scaled_zero3`] (tertiary `SmTol::GetScaledZeroSqrt`).
#[must_use]
pub fn scaled_zero_sqrt3<A, B, C>(a: &A, b: &B, c: &C) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
{
    tol3d(sm_sqrt(scaled_zero3(a, b, c)))
}

/// Largest scaled zero of four values (quaternary `SmTol::GetScaledZero`).
#[must_use]
pub fn scaled_zero4<A, B, C, D>(a: &A, b: &B, c: &C, d: &D) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
    D: ScaledZero + ?Sized,
{
    max4(
        a.scaled_zero(),
        b.scaled_zero(),
        c.scaled_zero(),
        d.scaled_zero(),
    )
}

/// Square of [`scaled_zero4`] (quaternary `SmTol::GetScaledZeroSq`).
#[must_use]
pub fn scaled_zero_sq4<A, B, C, D>(a: &A, b: &B, c: &C, d: &D) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
    D: ScaledZero + ?Sized,
{
    let r = scaled_zero4(a, b, c, d);
    tol3d(r * r)
}

/// Square root of [`scaled_zero4`] (quaternary `SmTol::GetScaledZeroSqrt`).
#[must_use]
pub fn scaled_zero_sqrt4<A, B, C, D>(a: &A, b: &B, c: &C, d: &D) -> f64
where
    A: ScaledZero + ?Sized,
    B: ScaledZero + ?Sized,
    C: ScaledZero + ?Sized,
    D: ScaledZero + ?Sized,
{
    tol3d(sm_sqrt(scaled_zero4(a, b, c, d)))
}

// ---- SmRelation ----------------------------------------------------------------------

/// Cardinality of a [`Relation`] (`SmRelationCardinalityType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Cardinality {
    /// `SM_RC_ONE_TO_ONE`: each first and each second in at most one pair.
    OneToOne,
    /// `SM_RC_ONE_TO_MANY`: each second has at most one first.
    OneToMany,
    /// `SM_RC_MANY_TO_ONE`: each first has at most one second.
    ManyToOne,
    /// `SM_RC_MANY_TO_MANY`: unrestricted (upstream default).
    #[default]
    ManyToMany,
}

impl Cardinality {
    /// Upstream enum value (`SM_RC_ONE_TO_ONE` = 0 .. `SM_RC_MANY_TO_MANY` = 3).
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::OneToOne => 0,
            Self::OneToMany => 1,
            Self::ManyToOne => 2,
            Self::ManyToMany => 3,
        }
    }

    /// The cardinality with upstream value `code`, if any.
    #[must_use]
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::OneToOne),
            1 => Some(Self::OneToMany),
            2 => Some(Self::ManyToOne),
            3 => Some(Self::ManyToMany),
            _ => None,
        }
    }
}

/// A many-to-many (or restricted) relation between values of `A` and `B`, typically
/// entity ids (`SmRelation<TYPE1, TYPE2>`).
///
/// Both directions are indexed. Query results are sorted by key (upstream: hash
/// order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation<A, B> {
    first_to_second: BTreeMap<A, BTreeSet<B>>,
    second_to_first: BTreeMap<B, BTreeSet<A>>,
    cardinality: Cardinality,
}

impl<A: Ord + Copy, B: Ord + Copy> Default for Relation<A, B> {
    fn default() -> Self {
        Self::new(Cardinality::default())
    }
}

impl<A: Ord + Copy, B: Ord + Copy> Relation<A, B> {
    /// An empty relation with the given cardinality (`SmRelation(crContext,
    /// eCardinality)`).
    #[must_use]
    pub const fn new(cardinality: Cardinality) -> Self {
        Self {
            first_to_second: BTreeMap::new(),
            second_to_first: BTreeMap::new(),
            cardinality,
        }
    }

    /// The cardinality restriction (`m_eCardinality`).
    #[must_use]
    pub const fn cardinality(&self) -> Cardinality {
        self.cardinality
    }

    /// Relates `first` with `second` (`RelatePair`). Relating an existing pair again
    /// is a no-op when the cardinality allows it.
    ///
    /// # Errors
    ///
    /// [`SolidError::Upstream`] with code 1001 (`SM_ERR`) when the pair would violate
    /// the cardinality; nothing is changed then.
    pub fn relate_pair(&mut self, first: A, second: B) -> Result<()> {
        let has_second = self.first_to_second.contains_key(&first);
        let has_first = self.second_to_first.contains_key(&second);
        let violates = match self.cardinality {
            Cardinality::OneToOne => has_first || has_second,
            Cardinality::OneToMany => has_first,
            Cardinality::ManyToOne => has_second,
            Cardinality::ManyToMany => false,
        };
        if violates {
            return Err(SolidError::Upstream {
                operation: "geom::tol::Relation::relate_pair",
                code: SM_ERR,
                entity: None,
            });
        }
        self.first_to_second
            .entry(first)
            .or_default()
            .insert(second);
        self.second_to_first
            .entry(second)
            .or_default()
            .insert(first);
        Ok(())
    }

    /// Relates `first` with each of `seconds` in order (`RelateToMany`).
    ///
    /// # Errors
    ///
    /// The first [`Relation::relate_pair`] error; the pairs before it stay related.
    pub fn relate_to_many(&mut self, first: A, seconds: &[B]) -> Result<()> {
        for &second in seconds {
            self.relate_pair(first, second)?;
        }
        Ok(())
    }

    /// Removes every pair (`RemoveAll`).
    pub fn remove_all(&mut self) {
        self.first_to_second.clear();
        self.second_to_first.clear();
    }

    /// The only second related to `first`, or `None` when there is none or more than
    /// one (`GetSecond`).
    #[must_use]
    pub fn second(&self, first: A) -> Option<B> {
        let set = self.first_to_second.get(&first)?;
        if set.len() == 1 {
            set.iter().next().copied()
        } else {
            None
        }
    }

    /// Every second related to `first`, sorted (`GetSeconds`).
    #[must_use]
    pub fn seconds(&self, first: A) -> Vec<B> {
        self.first_to_second
            .get(&first)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Every first that is in a pair, sorted (`GetAllFirsts`).
    #[must_use]
    pub fn all_firsts(&self) -> Vec<A> {
        self.first_to_second.keys().copied().collect()
    }

    /// Every second that is in a pair, sorted (`GetAllSeconds`).
    #[must_use]
    pub fn all_seconds(&self) -> Vec<B> {
        self.second_to_first.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_zero_of_unbounded_volume() {
        use crate::geom::transform::Transform;
        use crate::surface::volume::VolumeKind;
        let v = Volume::new(VolumeKind::Transform(Transform::IDENTITY));
        assert_eq!(scaled_zero_of_volume(&v), Ok(100.0 * EFF_ZERO));
        let t = Volume::new(VolumeKind::Base);
        assert!(matches!(
            scaled_zero_of_volume(&t),
            Err(SolidError::Unported { .. })
        ));
    }
    use crate::kernel::KernelOptions;

    fn kernel(model: f64, ratio: f64) -> Kernel {
        let model = if model == USE_DEFAULT {
            MODEL_SIZE_ESTIMATE
        } else {
            model
        };
        let zone = size_estimate_to_zone_tol_3d(model);
        let mut o = KernelOptions::default();
        o.tolerances.model_size_estimate = model;
        o.tolerances.zone_tol_3d = zone;
        o.tolerances.xsect_tol_3d = zone * 2.0;
        o.tolerances.approx_tol_3d = zone / 2.0;
        o.tolerances.large_small_size_ratio = ratio;
        Kernel::new(&o)
    }

    #[test]
    fn size_estimate_table() {
        // SmTol.cpp sm_TemplateTrial.
        assert_eq!(size_estimate_to_zone_tol_3d(50.0), 1.0e-5);
        assert_eq!(size_estimate_to_zone_tol_3d(50000.0), 1.0e-4);
        assert_eq!(size_estimate_to_zone_tol_3d(0.005), 1.0e-6);
        assert_eq!(size_estimate_to_zone_tol_3d(USE_DEFAULT), 1.0e-5);
        assert_eq!(size_estimate_to_zone_tol_3d(1.0e8), 1.0e-1);
        assert_eq!(size_estimate_to_zone_tol_3d(0.0), 0.0);
    }

    #[test]
    fn system_tolerances() {
        let k = Kernel::default();
        assert_eq!(zone_tol_3d(&k, SmallTopoType::NormalSize), 1.0e-5);
        assert_eq!(xsect_tol_3d(&k, SmallTopoType::NormalSize), 2.0e-5);
        assert_eq!(approx_tol_3d(&k, SmallTopoType::NormalSize), 5.0e-6);
        assert_eq!(zone_tol_3d(&k, SmallTopoType::SmallSize), 1000.0 * 1.0e-5);
        assert_eq!(
            context_zone_tol_3d(&k, None, SmallTopoType::ForceSmallSize),
            1.0e-5
        );
        assert_eq!(gap_gain(&k), 1.1);
        assert_eq!(model_size_estimate(&k), 50.0);
        let k = kernel(50000.0, -2.0);
        assert_eq!(zone_tol_3d(&k, SmallTopoType::ForceSmallSize), 2.0e-4);
    }

    #[test]
    fn tol_constructors() {
        let k = Kernel::default();
        assert_eq!(tol3d(-0.5), 0.5);
        assert_eq!(zone_tol3d(&k, USE_DEFAULT), 1.0e-5);
        assert_eq!(xsect_tol3d(&k, USE_DEFAULT), 2.0e-5);
        assert_eq!(approx_tol3d(&k, USE_DEFAULT), 5.0e-6);
        assert_eq!(zone_tol3d(&k, -3.0), 3.0);
        assert_eq!(tol_mul(tol3d(-2.0), -3.0), -6.0);
    }

    #[test]
    fn gap_zone_tolerances() {
        let k = Kernel::default();
        assert_eq!(calc_edge_zone_tol_3d(&k, 0.0, 0.0, None, None), 1.0e-5);
        assert_eq!(calc_edge_zone_tol_3d(&k, 2.0, 0.0, None, None), 1.1);
        assert_eq!(
            calc_vertex_zone_tol_3d(&k, 0.0, 0.0, Some(0.25), None),
            0.25
        );
        assert_eq!(calc_face_zone_tol_3d(&k, None), 1.0e-5);
    }

    #[test]
    fn in_tol_and_predicates() {
        // SmTol.cpp sm_TemplateTrial: InTol(.001, .004) etc.
        assert!(in_tol(0.001, 0.004));
        assert!(in_tol(-0.001, 0.004));
        assert!(!in_tol(0.008, 0.004));
        assert!(is_zero_speed(1e-13, 0.0, None));
        assert!(!is_zero_speed(1e-11, 0.0, None));
        assert!(is_zero_speed(0.5, 0.0, Some(1.0)));
        let k = Kernel::default();
        assert!(is_degenerate_vec3(&k, Vec3::new(1e-6, 0.0, 0.0), None));
        assert!(!is_degenerate_vec3(&k, Vec3::new(1e-4, 0.0, 0.0), None));
        assert!(are_parallel(Vec3::X, -Vec3::X, None));
        assert!(!are_parallel(Vec3::X, Vec3::Y, None));
        assert!(are_perpendicular(Vec3::X, Vec3::Y, Some(0.1)));
        assert!(!are_perpendicular(Vec3::X, Vec3::ZERO, Some(0.1)));
    }

    #[test]
    fn scaled_zeros() {
        // SmTol.cpp sm_TemplateTrial values.
        let v2 = Vec2::new(1.0, -80.0);
        let v3 = Vec3::new(16.0, 100.0, 1000.0);
        let e3 = Extent3d::from_bounds(0.0, -30.0, -600.0, 2000.0, 100.0, 10000.0);
        assert_eq!(v2.scaled_zero(), EFF_ZERO * 81.0);
        assert_eq!(v3.scaled_zero(), EFF_ZERO * 1001.0);
        assert_eq!(e3.scaled_zero(), EFF_ZERO * 10001.0);
        assert_eq!(scaled_zero2(&v2, &v3), EFF_ZERO * 1001.0);
        assert_eq!(scaled_zero3(&v2, &v3, &e3), EFF_ZERO * 10001.0);
        assert_eq!(scaled_zero4(&1.0, &v2, &v3, &e3), EFF_ZERO * 10001.0);
        let z = 7.0f64.scaled_zero();
        assert_eq!(scaled_zero_sq_of(&7.0), z * z);
        assert_eq!(scaled_zero_sqrt_of(&7.0), z.sqrt());
        assert_eq!(scaled_zero(), EFF_ZERO);
        assert_eq!(scaled_zero_sq(), EFF_ZERO_SQ);
        assert_eq!(scaled_zero_sqrt(), EFF_ZERO_SQRT);
        assert_eq!(eff_zero_param(), EFF_ZERO_PARAM);
        assert_eq!(eff_zero_ang_rad(), EFF_ZERO_RAD);
        assert_eq!(eff_zero(), EFF_ZERO);
        assert_eq!(eff_zero_sq(), EFF_ZERO_SQ);
    }

    #[test]
    fn relation_cardinalities() {
        let mut r: Relation<u32, u32> = Relation::new(Cardinality::ManyToMany);
        assert!(r.relate_to_many(1, &[5, 3, 4]).is_ok());
        assert!(r.relate_pair(2, 3).is_ok());
        assert!(r.relate_pair(2, 3).is_ok());
        assert_eq!(r.seconds(1), vec![3, 4, 5]);
        assert_eq!(r.second(1), None);
        assert_eq!(r.second(2), Some(3));
        assert_eq!(r.second(9), None);
        assert_eq!(r.all_firsts(), vec![1, 2]);
        assert_eq!(r.all_seconds(), vec![3, 4, 5]);
        r.remove_all();
        assert!(r.all_firsts().is_empty());

        let mut r: Relation<u32, u32> = Relation::new(Cardinality::OneToOne);
        assert!(r.relate_pair(1, 1).is_ok());
        assert!(r.relate_pair(1, 2).is_err());
        assert!(r.relate_pair(2, 1).is_err());
        assert!(r.relate_pair(1, 1).is_err());

        let mut r: Relation<u32, u32> = Relation::new(Cardinality::OneToMany);
        assert!(r.relate_to_many(1, &[1, 2]).is_ok());
        assert!(r.relate_pair(2, 2).is_err());
        assert!(r.relate_to_many(3, &[3, 1, 4]).is_err());
        assert_eq!(r.seconds(3), vec![3]);

        let mut r: Relation<u32, u32> = Relation::new(Cardinality::ManyToOne);
        assert!(r.relate_pair(1, 1).is_ok());
        assert!(r.relate_pair(2, 1).is_ok());
        assert!(r.relate_pair(1, 2).is_err());
        assert_eq!(r.cardinality(), Cardinality::ManyToOne);
        assert_eq!(Cardinality::from_code(2), Some(Cardinality::ManyToOne));
        assert_eq!(
            SmallTopoType::from_code(3).map(SmallTopoType::code),
            Some(3)
        );
    }
}
