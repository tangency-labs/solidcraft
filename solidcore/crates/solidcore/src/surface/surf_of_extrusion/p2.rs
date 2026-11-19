//! `SmSurfOfExtrusion`, part 2: the special-case surface/surface intersections of a
//! surface of extrusion (with a plane, a cylinder and another surface of extrusion,
//! and their dispatcher `GlobalSurfaceIntersect`), the recognition of a B-spline
//! surface as an extrusion (`IsNurbSurfaceSurfOfExtrusion`), trimming to a NURB
//! domain and database input.
//!
//! Upstream: `source/SMLib/src/SmSurfOfExtrusion.cpp`,
//! `source/SMLib/inc/SmSurfOfExtrusion.h`.
//!
//! | Rust                                              | Upstream                                         |
//! |---------------------------------------------------|--------------------------------------------------|
//! | [`SurfOfExtrusion::global_surface_intersect`], [`IntersectTarget`] | `GlobalSurfaceIntersect`        |
//! | [`SurfOfExtrusion::intersect_with_plane`], [`ExtrusionIntersection`] | `IntersectWithPlane`          |
//! | [`SurfOfExtrusion::intersect_with_cylinder`]      | `IntersectWithCylinder`                          |
//! | [`SurfOfExtrusion::intersect_with_surf_of_extrusion`] | `IntersectWithSurfOfExtrusion`               |
//! | [`SurfOfExtrusion::is_nurb_surface_surf_of_extrusion`] | `IsNurbSurfaceSurfOfExtrusion` (static)     |
//! | [`SurfOfExtrusion::trim_with_domain`]             | `TrimWithDomain`                                 |
//! | [`SurfOfExtrusion::read_from_db`]                 | `ReadFromDB` (static)                            |
//!
//! Deviations:
//! - **Free surfaces.** `GlobalSurfaceIntersect` takes its other surface as an
//!   [`IntersectTarget`] (a plane, a cone, a surface of extrusion, or a [`Surface`] of
//!   any kind); `IntersectWithPlane`'s classification and the general intersection
//!   run on ownerless [`Surface`] stand-ins of the surfaces (no face reaches them).
//!   Through
//!   [`crate::surface::surface::SurfaceKind::SurfOfExtrusion`] the `SmSurface` dispatch
//!   passes the real surfaces instead (`global_surface_intersect_on`,
//!   `intersect_with_plane_on`, with the plane's face tolerance for its boxes and the
//!   plane/plane intersection).
//! - **Outputs.** Out-params become return values ([`ExtrusionIntersection`]). Upstream
//!   lets the caller pass `NULL` for each output array, but `IntersectWithPlane`,
//!   `IntersectWithCylinder` and `IntersectWithSurfOfExtrusion` dereference the 3D
//!   curve array unconditionally; here every output is always produced (with
//!   `pOpt3DCurves` and `pOptCurveTypes` present upstream's choices do not depend on
//!   the others). The UV curves upstream adds are `NULL` (`None`). `SmContext`
//!   arguments and the arguments upstream marks `NotUsed` are dropped; the
//!   approximation tolerance is the caller's `double`, converted as upstream's
//!   `SmApproxTol3d` parameter (its magnitude; `SM_USE_DEFAULT` is half the zone
//!   tolerance) by each special case and before the general intersection.
//! - **Statuses.** Calls whose status upstream ignores keep upstream's values, except
//!   that [`SolidError::Unported`] always propagates and a failure whose output
//!   upstream would read uninitialised (the generator's evaluation at a plane
//!   intersection, the projection of the generator, the surface evaluations of
//!   `IsNurbSurfaceSurfOfExtrusion`) is returned. A failed `CreateDirectrixFromPoint`
//!   (`pCrv` stays `NULL`) or `CreateIsoCircleFromPoint` (the circle stays `NULL`)
//!   is skipped as upstream; the null circle is then an `SM_ERR_NULL_POINTER` where
//!   upstream would dereference it (parallel extrusion and cylinder axes).
//!   `IsNurbSurfaceSurfOfExtrusion` returns `SmBoolean`; its `SER`s return the
//!   failing status as the boolean: here they are errors.
//! - **`IntersectWithPlane` tangent heuristic.** The first plane intersection point
//!   is only recorded when the first solution makes a curve (upstream then compares
//!   later points with an uninitialised point): here such a comparison is false.
//! - **Out-of-bounds evaluation.** As in part 1, `SmTemporaryChangeValue` gives the
//!   generator the surface's out-of-bounds flag (only a B-spline generator reads it,
//!   and the surface's setter already keeps both equal).
//! - **`ReadFromDB`.** The generator's owner is not set (as part 1). An orientation
//!   code outside `SmCurveOrientType` is `InvalidInput` (upstream stores it). A
//!   generator not derived from `SmBSplineCurve` is `InvalidInput` (upstream only
//!   asserts, then casts). An ASCII stream whose curve type cannot be read is
//!   `InvalidInput` (upstream reads its uninitialised type variable).
//! - **`IsNurbSurfaceSurfOfExtrusion`** does not copy the attributes of the test
//!   surface (`Notify(SM_NO_COPY, ..)`): a free [`BSplineSurface`] has none here.
//!   `TrimWithDomain`'s edit notifications are not modelled either.
//! - **General intersection.** When a special case needs more, the general
//!   intersection replaces its curves (`SmSurfaceIntersector::DoIntersection` resets
//!   the output arrays upstream).
//! - **`sm_IntersectGenerators` status.** `IntersectWithCylinder` and
//!   `IntersectWithSurfOfExtrusion` ignore it as upstream: a failure keeps the curves
//!   and types already added, with no more intersections needed (upstream clears the
//!   flag before its first failure). [`SolidError::Unported`] and
//!   [`SolidError::InvalidInput`] (an input upstream would not survive) propagate.
//! - **Coincident patch (not upstream).** `IntersectWithPlane` hands a generator lying
//!   in the plane over a range to the general intersection, whose answer then depends
//!   on its tolerance. Here the patch's boundary is the answer (typed
//!   `RegionBoundary`, cut to the plane domain; `plane_patch`), when a control-net
//!   certificate shows the patch in the plane; else the general intersection as
//!   upstream.
//! - **Plane-parallel sweep.** Classify against the supplied plane using the sweep
//!   displacement over the full sweep interval and the generator's control-net
//!   certificate before reducing to intersecting planes.
//!   A normal inferred from a thin generator triangle can lose accuracy when its
//!   points are far from the origin. The in-plane route's deviations include this
//!   displacement plus the profile's residual, including after domain clipping.
//! - **Unresolved zero-tolerance crossing.** If sweep drift rules out the parallel
//!   construction, the general fallback must not claim empty when a positive-weight
//!   control net proves a crossing inside both requested domains. Return an explicit
//!   failure when that crossing cannot be resolved at zero tolerance.
//! - **Debug code.** The `SM_DEBUG_CODE` drawing and dumps are omitted.

use crate::curve::curve::p7::PropertyAnalysisOptions;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::curve::{Curve, CurveKind, CurvePropertyType};
use crate::error::{ignore_status, ignored, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use crate::geom::tol::{approx_tol3d, is_zero};
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::{Point3, Vec3};
use crate::intersect::curve_class::CurveClassification;
use crate::intersect::point_class::{ClassGeometry, ClassObject, ClassObjectKind, PointClassType};
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_PARAM, EFF_ZERO_SQRT, PI};
use crate::nurbs::b_spline_curve::BoundingBoxRequest as CurveBoxRequest;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::{
    BSplineSurface, BoundingBoxRequest as SurfaceBoxRequest, IsoCurveOptions, SurfParamType,
};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::surface::cone::Cone;
use crate::surface::plane::p2::PlaneOwner;
use crate::surface::plane::Plane;
use crate::surface::surface::p1::SurfaceIntersectContext;
use crate::surface::surface::p2::{
    base_global_surface_intersect_in, SurfaceIntersection, TsectCurveType, CONE_TYPE,
};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::surface::volume::Volume;
use crate::topology::database_io::{DatabaseReader, FileType, InputStream};

use super::p1::{
    gen_base, gen_with_out_of_bounds, intersect_generators, CurveOrientType, SurfOfExtrusion,
};
use super::plane_patch::{
    coincident_patch, curve_plane_deviation, curve_source_deviation, generator_crosses_plane,
    generator_in_plane, PatchInputs,
};

const OP_PLANE: &str = "surface::surf_of_extrusion::p2::SurfOfExtrusion::intersect_with_plane";
const OP_CYL: &str = "surface::surf_of_extrusion::p2::SurfOfExtrusion::intersect_with_cylinder";
const OP_EXT: &str =
    "surface::surf_of_extrusion::p2::SurfOfExtrusion::intersect_with_surf_of_extrusion";
const OP_GLOBAL: &str = "surface::surf_of_extrusion::p2::SurfOfExtrusion::global_surface_intersect";
const OP_IS_NURB: &str =
    "surface::surf_of_extrusion::p2::SurfOfExtrusion::is_nurb_surface_surf_of_extrusion";
const OP_TRIM: &str = "surface::surf_of_extrusion::p2::SurfOfExtrusion::trim_with_domain";
const OP_READ: &str = "surface::surf_of_extrusion::p2::SurfOfExtrusion::read_from_db";

/// `ANALYTIC_TOL_SCALE` (`SmConfig.h`).
const ANALYTIC_TOL_SCALE: f64 = 100.0;

// ---- outputs ---------------------------------------------------------------------------

/// The outputs of the special-case intersections (`IntersectWithPlane`,
/// `IntersectWithCylinder`, `IntersectWithSurfOfExtrusion`, and
/// `GlobalSurfaceIntersect` when a special case answers): one entry per curve in each
/// array.
#[derive(Debug, Clone, Default)]
pub struct ExtrusionIntersection {
    /// `rbNeedsMoreIntersections`: the special case could not answer, use the general
    /// surface/surface intersection (always `false` from `GlobalSurfaceIntersect`).
    pub needs_more_intersections: bool,
    /// `pOpt3DCurves`: the intersection curves.
    pub curves_3d: Vec<Curve>,
    /// `pOptSurface1UVCurves`: their UV curves on this surface (`NULL` upstream).
    pub surface1_uv_curves: Vec<Option<Curve>>,
    /// `pOptSurface2UVCurves`: their UV curves on the other surface (`NULL` upstream).
    pub surface2_uv_curves: Vec<Option<Curve>>,
    /// `pOptCurveTypes`: the kind of each intersection.
    pub curve_types: Vec<TsectCurveType>,
    /// `pOptDeviations`: distance bounds from the surfaces. Coincident patch
    /// boundaries carry their certified control-net bound.
    pub deviations: Vec<f64>,
}

impl ExtrusionIntersection {
    /// The "needs the general intersection" answer with empty outputs.
    fn needs_more() -> Self {
        Self {
            needs_more_intersections: true,
            ..Self::default()
        }
    }

    /// Adds the `NULL` UV curves and zero deviations of every curve (the closing
    /// loop of `IntersectWithCylinder` and `IntersectWithSurfOfExtrusion`).
    fn finish_null_outputs(&mut self) {
        for _ in 0..self.curves_3d.len() {
            self.surface1_uv_curves.push(None);
            self.surface2_uv_curves.push(None);
            self.deviations.push(0.0);
        }
    }
}

/// The other surface of [`SurfOfExtrusion::global_surface_intersect`] (upstream's
/// `const SmSurface&`, dispatched on its type).
#[derive(Debug, Clone, Copy)]
pub enum IntersectTarget<'a> {
    /// An `SmPlane`.
    Plane(&'a Plane),
    /// An `SmCone` or `SmCylinder` (only the `SmCone_TYPE` is a special case).
    Cone(&'a Cone),
    /// An `SmSurfOfExtrusion`.
    SurfOfExtrusion(&'a SurfOfExtrusion),
    /// A surface of a ported [`SurfaceKind`].
    Surface(&'a Surface),
}

/// The generator's handle in its plane intersection solutions (upstream's curve
/// pointer): the solver's duplicate test reads its natural interval.
#[derive(Debug, Clone, Copy, PartialEq)]
struct GenHandle {
    ivl: Extent1d,
}

impl SolutionObject for GenHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.ivl)
    }
}

/// `TO_EUCLID(from, to)` (`SmNurbsCrv.h`): divides by the weight unless non-rational;
/// a missing `z` becomes `0`.
fn to_euclid(p: CPoint) -> Point3 {
    if p.w == NOW {
        let z = if p.z == NOZ { 0.0 } else { p.z };
        Point3::new(p.x, p.y, z)
    } else {
        let z = if p.z == NOZ { 0.0 } else { p.z / p.w };
        Point3::new(p.x / p.w, p.y / p.w, z)
    }
}

/// `COPY_XYZ(from, to)` (`SmNurbsCrv.h`): overwrites the stored `x`, `y`, `z`.
fn copy_xyz(from: Point3, to: &mut CPoint) {
    to.x = from.x;
    to.y = from.y;
    to.z = from.z;
}

/// `SM_DEG2RAD`.
fn deg2rad(adeg: f64) -> f64 {
    adeg * (PI / 180.0)
}

/// The handles of the classification `IntersectWithPlane` runs on this surface: the
/// plane/plane line and this surface (upstream's pointers), plus the boundary
/// temporaries the classification makes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ClassifyHandle {
    Curve,
    Surface,
    Temporary(usize, ClassObjectKind),
}

impl ClassObject for ClassifyHandle {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve => ClassObjectKind::Curve,
            Self::Surface => ClassObjectKind::Surface,
            Self::Temporary(_, kind) => *kind,
        }
    }

    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        Some(Self::Temporary(index, kind))
    }
}

/// The geometry behind [`ClassifyHandle`]s.
struct ClassifyGeometry<'a> {
    curve: &'a Curve,
    surface: &'a Surface,
}

impl ClassGeometry<ClassifyHandle> for ClassifyGeometry<'_> {
    fn curve(&self, o: ClassifyHandle) -> Option<&Curve> {
        (o == ClassifyHandle::Curve).then_some(self.curve)
    }

    fn surface(&self, o: ClassifyHandle) -> Option<&Surface> {
        (o == ClassifyHandle::Surface).then_some(self.surface)
    }

    fn volume(&self, _: ClassifyHandle) -> Option<&Volume> {
        None
    }

    fn point(&self, _: ClassifyHandle) -> Option<Point3> {
        None
    }
}

impl From<ExtrusionIntersection> for SurfaceIntersection {
    fn from(x: ExtrusionIntersection) -> Self {
        Self {
            curves_3d: x.curves_3d,
            surface1_uv_curves: x.surface1_uv_curves,
            surface2_uv_curves: x.surface2_uv_curves,
            curve_types: x.curve_types,
            deviations: x.deviations,
        }
    }
}

impl SurfOfExtrusion {
    /// The generator (`m_pGenCurve`).
    fn gen_or_null(&self, op: &'static str) -> Result<&Curve> {
        self.gen_curve
            .as_ref()
            .ok_or_else(|| SolidError::surface_missing(op, "the generator curve"))
    }

    /// Maps only the sweep coordinate, including for a lazy associated NURB.
    fn plane_sweep_interval(&self, k: &Kernel, uv_domain: &Extent2d) -> Option<Extent1d> {
        let natural = self.base_with_nurb(k).ok()?.natural_uv_domain();
        let (caller, nurb) = if self.swap_uv {
            (uv_domain.u_interval(), natural.u_interval())
        } else {
            (uv_domain.v_interval(), natural.v_interval())
        };
        if !nurb.length().is_finite() || nurb.length() <= 0.0 {
            return None;
        }
        let full = self.anal_uv_domain.v_interval();
        let a = full.evaluate((caller.min() - nurb.min()) / nurb.length());
        let b = full.evaluate((caller.max() - nurb.min()) / nurb.length());
        (a.is_finite() && b.is_finite()).then(|| Extent1d::from_min_max(a.min(b), a.max(b)))
    }

    /// Bounds normal drift over both the directrix and caller patch intervals.
    fn plane_sweep_displacement(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        normal: Vec3,
    ) -> Option<f64> {
        let caller = self.plane_sweep_interval(k, uv_domain)?;
        let full = self.anal_uv_domain.v_interval();
        let ends = [caller.min(), caller.max(), full.min(), full.max()];
        if ends.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let max_param = ends.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        // A swept point differs from its generator point by v E. Enclose the
        // dot product and multiplication without subtracting large plane equations.
        let e = self.extrusion_vec;
        let dot_err =
            gamma(5) * ((normal.x * e.x).abs() + (normal.y * e.y).abs() + (normal.z * e.z).abs());
        let bound = (normal.dot(e).abs() + dot_err) * max_param * (1.0 + gamma(2));
        bound.is_finite().then_some(bound)
    }

    // Called only after the special case declined and its general fallback returned
    // no curves. A proved crossing must remain unresolved, rather than become empty,
    // when the zero-tolerance drift certificate rules out a parallel approximation.
    fn check_empty_plane_fallback(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        plane: &Plane,
        other_uv_domain: &Extent2d,
        approx_tol_3d: Option<f64>,
    ) -> Result<()> {
        let Some(gen) = self.gen_curve() else {
            return Ok(());
        };
        let gen_ivl = if self.swap_uv {
            uv_domain.v_interval()
        } else {
            uv_domain.u_interval()
        };
        if approx_tol_3d.is_some_and(|t| approx_tol3d(k, t) == 0.0)
            && self
                .extrusion_vec
                .is_perpendicular_to(plane.position().z_axis(), EFF_ZERO_SQRT)
            && self
                .plane_sweep_displacement(k, uv_domain, plane.position().z_axis())
                .is_some_and(|drift| drift > 0.0)
            && uv_domain.are_equal(&self.base.natural_uv_domain(), 0.0)
            && gen_ivl.are_equal(&gen.natural_interval(), 0.0)
            && self.anal_uv_domain.v_interval().contains_value(0.0, 0.0)
            && generator_crosses_plane(gen, self.extrusion_vec, plane, other_uv_domain)
        {
            return Err(SolidError::surface_failed(
                OP_GLOBAL,
                "a certified plane crossing was not resolved at zero tolerance",
            ));
        }
        Ok(())
    }

    // ---- GlobalSurfaceIntersect -------------------------------------------------------

    /// The intersection curves of this surface over `uv_domain` with `other` over
    /// `other_uv_domain` (`GlobalSurfaceIntersect`): a plane, an `SmCone_TYPE` cone
    /// and a surface of extrusion first try their special case
    /// ([`Self::intersect_with_plane`], [`Self::intersect_with_cylinder`],
    /// [`Self::intersect_with_surf_of_extrusion`]); when that needs more, and for any
    /// other surface, the general intersection (`SmSurface::GlobalSurfaceIntersect`)
    /// runs.
    ///
    /// # Errors
    ///
    /// Those of the special cases and of the general intersection
    /// (`SmAdvSurfaceIntersector`, on ownerless [`Surface`] stand-ins of both
    /// surfaces), including an unresolved certified plane crossing at zero tolerance.
    #[allow(clippy::too_many_arguments)] // upstream's signature
    pub fn global_surface_intersect(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        other: IntersectTarget<'_>,
        other_uv_domain: &Extent2d,
        use_surface_edges: [bool; 2],
        approx_tol_3d: Option<f64>,
        ang_tol_rad: Option<f64>,
    ) -> Result<ExtrusionIntersection> {
        // (the general intersection takes the edge flags and angle tolerance)

        // switch on the other surface's type: a wrapped surface by its kind, as
        // `global_surface_intersect_on_in`
        let target = match other {
            IntersectTarget::Surface(s) => match s.kind_with_nurb(k)? {
                SurfaceKind::Plane(p) => IntersectTarget::Plane(p),
                SurfaceKind::Cone(c) => IntersectTarget::Cone(c.as_ref()),
                SurfaceKind::SurfOfExtrusion(e) => IntersectTarget::SurfOfExtrusion(e.as_ref()),
                _ => other,
            },
            _ => other,
        };
        let special = match target {
            IntersectTarget::Plane(p) => {
                Some(self.intersect_with_plane(k, uv_domain, p, other_uv_domain, approx_tol_3d)?)
            }
            // (an SmCylinder is not SmCone_TYPE: the default branch)
            IntersectTarget::Cone(c) if c.type_code() == CONE_TYPE => {
                Some(self.intersect_with_cylinder(k, c, approx_tol_3d)?)
            }
            IntersectTarget::SurfOfExtrusion(s) => {
                Some(self.intersect_with_surf_of_extrusion(k, s, approx_tol_3d)?)
            }
            _ => None,
        };

        // when bNeedNurbIntersection == TRUE, intersection was too complicated for
        // analytic intersectors
        if let Some(r) = special {
            if !r.needs_more_intersections {
                return Ok(r);
            }
        }
        // SmSurface::GlobalSurfaceIntersect of a free extrusion: the general intersection
        // of ownerless stand-ins of both surfaces (no face reaches them, as upstream for
        // free surfaces); it replaces the special case's curves
        let this = Surface::from(self.clone());
        let stand_in;
        let other = match other {
            IntersectTarget::Surface(s) => s,
            IntersectTarget::Plane(p) => {
                stand_in = Surface::from(p.clone());
                &stand_in
            }
            IntersectTarget::Cone(c) => {
                stand_in = Surface::from(c.clone());
                &stand_in
            }
            IntersectTarget::SurfOfExtrusion(s) => {
                stand_in = Surface::from(s.clone());
                &stand_in
            }
        };
        let r = base_global_surface_intersect_in::<NoObject>(
            k,
            &this,
            uv_domain,
            other,
            other_uv_domain,
            use_surface_edges,
            approx_tol_3d.map(|t| approx_tol3d(k, t)),
            ang_tol_rad,
            None,
        )?;
        if r.curves_3d.is_empty() {
            if let IntersectTarget::Plane(p) = target {
                self.check_empty_plane_fallback(k, uv_domain, p, other_uv_domain, approx_tol_3d)?;
            }
        }
        Ok(ExtrusionIntersection {
            needs_more_intersections: false,
            curves_3d: r.curves_3d,
            surface1_uv_curves: r.surface1_uv_curves,
            surface2_uv_curves: r.surface2_uv_curves,
            curve_types: r.curve_types,
            deviations: r.deviations,
        })
    }

    /// [`Self::global_surface_intersect`] of the surface `this` holding this extrusion
    /// with the surface `other` (the `SmSurface` dispatch): the special cases of a
    /// plane (with its face's tolerance), an `SmCone_TYPE` cone and a surface of
    /// extrusion, then, where they need more, the general intersection of `this` and
    /// `other` (which replaces the special case's curves).
    ///
    /// # Errors
    ///
    /// Those of [`Self::global_surface_intersect`].
    #[allow(clippy::too_many_arguments)] // upstream's signature
    pub(crate) fn global_surface_intersect_on_in<O: SolutionObject + Copy + PartialEq>(
        &self,
        k: &Kernel,
        this: &Surface,
        uv_domain: &Extent2d,
        other: &Surface,
        other_uv_domain: &Extent2d,
        use_surface_edges: [bool; 2],
        approx_tol_3d: Option<f64>,
        ang_tol_rad: Option<f64>,
        context: Option<SurfaceIntersectContext<'_, O>>,
    ) -> Result<SurfaceIntersection> {
        let special = match other.kind_with_nurb(k)? {
            SurfaceKind::Plane(p) => Some(self.intersect_with_plane_on(
                k,
                this,
                uv_domain,
                p,
                PlaneOwner::of(other)?,
                other_uv_domain,
                approx_tol_3d,
            )?),
            // (an SmCylinder is not SmCone_TYPE: the default branch)
            SurfaceKind::Cone(c) if c.type_code() == CONE_TYPE => {
                Some(self.intersect_with_cylinder(k, c, approx_tol_3d)?)
            }
            SurfaceKind::SurfOfExtrusion(s) => {
                Some(self.intersect_with_surf_of_extrusion(k, s, approx_tol_3d)?)
            }
            _ => None,
        };
        if let Some(r) = special {
            if !r.needs_more_intersections {
                return Ok(SurfaceIntersection::from(r));
            }
        }
        // (SmSurfaceIntersector::DoIntersection resets the output arrays: the special
        // case's curves are dropped; the tolerance is the SmApproxTol3d upstream passes
        // on, already converted)
        let r = base_global_surface_intersect_in(
            k,
            this,
            uv_domain,
            other,
            other_uv_domain,
            use_surface_edges,
            approx_tol_3d.map(|t| approx_tol3d(k, t)),
            ang_tol_rad,
            context,
        )?;
        if r.curves_3d.is_empty() {
            if let SurfaceKind::Plane(p) = other.kind_with_nurb(k)? {
                self.check_empty_plane_fallback(k, uv_domain, p, other_uv_domain, approx_tol_3d)?;
            }
        }
        Ok(r)
    }

    // ---- IntersectWithPlane -----------------------------------------------------------

    /// The special cases of the intersection with `plane` over `other_uv_domain`
    /// (`IntersectWithPlane`); `uv_domain` (this surface's NURB domain) gives the
    /// generator's interval for the closed test. The tolerance is the smaller of
    /// `approx_tol_3d` and `SM_EFF_ZERO_SQRT`.
    ///
    /// * A linear or coplanar generator whose swept plane is transverse to the
    ///   plane: the common line classified on this surface and trimmed to the
    ///   pieces inside it (crossing). A generator certified in the supplied plane
    ///   instead takes the coincident-patch construction below.
    /// * A perpendicular or independent generator whose plane is parallel to the
    ///   plane: nothing when the plane cuts the extrusion axis outside the extrusion
    ///   interval (ends snapped within the tolerance), else the generator projected
    ///   onto the plane along the extrusion, trimmed to the plane domain.
    /// * An extrusion parallel to the plane: a directrix through every generator /
    ///   plane intersection point, trimmed to the plane domain (a degenerate one when
    ///   shorter than the tolerance), touching, tangent (generator tangent within 2
    ///   degrees of the plane) or crossing; of close tangent points, and of close
    ///   points at the two ends of a closed generator, only the better solution is
    ///   kept. A range solution (generator in the plane over a range) is a coincident
    ///   patch: its boundary curves typed `RegionBoundary`, cut to the plane domain,
    ///   after the directrices of the point solutions outside the ranges (a point
    ///   solution whose directrix meets the patch declines it; one inside a range
    ///   the patch extends over is absorbed). When the patch fails a certificate, the
    ///   general intersection is needed (as upstream for every range).
    /// * Otherwise the general intersection is needed.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without generator; those of the generator's plane
    /// intersection, evaluations, closed and degenerate tests;
    /// [`SolidError::Unported`]: the first case when the planes meet in a line
    /// (`SmSurface::CurveOnClassify` up to `IntersectNaturalBounds`, unit
    /// `intersect.curve_class_p8`); the status of
    /// the plane/plane intersection is ignored as upstream (a failure gives no
    /// curves).
    pub fn intersect_with_plane(
        &self,
        k: &Kernel,
        uv_domain: &Extent2d,
        plane: &Plane,
        other_uv_domain: &Extent2d,
        approx_tol_3d: Option<f64>,
    ) -> Result<ExtrusionIntersection> {
        // (a free surface: this surface as an ownerless stand-in, the plane faceless)
        let this = Surface::from(self.clone_exact());
        self.intersect_with_plane_on(
            k,
            &this,
            uv_domain,
            plane,
            PlaneOwner::NoFace,
            other_uv_domain,
            approx_tol_3d,
        )
    }

    /// [`Self::intersect_with_plane`] of the surface `this` holding this extrusion (the
    /// classification's surface), with the owner of `plane` (its face's zone tolerance
    /// expands the plane's boxes, as `GetFace()->GetTolerance()` upstream).
    ///
    /// # Errors
    ///
    /// Those of [`Self::intersect_with_plane`].
    #[allow(clippy::too_many_lines)] // one upstream function
    #[allow(clippy::too_many_arguments)] // upstream's arguments and the owners
    pub(crate) fn intersect_with_plane_on(
        &self,
        k: &Kernel,
        this: &Surface,
        uv_domain: &Extent2d,
        plane: &Plane,
        plane_owner: PlaneOwner,
        other_uv_domain: &Extent2d,
        approx_tol_3d: Option<f64>,
    ) -> Result<ExtrusionIntersection> {
        // init output
        let mut out = ExtrusionIntersection::default();
        let mut deviation = 0.0_f64;
        let mut patch_output = None;

        // plane and ExtrusionSurface locals
        let plane_normal = plane.position().z_axis();
        let plane_origin = plane.position().origin();
        let ang_tol = EFF_ZERO_SQRT;
        let tol = match approx_tol_3d {
            Some(t) => approx_tol3d(k, t).min(EFF_ZERO_SQRT),
            None => EFF_ZERO_SQRT,
        };

        let gen_curve_nurb_ivl = if self.swap_uv {
            uv_domain.v_interval()
        } else {
            uv_domain.u_interval()
        };
        let swept_normal = match self.orientation {
            CurveOrientType::Linear => self.curve_vec.cross(self.extrusion_vec),
            CurveOrientType::Coplanar => self.curve_vec,
            _ => Vec3::UNDEF,
        };

        // If we are allowing out-of-bounds eval, then the gen curve needs to too.
        let gen_own = self.gen_or_null(OP_PLANE)?;
        let tmp = gen_with_out_of_bounds(gen_own, self.base.is_out_of_bounds_enabled());
        let gen = tmp.as_ref().unwrap_or(gen_own);
        let sweep_displacement = self
            .plane_sweep_displacement(k, uv_domain, plane_normal)
            .unwrap_or(f64::INFINITY);
        let sweep_parallel_to_plane = self
            .extrusion_vec
            .is_perpendicular_to(plane_normal, ang_tol)
            && sweep_displacement <= tol;
        if sweep_parallel_to_plane {
            deviation = sweep_displacement;
        }

        // When GenCurve is linear or Planar and swept in its own plane
        // and SweptPlanarSurface is not parallel to input Surface
        // Then,
        //   XSect = Line
        // A certified coincident profile uses the patch construction below. Read
        // the supplied plane directly: an inferred normal can lose accuracy on a
        // far, thin triangle. Transverse planar profiles keep classification of
        // the common line, uniting overlapping intervals from distinct roots.
        let coincident_generator =
            sweep_parallel_to_plane && generator_in_plane(gen, self.extrusion_vec, plane);
        if !coincident_generator
            && matches!(
                self.orientation,
                CurveOrientType::Linear | CurveOrientType::Coplanar
            )
            && !swept_normal.is_parallel_to(plane_normal, ang_tol)
        {
            // build a plane bigger than the Surface
            let this_plane = Plane::from_normal(self.curve_point, swept_normal);

            // get the Plane/Plane intersection line trimmed to input Plane
            //  builds an SmLine or an SmBSplineCurve made by CreateDegenerateCurve().
            //  (status ignored upstream: every failure of SmPlane::IntersectWithPlane
            //  comes before its first output, which it resets with the needs-more
            //  flag, so a failure is "no curves, no more intersections")
            // Parallel sweeps share this special case's capped distance tolerance.
            let plane_tol = if sweep_parallel_to_plane {
                Some(tol)
            } else {
                approx_tol_3d.map(|t| approx_tol3d(k, t))
            };
            let (needs_more, plane_x) = match plane.intersect_with_plane_owned(
                k,
                other_uv_domain,
                &this_plane,
                &this_plane.step_uv_domain(),
                plane_tol,
                [plane_owner, PlaneOwner::NoFace],
            ) {
                // (its warning, a status ignored upstream, comes with no curves and no
                // more intersections, as a failure does)
                Ok((needs_more, plane_x, _warning)) => (needs_more, plane_x),
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => (false, SurfaceIntersection::default()),
            };
            out.needs_more_intersections = needs_more;

            // no work - no solutions
            if plane_x.curves_3d.is_empty() {
                return Ok(out);
            }

            // arrive here when two planes intersect in one line
            let mut lines = plane_x.curves_3d;
            if lines.is_empty() {
                return Err(SolidError::surface_failed(
                    OP_PLANE,
                    "the plane intersection gave no line",
                ));
            }
            let line = lines.swap_remove(0);
            let line_ivl = line.natural_interval();

            // Solve near the profile, independently of a distant interval midpoint
            // and the sweep caps. A long bounded fiber query can lose a valid root
            // near its end. The positive control hull encloses every generator
            // point, so this short line still searches every possible root.
            let source_fibers = if sweep_parallel_to_plane {
                let line_data = match line.kind() {
                    CurveKind::Line(l) => {
                        let data = l.canonical();
                        Some((data.point, data.vector))
                    }
                    _ => line.is_line(k, 0, tol)?,
                };
                let Some((line_origin, line_direction)) = line_data else {
                    return Ok(ExtrusionIntersection::needs_more());
                };
                let Some((length, direction)) = line_direction.length_and_unit() else {
                    return Ok(ExtrusionIntersection::needs_more());
                };
                if !length.is_finite() {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                let Some(hull) = gen.piece_hull_points(&gen_curve_nurb_ivl) else {
                    return Ok(ExtrusionIntersection::needs_more());
                };
                let norm1 = |p: Point3| p.x.abs() + p.y.abs() + p.z.abs();
                let Some(&(reference, _)) = hull.first() else {
                    return Ok(ExtrusionIntersection::needs_more());
                };
                let shift = (reference - line_origin).dot(direction);
                if !shift.is_finite() {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                // Any computed shift defines a point on the supporting line.
                // Fused affine coordinates avoid large-product cancellation:
                // their rounding depends on the nearby result, not marker ends.
                let origin = Point3::new(
                    direction.x.mul_add(shift, line_origin.x),
                    direction.y.mul_add(shift, line_origin.y),
                    direction.z.mul_add(shift, line_origin.z),
                );
                if !norm1(origin).is_finite() {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                let mut radius = 0.0_f64;
                for (p, error) in hull {
                    if !norm1(p).is_finite() || !error.is_finite() || error < 0.0 {
                        return Ok(ExtrusionIntersection::needs_more());
                    }
                    radius = radius.max(
                        norm1(p - origin) + 3.0 * error + gamma(16) * (norm1(p) + norm1(origin)),
                    );
                }
                radius = (radius + 1.0) * (1.0 + gamma(16));
                if !radius.is_finite() {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                let query_tol = tol + gamma(32) * (norm1(origin) + radius + shift.abs());
                if !query_tol.is_finite() {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                let fiber = Curve::from(crate::curve::line::Line::bounded(
                    origin,
                    direction,
                    &Extent1d::from_min_max(-radius, radius),
                    &crate::curve::line::BoundedLineOptions::default(),
                ));
                let roots = gen.global_curve_intersect::<NoObject>(
                    k,
                    &gen_curve_nurb_ivl,
                    &fiber,
                    &fiber.natural_interval(),
                    query_tol,
                    &CurveIntersectOptions::default(),
                )?;
                let mut witnesses = Vec::new();
                for root in roots.as_slice() {
                    // A coincident range is a continuum of source fibers; a
                    // single endpoint cannot certify its swept coverage.
                    if root.solution_type == SolutionType::RangeOfValues {
                        return Ok(ExtrusionIntersection::needs_more());
                    }
                    let Some(&t) = root.start.parameters.first() else {
                        return Ok(ExtrusionIntersection::needs_more());
                    };
                    if gen_curve_nurb_ivl.contains_value(t, 0.0) {
                        let Some(error) = gen.point_error_bound(t) else {
                            return Ok(ExtrusionIntersection::needs_more());
                        };
                        witnesses.push((gen.evaluate_point(k, t)?, error));
                    }
                }
                Some(witnesses)
            } else {
                None
            };

            // classify the 3dCurves against the natural boundaries of this surface (for
            // trimming)
            let mut classification =
                CurveClassification::with_curve_interval(k, ClassifyHandle::Curve, line_ivl, tol);
            // Split at requested source bounds as well as natural bounds, so
            // midpoint membership represents an entire classification interval.
            let bounded;
            let classification_surface = if sweep_parallel_to_plane
                && !uv_domain.are_equal(&this.natural_uv_domain_with_nurb(k)?, 0.0)
            {
                if !uv_domain.is_contained_by(&this.natural_uv_domain_with_nurb(k)?, 0.0) {
                    return Ok(ExtrusionIntersection::needs_more());
                }
                let mut extrusion = self.clone_exact();
                extrusion.trim_with_domain(k, uv_domain)?;
                bounded = Surface::from(extrusion);
                &bounded
            } else {
                this
            };
            classification_surface.curve_on_classify(
                k,
                ClassifyHandle::Surface,
                &ClassifyGeometry {
                    curve: &line,
                    surface: classification_surface,
                },
                &mut classification,
            )?;

            // for every classification interval in this surface (the line moves to the
            // output with the first whole-curve interval; later copies copy it there)
            let mut line = Some(line);
            let mut moved_to = 0;
            for interval in classification.intervals() {
                // skip intervals outside the surface
                let class = interval.mid.point_class();
                if matches!(class, PointClassType::Unknown | PointClassType::Nothing) {
                    continue;
                }

                let mut interval_fibers = None;
                if let Some(witnesses) = &source_fibers {
                    // Normal projection can label exterior intervals as Surface.
                    // Accept a midpoint only when a generator witness reaches it
                    // inside the actual bounded sweep, allowing evaluation roundoff.
                    let point = line
                        .as_ref()
                        .or_else(|| out.curves_3d.get(moved_to))
                        .ok_or_else(|| {
                            SolidError::surface_failed(
                                OP_PLANE,
                                "the classification line is missing",
                            )
                        })?
                        .evaluate_point(k, interval.interval().mid())?;
                    let Some(caller_sweep) = self.plane_sweep_interval(k, uv_domain) else {
                        return Ok(ExtrusionIntersection::needs_more());
                    };
                    let full_sweep = self.anal_uv_domain.v_interval();
                    let lo = caller_sweep.min().max(full_sweep.min());
                    let hi = caller_sweep.max().min(full_sweep.max());
                    if lo > hi {
                        continue;
                    }
                    let e2 = self.extrusion_vec.dot(self.extrusion_vec);
                    if !e2.is_finite() || e2 <= 0.0 {
                        return Ok(ExtrusionIntersection::needs_more());
                    }
                    let norm1 = |p: Point3| p.x.abs() + p.y.abs() + p.z.abs();
                    let mut accepted = Vec::new();
                    for &(c, error) in witnesses {
                        let v = ((point - c).dot(self.extrusion_vec) / e2).max(lo).min(hi);
                        let q = c + v * self.extrusion_vec;
                        let roundoff = 3.0 * error
                            + gamma(32)
                                * (norm1(point) + norm1(c) + v.abs() * norm1(self.extrusion_vec));
                        let distance = (point - q).length();
                        if !distance.is_finite() || !roundoff.is_finite() {
                            return Ok(ExtrusionIntersection::needs_more());
                        }
                        if distance <= tol + roundoff {
                            accepted.push((c, error));
                        }
                    }
                    if accepted.is_empty() {
                        continue;
                    }
                    interval_fibers = Some((accepted, Extent1d::from_min_max(lo, hi)));
                }

                // add interval to output
                let mut ivl = *interval.interval();

                // Move the whole line once; otherwise trim a copy.
                let curve = if ivl.are_equal(&line_ivl, EFF_ZERO) {
                    match line.take() {
                        Some(l) => {
                            moved_to = out.curves_3d.len();
                            l
                        }
                        None => out
                            .curves_3d
                            .get(moved_to)
                            .ok_or_else(|| {
                                SolidError::surface_failed(
                                    OP_PLANE,
                                    "the moved intersection curve is missing",
                                )
                            })?
                            .clone(),
                    }
                } else {
                    let source = line
                        .as_ref()
                        .or_else(|| out.curves_3d.get(moved_to))
                        .ok_or_else(|| {
                            SolidError::surface_failed(
                                OP_PLANE,
                                "the moved intersection curve is missing",
                            )
                        })?;
                    let mut curve = source.copy(k)?;
                    ignore_status(curve.trim(k, &mut ivl, TrimMode::Notify))?;
                    curve
                };
                if let Some((witnesses, sweep)) = interval_fibers {
                    let Some(bound) =
                        curve_source_deviation(&curve, &witnesses, self.extrusion_vec, sweep)
                    else {
                        return Ok(ExtrusionIntersection::needs_more());
                    };
                    deviation = deviation.max(bound);
                }
                out.curves_3d.push(curve);
                out.curve_types.push(TsectCurveType::Crossing);
            }
        }
        // When GenCurve is planar with an independent sweep direction
        // and crPlane and plane containing GenCurve are parallel
        // Then,
        //   XSect = translated copy of sweptCurve
        else if matches!(
            self.orientation,
            CurveOrientType::Perpendicular | CurveOrientType::Independent
        ) && self.curve_vec.is_parallel_to(plane_normal, ang_tol)
        {
            // get param along Extrusion Vector where InputPlane intersects
            // ExtrusionVector (AngleBetween's status ignored: 0 on failure)
            let dist_center_base_to_plane = (plane_origin - self.origin).dot(self.curve_vec);
            let angle_rad = self
                .curve_vec
                .angle_between(self.extrusion_vec)
                .unwrap_or(0.0);
            let dist_along_extrusion = dist_center_base_to_plane * angle_rad.cos();
            let param_along_extrusion = dist_along_extrusion / self.extrusion_vec.length();

            // get extrusionSurface's sweep interval
            let orig_ivl = self.anal_uv_domain.v_interval();

            // snap nearby distanceValues to extrusion begin / end position (the
            // snapped value is not read again), skip planes outside of the
            // extrusion domain
            let snapped = (param_along_extrusion - orig_ivl.min()).abs() < tol
                || (param_along_extrusion - orig_ivl.max()).abs() < tol;
            if !snapped && !orig_ivl.contains_value(param_along_extrusion, 0.0) {
                return Ok(out); // No intersections
            }

            // intersectionCurve = sweptCurveCopy projected to the plane along the
            // extrusion vector (status ignored upstream, which then reads the curve)
            let proj_curve = gen_base(gen)?.create_plane_projection(
                crate::nurbs::b_spline_curve::ProjectionType::Parallel,
                plane_origin,
                plane_normal,
                self.extrusion_vec,
            )?;

            // get plane and intersection curve bounding boxes (statuses ignored: a
            // failed box stays initialised)
            // (bExpandPosBoxesByZoneTol3d defaults to TRUE)
            let plane_req = SurfaceBoxRequest {
                pseudo: true,
                expand_by_zone_tol: true,
                // (the zone tolerance of the plane's face)
                expand_tolerance: plane_owner.expand_tolerance()?,
                ..SurfaceBoxRequest::default()
            };
            let plane_nurb = plane.base_with_nurb()?;
            let mut plane_pbox = ignored(plane_nurb.calculate_bounding_box(
                k,
                &plane.natural_uv_domain(),
                &plane_req,
            ))?
            .and_then(|b| b.pseudo)
            .unwrap_or_default();
            let curve_req = CurveBoxRequest {
                pseudo: true,
                ..CurveBoxRequest::default()
            };
            let curve_pbox = ignored(
                proj_curve.calculate_bounding_box(&proj_curve.natural_interval(), curve_req),
            )?
            .and_then(|b| b.pseudo)
            .unwrap_or_default();

            // only use curves intersecting plane domain
            plane_pbox.expand_absolute(tol);
            if !plane_pbox.are_disjoint(&curve_pbox, None) {
                // trim curves to plane domain (status ignored upstream)
                let proj = Curve::from(proj_curve);
                let curves_3d =
                    ignored(plane.trim_curve_to_plane_domain(k, other_uv_domain, &proj, tol))?
                        .unwrap_or_default();

                // add trim curves to output
                out.curves_3d.extend(curves_3d);
            }
        }
        // when extrusion vec is perpendicular to plane normal
        // - xsect is one or more lines each line generated by sweeping
        //   a plane/GenCurve intersection point.
        else if sweep_parallel_to_plane {
            // get sweptCurve/plane intersection point
            let d = -plane_origin.dot(plane_normal);
            let vecs = [plane_normal];
            let opts = PropertyAnalysisOptions {
                property_value: Some(d),
                opt_vectors: &vecs,
                want_comb: false,
                handle: Some(GenHandle {
                    ivl: gen.natural_interval(),
                }),
            };
            let solutions = gen
                .global_property_analysis(
                    k,
                    &gen.natural_interval(),
                    CurvePropertyType::PlaneIntersection,
                    tol,
                    &opts,
                )?
                .solutions;
            let sols = solutions.as_slice();

            let is_closed = gen.is_closed(k, &gen_curve_nurb_ivl, 0.0)?;

            // for every SweptCurve/Plane intersection: a range is a Surf/Surf
            // coincidence. Not upstream (which hands it to the general intersection,
            // whose answer depends on its tolerance): the coincident patch's boundary,
            // when certified in the plane (else the general intersection as upstream).
            let first_param = |p: &[f64]| p.first().copied().unwrap_or(f64::NAN);
            let ranges: Vec<(f64, f64)> = sols
                .iter()
                .filter(|s| s.solution_type == SolutionType::RangeOfValues)
                .map(|s| {
                    (
                        first_param(&s.start.parameters),
                        first_param(&s.end.parameters),
                    )
                })
                .collect();
            // the point solutions outside the ranges (all of them without a range)
            let in_range = |t: f64| {
                ranges
                    .iter()
                    .any(|&(a, b)| a.min(b) - EFF_ZERO_PARAM <= t && t <= a.max(b) + EFF_ZERO_PARAM)
            };
            let mut sols: Vec<_> = sols
                .iter()
                .filter(|s| {
                    s.solution_type != SolutionType::RangeOfValues
                        && !in_range(first_param(&s.start.parameters))
                })
                .collect();

            let points: Vec<f64> = sols
                .iter()
                .map(|s| first_param(&s.start.parameters))
                .collect();
            let mut patch = None;
            if !ranges.is_empty() {
                let inputs = PatchInputs {
                    gen,
                    extrusion_vec: self.extrusion_vec,
                    gen_ivl: gen_curve_nurb_ivl,
                    gen_closed: is_closed,
                    v_ivl: if self.swap_uv {
                        uv_domain.u_interval()
                    } else {
                        uv_domain.v_interval()
                    },
                    plane,
                    plane_domain: *other_uv_domain,
                    tol,
                };
                let Some(p) = coincident_patch(k, &inputs, &ranges, &points)? else {
                    // Some Surf/Surf coincidence - handle it at a higher level
                    out.needs_more_intersections = true;
                    return Ok(out);
                };
                // (the point solutions inside the ranges the patch extends over are
                // part of it)
                sols.retain(|s| {
                    let t = first_param(&s.start.parameters);
                    !p.ranges
                        .iter()
                        .any(|&(a, b)| a - EFF_ZERO_PARAM <= t && t <= b + EFF_ZERO_PARAM)
                });
                patch = Some(p);
            }
            // arrive here for the PointSolutions

            // for every SweptCurve/Plane point intersection - build, trim and if
            // nonNULL, output a line
            let mut last_tangent = false;
            let mut last_crv_pt = Vec3::UNDEF;
            let mut first_crv_pt: Option<Point3> = None;
            for (ii, sol) in sols.iter().enumerate() {
                // create line segment intersection curve (Evaluate's status ignored
                // upstream, which then reads the points)
                let crv_pt = gen.evaluate(k, sol.start.parameters[0], 1, true, true)?;
                let (Some(&pt0), Some(&pt1)) = (crv_pt.first(), crv_pt.get(1)) else {
                    return Err(SolidError::surface_failed(
                        OP_PLANE,
                        "the generator evaluation gave no point and tangent",
                    ));
                };
                let crv = match self.create_directrix_from_point(k, pt0, EFF_ZERO_SQRT) {
                    Ok(c) => c,
                    Err(e) if e.is_unported_or_cancelled() => return Err(e),
                    Err(_) => continue, // pCrv stays NULL
                };
                let mut crv = crv;

                // trim line control Points to plane
                let nurb = crv.get_or_create_gw_nurb()?;
                let (Some(&pw0), Some(&pw1)) = (nurb.cpts().first(), nurb.cpts().get(1)) else {
                    return Err(SolidError::surface_failed(
                        OP_PLANE,
                        "the directrix has fewer than two control points",
                    ));
                };
                let start_pt = to_euclid(pw0);
                let end_pt = to_euclid(pw1);
                let Some((mut start_pt, mut end_pt)) =
                    plane.trim_plane_line_to_plane_domain(other_uv_domain, start_pt, end_pt, tol)
                else {
                    // only use curves that intersect plane domain
                    continue;
                };

                // get trimmed xsectCurve length
                let length = start_pt.distance(end_pt);

                // turn short curves into degenerate ones by snapping endPoints
                if length < tol {
                    // make a degenerate line
                    start_pt = (start_pt + end_pt) / 2.0;
                    end_pt = start_pt;
                }

                // Trim the curve definition - move control points
                let nurb = crv.get_or_create_gw_nurb()?;
                let cpts = nurb.cpts_mut();
                if let Some(p) = cpts.get_mut(0) {
                    copy_xyz(start_pt, p);
                }
                if let Some(p) = cpts.get_mut(1) {
                    copy_xyz(end_pt, p);
                }

                // classify the solution - crossing or tangent
                let crv_speed = pt1.length();
                let scaled_zero = EFF_ZERO * (1.0 + crv_speed.max(pt0.max_dimension()));
                let angle_tol = deg2rad(2.0);
                let tangent = crv_speed > scaled_zero
                    && (pt1 / crv_speed).is_perpendicular_to(plane_normal, angle_tol);

                // cheesy heuristic - GlobalPropertyAnalysis returns multiple close hits
                // for a single tangent intersection and intersections on closed seam
                // boundaries; the solutions are sorted by param value - so just save
                // the best one.
                if ii > 0 {
                    let value = sol.start.solution_value;
                    // remove multiple solutions due to tangent points
                    if tangent && last_tangent && last_crv_pt.distance(pt0) < tol {
                        let prev = sols.get(ii - 1).map_or(0.0, |s| s.start.solution_value);
                        if value < prev {
                            // replace the last saved solution with this solution
                            out.curves_3d.pop();
                            out.curve_types.pop();
                        } else {
                            // don't save this solution
                            continue;
                        }
                    }
                    // remove multiple solutions due to close curve start/end hits
                    // (the other end of the seam)
                    else if is_closed && first_crv_pt.is_some_and(|f| f.distance(pt0) < tol) {
                        let first = sols.first().map_or(0.0, |s| s.start.solution_value);
                        if value < first {
                            // replace the first saved solution with this solution
                            if !out.curves_3d.is_empty() {
                                out.curves_3d.remove(0);
                            }
                            if !out.curve_types.is_empty() {
                                out.curve_types.remove(0);
                            }
                        } else {
                            // don't save this solution
                            continue;
                        }
                    }
                }

                // prepare for next iteration
                last_tangent = tangent;
                last_crv_pt = pt0;
                if ii == 0 {
                    first_crv_pt = Some(pt0);
                }

                // add curve to ouput
                let curve_type = if crv.is_degenerate(k, EFF_ZERO, None)? {
                    TsectCurveType::Touching
                } else if tangent {
                    TsectCurveType::Tangent
                } else {
                    TsectCurveType::Crossing
                };
                out.curves_3d.push(Curve::from(crv));
                out.curve_types.push(curve_type);
            }

            // the coincident patches' boundaries (not upstream)
            if let Some(p) = patch {
                // The patch certificate covers profile residual and boundary rounding;
                // add sweep drift even when clipping makes every side on the plane.
                let bound = (p.deviation + sweep_displacement) * (1.0 + gamma(1));
                patch_output = Some((out.curves_3d.len(), bound));
                for (c, t) in p.curves {
                    out.curves_3d.push(c);
                    out.curve_types.push(t);
                }
            }
        } else {
            // this is not a special case intersection:
            //  plane cuts through SurfOfExtrusion at an angle,
            out.needs_more_intersections = true;
            return Ok(out);
        }

        // Retain the source profile certificate as well as the residual of the
        // constructed curves: plane-domain sides can be offset from the extrusion.
        if sweep_parallel_to_plane {
            for curve in &out.curves_3d {
                let Some(bound) = curve_plane_deviation(curve, plane) else {
                    return Ok(ExtrusionIntersection::needs_more());
                };
                deviation = deviation.max(bound);
            }
        }

        // set output
        let cnt = out.curves_3d.len();
        for ii in 0..cnt {
            out.surface1_uv_curves.push(None);
            out.surface2_uv_curves.push(None);
            // Line solutions have already added the curve types
            if cnt != out.curve_types.len() {
                let crv = out.curves_3d.get(ii).ok_or_else(|| {
                    SolidError::surface_failed(OP_PLANE, "an intersection curve is missing")
                })?;
                let t = if crv.is_degenerate(k, EFF_ZERO, None)? {
                    TsectCurveType::Touching
                } else {
                    TsectCurveType::Crossing
                };
                out.curve_types.push(t);
            }
            let deviation = patch_output
                .filter(|(start, _)| ii >= *start)
                .map_or(deviation, |(_, bound)| deviation.max(bound));
            out.deviations.push(deviation);
        }

        // all done
        Ok(out)
    }

    // ---- IntersectWithCylinder --------------------------------------------------------

    /// The intersection with the cone `cylinder` when its radii agree within
    /// `SM_EFF_ZERO * (1 + max radius)` (`IntersectWithCylinder`): the generators'
    /// intersection of [`intersect_generators`] with the cylinder's bottom iso circle
    /// swept along its axis over its heights (lines along a cylinder axis parallel to
    /// the extrusion). Otherwise, and for an axis not parallel to the extrusion, the
    /// general intersection is needed. The tolerance is `approx_tol_3d`
    /// (`SM_EFF_ZERO_SQRT` without).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` when the iso circle could not be made for a parallel
    /// axis (upstream dereferences `NULL`), and when this surface has none;
    /// [`SolidError::Unported`] and [`SolidError::InvalidInput`] of
    /// [`intersect_generators`] (its other failures are ignored as upstream).
    pub fn intersect_with_cylinder(
        &self,
        k: &Kernel,
        cylinder: &Cone,
        approx_tol_3d: Option<f64>,
    ) -> Result<ExtrusionIntersection> {
        // locals
        let other_bot_radius = cylinder.bot_radius();
        let other_top_radius = cylinder.top_radius();

        // no work - other surface not a rectilinear cylinder:
        let radius_diff_tolerance = EFF_ZERO * (1.0 + other_bot_radius.max(other_top_radius));
        if (other_bot_radius - other_top_radius).abs() > radius_diff_tolerance {
            return Ok(ExtrusionIntersection::needs_more());
        }

        // pick tolerance
        let tol = approx_tol_3d.map_or(EFF_ZERO_SQRT, |t| approx_tol3d(k, t));

        // otherCylinderSurface locals
        let other_position = cylinder.rev().position();
        let other_origin = other_position.origin();
        let other_z_axis = other_position.z_axis();
        let other_ivl = Extent1d::from_min_max(cylinder.bot_height(), cylinder.top_height());
        let other_axis_segment_start_point = other_origin + other_ivl.min() * other_z_axis;
        // (CreateIsoCircleFromPoint's status ignored: the circle stays NULL)
        let other_gen_curve =
            match cylinder.create_iso_circle_from_point(k, other_axis_segment_start_point, tol) {
                Ok(c) => c,
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(_) => None,
            };

        // compute intersections based on overlap of extrusion lines that coalign from
        // the GenCurves
        let mut out = ExtrusionIntersection::default();
        out.needs_more_intersections = self.intersect_generators_with(
            k,
            tol,
            other_gen_curve.as_ref(),
            other_z_axis,
            &other_ivl,
            &mut out,
            OP_CYL,
        )?;

        // for every intersection found - finish the outputs
        out.finish_null_outputs();
        Ok(out)
    }

    // ---- IntersectWithSurfOfExtrusion -------------------------------------------------

    /// The intersection with the surface of extrusion `other`
    /// (`IntersectWithSurfOfExtrusion`): the generators' intersection of
    /// [`intersect_generators`] with the other generator swept along its extrusion
    /// over its STEP V interval (lines along parallel extrusions); not parallel
    /// extrusions need the general intersection. The tolerance is `approx_tol_3d`
    /// (`SM_EFF_ZERO_SQRT` without).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for parallel extrusions when `other` has no generator
    /// (upstream dereferences `NULL`), and when this surface has none;
    /// [`SolidError::Unported`] and [`SolidError::InvalidInput`] of
    /// [`intersect_generators`] (its other failures are ignored as upstream).
    pub fn intersect_with_surf_of_extrusion(
        &self,
        k: &Kernel,
        other: &Self,
        approx_tol_3d: Option<f64>,
    ) -> Result<ExtrusionIntersection> {
        // pick tolerance
        let tol = approx_tol_3d.map_or(EFF_ZERO_SQRT, |t| approx_tol3d(k, t));

        // OtherExtrusionSurface Extrusion Segment
        let other_extrusion_vec = other.extrusion_vector();
        let other_ivl = other.step_uv_domain().v_interval();
        let other_gen_curve = other.gen_curve();

        // compute intersections based on overlap of extrusion lines that coalign from
        // the GenCurves
        let mut out = ExtrusionIntersection::default();
        out.needs_more_intersections = self.intersect_generators_with(
            k,
            tol,
            other_gen_curve,
            other_extrusion_vec,
            &other_ivl,
            &mut out,
            OP_EXT,
        )?;

        // for every intersection found - finish the outputs
        out.finish_null_outputs();
        Ok(out)
    }

    /// `sm_IntersectGenerators(*this, tol, *pOtherGenCurve, ...)` with a generator
    /// that may be `NULL`: dereferenced only past the parallel test.
    #[allow(clippy::too_many_arguments)]
    fn intersect_generators_with(
        &self,
        k: &Kernel,
        tol: f64,
        other_gen: Option<&Curve>,
        other_vec: Vec3,
        other_ivl: &Extent1d,
        out: &mut ExtrusionIntersection,
        op: &'static str,
    ) -> Result<bool> {
        // (sm_IntersectGenerators reads this surface's generator before anything
        // else: upstream dereferences NULL)
        self.gen_or_null(op)?;
        let Some(other_gen) = other_gen else {
            // (sm_IntersectGenerators returns at its parallel test before touching
            // the other generator)
            if !other_vec.is_parallel_to(self.extrusion_vec, crate::math::EFF_ZERO_RAD) {
                return Ok(true);
            }
            return Err(SolidError::surface_missing(op, "the other generator curve"));
        };
        // (status ignored upstream: every failure comes after sm_IntersectGenerators
        // cleared the needs-more flag, and keeps the curves and types it already
        // added; an Unported callee, or an input upstream would not survive, still
        // propagates)
        match intersect_generators(
            k,
            self,
            tol,
            other_gen,
            other_vec,
            other_ivl,
            Some(&mut out.curves_3d),
            Some(&mut out.curve_types),
        ) {
            Ok(r) => Ok(r.needs_more_intersections),
            Err(e)
                if e.is_unported_or_cancelled()
                    || matches!(
                        e,
                        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }
                    ) =>
            {
                Err(e)
            }
            Err(_) => Ok(false),
        }
    }

    // ---- IsNurbSurfaceSurfOfExtrusion -------------------------------------------------

    /// The surface of extrusion whose B-spline surface equals `test_surface`, if it is
    /// one (`IsNurbSurfaceSurfOfExtrusion`): degree 1 with 2 control points in exactly
    /// one direction (the sweep), and the iso curve at the end of the sweep a
    /// translation of the one at its start (control points within `tol =
    /// tolerance_scale * ANALYTIC_TOL_SCALE * SM_EFF_ZERO * (1 + |min corner| + |max
    /// corner|)`, weight ratios too for a rational surface). The generator is the
    /// start iso curve (translated to sweep parameter 0); the extrusion vector spans
    /// one unit of the sweep parameter; the NURB is reparameterised to the test
    /// surface's domain. `None` when the surface is not an extrusion.
    ///
    /// # Errors
    ///
    /// Those of the iso curves, their control polygons and the surface evaluations
    /// (upstream returns the failing status as its boolean), and of the new surface's
    /// construction.
    pub fn is_nurb_surface_surf_of_extrusion(
        k: &Kernel,
        test_surface: &BSplineSurface,
        tolerance_scale: f64,
    ) -> Result<Option<Self>> {
        // surface locals
        let deg_u = test_surface.degree(SurfParamType::U);
        let deg_v = test_surface.degree(SurfParamType::V);

        // check parameterization
        // - must be deg 1 in just one direction and
        //   have more than 2 control points in the other direction
        // (both linear: a bilinear or plane; neither linear; or a linear direction with
        // more than 2 control points: only 2-CPnt lines)
        let (lin_u, lin_v) = (deg_u == 1, deg_v == 1);
        if lin_u == lin_v
            || (lin_u && test_surface.number_control_points(SurfParamType::U) > 2)
            || (lin_v && test_surface.number_control_points(SurfParamType::V) > 2)
        {
            return Ok(None);
        }

        // surface locals
        let nurb_uv_domain = test_surface.natural_uv_domain();

        // get a domain corner surface point to act as the sweep curve's end point
        let (surf_param, end_param, start_knot, end_knot, swap_uv) = if deg_u == 1 {
            (
                SurfParamType::U,
                nurb_uv_domain.evaluate(1.0, 0.0),
                nurb_uv_domain.min().x, // begin sweep direction Domain Value
                nurb_uv_domain.max().x, // end   sweep direction Domain Value
                true,                   // so that the STEP standard of linear in V is maintained.
            )
        } else {
            (
                SurfParamType::V,
                nurb_uv_domain.evaluate(0.0, 1.0),
                nurb_uv_domain.min().y,
                nurb_uv_domain.max().y,
                false,
            )
        };

        // evaluate domain upper and lower corner surface points and sweep end point
        // (statuses ignored upstream, which then reads the points)
        let mut origin = test_surface.evaluate_point(nurb_uv_domain.min())?;
        let max_pnt = test_surface.evaluate_point(nurb_uv_domain.max())?;
        let line_end = test_surface.evaluate_point(end_param)?;

        // compute a tolerance
        let pt_tol = tolerance_scale
            * ANALYTIC_TOL_SCALE
            * EFF_ZERO
            * (1.0 + origin.max_dimension() + max_pnt.max_dimension());

        // let extrusion vector = sweep line endPoint - begPoint
        let mut extrusion_vec = line_end - origin;
        let extrusion_length = extrusion_vec.length();

        // not an extrusion surface - sweep vector is zero length
        if extrusion_length < EFF_ZERO {
            return Ok(None);
        }

        // build GenCurve = Start of sweep IsoParameter Curve
        let iso_opts = IsoCurveOptions {
            domain: None,
            uv_iso_curve: false,
        };
        let mut gen_curve = test_surface
            .create_iso_parametric_curve(k, surf_param, start_knot, iso_opts)?
            .curve;

        // Get GenCurve locals
        let crv_step_ivl = gen_curve.step_interval();

        // convert GenCurve rational to euclidean
        let (start_ctrl_pts, start_weights) = gen_curve.control_polygon()?;
        let rational = gen_curve.is_rational()?;

        // build EndCurve = End of Sweep IsoParameter Curve
        let end_crv = test_surface
            .create_iso_parametric_curve(k, surf_param, end_knot, iso_opts)?
            .curve;

        // convert EndCurve rational to euclidean
        let (end_ctrl_pts, end_weights) = end_crv.control_polygon()?;

        // test every end curve controlPoint for constant sweep vectors
        let weight = |w: &[f64], i: usize| -> Result<f64> {
            w.get(i).copied().ok_or_else(|| {
                SolidError::surface_failed(OP_IS_NURB, "an end curve weight is missing")
            })
        };
        for (i, end_pt) in end_ctrl_pts.iter().enumerate() {
            let start_pt = start_ctrl_pts.get(i).ok_or_else(|| {
                SolidError::surface_failed(
                    OP_IS_NURB,
                    "the start curve has fewer control points than the end curve",
                )
            })?;

            // get sVec = this controlPoint sweep - sweepVector
            let vec = (*end_pt - *start_pt) - extrusion_vec;

            // when any of the EndControlPoints is not a constant sweep of the
            // StartControlPoints this is not a SurfOfExtrusion
            if vec.length() > pt_tol {
                return Ok(None);
            }

            // for rational surfaces also test the weights: the ratio between the
            // weights must be constant
            if rational
                && (weight(&start_weights, i)? * weight(&end_weights, 0)?
                    - weight(&end_weights, i)? * weight(&start_weights, 0)?)
                .abs()
                    > pt_tol
            {
                return Ok(None);
            }
        }

        // surface is an extrusion - build the output surface

        // scale extrusion by the sweep range to match ExtrusionLength and domain ranges
        extrusion_vec /= end_knot - start_knot;

        // UVDomain is not required to start at 0.0 so GenCurve has to be translated
        // to keep AnalUVDomain == NurbUVDomain and to have equivalent evaluations
        if !is_zero(start_knot) {
            let mut shift_rf = Axis2Placement::IDENTITY;
            shift_rf.translate(-start_knot * extrusion_vec);

            // translate (status ignored)
            ignore_status(gen_curve.transform(&shift_rf, None))?;
            origin -= start_knot * extrusion_vec;
        }

        // build new SmSurfOfExtrusion - constructor sets all internal values
        let anal_uv_domain = Extent2d::from_points(
            Point2::new(crv_step_ivl.min(), start_knot),
            Point2::new(crv_step_ivl.max(), end_knot),
        );
        let mut surf = Self::new(
            k,
            Curve::from(gen_curve),
            origin,
            extrusion_vec,
            anal_uv_domain,
            swap_uv,
        )?;

        // reparameterize the NURB so that AnalyticSurface->NurbDomain ==
        // InputSurface->NurbDomain (status ignored)
        ignore_status(surf.reparameterize(k, &test_surface.natural_uv_domain()))?;

        // all done
        Ok(Some(surf))
    }

    // ---- TrimWithDomain ---------------------------------------------------------------

    /// Trims the surface to the NURB domain `new_nurb_uv_domain` (`TrimWithDomain`):
    /// the analytic domain and the generator follow
    /// ([`Self::update_analytical_domain`], status ignored), then the NURB is rebuilt
    /// and reparameterised to the new domain. Nothing changes when the domain is the
    /// current one (within `SM_EFF_ZERO_PARAM`). A missing NURB is made first
    /// (`GetNaturalUVDomain`, status ignored).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the domain is not inside the natural domain (within
    /// `SM_EFF_ZERO_PARAM`); those of [`Self::make_nurb`] and
    /// [`Self::reparameterize`].
    pub fn trim_with_domain(&mut self, k: &Kernel, new_nurb_uv_domain: &Extent2d) -> Result<()> {
        // locals (GetNaturalUVDomain makes a missing NURB; its status is ignored)
        if self.base.gw_nurb().is_none() {
            ignore_status(self.make_nurb(k))?;
        }
        let current_nurb_uv_domain = self.base.natural_uv_domain();

        // check input - new trim UV domain must be inside current natural UV domain
        if !new_nurb_uv_domain.is_contained_by(&current_nurb_uv_domain, EFF_ZERO_PARAM) {
            return Err(SolidError::surface_failed(
                OP_TRIM,
                "the trim domain is outside the natural domain",
            ));
        }

        // no work - new and current domains are same size
        if current_nurb_uv_domain.is_contained_by(new_nurb_uv_domain, EFF_ZERO_PARAM) {
            return Ok(());
        }

        // Update just the analytic domain and the GenCurve (both analytic and NURB)
        // (status ignored)
        ignore_status(self.update_analytical_domain(k, new_nurb_uv_domain))?;

        // Now rebuild the Nurbs surface using the updated analytic domain.
        self.make_nurb(k)?;
        self.reparameterize(k, new_nurb_uv_domain)?;

        // all done
        Ok(())
    }

    // ---- ReadFromDB -------------------------------------------------------------------

    /// Reads a surface of extrusion (`ReadFromDB`): the origin, extrusion vector,
    /// analytic domain corners, orientation, swap flag, generator plane point and
    /// vector, then the generator curve (its type header and record), then the
    /// B-spline surface record. An analytic domain with `min > max` is not set (the
    /// empty surface's domain stays).
    ///
    /// # Errors
    ///
    /// Those of the stream reads, of `SmCurve::ReadFromDB` and of
    /// `SmBSplineSurface::ReadFromDB`; `InvalidInput` for an orientation code outside
    /// `SmCurveOrientType`, a generator not derived from `SmBSplineCurve`, or an ASCII
    /// curve type header that could not be read.
    pub fn read_from_db(k: &Kernel, db: &mut DatabaseReader, version: u64) -> Result<Self> {
        // init output object
        let mut surf = Self::empty();

        // locals
        let mut orig = Vec3::ZERO;
        let mut extrusion_vec = Vec3::ZERO;
        let mut anal_min = Point2::ZERO;
        let mut anal_max = Point2::ZERO;
        let mut orientation: u64 = 0;
        let mut swap_uv = false;
        let mut curve_point = Vec3::ZERO;
        let mut curve_vec = Vec3::ZERO;

        // file type
        let ascii = db.file_type() == FileType::Ascii;
        if ascii {
            let dbl = |db: &mut DatabaseReader, slot: &mut f64| {
                if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_double) {
                    *slot = v;
                }
            };
            dbl(db, &mut orig.x);
            dbl(db, &mut orig.y);
            dbl(db, &mut orig.z);
            db.go_to_next_line();
            dbl(db, &mut extrusion_vec.x);
            dbl(db, &mut extrusion_vec.y);
            dbl(db, &mut extrusion_vec.z);
            db.go_to_next_line();

            dbl(db, &mut anal_min.x);
            dbl(db, &mut anal_min.y);
            dbl(db, &mut anal_max.x);
            dbl(db, &mut anal_max.y);
            db.go_to_next_line();

            if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_ulong) {
                orientation = v;
            }
            db.go_to_next_line();
            if let Some(v) = db.in_stream_mut().and_then(InputStream::extract_int) {
                swap_uv = v != 0;
            }
            db.go_to_next_line();

            dbl(db, &mut curve_point.x);
            dbl(db, &mut curve_point.y);
            dbl(db, &mut curve_point.z);
            db.go_to_next_line();
            dbl(db, &mut curve_vec.x);
            dbl(db, &mut curve_vec.y);
            dbl(db, &mut curve_vec.z);
            db.go_to_next_line();
        } else {
            orig.x = db.read_double()?;
            orig.y = db.read_double()?;
            orig.z = db.read_double()?;

            extrusion_vec.x = db.read_double()?;
            extrusion_vec.y = db.read_double()?;
            extrusion_vec.z = db.read_double()?;

            anal_min.x = db.read_double()?;
            anal_min.y = db.read_double()?;
            anal_max.x = db.read_double()?;
            anal_max.y = db.read_double()?;

            orientation = db.read_ulong()?;

            swap_uv = db.read_boolean()? != 0;

            curve_point.x = db.read_double()?;
            curve_point.y = db.read_double()?;
            curve_point.z = db.read_double()?;

            curve_vec.x = db.read_double()?;
            curve_vec.y = db.read_double()?;
            curve_vec.z = db.read_double()?;
        }

        // base curve
        if ascii {
            db.go_to_next_line();
        }
        let header = db.read_type(true)?;
        let gen_type = header
            .type_code
            .ok_or(SolidError::invalid_input(OP_READ, "curve type not read"))?;
        let gen_dim = header.dim.ok_or(SolidError::invalid_input(
            OP_READ,
            "curve dimension not read",
        ))?;
        let gen_curve = Curve::read_from_db(k, gen_type, db, gen_dim, version)?;
        if gen_base(&gen_curve).is_err() {
            return Err(SolidError::invalid_input(
                OP_READ,
                "generator is not a kind of SmBSplineCurve",
            ));
        }

        // load the analytic
        surf.gen_curve = Some(gen_curve);
        surf.origin = orig;
        surf.extrusion_vec = extrusion_vec;
        // (SetMinMax's status ignored: an inverted domain is not set)
        let _ = surf.anal_uv_domain.set_min_max(anal_min, anal_max);
        surf.orientation = orientation_from_code(orientation)?;
        surf.swap_uv = swap_uv;
        surf.set_curve_point(curve_point);
        surf.set_curve_vec(curve_vec);

        // read the parent object
        surf.base = BSplineSurface::read_from_db(db, version)?;

        // all done
        Ok(surf)
    }
}

/// The `SmCurveOrientType` of a stored code.
fn orientation_from_code(code: u64) -> Result<CurveOrientType> {
    Ok(match code {
        0 => CurveOrientType::NotPlanar,
        1 => CurveOrientType::Perpendicular,
        2 => CurveOrientType::Independent,
        3 => CurveOrientType::Coplanar,
        4 => CurveOrientType::Linear,
        5 => CurveOrientType::Undefined,
        _ => {
            return Err(SolidError::invalid_input(
                OP_READ,
                "orientation code outside SmCurveOrientType",
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::line::Line;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::topology::database_io::DatabaseReader;

    fn k() -> Kernel {
        Kernel::default()
    }

    /// A quarter-circle-like quadratic B-spline in the XY plane.
    fn arc_gen() -> Curve {
        let pw = vec![
            CPoint::new(1.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 1.0, 0.0, NOW),
            CPoint::new(0.0, 1.0, 0.0, NOW),
        ];
        let cur = NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(cur, 3, false))
    }

    fn extrusion() -> SurfOfExtrusion {
        let k = k();
        SurfOfExtrusion::create_canonical(&k, arc_gen(), Vec3::new(0.0, 0.0, 2.0)).unwrap()
    }

    #[test]
    fn surface_wrapped_cone_takes_the_cone_special_case() {
        let k = k();
        // a line along X swept along Z, against a vertical SmCone_TYPE cylinder shape
        let pw = vec![
            CPoint::new(-2.0, 0.0, 0.0, NOW),
            CPoint::new(2.0, 0.0, 0.0, NOW),
        ];
        let line = NurbCurve::new(pw, 1, vec![0.0, 0.0, 1.0, 1.0]).unwrap();
        let s = SurfOfExtrusion::create_canonical(
            &k,
            Curve::from(BSplineCurve::from_nurb(line, 3, false)),
            Vec3::new(0.0, 0.0, 2.0),
        )
        .unwrap();
        let cone =
            Cone::create_canonical(&k, &Axis2Placement::IDENTITY, 1.0, 1.0, 3.0, false, false)
                .unwrap();
        assert_eq!(cone.type_code(), CONE_TYPE);
        let wrapped = Surface::from(cone.clone());
        let d = s.base().natural_uv_domain();
        let od = cone.rev().base().natural_uv_domain();
        let run = |target| {
            s.global_surface_intersect(&k, &d, target, &od, [true, true], None, None)
                .unwrap()
        };
        let direct = run(IntersectTarget::Cone(&cone));
        let via_surface = run(IntersectTarget::Surface(&wrapped));
        // the direct cone takes the special case: curves, no UV curves (NULL upstream)
        assert!(!direct.curves_3d.is_empty());
        assert!(direct.surface1_uv_curves.iter().all(Option::is_none));
        // upstream switches on the other surface's type (SmSurfOfExtrusion.cpp:1677-1695
        // at c9c5979: SmPlane_TYPE, SmCone_TYPE, SmSurfOfExtrusion_TYPE), so the wrapped
        // cone takes the same special case
        assert_eq!(direct.curves_3d.len(), via_surface.curves_3d.len());
        assert_eq!(direct.curve_types, via_surface.curve_types);
        let none = |r: &ExtrusionIntersection| {
            r.surface1_uv_curves
                .iter()
                .map(Option::is_none)
                .collect::<Vec<_>>()
        };
        assert_eq!(none(&direct), none(&via_surface));
    }

    #[test]
    fn nurb_of_an_extrusion_is_recognised() {
        let k = k();
        let s = extrusion();
        let mut found = SurfOfExtrusion::is_nurb_surface_surf_of_extrusion(&k, s.base(), 1.0)
            .unwrap()
            .unwrap();
        // (the domains already agree: Reparameterize leaves the NURB to be made)
        assert!(found.base().gw_nurb().is_none());
        found.make_nurb(&k).unwrap();
        let p = found.base().evaluate_point(Point2::new(0.5, 0.25)).unwrap();
        let q = s.base().evaluate_point(Point2::new(0.5, 0.25)).unwrap();
        assert!(p.distance(q) < 1e-12);
    }

    #[test]
    fn plane_parallel_to_the_extrusion_gives_directrices() {
        let k = k();
        let s = extrusion();
        // the plane x = 0.5 (normal along X) cuts the arc once
        let plane = Plane::from_normal(Point3::new(0.5, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        let mut plane = plane;
        plane.make_nurb().unwrap();
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.natural_uv_domain(),
                None,
            )
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert_eq!(r.curves_3d.len(), 1);
        assert_eq!(r.curve_types, vec![TsectCurveType::Crossing]);
        assert_eq!(r.deviations.len(), r.curves_3d.len());
        for (curve, &deviation) in r.curves_3d.iter().zip(&r.deviations) {
            assert!(deviation <= EFF_ZERO_SQRT);
            let net = gen_base(curve).unwrap().canonical().unwrap();
            assert_eq!(net.degree, 1);
            for p in net.control_points {
                assert!((p.x - 0.5).abs() <= deviation);
            }
        }
    }

    /// A directrix at plane u = 0 misses a caller domain beginning at u = 0.25.
    /// When it is included, the caller's v bounds clip its endpoints as well.
    #[test]
    fn parallel_directrix_respects_the_requested_plane_domain() {
        let k = k();
        let surfaces = [
            line_extrusion(&k),
            swept(
                &k,
                &[(0.0, 1.0, 0.0), (0.5, -1.0, 0.0), (1.0, 1.0, 0.0)],
                Some(1.0),
                Vec3::Z,
            ),
        ];
        let natural = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let plane = Plane::new(
            Point3::new(0.5, 0.0, 0.0),
            Vec3::Y,
            Vec3::Z,
            crate::geom::vector2d::Vec2::new(1.0, 1.0),
            &natural,
        );
        for s in surfaces {
            for (umin, expected) in [(0.25, 0), (-0.25, 1)] {
                let domain = Extent2d::from_bounds(umin, 0.25, 1.0, 0.75);
                let r = s
                    .intersect_with_plane(&k, &s.base().natural_uv_domain(), &plane, &domain, None)
                    .unwrap();
                assert!(!r.needs_more_intersections);
                assert_eq!(r.curves_3d.len(), expected);
                for curve in &r.curves_3d {
                    let points = gen_base(curve).unwrap().canonical().unwrap();
                    let mut heights: Vec<f64> = points
                        .control_points
                        .iter()
                        .map(|p| {
                            assert!((p.x - 0.5).abs() < 1e-12);
                            assert!(p.y.abs() < 1e-12);
                            p.z
                        })
                        .collect();
                    heights.sort_by(f64::total_cmp);
                    assert_eq!(heights.len(), 2);
                    // The directrix is built at the infinite marker endpoints before
                    // clipping, so allow their interpolation's rounding error.
                    let roundoff = gamma(16) * crate::math::INFINITE_PARAMETER;
                    assert!((heights[0] - 0.25).abs() <= roundoff);
                    assert!((heights[1] - 0.75).abs() <= roundoff);
                }
            }
        }
    }

    /// A rotated parabolic cylinder has a plane crossing at s ~= 3.38834,
    /// for every v in [0,1]. A failed zero-tolerance trace must not claim empty.
    #[test]
    fn zero_tolerance_plane_fallback_does_not_claim_empty() {
        let k = k();
        let interval = Extent1d::from_min_max(0.8356207714308481, 4.420145634594439);
        let generator = crate::curve::parabola::Parabola::new(
            Point3::ZERO,
            Vec3::new(-0.5222483795623447, -0.5164133560594942, 0.6786559332444352),
            Vec3::new(0.7920279342718203, -0.588734748734649, 0.16150277695898263),
            interval,
            0.23681154935845689,
            3,
        );
        let domain = Extent2d::from_bounds(interval.min(), 0.0, interval.max(), 1.0);
        let mut s = SurfOfExtrusion::new(
            &k,
            Curve::from(generator),
            Point3::ZERO,
            Vec3::new(
                -0.3291151918968259,
                -0.6473691290951337,
                -0.7458712366999675,
            ),
            domain,
            false,
        )
        .unwrap();
        s.make_nurb(&k).unwrap();
        let plane = Plane::from_normal(
            Point3::new(-0.3132374718803482, -2.289191366692601, 2.0336234358415344),
            Vec3::new(-0.4900887030536102, -0.539591788730856, 0.6845829129283505),
        );
        let od = plane.natural_uv_domain();
        let d = s.base().natural_uv_domain();
        // Endpoint signs hold at both sweep ends; affine dependence on v keeps
        // them opposite throughout [0,1], so continuity proves a root in every slice.
        let n = plane.position().z_axis();
        let o = plane.position().origin();
        for v in [0.0, 1.0] {
            let low = s
                .evaluate_step_point(&k, Point2::new(interval.min(), v))
                .unwrap();
            let high = s
                .evaluate_step_point(&k, Point2::new(interval.max(), v))
                .unwrap();
            assert!(n.dot(low - o) < -2.0);
            assert!(n.dot(high - o) > 1.0);
        }
        let special = s
            .intersect_with_plane(&k, &d, &plane, &od, Some(0.0))
            .unwrap();
        assert!(special.needs_more_intersections);
        let wrapped_plane = Surface::from(plane.clone());
        let check_error = |error: SolidError| {
            assert_eq!(error.operation(), OP_GLOBAL);
            assert!(matches!(
                error.surface_failure(),
                Some(crate::error::surface::SurfaceFailure::Failed(reason))
                    if reason.contains("certified plane crossing") && reason.contains("zero tolerance")
            ));
        };
        for target in [
            IntersectTarget::Plane(&plane),
            IntersectTarget::Surface(&wrapped_plane),
        ] {
            match s.global_surface_intersect(&k, &d, target, &od, [true, true], Some(0.0), None) {
                Ok(result) => assert!(
                    !result.curves_3d.is_empty(),
                    "a crossing is not an empty intersection"
                ),
                Err(error) => check_error(error),
            }
        }
        // A disjoint plane still gives a successful empty result at zero tolerance.
        let missed = Plane::from_normal(o + 100.0 * n, n);
        let empty = s
            .global_surface_intersect(
                &k,
                &d,
                IntersectTarget::Plane(&missed),
                &missed.natural_uv_domain(),
                [true, true],
                Some(0.0),
                None,
            )
            .unwrap();
        assert!(empty.curves_3d.is_empty());
        let wrapped_s = Surface::from(s);
        match wrapped_s.global_surface_intersect(
            &k,
            &d,
            &wrapped_plane,
            &od,
            [true, true],
            Some(0.0),
            None,
        ) {
            Ok(result) => assert!(
                !result.curves_3d.is_empty(),
                "surface dispatch must retain the unresolved result"
            ),
            Err(error) => check_error(error),
        }
    }

    /// Opposite endpoint signs may jump across the plane at a full-multiplicity knot.
    #[test]
    fn discontinuous_generator_does_not_certify_a_plane_crossing() {
        // Each linear piece has constant x=-1 or x=1, so neither meets x=0.
        let net = NurbCurve::new(
            vec![
                CPoint::new(-1.0, 0.0, 0.0, NOW),
                CPoint::new(-1.0, 1.0, 0.0, NOW),
                CPoint::new(1.0, 2.0, 0.0, NOW),
                CPoint::new(1.0, 3.0, 0.0, NOW),
            ],
            1,
            vec![0.0, 0.0, 1.0, 1.0, 2.0, 2.0],
        )
        .unwrap();
        let curve = Curve::from(BSplineCurve::from_nurb(net, 3, false));
        let plane = Plane::from_normal(Point3::ZERO, Vec3::X);
        assert!(!generator_crosses_plane(
            &curve,
            Vec3::Z,
            &plane,
            &plane.natural_uv_domain()
        ));
    }

    /// Two generator roots sweep the same line where z = 0 meets x = 0.
    /// The intersection is their union, a single segment along the Y axis.
    #[test]
    fn parallel_coplanar_sweep_intersection_is_not_duplicated() {
        let k = k();
        for (finite, drift) in [(false, 0.0), (true, 0.0), (true, 2.0_f64.powi(-44))] {
            let mut s = swept(
                &k,
                &[(-1.0, -1.0, 0.0), (1.0, 0.0, 0.0), (-1.0, 1.0, 0.0)],
                None,
                Vec3::new(drift, if finite { 2.0 } else { 1.0 }, 0.0),
            );
            if finite {
                s.trim_with_domain(&k, &Extent2d::from_bounds(0.0, 0.0, 2.0, 1.0))
                    .unwrap();
            }
            let bound = if finite { 3.0 } else { 2.0 };
            let domain = Extent2d::from_bounds(-bound, -1.0, bound, 1.0);
            let plane = Plane::new(
                Point3::ZERO,
                Vec3::Y,
                Vec3::Z,
                crate::geom::vector2d::Vec2::new(1.0, 1.0),
                &domain,
            );
            let r = s
                .intersect_with_plane(&k, &s.base().natural_uv_domain(), &plane, &domain, None)
                .unwrap();
            assert!(!r.needs_more_intersections);
            assert!(!r.curves_3d.is_empty());
            assert!(r.curve_types.iter().all(|&t| t == TsectCurveType::Crossing));
            // This classification route also retains the full sweep-drift bound.
            assert!(r.deviations.iter().all(|&d| d >= drift));
            let mut intervals: Vec<_> = r
                .curves_3d
                .iter()
                .map(|curve| {
                    let interval = curve.natural_interval();
                    let a = curve.evaluate_point(&k, interval.min()).unwrap();
                    let b = curve.evaluate_point(&k, interval.max()).unwrap();
                    assert!(a.x.abs() < 1e-9 && a.z.abs() < 1e-9);
                    assert!(b.x.abs() < 1e-9 && b.z.abs() < 1e-9);
                    (a.y.min(b.y), a.y.max(b.y))
                })
                .collect();
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            // In the finite case, [-0.5, 1.5] union [0.5, 2.5] is [-0.5, 2.5].
            let (start, finish) = if finite { (-0.5, 2.5) } else { (-2.0, 2.0) };
            let mut end = start;
            for (lo, hi) in intervals {
                // Adjacent pieces may partition the answer, but never overlap.
                assert!((lo - end).abs() < 1e-9);
                assert!(hi > lo);
                end = hi;
            }
            assert!((end - finish).abs() < 1e-9);
        }
    }

    /// Caller NURB sweep bounds must be converted before testing STEP fibers.
    #[test]
    fn parallel_coplanar_sweep_respects_reparameterized_source_domain() {
        let k = k();
        let drift = 2.0_f64.powi(-44);
        let mut s = swept(
            &k,
            &[(-1.0, -1.0, 0.0), (1.0, 0.0, 0.0), (-1.0, 1.0, 0.0)],
            None,
            Vec3::new(drift, 2.0, 0.0),
        );
        s.trim_with_domain(&k, &Extent2d::from_bounds(0.0, 0.0, 2.0, 1.0))
            .unwrap();
        s.reparameterize(&k, &Extent2d::from_bounds(0.0, 10.0, 2.0, 20.0))
            .unwrap();
        let source_domain = Extent2d::from_bounds(0.0, 11.0, 2.0, 12.0);
        let domain = Extent2d::from_bounds(-3.0, -1.0, 3.0, 1.0);
        let plane = Plane::new(
            Point3::ZERO,
            Vec3::Y,
            Vec3::Z,
            crate::geom::vector2d::Vec2::new(1.0, 1.0),
            &domain,
        );
        let bound = s
            .plane_sweep_displacement(&k, &source_domain, Vec3::X)
            .unwrap();
        assert!(bound >= drift && bound <= drift * (1.0 + 1e-10));
        let r = s
            .intersect_with_plane(&k, &source_domain, &plane, &domain, None)
            .unwrap();
        assert!(!r.needs_more_intersections);
        let mut intervals: Vec<_> = r
            .curves_3d
            .iter()
            .map(|curve| {
                let interval = curve.natural_interval();
                let a = curve.evaluate_point(&k, interval.min()).unwrap().y;
                let b = curve.evaluate_point(&k, interval.max()).unwrap().y;
                (a.min(b), a.max(b))
            })
            .collect();
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(intervals.len(), 2);
        // v=[.1,.2] sweeps roots y=-.5,+.5 by [.2,.4].
        for (actual, expected) in intervals.iter().zip([(-0.3, -0.1), (0.7, 0.9)]) {
            assert!((actual.0 - expected.0).abs() < 1e-9, "{intervals:?}");
            assert!((actual.1 - expected.1).abs() < 1e-9, "{intervals:?}");
        }
    }

    #[test]
    fn parallel_flat_generator_range_declines_without_partial_coverage() {
        let k = k();
        let s = swept(
            &k,
            &[
                (-1.0, 0.0, 0.0),
                (0.0, 0.0, 0.0),
                (0.0, 10.0, 0.0),
                (1.0, 10.0, 0.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = Plane::from_normal(Point3::ZERO, Vec3::X);
        let result = s
            .intersect_with_plane(
                &k,
                &Extent2d::from_bounds(0.0, 0.0, 3.0, 1.0),
                &plane,
                &plane.natural_uv_domain(),
                None,
            )
            .unwrap();
        // The complete coincident generator span sweeps Y=[0,11]. Keeping
        // only either endpoint's fiber would lose its middle.
        assert!(result.needs_more_intersections);
        assert!(result.curves_3d.is_empty());
    }

    #[test]
    fn parallel_rational_sweep_keeps_both_far_tails() {
        let k = k();
        let a = 2.6732068221646728;
        let b = 6.494791779629098;
        let net = NurbCurve::new(
            vec![
                CPoint::new(
                    0.7787532160592693,
                    0.41516730641944366,
                    0.0,
                    1.058369559093775,
                ),
                CPoint::new(
                    1.6449735124840872,
                    -1.639375175700008,
                    0.0,
                    0.9771324625698774,
                ),
                CPoint::new(
                    3.095321822027647,
                    -1.6984076619721575,
                    0.0,
                    1.255089018424894,
                ),
                CPoint::new(
                    5.598401909924472,
                    -1.2735666915485684,
                    0.0,
                    1.9935203457971689,
                ),
            ],
            3,
            vec![a, a, a, a, b, b, b, b],
        )
        .unwrap();
        let s = SurfOfExtrusion::create_canonical(
            &k,
            Curve::from(BSplineCurve::from_nurb(net, 3, false)),
            Vec3::new(0.6300223112498824, 0.0, 0.0),
        )
        .unwrap();
        let plane = Plane::from_normal(
            Point3::new(
                2.5943748446032284,
                -0.8692503151488464,
                5.923072763725463e-09,
            ),
            Vec3::new(0.0, -0.08134453898884497, 0.9966860418289665),
        );
        let result = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.natural_uv_domain(),
                Some(1e-6),
            )
            .unwrap();
        assert!(!result.needs_more_intersections);
        let mut intervals = Vec::new();
        for curve in &result.curves_3d {
            let ivl = curve.natural_interval();
            let p = curve.evaluate_point(&k, ivl.min()).unwrap();
            let q = curve.evaluate_point(&k, ivl.max()).unwrap();
            intervals.push((p.x.min(q.x), p.x.max(q.x)));
        }
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Independent cubic roots give C.x=1.601203589467039 and
        // 2.663509647870795. Their sweeps overlap; retain both outside tails.
        assert!(!intervals.is_empty());
        assert!((intervals[0].0 + 777803.1535292441).abs() < 1e-6);
        assert!((intervals.last().unwrap().1 - 777807.4182424814).abs() < 1e-6);
        for pair in intervals.windows(2) {
            assert!((pair[0].1 - pair[1].0).abs() < 1e-8, "{intervals:?}");
        }
    }

    #[test]
    fn parallel_line_deviation_covers_bounded_source_endpoints() {
        let k = k();
        let a = Point3::new(1.6424282115340212, 4.191812055542643, -4.002740938533824);
        let b = Point3::new(a.x, 5.734466112976998, -4.320040760290739);
        let sweep = Extent1d::from_min_max(2.736341766260848, 8.229820056406474);
        let mut line = Line::from_points(a, b, 3);
        line.make_nurb().unwrap();
        let mut s = SurfOfExtrusion::new(
            &k,
            Curve::from(line),
            Point3::new(2.1834703041803483, 4.7236608744811885, -4.159176543571766),
            Vec3::new(0.001, 0.0, 0.0),
            Extent2d::from_bounds(0.0, sweep.min(), 1.0, sweep.max()),
            true,
        )
        .unwrap();
        s.make_nurb(&k).unwrap();
        s.set_out_of_bounds_enabled(true);
        let plane = Plane::from_normal(
            Point3::new(1.645727841106027, b.y, b.z),
            Vec3::new(0.0, -0.8238675923367362, -0.5667823129714524),
        );
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.natural_uv_domain(),
                Some(0.0),
            )
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert_eq!(r.curves_3d.len(), 1);
        let curve = &r.curves_3d[0];
        let ivl = curve.natural_interval();
        let p = curve.evaluate_point(&k, ivl.min()).unwrap();
        let q = curve.evaluate_point(&k, ivl.max()).unwrap();
        let expected_min = b.x + 0.001 * sweep.min();
        let expected_max = b.x + 0.001 * sweep.max();
        // Plane incidence alone cannot bound tangential endpoint error.
        let deviation = r.deviations[0];
        assert!((p.x.min(q.x) - expected_min).abs() <= deviation);
        assert!((p.x.max(q.x) - expected_max).abs() <= deviation);
        assert!(deviation < EFF_ZERO_SQRT);
    }

    #[test]
    fn oblique_plane_needs_more() {
        let k = k();
        let s = extrusion();
        let plane = Plane::from_normal(Point3::ZERO, Vec3::new(1.0, 0.0, 1.0));
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.step_uv_domain(),
                None,
            )
            .unwrap();
        assert!(r.needs_more_intersections);
    }

    /// A line along X extruded along Z: its swept plane is `y = 0`.
    fn line_extrusion(k: &Kernel) -> SurfOfExtrusion {
        let mut line = Line::from_points(Point3::ZERO, Point3::new(1.0, 0.0, 0.0), 3);
        line.make_nurb().unwrap();
        SurfOfExtrusion::create_canonical(k, Curve::from(line), Vec3::new(0.0, 0.0, 1.0)).unwrap()
    }

    #[test]
    fn linear_generator_and_missed_bounded_plane_give_nothing() {
        let k = k();
        let s = line_extrusion(&k);
        // a bounded plane x = 0 over y in [49, 51]: the swept plane misses it
        let dom = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let mut plane = Plane::new(
            Point3::new(0.0, 50.0, 0.0),
            Vec3::Y,
            Vec3::Z,
            crate::geom::vector2d::Vec2::new(1.0, 1.0),
            &dom,
        );
        plane.make_nurb().unwrap();
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.step_uv_domain(),
                None,
            )
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert!(r.curves_3d.is_empty());
    }

    #[test]
    fn linear_generator_line_is_classified_on_the_surface() {
        let k = k();
        let s = line_extrusion(&k);
        let plane = Plane::from_normal(Point3::new(0.5, 0.0, 0.0), Vec3::X);
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.step_uv_domain(),
                None,
            )
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert!(!r.curves_3d.is_empty());
        assert_eq!(r.curves_3d.len(), r.curve_types.len());
        assert!(r.curve_types.iter().all(|t| *t == TsectCurveType::Crossing));
        assert_eq!(r.surface1_uv_curves.len(), r.curves_3d.len());
    }

    /// A curve through `pts` (degree 1), or a rational quadratic arc through
    /// `pts` with middle weight `w`, swept along `e`.
    fn swept(k: &Kernel, pts: &[(f64, f64, f64)], w: Option<f64>, e: Vec3) -> SurfOfExtrusion {
        let pw: Vec<CPoint> = pts
            .iter()
            .enumerate()
            .map(|(i, &(x, y, z))| {
                match w {
                    // homogeneous coordinates, end weights 1
                    Some(w) => {
                        let wi = if i == 1 { w } else { 1.0 };
                        CPoint::new(x * wi, y * wi, z * wi, wi)
                    }
                    None => CPoint::new(x, y, z, NOW),
                }
            })
            .collect();
        let cur = if w.is_some() {
            NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
        } else {
            let n = pts.len();
            let mut kv = vec![0.0];
            kv.extend((0..n).map(|i| i as f64));
            kv.push((n - 1) as f64);
            NurbCurve::new(pw, 1, kv).unwrap()
        };
        SurfOfExtrusion::create_canonical(k, Curve::from(BSplineCurve::from_nurb(cur, 3, false)), e)
            .unwrap()
    }

    /// Checks the coincident patch answer of `s` and the plane `z = 0` over `od`:
    /// answered, curves in the plane and its domain, closed `RegionBoundary` loops.
    /// Returns the answer.
    fn patch_in_xy_plane(k: &Kernel, s: &SurfOfExtrusion, plane: &Plane) -> ExtrusionIntersection {
        let od = plane.natural_uv_domain();
        let r = s
            .intersect_with_plane(k, &s.base().natural_uv_domain(), plane, &od, None)
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert_eq!(r.curve_types.len(), r.curves_3d.len());
        assert_eq!(r.deviations.len(), r.curves_3d.len());
        assert!(r.curve_types.contains(&TsectCurveType::RegionBoundary));
        let mut ends = Vec::new();
        for (c, t) in r.curves_3d.iter().zip(&r.curve_types) {
            let ivl = c.natural_interval();
            for i in 0..=8 {
                let p = c
                    .evaluate(
                        k,
                        ivl.min() + ivl.length() * f64::from(i) / 8.0,
                        0,
                        true,
                        true,
                    )
                    .unwrap()[0];
                let uv = plane.project_point_to_uv_domain(p);
                if *t == TsectCurveType::RegionBoundary {
                    assert!(p.z.abs() <= 1e-12, "in the plane: {p:?}");
                }
                assert!(
                    od.u_interval().min() - 1e-12 <= uv.x
                        && uv.x <= od.u_interval().max() + 1e-12
                        && od.v_interval().min() - 1e-12 <= uv.y
                        && uv.y <= od.v_interval().max() + 1e-12,
                    "inside the plane's domain: {p:?}"
                );
            }
            if *t == TsectCurveType::RegionBoundary {
                for t in [ivl.min(), ivl.max()] {
                    ends.push(c.evaluate(k, t, 0, true, true).unwrap()[0]);
                }
            }
        }
        for (i, p) in ends.iter().enumerate() {
            let met = ends
                .iter()
                .enumerate()
                .any(|(j, q)| j != i && p.distance(*q) <= 1e-12);
            assert!(met, "open boundary end {p:?}");
        }
        r
    }

    /// The plane `z = 0` over the UV box `dom` (unit scale).
    fn xy_plane(dom: Extent2d) -> Plane {
        let mut plane = Plane::new(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            crate::geom::vector2d::Vec2::new(1.0, 1.0),
            &dom,
        );
        plane.make_nurb().unwrap();
        plane
    }

    /// Asserts the answer is exactly the segments `expected` (each once, either
    /// orientation, ends within `1e-9` relative): the boundary is complete, and no
    /// curve repeats or overlaps another.
    fn assert_segments(k: &Kernel, r: &ExtrusionIntersection, expected: &[[(f64, f64); 2]]) {
        let got: Vec<(Point3, Point3)> = r
            .curves_3d
            .iter()
            .map(|c| {
                let ivl = c.natural_interval();
                let a = c.evaluate(k, ivl.min(), 0, true, true).unwrap()[0];
                let b = c.evaluate(k, ivl.max(), 0, true, true).unwrap()[0];
                // a straight segment: its middle is the ends' midpoint
                let m = c.evaluate(k, ivl.mid(), 0, true, true).unwrap()[0];
                assert!(m.distance((a + b) / 2.0) <= 1e-9 * (1.0 + a.max_dimension()));
                (a, b)
            })
            .collect();
        let near = |p: Point3, q: (f64, f64)| {
            let q = Point3::new(q.0, q.1, 0.0);
            p.distance(q) <= 1e-9 * (1.0 + q.max_dimension())
        };
        assert_eq!(got.len(), expected.len(), "{got:?}");
        let mut used = vec![false; got.len()];
        for [p, q] in expected {
            let hit = got.iter().enumerate().position(|(i, &(a, b))| {
                !used[i] && ((near(a, *p) && near(b, *q)) || (near(a, *q) && near(b, *p)))
            });
            let Some(i) = hit else {
                panic!("missing {p:?}-{q:?} in {got:?}");
            };
            used[i] = true;
        }
    }

    /// A bounded plane covering part of an in-plane extrusion (`mu = x` monotone):
    /// the region `0.5 <= x <= 2` of the box, closed by the domain's side `x = 0.5`.
    #[test]
    fn extrusion_in_a_bounded_plane_gives_the_clipped_patch() {
        let k = k();
        let s = swept(
            &k,
            &[(0.0, 0.0, 0.0), (1.0, 0.5, 0.0), (2.0, 0.0, 0.0)],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(0.5, -5.0, 5.0, 5.0));
        let r = patch_in_xy_plane(&k, &s, &plane);
        assert_segments(
            &k,
            &r,
            &[
                [(0.5, -5.0), (0.5, 5.0)],
                [(2.0, -5.0), (2.0, 5.0)],
                [(0.5, -5.0), (2.0, -5.0)],
                [(0.5, 5.0), (2.0, 5.0)],
            ],
        );
    }

    /// A generator folding back across the sweep (`y`: 0, 1, 0.5, swept along `x`):
    /// the region is the strip `0 <= y <= 1` of the domain, bounded by the fold's
    /// directrix at `y = 1`, not by the end's at `y = 0.5` (inside the strip).
    #[test]
    fn folded_generator_gives_its_strip() {
        let k = k();
        let s = swept(
            &k,
            &[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0), (2.0, 0.5, 0.0)],
            None,
            Vec3::X,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        let r = patch_in_xy_plane(&k, &s, &plane);
        assert_segments(
            &k,
            &r,
            &[
                [(-5.0, 0.0), (5.0, 0.0)],
                [(-5.0, 1.0), (5.0, 1.0)],
                [(-5.0, 0.0), (-5.0, 1.0)],
                [(5.0, 0.0), (5.0, 1.0)],
            ],
        );
    }

    /// An infinite extrusion in an unbounded plane whose copies lie on the domain's
    /// sides: each boundary piece once (no copy and side both).
    #[test]
    fn copies_on_the_domain_sides_are_not_repeated() {
        let k = k();
        let s = swept(&k, &[(0.0, 0.0, 0.0), (0.0, 1.0, 0.0)], None, Vec3::X);
        let plane = Plane::from_normal(Point3::ZERO, Vec3::Z);
        let r = patch_in_xy_plane(&k, &s, &plane);
        let big = plane.natural_uv_domain().u_interval().max();
        let v = s.base().natural_uv_domain().v_interval();
        assert_eq!(v.max(), big, "the copies lie on the sides");
        assert_segments(
            &k,
            &r,
            &[
                [(-big, 0.0), (big, 0.0)],
                [(-big, 1.0), (big, 1.0)],
                [(-big, 0.0), (-big, 1.0)],
                [(big, 0.0), (big, 1.0)],
            ],
        );
    }

    /// Two ranges in the plane apart across the sweep: two regions, each closed once.
    #[test]
    fn two_ranges_give_two_regions() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (2.0, 0.0, 1.0),
                (3.0, 0.0, 0.0),
                (4.0, 0.0, 0.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(-1.0, -5.0, 5.0, 5.0));
        let r = patch_in_xy_plane(&k, &s, &plane);
        let rb: Vec<_> = r
            .curve_types
            .iter()
            .filter(|t| **t == TsectCurveType::RegionBoundary)
            .collect();
        assert_eq!(rb.len(), r.curves_3d.len(), "{:?}", r.curve_types);
        assert_segments(
            &k,
            &r,
            &[
                [(0.0, -5.0), (0.0, 5.0)],
                [(1.0, -5.0), (1.0, 5.0)],
                [(0.0, -5.0), (1.0, -5.0)],
                [(0.0, 5.0), (1.0, 5.0)],
                [(3.0, -5.0), (3.0, 5.0)],
                [(4.0, -5.0), (4.0, 5.0)],
                [(3.0, -5.0), (4.0, -5.0)],
                [(3.0, 5.0), (4.0, 5.0)],
            ],
        );
    }

    /// A finite sweep of a folded generator whose copies lie inside the domain: no
    /// certificate covers the region (its slices need not be one interval), so the
    /// general intersection, as upstream.
    #[test]
    fn folded_generator_with_a_short_sweep_declines() {
        let k = k();
        let gen = swept(
            &k,
            &[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0), (3.0, 0.5, 0.0)],
            None,
            Vec3::X,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        let dom = Extent2d::from_bounds(0.0, 0.0, 2.0, 1.0);
        let r = gen
            .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), None)
            .unwrap();
        assert!(r.needs_more_intersections);
        assert!(r.curves_3d.is_empty());
    }

    /// The general intersection's request (upstream's answer whenever a range exists):
    /// `needs_more` with no curve.
    fn assert_declines(k: &Kernel, s: &SurfOfExtrusion, dom: &Extent2d, plane: &Plane) {
        let r = s
            .intersect_with_plane(k, dom, plane, &plane.natural_uv_domain(), None)
            .unwrap();
        assert!(r.needs_more_intersections, "{:?}", r.curve_types);
        assert!(r.curves_3d.is_empty());
    }

    /// Review r2 B1: a generator in the plane over `0 <= x <= 1` that leaves it and
    /// crosses back at `x = 0.75`; the crossing's directrix would run through the
    /// region, so the region declines.
    #[test]
    fn crossing_directrix_through_the_region_declines() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.0, 0.0, 1.0),
                (0.5, 0.0, -1.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        assert_declines(&k, &s, &s.base().natural_uv_domain(), &plane);
    }

    /// Review r2 B1: a generator in the plane over `0 <= x <= 1` that comes back to
    /// touch it at `x = 1`; the touching directrix would repeat the region's end edge,
    /// so the region declines.
    #[test]
    fn touching_directrix_on_the_region_edge_declines() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.5, 0.0, 1.0),
                (1.0, 0.0, 0.0),
                (0.5, 0.0, 1.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        assert_declines(&k, &s, &s.base().natural_uv_domain(), &plane);
    }

    /// Review r2 B2: a domain side parallel to the sweep at (or within the slack of)
    /// the generator's end bounds the region there, whichever side of the end it lies;
    /// the boundary keeps four edges.
    #[test]
    fn side_at_a_range_end_keeps_its_edge() {
        let k = k();
        let s = swept(&k, &[(0.0, 0.0, 0.0), (2.0, 0.0, 0.0)], None, Vec3::Y);
        for umin in [0.0, 1e-13, -1e-13, 1e-11] {
            let plane = xy_plane(Extent2d::from_bounds(umin, -5.0, 5.0, 5.0));
            let r = patch_in_xy_plane(&k, &s, &plane);
            let x0 = umin.max(0.0);
            assert_segments(
                &k,
                &r,
                &[
                    [(x0, -5.0), (x0, 5.0)],
                    [(2.0, -5.0), (2.0, 5.0)],
                    [(x0, -5.0), (2.0, -5.0)],
                    [(x0, 5.0), (2.0, 5.0)],
                ],
            );
        }
    }

    /// Review r2 B3: a monotone generator with a stretch along the sweep longer than
    /// the sweep: its two copies would overlap along the stretch, so the region
    /// declines (no slab covers it either).
    #[test]
    fn flat_stretch_longer_than_the_sweep_declines() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.0, 3.0, 0.0),
                (2.0, 3.0, 0.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        assert_declines(&k, &s, &Extent2d::from_bounds(0.0, 0.0, 3.0, 1.0), &plane);
    }

    /// The same stretch shorter than the sweep: its copies bound the region on
    /// either side of the stretch, without overlap.
    #[test]
    fn flat_stretch_shorter_than_the_sweep_gives_its_copies() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.0, 0.5, 0.0),
                (2.0, 0.5, 0.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        let r = s
            .intersect_with_plane(
                &k,
                &Extent2d::from_bounds(0.0, 0.0, 3.0, 1.0),
                &plane,
                &plane.natural_uv_domain(),
                None,
            )
            .unwrap();
        assert!(!r.needs_more_intersections);
        // the copies at v = 0 and 1 (each the polyline) and the end directrices
        let mut got: Vec<Vec<(f64, f64)>> = r
            .curves_3d
            .iter()
            .map(|c| {
                let canon = gen_base(c).unwrap().canonical().unwrap();
                canon.control_points.iter().map(|p| (p.x, p.y)).collect()
            })
            .collect();
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            got,
            vec![
                vec![(0.0, 0.0), (0.0, 1.0)],
                vec![(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (2.0, 0.5)],
                vec![(0.0, 1.0), (1.0, 1.0), (1.0, 1.5), (2.0, 1.5)],
                vec![(2.0, 0.5), (2.0, 1.5)],
            ]
        );
    }

    /// A generator part in the plane and part crossing it: the patch for the part,
    /// and the crossing's directrix outside it as before.
    #[test]
    fn partly_coincident_generator_keeps_its_crossing_directrix() {
        let k = k();
        let s = swept(
            &k,
            &[
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.0, 0.0, 1.0),
                (2.0, 0.0, -1.0),
            ],
            None,
            Vec3::Y,
        );
        let plane = Plane::from_normal(Point3::ZERO, Vec3::Z);
        let r = patch_in_xy_plane(&k, &s, &plane);
        let crossing = r
            .curves_3d
            .iter()
            .zip(&r.curve_types)
            .filter(|(_, t)| **t != TsectCurveType::RegionBoundary)
            .any(|(c, _)| {
                let ivl = c.natural_interval();
                let a = c.evaluate(&k, ivl.min(), 0, true, true).unwrap()[0];
                (a.x - 1.5).abs() <= 1e-9 && a.z.abs() <= 1e-12
            });
        assert!(crossing, "the directrix at x = 1.5: {:?}", r.curve_types);
        assert_eq!(
            r.curve_types
                .iter()
                .filter(|t| **t == TsectCurveType::RegionBoundary)
                .count(),
            4,
            "{:?}",
            r.curve_types
        );
    }

    /// A rational (circular) profile in the plane, monotone across the sweep: the
    /// certificate takes the weights; the boundary is its two copies (arcs) and the
    /// two end directrices, in a bounded plane holding them.
    #[test]
    fn rational_profile_in_the_plane_gives_its_patch() {
        let k = k();
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let s = swept(
            &k,
            &[(1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)],
            Some(w),
            Vec3::new(2.0, 0.0, 0.0),
        );
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        let dom = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let r = s
            .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), None)
            .unwrap();
        assert!(!r.needs_more_intersections);
        assert_eq!(r.curves_3d.len(), 4, "{:?}", r.curve_types);
        // the copies: the arc at x and at x + 2; the directrices at y = 0 and y = 1
        let mut arcs = 0;
        for c in &r.curves_3d {
            let ivl = c.natural_interval();
            let a = c.evaluate(&k, ivl.min(), 0, true, true).unwrap()[0];
            let m = c.evaluate(&k, ivl.mid(), 0, true, true).unwrap()[0];
            if m.distance((a + c.evaluate(&k, ivl.max(), 0, true, true).unwrap()[0]) / 2.0) > 0.1 {
                // on the circle of radius 1 about (0, 0) or (2, 0)
                let centre = if m.x > 1.5 {
                    Point3::new(2.0, 0.0, 0.0)
                } else {
                    Point3::ZERO
                };
                assert!((m.distance(centre) - 1.0).abs() <= 1e-12, "{m:?}");
                arcs += 1;
            }
        }
        assert_eq!(arcs, 2);
    }

    /// A profile 1e-9 off the plane (within the analysis tolerance, so a range,
    /// but not within the certificate's bound): the general intersection, as before.
    #[test]
    fn uncertified_range_falls_back_to_the_general_intersection() {
        let k = k();
        let s = swept(
            &k,
            &[(0.0, 0.0, 0.0), (1.0, 0.5, 1e-9), (2.0, 0.0, 0.0)],
            None,
            Vec3::Y,
        );
        let plane = Plane::from_normal(Point3::ZERO, Vec3::Z);
        let r = s
            .intersect_with_plane(
                &k,
                &s.base().natural_uv_domain(),
                &plane,
                &plane.natural_uv_domain(),
                None,
            )
            .unwrap();
        assert!(r.needs_more_intersections);
        assert!(r.curves_3d.is_empty());
    }

    /// The plane of the review's rotated probes: origin `(0.3, -1.7, 2.2)`, axes in
    /// general position, UV scale `(2, 0.5)`, over the UV box `dom`.
    fn rotated_plane(dom: Extent2d) -> Plane {
        let x = Vec3::new(1.0, 2.0, 3.0);
        let x = x / x.length();
        let t = Vec3::new(-2.0, 1.0, 0.5);
        let y = t - x * x.dot(t);
        let y = y / y.length();
        let mut plane = Plane::new(
            Point3::new(0.3, -1.7, 2.2),
            x,
            y,
            crate::geom::vector2d::Vec2::new(2.0, 0.5),
            &dom,
        );
        plane.make_nurb().unwrap();
        plane
    }

    /// A polyline generator through the plane's UV points `uv` swept along the UV
    /// direction `euv` (its 3D vector `E`), over the extrusion interval `v`.
    fn uv_polyline(
        k: &Kernel,
        plane: &Plane,
        uv: &[(f64, f64)],
        euv: (f64, f64),
        v: (f64, f64),
    ) -> ExtrusionIntersection {
        let o = plane.evaluate_point_fast(Point2::new(0.0, 0.0));
        let e = plane.evaluate_point_fast(Point2::new(euv.0, euv.1)) - o;
        let pts: Vec<(f64, f64, f64)> = uv
            .iter()
            .map(|&(u, w)| {
                let p = plane.evaluate_point_fast(Point2::new(u, w));
                (p.x, p.y, p.z)
            })
            .collect();
        let s = swept(k, &pts, None, e);
        let dom = Extent2d::from_bounds(0.0, v.0, (uv.len() - 1) as f64, v.1);
        s.intersect_with_plane(k, &dom, plane, &plane.natural_uv_domain(), None)
            .unwrap()
    }

    /// The UV polygon `poly` cut to the half-plane `f >= 0` (Sutherland-Hodgman).
    fn cut(poly: &[Point2], f: &dyn Fn(Point2) -> f64) -> Vec<Point2> {
        let mut out = Vec::new();
        for (&p, &q) in poly.iter().zip(poly.iter().cycle().skip(1)) {
            let (fp, fq) = (f(p), f(q));
            if fp >= 0.0 {
                out.push(p);
            }
            if (fp >= 0.0) != (fq >= 0.0) {
                let t = fp / (fp - fq);
                out.push(Point2::new(p.x + t * (q.x - p.x), p.y + t * (q.y - p.y)));
            }
        }
        out
    }

    /// The UV polygon `poly` cut to the box `dom`.
    fn cut_to_box(poly: &[Point2], dom: &Extent2d) -> Vec<Point2> {
        let (u0, u1) = (dom.u_interval().min(), dom.u_interval().max());
        let (w0, w1) = (dom.v_interval().min(), dom.v_interval().max());
        let p = cut(poly, &|p| p.x - u0);
        let p = cut(&p, &|p| u1 - p.x);
        let p = cut(&p, &|p| p.y - w0);
        cut(&p, &|p| w1 - p.y)
    }

    /// The signed area of a UV polygon (shoelace).
    fn uv_area(poly: &[Point2]) -> f64 {
        let Some(&origin) = poly.first() else {
            return 0.0;
        };
        poly.iter()
            .zip(poly.iter().cycle().skip(1))
            .map(|(p, q)| {
                let (p, q) = (*p - origin, *q - origin);
                p.x * q.y - q.x * p.y
            })
            .sum::<f64>()
            / 2.0
    }

    /// Asserts the answer is the boundary of the UV region of area `want` in `plane`
    /// (UV scale at least `1/2`): every curve `RegionBoundary` and of degree 1 (its
    /// control polygon is the curve), each end meeting exactly one other end within
    /// `gap = 16 eps m` in space for the largest coordinate `m` (closed, no curve
    /// repeated or overlapping another: the corners close to rounding), the loops
    /// walked end to end without revisiting, and their total area `want` within
    /// `2 gap` times the perimeter. A corner is one point
    /// computed twice (a copy's end and a directrix's end: a plane evaluation and a
    /// translation, a few roundings of `m` each), and every boundary point is off its
    /// exact place by as much (the generator's points are plane evaluations), which
    /// moves the area by at most the perimeter times that in UV (`2 gap`).
    fn assert_region(r: &ExtrusionIntersection, plane: &Plane, want: f64) {
        assert_region_closing_to(r, plane, want, 16.0 * f64::EPSILON);
    }

    /// `assert_region` with the closing gap `rel m` (`16 eps` there).
    fn assert_region_closing_to(r: &ExtrusionIntersection, plane: &Plane, want: f64, rel: f64) {
        assert!(!r.needs_more_intersections);
        assert!(!r.curves_3d.is_empty());
        assert!(
            r.curve_types
                .iter()
                .all(|t| *t == TsectCurveType::RegionBoundary),
            "{:?}",
            r.curve_types
        );
        let nets: Vec<Vec<Point3>> = r
            .curves_3d
            .iter()
            .map(|c| {
                let canon = gen_base(c).unwrap().canonical().unwrap();
                assert_eq!(canon.degree, 1);
                canon.control_points.clone()
            })
            .collect();
        let ends: Vec<(usize, Point3)> = nets
            .iter()
            .enumerate()
            .flat_map(|(i, n)| [(i, n[0]), (i, *n.last().unwrap())])
            .collect();
        let m = nets
            .iter()
            .flatten()
            .map(|p| p.max_dimension())
            .fold(0.0, f64::max);
        let gap = rel * m;
        let mut mate = vec![0; ends.len()];
        for (a, &(_, p)) in ends.iter().enumerate() {
            let near: Vec<usize> = (0..ends.len())
                .filter(|&b| b != a && ends[b].1.distance(p) <= gap)
                .collect();
            assert_eq!(near.len(), 1, "end {p:?} meets {} ends", near.len());
            mate[a] = near[0];
        }
        let mut used = vec![false; nets.len()];
        let (mut total, mut perimeter) = (0.0, 0.0);
        for start in 0..nets.len() {
            if used[start] {
                continue;
            }
            let mut loop_uv: Vec<Point2> = Vec::new();
            let (mut cur, mut reversed) = (start, false);
            loop {
                assert!(!used[cur], "a loop revisits a curve");
                used[cur] = true;
                let mut pts: Vec<Point3> = nets[cur].clone();
                if reversed {
                    pts.reverse();
                }
                loop_uv.extend(
                    pts.iter()
                        .skip(1)
                        .map(|&p| plane.project_point_to_uv_domain(p)),
                );
                let exit = 2 * cur + usize::from(!reversed);
                let next = mate[exit];
                let (nc, at_end) = (next / 2, next % 2 == 1);
                if nc == start {
                    break;
                }
                cur = nc;
                reversed = at_end;
            }
            total += uv_area(&loop_uv).abs();
            perimeter += loop_uv
                .iter()
                .zip(loop_uv.iter().cycle().skip(1))
                .map(|(p, q)| (*q - *p).length())
                .sum::<f64>();
        }
        assert!(
            (total - want).abs() <= 2.0 * gap * perimeter,
            "area {total:e}, want {want:e}"
        );
    }

    /// Review r3 B4: a generator in the plane over `0 <= x <= 1`, leaving it by `d`
    /// over the next span (shape 0: then parallel to it; 1: back to it; 2: across it
    /// and sideways), swept along `Y`. At `d = 1e-12` the whole generator is within
    /// the in-plane bound: the solver's ranges extend over the spans between them and
    /// the region is the whole sweep, with its point solutions absorbed. Beyond, the
    /// generator stays within the in-plane bound past the range end for longer than
    /// the tolerance (a slow departure): the general intersection, as before, never a
    /// region mixed with the departure's tangent directrices.
    #[test]
    fn slow_departure_extends_the_range_or_declines() {
        let k = k();
        let plane = xy_plane(Extent2d::from_bounds(-5.0, -5.0, 5.0, 5.0));
        for d in [1e-12, 1e-11, 1e-10, 1e-9, 1e-8, 1e-7, 5e-7, 2e-6] {
            for shape in 0..3 {
                let pts = match shape {
                    0 => [
                        (0.0, 0.0, 0.0),
                        (1.0, 0.0, 0.0),
                        (2.0, 0.0, d),
                        (3.0, 0.0, d),
                    ],
                    1 => [
                        (0.0, 0.0, 0.0),
                        (1.0, 0.0, 0.0),
                        (2.0, 0.0, d),
                        (3.0, 0.0, 0.0),
                    ],
                    _ => [
                        (0.0, 0.0, 0.0),
                        (1.0, 0.0, 0.0),
                        (2.0, 0.5, d),
                        (3.0, 0.0, -d),
                    ],
                };
                let s = swept(&k, &pts, None, Vec3::Y);
                for tol in [None, Some(1e-9)] {
                    let dom = Extent2d::from_bounds(0.0, -1.0, 3.0, 1.0);
                    let r = s
                        .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), tol)
                        .unwrap();
                    if d > 1e-12 {
                        assert!(r.needs_more_intersections, "d {d:e} shape {shape}");
                        assert!(r.curves_3d.is_empty());
                        continue;
                    }
                    // Each copy reaches |z| = d at a generator control point.
                    // A reported deviation must enclose that distance, including
                    // the rounding of computing the control-net bound.
                    assert_eq!(r.deviations.len(), r.curves_3d.len());
                    assert!(r.deviations.iter().all(|&v| v.is_finite() && v >= d));
                    for (curve, &deviation) in r.curves_3d.iter().zip(&r.deviations) {
                        let ivl = curve.natural_interval();
                        for t in [ivl.min(), ivl.mid(), ivl.max()] {
                            let distance = curve.evaluate_point(&k, t).unwrap().z.abs();
                            assert!(
                                distance <= deviation,
                                "d {d:e} shape {shape}: distance {distance:e} exceeds deviation {deviation:e}"
                            );
                        }
                    }
                    // the sweep (length 2 along Y) of a generator 3 long in X
                    assert_region(&r, &plane, 6.0);
                    if shape < 2 {
                        assert_segments(
                            &k,
                            &r,
                            &[
                                [(0.0, -1.0), (3.0, -1.0)],
                                [(0.0, 1.0), (3.0, 1.0)],
                                [(0.0, -1.0), (0.0, 1.0)],
                                [(3.0, -1.0), (3.0, 1.0)],
                            ],
                        );
                    }
                }
            }
        }
    }

    /// Positive rational weights keep the generator at z = d. Clipping the
    /// sweep to the plane's domain returns plane-made sides at z = 0, whose
    /// deviation must still cover their distance d from the extrusion.
    #[test]
    fn clipped_rational_patch_reports_the_generator_offset() {
        let k = k();
        for scale in [1e-3, 1.0, 1e3] {
            let d = 1e-12 * scale;
            let s = swept(
                &k,
                &[(0.0, 0.0, d), (scale, 0.0, d), (2.0 * scale, 0.0, d)],
                Some(0.5),
                Vec3::Y * scale,
            );
            let plane = xy_plane(Extent2d::from_bounds(
                0.5 * scale,
                -0.5 * scale,
                1.5 * scale,
                0.5 * scale,
            ));
            let dom = Extent2d::from_bounds(0.0, -1.0, 1.0, 1.0);
            let r = s
                .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), None)
                .unwrap();
            assert!(!r.needs_more_intersections, "scale {scale:e}");
            assert_eq!(r.curves_3d.len(), 4);
            assert_eq!(r.surface1_uv_curves.len(), r.curves_3d.len());
            assert_eq!(r.surface2_uv_curves.len(), r.curves_3d.len());
            assert_eq!(r.curve_types.len(), r.curves_3d.len());
            assert_eq!(r.deviations.len(), r.curves_3d.len());
            for ((curve, kind), &deviation) in
                r.curves_3d.iter().zip(&r.curve_types).zip(&r.deviations)
            {
                assert_eq!(*kind, TsectCurveType::RegionBoundary);
                assert!(deviation.is_finite() && deviation >= d);
                let ivl = curve.natural_interval();
                for t in [ivl.min(), ivl.mid(), ivl.max()] {
                    assert_eq!(curve.evaluate_point(&k, t).unwrap().z, 0.0);
                }
            }
        }
    }

    /// Review r4 M1: a quadratic generator in the plane over `[0, 1]` that leaves it
    /// tangentially over `[1, 2]` (control values `0, 0, h`: a fillet's foot), swept
    /// along `Y`. Its distance off the plane grows as `t^2` past the range end, so
    /// the departure's Bernstein bound takes the power 2; with power 1 the end point
    /// would pass as within the tolerance of the point where the generator certainly
    /// leaves, and the region would be cut short by about `sqrt(B / h)`. The
    /// generator stays within the in-plane bound past the end for longer than the
    /// tolerance: the general intersection.
    #[test]
    fn tangential_departure_declines() {
        let k = k();
        let plane = xy_plane(Extent2d::from_bounds(-50.0, -50.0, 50.0, 50.0));
        for h in [1e-3, 1.0, 1e3] {
            let pw: Vec<CPoint> = [
                (0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0),
                (1.5, 0.0, 0.0),
                (2.0, 0.0, h),
            ]
            .iter()
            .map(|&(x, y, z)| CPoint::new(x, y, z, NOW))
            .collect();
            let cur = NurbCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0]).unwrap();
            let s = SurfOfExtrusion::create_canonical(
                &k,
                Curve::from(BSplineCurve::from_nurb(cur, 3, false)),
                Vec3::Y,
            )
            .unwrap();
            let dom = Extent2d::from_bounds(0.0, -1.0, 2.0, 1.0);
            for tol in [None, Some(1e-6)] {
                let r = s
                    .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), tol)
                    .unwrap();
                assert!(
                    r.needs_more_intersections,
                    "h {h:e} tol {tol:?}: {:?}",
                    r.curve_types
                );
                assert!(r.curves_3d.is_empty());
            }
        }
    }

    /// Review r4 M2: a generator in the plane over `[0, 1]` that leaves it by `h`
    /// (below the tolerance) over the next span, `dx` along `X`, and then crosses it
    /// steeply. The departure is certified (it leaves the in-plane bound within the
    /// tolerance of the range end) but never gets farther than the tolerance over
    /// that span, so its zone runs on past it. The solver reports point solutions in
    /// the zone (touches along the span, then the crossing) whose directrices lie off
    /// the region, at `x` from about `1 + dx / 4` to just past `1 + dx`: without the zone's
    /// decline they would be answered next to the region, with the tolerance-close
    /// stretch between them given only as tangent lines. The general intersection.
    #[test]
    fn point_solutions_in_a_departure_zone_decline() {
        let k = k();
        let plane = xy_plane(Extent2d::from_bounds(-50.0, -50.0, 50.0, 50.0));
        for h in [1e-7, 5e-7, 9e-7] {
            for dx in [1e-3, 1e-2] {
                let s = swept(
                    &k,
                    &[
                        (0.0, 0.0, 0.0),
                        (1.0, 0.0, 0.0),
                        (1.0 + dx, 0.0, h),
                        (5.0, 0.0, -1.0),
                    ],
                    None,
                    Vec3::Y,
                );
                let dom = Extent2d::from_bounds(0.0, -1.0, 3.0, 1.0);
                for tol in [None, Some(1e-6)] {
                    let r = s
                        .intersect_with_plane(&k, &dom, &plane, &plane.natural_uv_domain(), tol)
                        .unwrap();
                    assert!(
                        r.needs_more_intersections,
                        "h {h:e} dx {dx:e} tol {tol:?}: {:?}",
                        r.curve_types
                    );
                    assert!(r.curves_3d.is_empty());
                }
            }
        }
    }

    /// Review r3 B4, the rotated fuzz at scale `1e6` (seed 7, cases 9, 263 and 295):
    /// a monotone polyline in a plane in general position, a sweep oblique to the UV
    /// axes. The region was cut short at a range end with tangent directrices after
    /// it; now it is the whole clipped sweep, closed, with the exact area.
    #[test]
    fn far_rotated_regions_are_whole() {
        type Case = (&'static [(f64, f64)], (f64, f64), (f64, f64), [f64; 4]);
        let k = k();
        let cases: [Case; 3] = [
            (
                &[
                    (562688.465356203, 142952.31039695354),
                    (-667267.8924107907, -412543.76405498874),
                    (-162335.81507902325, -739602.3145067472),
                    (286749.2593539145, -1034862.3717900242),
                ],
                (-0.9996072490739965, -0.02802405393081653),
                (481030.0799755236, 2118761.057265442),
                [
                    -1787116.426513385,
                    -1155947.2023147312,
                    -268241.6221215105,
                    1472542.7257044706,
                ],
            ),
            (
                &[
                    (198301.87075739048, -979548.7612060617),
                    (16164.094044531841, 360873.9146343507),
                    (258988.53315406173, 180662.42693427886),
                    (1187720.5897254841, -513446.0198680909),
                    (1404052.4932117404, -468659.68286636716),
                ],
                (0.32641331590522, -0.9452271405327711),
                (-743923.2371680021, 414544.6994434725),
                [
                    -1730936.9026290602,
                    57869.12514012022,
                    439369.71942081506,
                    2215342.4406051408,
                ],
            ),
            (
                &[
                    (-196740.74656389002, 626253.4783405891),
                    (-541820.6501602847, 79968.19499988858),
                    (-659768.832935693, -107521.40912442948),
                    (-934369.5466458122, -261140.02759837),
                    (-787099.2292372237, -481367.9300678842),
                ],
                (-0.7702605656069884, 0.6377293007780044),
                (481397.686141372, 2147538.1813933733),
                [
                    -1113501.2017365587,
                    -1205839.219819479,
                    -572033.279387745,
                    1748459.2408306338,
                ],
            ),
        ];
        for (uv, euv, v, [u0, w0, u1, w1]) in cases {
            let dom = Extent2d::from_bounds(u0, w0, u1, w1);
            let plane = rotated_plane(dom);
            let r = uv_polyline(&k, &plane, uv, euv, v);
            let mut sweep: Vec<Point2> = uv
                .iter()
                .map(|&(a, b)| Point2::new(a + euv.0 * v.0, b + euv.1 * v.0))
                .collect();
            sweep.extend(
                uv.iter()
                    .rev()
                    .map(|&(a, b)| Point2::new(a + euv.0 * v.1, b + euv.1 * v.1)),
            );
            let want = uv_area(&cut_to_box(&sweep, &dom)).abs();
            assert_region(&r, &plane, want);
        }
    }

    /// Review r3 B5: the fuzz's case 117 (axis-aligned plane) and rotated case 280
    /// (seed 7) and slab case 19 (seed 11, an infinite sweep) at scales `1e-3`, `1e-5`
    /// and `1e-6`. Every bound is relative to the magnitudes it compares, and every
    /// generator piece is split exactly (no zone tolerance dropping a short end span
    /// and cutting a corner of the copies): each region is closed, without overlaps,
    /// with the exact area. (The copies cut a corner at `1e-3`; at `1e-5` the sweep's
    /// slack, relative to the infinite sweep, dropped whole runs.)
    #[test]
    fn tiny_regions_are_exact() {
        let k = k();
        let case_117: &[(f64, f64)] = &[
            (1.0327866959632181, 0.26703211727930065),
            (-0.05926350971526267, -0.3611441030101076),
            (0.17752993222538088, -0.7155730224847894),
            (-0.947039949845586, -1.1766257639222346),
            (0.16758925544507486, -1.7908016065640358),
        ];
        let case_280: &[(f64, f64)] = &[
            (-0.3046276560209673, 0.7163869347209829),
            (-0.04831049147074139, -0.2309458413693064),
            (0.27154988620888476, 0.24380258684344763),
        ];
        let slab_19: &[(f64, f64)] = &[
            (0.3195513055574201, -0.9605401256962343),
            (0.9090117337835634, -0.1849036346823752),
            (0.45791172581877326, -0.2617570663046048),
            (0.34217941953255465, 0.9142898701673123),
            (-0.7398659000072707, 0.4908003779576198),
        ];
        for scale in [1e-3, 1e-5, 1e-6] {
            let sc = |p: &[(f64, f64)]| -> Vec<(f64, f64)> {
                p.iter().map(|&(a, b)| (a * scale, b * scale)).collect()
            };
            // (generator, sweep, V, box, rotated)
            let monotone = [
                (
                    sc(case_117),
                    (-0.9891847911799043, 0.14667463617943338),
                    (-0.8991957428959085 * scale, -0.5440694482425046 * scale),
                    [
                        -0.6721708812185043,
                        -1.3713910036936614,
                        1.3113552358355924,
                        -0.48806237109542194,
                    ],
                    false,
                ),
                (
                    sc(case_280),
                    (-1.8369701987210297e-16, -1.0),
                    (-0.7619615177941612 * scale, 0.4513821250947778 * scale),
                    [
                        -0.051955980183495454,
                        -1.06544553462776,
                        2.764494625867272,
                        1.5073483367603193,
                    ],
                    true,
                ),
            ];
            for (uv, euv, v, b, rot) in monotone {
                let dom =
                    Extent2d::from_bounds(b[0] * scale, b[1] * scale, b[2] * scale, b[3] * scale);
                let plane = if rot {
                    rotated_plane(dom)
                } else {
                    xy_plane(dom)
                };
                let r = uv_polyline(&k, &plane, &uv, euv, v);
                let mut sweep: Vec<Point2> = uv
                    .iter()
                    .map(|&(a, b)| Point2::new(a + euv.0 * v.0, b + euv.1 * v.0))
                    .collect();
                sweep.extend(
                    uv.iter()
                        .rev()
                        .map(|&(a, b)| Point2::new(a + euv.0 * v.1, b + euv.1 * v.1)),
                );
                let want = uv_area(&cut_to_box(&sweep, &dom)).abs();
                assert_region(&r, &plane, want);
            }
            // the slab case: an infinite sweep (ten times the UV direction) over a
            // monotone polyline, so the region is the box cut to the polyline's mu range
            let euv = (-0.6432384559040851, -0.765665912030912);
            let uv = sc(slab_19);
            let dom = Extent2d::from_bounds(
                -0.057647689489415566 * scale,
                -1.6318337630541604 * scale,
                1.2209494150168338 * scale,
                -0.925508607302209 * scale,
            );
            let plane = xy_plane(dom);
            let r = uv_polyline(
                &k,
                &plane,
                &uv,
                (euv.0 * 10.0, euv.1 * 10.0),
                (-1234567.0, 1234567.0),
            );
            let mu = |p: Point2| -euv.1 * p.x + euv.0 * p.y;
            let mus: Vec<f64> = uv.iter().map(|&(a, b)| mu(Point2::new(a, b))).collect();
            let (lo, hi) = mus
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &m| {
                    (l.min(m), h.max(m))
                });
            let (u0, u1) = (dom.u_interval().min(), dom.u_interval().max());
            let (w0, w1) = (dom.v_interval().min(), dom.v_interval().max());
            let corners = [
                Point2::new(u0, w0),
                Point2::new(u1, w0),
                Point2::new(u1, w1),
                Point2::new(u0, w1),
            ];
            let slab = cut(&cut(&corners, &|p| mu(p) - lo), &|p| hi - mu(p));
            let want = uv_area(&slab).abs();
            assert_region(&r, &plane, want);
        }
    }

    /// The supplied plane contains the generator and sweep, but the normal inferred
    /// from this thin generator triangle differs by more than the dispatch's angular
    /// tolerance at UV coordinates near 1e6. It must still give the slab boundary.
    #[test]
    fn far_origin_corner_slab_has_a_closed_boundary() {
        let k = k();
        let o = 1e6;
        let dom = Extent2d::from_bounds(o, o, o + 10.0, o + 10.0);
        let plane = rotated_plane(dom);
        let th: f64 = 2.68865247675069297;
        let euv = (th.cos(), th.sin());
        // Work relative to the box origin so the expected area does not lose
        // precision by subtracting products of coordinates near 1e6.
        let mu = |p: Point2| -euv.1 * (p.x - o) + euv.0 * (p.y - o);
        let corners = [
            Point2::new(o, o),
            Point2::new(o + 10.0, o),
            Point2::new(o + 10.0, o + 10.0),
            Point2::new(o, o + 10.0),
        ];
        let c = *corners
            .iter()
            .min_by(|&&p, &&q| mu(p).total_cmp(&mu(q)))
            .unwrap();
        let f = (0.399576720803022, 0.21708714905902105, 0.5446327714186807);
        let inner = |a, b| Point2::new(o + (1.0 + 8.0 * a), o + (1.0 + 8.0 * b));
        let (a, b) = (inner(f.0, f.1), inner(f.2, f.0));
        let uv = [(a.x, a.y), (c.x, c.y), (b.x, b.y)];
        let r = uv_polyline(&k, &plane, &uv, euv, (-1e4, 1e4));
        let beta = mu(a).max(mu(b));
        let want = uv_area(&cut(&corners, &|p| beta - mu(p))).abs();
        assert_region(&r, &plane, want);
    }

    /// An angularly parallel sweep can drift 1e-3 off the plane over its extent.
    /// Both a generator in the plane and one crossing it must give curves on the
    /// plane within their reported deviation, or decline. A short interval far
    /// from zero still has the same drift from the generator.
    #[test]
    fn nearly_parallel_long_sweep_respects_reported_plane_deviation() {
        let k = k();
        for offset in [0.0, 1e6] {
            let origin = Point3::new(offset, offset, offset);
            let plane = Plane::from_normal(origin, Vec3::X);
            for generator_direction in [Vec3::Y, Vec3::X] {
                for (vmin, vmax) in [(-1e6, 1e6), (1e6, 1e6 + 1.0)] {
                    let a = origin - generator_direction;
                    let b = origin + generator_direction;
                    let mut s = swept(
                        &k,
                        &[(a.x, a.y, a.z), (b.x, b.y, b.z)],
                        None,
                        Vec3::new(1e-9, 0.0, 1.0),
                    );
                    let domain = Extent2d::from_bounds(0.0, vmin, 1.0, vmax);
                    s.trim_with_domain(&k, &domain).unwrap();
                    let r = s
                        .intersect_with_plane(&k, &domain, &plane, &plane.step_uv_domain(), None)
                        .unwrap();
                    if r.needs_more_intersections {
                        assert!(r.curves_3d.is_empty());
                        continue;
                    }
                    assert!(!r.curves_3d.is_empty());
                    assert_eq!(r.curves_3d.len(), r.deviations.len());
                    for (curve, &deviation) in r.curves_3d.iter().zip(&r.deviations) {
                        let net = gen_base(curve).unwrap().canonical().unwrap();
                        // Degree-one curves lie on their control segments, so this
                        // checks every point of every output curve, not samples.
                        assert_eq!(net.degree, 1);
                        for &p in &net.control_points {
                            let distance = plane.position().z_axis().dot(p - origin).abs();
                            assert!(
                                distance <= deviation,
                                "offset {offset}, generator {generator_direction:?}, sweep \
                                 [{vmin}, {vmax}]: distance {distance} exceeds {deviation}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// A directrix tilted off x = 0 drifts 1e-7 over [-100, 100], below tol.
    /// A smaller tilt also passes the patch certificate; both must report the drift.
    #[test]
    fn below_threshold_sweep_reports_plane_deviation() {
        let k = k();
        let plane = Plane::from_normal(Point3::ZERO, Vec3::X);
        for (generator_direction, tilt) in [(Vec3::X, 1e-9), (Vec3::Y, 1e-13)] {
            let a = -generator_direction;
            let b = generator_direction;
            let mut s = swept(
                &k,
                &[(a.x, a.y, a.z), (b.x, b.y, b.z)],
                None,
                Vec3::new(tilt, 0.0, 1.0),
            );
            let domain = Extent2d::from_bounds(0.0, -100.0, 1.0, 100.0);
            s.trim_with_domain(&k, &domain).unwrap();
            let r = s
                .intersect_with_plane(&k, &domain, &plane, &plane.step_uv_domain(), None)
                .unwrap();
            assert!(!r.needs_more_intersections);
            assert!(!r.curves_3d.is_empty());
            assert_eq!(r.curves_3d.len(), r.deviations.len());
            for (curve, &deviation) in r.curves_3d.iter().zip(&r.deviations) {
                assert!(deviation > 0.0 && deviation <= EFF_ZERO_SQRT);
                assert!(
                    deviation >= tilt * 100.0,
                    "the full sweep drift must be covered"
                );
                let net = gen_base(curve).unwrap().canonical().unwrap();
                // Degree-one curves lie on their control segments, so this checks
                // every output point, including the endpoints at maximum drift.
                assert_eq!(net.degree, 1);
                for &p in &net.control_points {
                    let distance = plane.position().z_axis().dot(p).abs();
                    assert!(
                        distance <= deviation,
                        "generator {generator_direction:?}: distance {distance} exceeds {deviation}"
                    );
                }
            }
        }
    }

    /// The profile residual and sweep drift add. Plane-domain sides also retain
    /// the profile residual, even when clipping constructs them exactly on x = 0.
    #[test]
    fn parallel_patch_deviation_covers_profile_and_sweep() {
        let k = k();
        let residual = 2.0_f64.powi(-44);
        for (e, vmin, domain, expected) in [
            (
                Vec3::new(residual, 0.0, 1.0),
                0.0,
                Extent2d::from_bounds(-2.0, -2.0, 2.0, 2.0),
                2.0 * residual,
            ),
            (
                Vec3::Z,
                -1.0,
                Extent2d::from_bounds(-0.25, -0.25, 0.25, 0.25),
                residual,
            ),
        ] {
            let mut s = swept(&k, &[(residual, -1.0, 0.0), (residual, 1.0, 0.0)], None, e);
            let sweep = Extent2d::from_bounds(0.0, vmin, 1.0, 1.0);
            s.trim_with_domain(&k, &sweep).unwrap();
            let plane = Plane::new(
                Point3::ZERO,
                Vec3::Y,
                Vec3::Z,
                crate::geom::vector2d::Vec2::new(1.0, 1.0),
                &domain,
            );
            let r = s
                .intersect_with_plane(&k, &sweep, &plane, &domain, None)
                .unwrap();
            assert!(!r.needs_more_intersections);
            assert!(!r.curves_3d.is_empty());
            assert_eq!(r.curves_3d.len(), r.deviations.len());
            for (curve, &deviation) in r.curves_3d.iter().zip(&r.deviations) {
                assert!(deviation >= expected && deviation <= EFF_ZERO_SQRT);
                let net = gen_base(curve).unwrap().canonical().unwrap();
                assert_eq!(net.degree, 1);
                for p in net.control_points {
                    assert!(p.x.abs() <= deviation);
                    if e == Vec3::Z {
                        assert!((p.x - residual).abs() <= deviation);
                    }
                }
            }
        }
    }

    /// Review r5: a `0.5 x 0.5` swept square at `x = 1e12`, swept
    /// along `x` and along `y`. Its directrices (`0.5` long) are below `SM_EFF_ZERO`
    /// of their coordinates (`1.0`) but are real edges: the boundary is the whole
    /// closed square, as at the origin, not its two copies alone.
    #[test]
    fn small_regions_far_from_the_origin_are_closed() {
        let k = k();
        for x0 in [0.0, 1e12] {
            for euv in [(1.0, 0.0), (0.0, 1.0)] {
                let dom = Extent2d::from_bounds(x0 - 10.0, -10.0, x0 + 10.0, 10.0);
                let plane = xy_plane(dom);
                let gen: &[(f64, f64)] = if euv.0 == 1.0 {
                    &[(x0, 0.0), (x0, 0.5)]
                } else {
                    &[(x0, 0.0), (x0 + 0.5, 0.0)]
                };
                let r = uv_polyline(&k, &plane, gen, euv, (0.0, 0.5));
                assert_eq!(r.curves_3d.len(), 4, "x0 {x0:e}, sweep {euv:?}");
                assert_region(&r, &plane, 0.25);
            }
        }
    }

    /// The review r1 corner probes of #18771: the UV box `[o, o + 10 s]^2` of the
    /// plane (`rotated_plane` or `xy_plane`), swept along the UV direction at angle
    /// `th`, with `c` the box corner of least `mu` (`-sin th u + cos th w`) and
    /// `inner(a, b) = o + (1 + 8 (a, b)) s`, `f = (f0, f1, f2)`:
    /// - kind 0: the generator `c`-`inner(f0, f1)` over `v` in `[-30 s, 30 s]` (its
    ///   directrix at `c` touches the box only there): the box cut to
    ///   `mu <= mu(inner(f0, f1))`;
    /// - kind 1: the fold `inner(f0, f1)`-`c`-`inner(f2, f0)` over `[-1e4 s, 1e4 s]`
    ///   (the slab, its `alpha` line touching the box at `c`): the box cut to `mu`
    ///   at most the larger of the two inner points';
    /// - kind 2: the line through `c` along `d = inner(f0, f1) - c`, from `c - d/2` to
    ///   `c + d`, over `[0, 3 s]` (its copy at `v = 0` crosses both sides at `c`): the
    ///   parallelogram cut to the box.
    ///
    /// The answer and the region's UV area.
    fn corner_probe(
        k: &Kernel,
        kind: usize,
        rot: bool,
        o: f64,
        s: f64,
        th: f64,
        f: (f64, f64, f64),
    ) -> (ExtrusionIntersection, Plane, f64) {
        type Probe = (Vec<(f64, f64)>, (f64, f64), Vec<Point2>);
        let dom = Extent2d::from_bounds(o, o, o + 10.0 * s, o + 10.0 * s);
        let plane = if rot {
            rotated_plane(dom)
        } else {
            xy_plane(dom)
        };
        let euv = (th.cos(), th.sin());
        let mu = |p: (f64, f64)| -euv.1 * p.0 + euv.0 * p.1;
        let corners = [
            (o, o),
            (o + 10.0 * s, o),
            (o + 10.0 * s, o + 10.0 * s),
            (o, o + 10.0 * s),
        ];
        let c = corners
            .iter()
            .copied()
            .min_by(|a, b| mu(*a).total_cmp(&mu(*b)))
            .unwrap();
        let inner = |a: f64, b: f64| (o + (1.0 + 8.0 * a) * s, o + (1.0 + 8.0 * b) * s);
        let pt = |p: (f64, f64)| Point2::new(p.0, p.1);
        let boxp: Vec<Point2> = corners.iter().map(|&p| pt(p)).collect();
        let below = |top: f64| cut(&boxp, &|p: Point2| top - mu((p.x, p.y)));
        let (gen, v, region): Probe = match kind {
            0 => {
                let a = inner(f.0, f.1);
                (vec![c, a], (-30.0 * s, 30.0 * s), below(mu(a)))
            }
            1 => {
                let (a, b) = (inner(f.0, f.1), inner(f.2, f.0));
                (vec![a, c, b], (-1e4 * s, 1e4 * s), below(mu(a).max(mu(b))))
            }
            _ => {
                let a = inner(f.0, f.1);
                let d = (a.0 - c.0, a.1 - c.1);
                let (p0, p1) = ((c.0 - 0.5 * d.0, c.1 - 0.5 * d.1), (c.0 + d.0, c.1 + d.1));
                let h = (3.0 * s * euv.0, 3.0 * s * euv.1);
                let par = [p0, p1, (p1.0 + h.0, p1.1 + h.1), (p0.0 + h.0, p0.1 + h.1)];
                let par: Vec<Point2> = par.iter().map(|&p| pt(p)).collect();
                (vec![p0, p1], (0.0, 3.0 * s), cut_to_box(&par, &dom))
            }
        };
        let r = uv_polyline(k, &plane, &gen, euv, v);
        (r, plane, uv_area(&region).abs())
    }

    /// The length of the shortest answer curve (end to end).
    fn shortest_curve(r: &ExtrusionIntersection) -> f64 {
        r.curves_3d
            .iter()
            .map(|c| {
                let n = gen_base(c)
                    .unwrap()
                    .canonical()
                    .unwrap()
                    .control_points
                    .clone();
                n[0].distance(*n.last().unwrap())
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Review r1 of #18771, items 2 and 4: at unit scale a directrix or slab line
    /// through a domain corner, the sweep near a side, gives two computed points
    /// `1e-13` apart that are one point of the region. They are one point to their
    /// computed errors (a `mu` error over the sweep's component along the side), so
    /// the region builds, closed and of the right area, as on main; the head of
    /// #18771 declined each (the gap above `16 eps m`, below `SM_EFF_ZERO`).
    #[test]
    fn corner_coincident_regions_at_unit_scale_build() {
        let k = k();
        let cases = [
            (
                0,
                4.714_068_012_625_690_86,
                (
                    0.784_676_612_296_307_9,
                    0.082_079_752_251_084_21,
                    0.121_534_392_499_686_12,
                ),
            ),
            (
                1,
                -1.153_269_977_747_679e-3,
                (
                    0.848_373_173_117_043_3,
                    0.887_478_476_998_965_9,
                    0.135_842_471_966_865_72,
                ),
            ),
            (
                1,
                4.714_645_337_743_938_57,
                (
                    0.907_369_364_922_563_3,
                    0.338_473_897_099_659_25,
                    0.037_817_098_211_816_56,
                ),
            ),
            (
                2,
                3.563_739_902_634_357_03,
                (
                    0.716_500_464_656_385,
                    0.904_557_459_929_209,
                    0.726_036_532_991_460_3,
                ),
            ),
        ];
        for (i, &(kind, th, f)) in cases.iter().enumerate() {
            let o = if i == 3 { 0.0 } else { 1.0 };
            let (r, plane, area) = corner_probe(&k, kind, true, o, 1.0, th, f);
            assert_region(&r, &plane, area);
        }
    }

    /// Review r1 of #18771, item 3: far from the origin the same coincidences leave
    /// two computed points of rounding noise apart (`6e-9` at `1e6` with `s = 1e3`,
    /// `2e-12` at `37`), above `16 eps m`: the head of #18771 emitted each as a tiny
    /// boundary edge. Within their computed errors and `SM_EFF_ZERO m` they are no
    /// edge: every curve is longer than `1e-3 s` and the boundary closes (to rounding
    /// at `37`, to the dropped noise below `SM_EFF_ZERO m` at `1e6`) around the
    /// region's area.
    #[test]
    fn corner_noise_far_from_the_origin_gives_no_tiny_edge() {
        let k = k();
        let (r, plane, area) = corner_probe(
            &k,
            0,
            true,
            1e6,
            1e3,
            4.689_916_370_853_183_03,
            (
                0.831_315_404_509_208_6,
                0.327_520_607_221_003_6,
                0.206_703_038_557_221_83,
            ),
        );
        assert!(shortest_curve(&r) > 1.0, "{}", shortest_curve(&r));
        assert_region_closing_to(&r, &plane, area, EFF_ZERO);
        let (r, plane, area) = corner_probe(
            &k,
            2,
            false,
            37.0,
            1.0,
            3.469_142_466_323_665_16e-3,
            (
                0.677_557_068_372_655_4,
                0.888_683_823_425_455_3,
                0.834_298_343_197_838_4,
            ),
        );
        assert!(shortest_curve(&r) > 1e-3, "{}", shortest_curve(&r));
        assert_region(&r, &plane, area);
    }

    /// Review r2 of #18771, B1: a slab in the box `[0, 1e-3]^2` of `z = 0`, swept at
    /// 30 degrees, from the fold `a`-`c`-`b` whose generator then leaves the plane to
    /// `z = 1e5`. The slab's `beta` line passes `1e-10` in `mu` above the corner
    /// `(0, 0)`, so the polygon has a real edge `1.15e-10` long there (above
    /// `SM_EFF_ZERO` and far above `SM_EFF_ZERO m`, `m = 1e-3`). The far control
    /// point (read by the evaluations at `b`) makes the error bound of the
    /// generator's evaluations about `2e-10`, which puts the two ends within their
    /// bounds: at a8f0d30cd the slab merged them (without the `SM_EFF_ZERO m` cap)
    /// and lost the edge. The edge stays, with the region's five sides and exact area.
    #[test]
    fn slab_keeps_a_real_short_edge_beside_a_far_generator() {
        let k = k();
        let dom = Extent2d::from_bounds(0.0, 0.0, 1e-3, 1e-3);
        let plane = xy_plane(dom);
        let th = std::f64::consts::FRAC_PI_6;
        let (sn, cs) = th.sin_cos();
        let mu = |p: (f64, f64)| -sn * p.0 + cs * p.1;
        let at_mu = |u: f64, m: f64| (u, (m + sn * u) / cs);
        let (a, c, b) = (at_mu(5e-4, 1e-10), at_mu(6e-4, -2e-4), at_mu(3e-4, -1e-4));
        let far = (b.0, b.1, 1e5);
        let pts = [(a.0, a.1, 0.0), (c.0, c.1, 0.0), (b.0, b.1, 0.0), far];
        let s = swept(&k, &pts, None, Vec3::new(cs, sn, 0.0));
        let gdom = Extent2d::from_bounds(0.0, -1.0, 3.0, 1.0);
        let r = s
            .intersect_with_plane(&k, &gdom, &plane, &plane.natural_uv_domain(), None)
            .unwrap();
        let (alpha, beta) = (mu(c), mu(a).max(mu(b)));
        let boxp = [
            Point2::new(0.0, 0.0),
            Point2::new(1e-3, 0.0),
            Point2::new(1e-3, 1e-3),
            Point2::new(0.0, 1e-3),
        ];
        let region = cut(&boxp, &|p: Point2| mu((p.x, p.y)) - alpha);
        let region = cut(&region, &|p: Point2| beta - mu((p.x, p.y)));
        assert_eq!(region.len(), 5);
        assert_eq!(r.curves_3d.len(), 5, "{:?}", r.curves_3d.len());
        let short = shortest_curve(&r);
        assert!(short > 1e-10 && short < 2e-10, "{short:e}");
        assert_region(&r, &plane, uv_area(&region).abs());
    }

    #[test]
    fn non_parallel_extrusions_need_more() {
        let k = k();
        let a = extrusion();
        let mut line = Line::from_points(Point3::ZERO, Point3::new(1.0, 1.0, 0.0), 3);
        line.make_nurb().unwrap();
        let b = SurfOfExtrusion::create_canonical(&k, Curve::from(line), Vec3::new(1.0, 0.0, 1.0))
            .unwrap();
        let r = a.intersect_with_surf_of_extrusion(&k, &b, None).unwrap();
        assert!(r.needs_more_intersections);
        assert!(r.curves_3d.is_empty());
    }

    #[test]
    fn trim_rejects_a_larger_domain() {
        let k = k();
        let mut s = extrusion();
        let d = s.base().natural_uv_domain();
        let mut big = d;
        big.expand_absolute(1.0);
        assert!(s.trim_with_domain(&k, &big).is_err());
        assert!(s.trim_with_domain(&k, &d).is_ok());
    }

    #[test]
    fn truncated_binary_record_fails() {
        let k = k();
        let mut db = DatabaseReader::new();
        db.open_for_read(vec![0u8; 20], FileType::Binary);
        assert!(SurfOfExtrusion::read_from_db(&k, &mut db, 30).is_err());
    }

    #[test]
    fn orientation_codes() {
        assert_eq!(orientation_from_code(4).unwrap(), CurveOrientType::Linear);
        assert!(orientation_from_code(6).is_err());
    }
}
