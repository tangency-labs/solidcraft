//! The `AssertValid` overrides of the compound and Hermite curve classes, reached
//! through the virtual curve check ([`super::edge`]'s `curve`).
//!
//! Upstream: `source/SMLib/src/SmOffsetCurve.cpp`, `SmProjectedCurve.cpp`,
//! `SmHermiteCurve.cpp`, `SmCrvOnSurf.cpp`, `SmCrvInVolume.cpp`.
//!
//! | Rust (private)    | Upstream                       |
//! |-------------------|--------------------------------|
//! | `offset_curve`    | `SmOffsetCurve::AssertValid`   |
//! | `projected_curve` | `SmProjectedCurve::AssertValid` |
//! | `hermite_curve`   | `SmHermiteCurve::AssertValid`  |
//! | `crv_on_surf`     | `SmCrvOnSurf::AssertValid` (also `SmTangentField`'s, inherited) |
//! | `crv_in_volume`   | `SmCrvInVolume::AssertValid`   |
//! | `Walk`            | the contained objects' identities (`this` pointers) |
//!
//! Deviations:
//! - **Reports.** The release `SM_ASSERT_*_REPORT` macros. Every test here is
//!   `SM_LEVEL_0`, so it always runs; the level selects whether `SmCurve::AssertValid`
//!   runs (not at `SM_LEVEL_GIVEN`) and is passed to the contained checks.
//! - **Owners.** The port holds contained curves, surfaces and volumes by value and
//!   does not keep their owner pointers back to the compound curve; each compound
//!   curve records instead whether upstream would have linked them (`SetOwner(this)`:
//!   [`OffsetCurve::base_linked`](crate::curve::offset_curve::OffsetCurve::base_linked),
//!   [`CrvOnSurf::owner_links`], [`CrvInVolume::owner_links`](crate::curve::crv_in_volume::CrvInVolume::owner_links)),
//!   which is what "the part's owner is this" reads. The contained check sees that
//!   owner (this object's type and key) on an evaluation copy of the part, or the
//!   part's own owner when it is not linked. The copy also receives any NURB the
//!   contained check builds (upstream caches it on the part).
//! - **Contexts.** One kernel: the context tests always pass (they still run, in
//!   native order).
//! - **Identities.** A contained object reports under `key ^ (n << 48)`, `n` its
//!   one-based order in the walk from the outermost checked object (an ellipse's polar
//!   converter takes one too); callers keep bits 48.. of their keys clear. Nesting is
//!   bounded by [`MAX_GEOMETRY_NESTING`] ([`SolidError::InvalidInput`] past it; the
//!   constructors already bound it, upstream has no bound).
//! - **Undefined upstream.** An empty offset curve (no base curve) dereferences the
//!   null base in test 1: [`SolidError::InvalidInput`]. A failed Hermite test 1 with a
//!   report list reads past the one-entry default label table (upstream registers no
//!   Hermite table): [`SolidError::InvalidInput`] from [`AssertReport::new`]. The
//!   status of `CalculateBoundingBox` (curve on surface) is ignored as upstream; a
//!   failure leaves the "inside out" box.

use super::edge;
use crate::curve::crv_on_surf::CrvOnSurf;
use crate::curve::curve::p2::{CRV_IN_VOLUME_TYPE, CRV_ON_SURF_TYPE, HERMITE_CURVE_TYPE};
use crate::curve::curve::{Curve, CurveKind};
use crate::error::{ignored, Result, SolidError};
use crate::geom::assert_array::labels::{OFFSET_CURVE_TYPE, PROJECTED_CURVE_TYPE};
use crate::geom::assert_array::{AssertArray, AssertReport, ObjectKey, ReportSpec};
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::object::ObjectRef;
use crate::geom::tol::{is_zero_to_tol, tol3d};
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, UNDEF_DOUBLE};
use crate::nurbs::b_spline_curve::p1::ProjectionType;
use crate::nurbs::b_spline_curve::BoundingBoxRequest;
use crate::surface::surface::p2::MAX_GEOMETRY_NESTING;
use crate::surface::surface::Surface;

const OP: &str = "topology::brep::validation::assert_valid::curves";

/// `SM_LEVEL_GIVEN`.
const LEVEL_GIVEN: u8 = 4;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The identities and depth of one walk over a curve and its contained objects.
pub(super) struct Walk {
    root: ObjectKey,
    ordinal: u64,
    depth: usize,
}

impl Walk {
    /// A walk from the outermost checked object `root`.
    pub(super) const fn new(root: ObjectKey) -> Self {
        Self {
            root,
            ordinal: 0,
            depth: 0,
        }
    }

    /// The key of the next contained object.
    pub(super) fn next_key(&mut self) -> Result<ObjectKey> {
        self.ordinal += 1;
        if self.ordinal > 0xFFFF {
            return Err(invalid("too many contained objects for the validity walk"));
        }
        Ok(ObjectKey(self.root.0 ^ (self.ordinal << 48)))
    }
}

/// The `SM_LEVEL_0` report of test `test` of class `class` about `key` (class name
/// `class_string`): see the module docs.
#[allow(clippy::too_many_arguments)]
fn report(
    list: Option<&mut AssertArray>,
    key: ObjectKey,
    class: i64,
    class_string: &'static str,
    test: u64,
    passed: bool,
    tol: f64,
    val: f64,
) -> Result<bool> {
    if passed {
        if let Some(l) = list {
            l.set_last_add(false);
        }
        return Ok(true);
    }
    if let Some(l) = list {
        let spec = ReportSpec {
            reporter: Some(key),
            test_index: test,
            test_class: class,
            owner: Some(key),
            owner_type: class,
            owner_type_string: Some(class_string),
            other: None,
            other_type: 0,
            other_type_string: Some("NULL"),
            tol3d: tol3d(tol),
            value: val,
            list_index: 0,
            message: None,
        };
        l.add_unique_delete_duplicate(AssertReport::new(spec)?);
    }
    Ok(false)
}

/// `SM_ASSERT_BOOLEAN_REPORT`.
fn boolean(
    list: Option<&mut AssertArray>,
    key: ObjectKey,
    class: i64,
    s: &'static str,
    test: u64,
    passed: bool,
) -> Result<bool> {
    report(
        list,
        key,
        class,
        s,
        test,
        passed,
        UNDEF_DOUBLE,
        UNDEF_DOUBLE,
    )
}

/// `part->AssertValid(pAList, eTestLevel, eWalkTree)` of a contained curve whose
/// owner is `owner`.
fn contained_curve(
    k: &Kernel,
    part: &Curve,
    owner: Option<ObjectRef>,
    list: Option<&mut AssertArray>,
    level: u8,
    walk: &mut Walk,
) -> Result<bool> {
    if walk.depth >= MAX_GEOMETRY_NESTING {
        return Err(invalid("contained curve validity nesting exceeds 32"));
    }
    let key = walk.next_key()?;
    let polar_key = walk.next_key()?;
    let mut copy = part.clone_for_evaluation()?;
    let (_, _) = copy.set_owner(owner, None);
    walk.depth += 1;
    let r = edge::curve_in_walk(k, &mut copy, list, level, key, polar_key, None, walk);
    walk.depth -= 1;
    r
}

/// `m_pSurface->AssertValid(pAList, eTestLevel, eWalkTree)` of a contained surface
/// whose owner is `owner`.
fn contained_surface(
    k: &Kernel,
    part: &Surface,
    owner: Option<ObjectRef>,
    list: Option<&mut AssertArray>,
    level: u8,
    walk: &mut Walk,
) -> Result<bool> {
    let key = walk.next_key()?;
    let mut copy = part.clone_for_evaluation()?;
    *copy.owner_mut() = owner;
    copy.assert_valid(k, list, level, key)
}

/// `SmOffsetCurve::AssertValid` of `c` (an offset curve).
pub(super) fn offset_curve(
    _k: &Kernel,
    c: &Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
) -> Result<bool> {
    const C: i64 = OFFSET_CURVE_TYPE;
    let CurveKind::Offset(x) = c.kind() else {
        return Err(invalid("offset curve check of another curve kind"));
    };
    let s = x.class_string();
    let mut list = list;
    let mut rtn = true;

    // call the base class AssertValid
    rtn &= if level == LEVEL_GIVEN {
        true
    } else {
        c.assert_valid(list.as_deref_mut(), false, key)?
    };

    // all contained pointers to crContext should be the same
    /*  0 */
    // m_pSurface shares the same context
    rtn &= boolean(list.as_deref_mut(), key, C, s, 0, true)?;

    /*  1 */
    // Bad m_pSurface Owner (upstream dereferences a null base curve)
    if x.base_curve().is_none() {
        return Err(invalid("offset curve without a base curve"));
    }
    rtn &= boolean(list, key, C, s, 1, x.base_linked())?;

    // all done
    Ok(rtn)
}

/// `SmProjectedCurve::AssertValid` of `c` (a projected curve).
pub(super) fn projected_curve(
    k: &Kernel,
    c: &Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
    walk: &mut Walk,
) -> Result<bool> {
    const C: i64 = PROJECTED_CURVE_TYPE;
    let CurveKind::Projected(x) = c.kind() else {
        return Err(invalid("projected curve check of another curve kind"));
    };
    let s = x.class_string();
    let mut list = list;
    let mut rtn = true;
    let ty = x.proj_type();

    // test for an initialized object
    rtn &= boolean(
        list.as_deref_mut(),
        key,
        C,
        s,
        0,
        ty != ProjectionType::Unknown,
    )?;

    // test initialized objects
    if rtn {
        // test contained curve (no owner link: it keeps its own owner)
        let base = x.curve();
        let ok = contained_curve(k, base, base.owner(), list.as_deref_mut(), level, walk)?;
        rtn &= boolean(list.as_deref_mut(), key, C, s, 1, ok)?;

        let (point, vec, aux) = (x.proj_point(), x.proj_vec(), x.aux_data());

        // test projection defining geometry for initialization
        let init = point.is_initialized()
            && vec.is_initialized()
            && (ty == ProjectionType::Parallel || aux.is_initialized());
        rtn &= boolean(list.as_deref_mut(), key, C, s, 2, init)?;

        // test projection defining geometry for nonZero vectors
        let non_zero =
            !vec.is_zero(EFF_ZERO) && (ty != ProjectionType::Rotation || !aux.is_zero(EFF_ZERO));
        rtn &= report(
            list.as_deref_mut(),
            key,
            C,
            s,
            3,
            non_zero,
            EFF_ZERO,
            vec.length(),
        )?;

        // test projection defining geometry for unit-vectors
        let unit = is_zero_to_tol(vec.length() - 1.0, EFF_ZERO)
            && (ty != ProjectionType::Rotation || is_zero_to_tol(aux.length() - 1.0, EFF_ZERO));
        rtn &= report(
            list.as_deref_mut(),
            key,
            C,
            s,
            4,
            unit,
            EFF_ZERO,
            vec.length() - 1.0,
        )?;

        // Check base curve context
        rtn &= boolean(list, key, C, s, 5, true)?;
    } // end defined projection type check

    // all done
    Ok(rtn)
}

/// `SmHermiteCurve::AssertValid` of `c` (a Hermite curve).
pub(super) fn hermite_curve(
    _k: &Kernel,
    c: &Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
) -> Result<bool> {
    const C: i64 = HERMITE_CURVE_TYPE;
    let CurveKind::Hermite(h) = c.kind() else {
        return Err(invalid("Hermite curve check of another curve kind"));
    };
    let s = h.class_string();
    let mut list = list;
    let mut rtn = true;

    // call the base class AssertValid
    rtn &= if level == LEVEL_GIVEN {
        true
    } else {
        c.assert_valid(list.as_deref_mut(), false, key)?
    };

    let dim = h.dim();
    let (p1, d1, p2, d2) = (
        h.start_point(),
        h.start_derivative(),
        h.end_point(),
        h.end_derivative(),
    );
    let (vc, vd) = h.power_basis();
    let def2 = |v: crate::math::Vec3| v.x != UNDEF_DOUBLE && v.y != UNDEF_DOUBLE;

    // check for initialized end values
    let ends = (dim == 3 && !p1.is_undef() && !d1.is_undef() && !p2.is_undef() && !d2.is_undef())
        || (dim == 2 && def2(p1) && def2(d1) && def2(p2) && def2(d2));
    rtn &= boolean(list.as_deref_mut(), key, C, s, 0, ends)?;

    // check for initialized Power Basis
    let basis =
        (dim == 3 && !vc.is_undef() && !vd.is_undef()) || (dim == 2 && def2(vc) && def2(vd));
    rtn &= boolean(list, key, C, s, 1, basis)?;

    // all done
    Ok(rtn)
}

/// `SmCrvOnSurf::AssertValid` of `x`, the curve on surface `c` is or (tangent field)
/// derives from.
pub(super) fn crv_on_surf(
    k: &Kernel,
    c: &Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
    walk: &mut Walk,
) -> Result<bool> {
    const C: i64 = CRV_ON_SURF_TYPE;
    let x: &CrvOnSurf = match c.kind() {
        CurveKind::CrvOnSurf(x) => x,
        CurveKind::TangentField(t) => t.base(),
        _ => return Err(invalid("curve-on-surface check of another curve kind")),
    };
    let s = c.class_string();
    let this = Some(ObjectRef {
        type_code: c.type_code(),
        key: crate::geom::attribute::ObjectKey(key.0),
    });
    let flag = x.owner_flag();
    let (uv_linked, surface_linked) = x.owner_links();
    let mut list = list;
    let mut rtn = true;

    // test contained curve
    let uv = x.uv_curve();
    let uv_owner = if uv_linked { this } else { uv.owner() };
    let ok = contained_curve(k, uv, uv_owner, list.as_deref_mut(), level, walk)?;
    rtn &= boolean(list.as_deref_mut(), key, C, s, 0, ok)?;

    // test contained surface
    let surface = x.surface();
    let surface_owner = if surface_linked {
        this
    } else {
        surface.owner()
    };
    let ok = contained_surface(k, surface, surface_owner, list.as_deref_mut(), level, walk)?;
    rtn &= boolean(list.as_deref_mut(), key, C, s, 1, ok)?;

    // Curve UV BBox (status ignored)
    let req = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    let curve_box = ignored(uv.calculate_bounding_box(k, &uv.natural_interval(), req, true))?
        .and_then(|b| b.normal)
        .unwrap_or_else(Extent3d::new);
    let (lo, hi) = (curve_box.min(), curve_box.max());
    let curve_uv_box = Extent2d::from_bounds(lo.x, lo.y, hi.x, hi.y);

    // Surface UVDomain
    let surface_uv_box = surface.natural_uv_domain_with_nurb(k)?;

    // Test containment of curve within surface domain
    let contained = curve_uv_box.is_contained_by(&surface_uv_box, 100.0 * EFF_ZERO);
    rtn &= boolean(list.as_deref_mut(), key, C, s, 2, contained)?;

    /*  3 */
    // When owned by CrvOnSurf, ContainedUVCurve->owner must be this SmCrvOnSurf object
    rtn &= boolean(
        list.as_deref_mut(),
        key,
        C,
        s,
        3,
        flag == 0 || uv_owner == this,
    )?;

    /*  4 */
    // When owned by CrvOnSurf, ContainedSurface->owner must be this SmCrvOnSurf object
    let ok = flag == 0 || flag == 1 || surface_owner == this;
    rtn &= boolean(list.as_deref_mut(), key, C, s, 4, ok)?;

    /*  5 */
    // When owned by CrvOnSurf, ContainedUVCurve->context must be this SmCrvOnSurf->context
    rtn &= boolean(list.as_deref_mut(), key, C, s, 5, true)?;

    /*  6 */
    // When owned by CrvOnSurf, ContainedSurface->context must be this SmCrvOnSurf->context
    rtn &= boolean(list, key, C, s, 6, true)?;

    // all done
    Ok(rtn)
}

/// `SmCrvInVolume::AssertValid` of `c` (a curve in volume).
pub(super) fn crv_in_volume(
    k: &Kernel,
    c: &Curve,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
    walk: &mut Walk,
) -> Result<bool> {
    const C: i64 = CRV_IN_VOLUME_TYPE;
    let CurveKind::CrvInVolume(x) = c.kind() else {
        return Err(invalid("curve-in-volume check of another curve kind"));
    };
    let s = x.class_string();
    let this = Some(ObjectRef {
        type_code: c.type_code(),
        key: crate::geom::attribute::ObjectKey(key.0),
    });
    let flag = x.owner_flag();
    let owns_curve = flag & 1 != 0;
    let owns_volume = flag & 2 != 0;
    let curve_linked = x.owner_links() & 1 != 0;
    let volume_linked = x.owner_links() & 2 != 0;
    let mut list = list;

    /*  0 */
    // test BaseCurve curve
    let curve = x.curve();
    let curve_owner = if curve_linked { this } else { curve.owner() };
    let ok = contained_curve(k, curve, curve_owner, list.as_deref_mut(), level, walk)?;
    let mut rtn = boolean(list.as_deref_mut(), key, C, s, 0, ok)?;

    /*  1 */
    // test contained volume
    let volume_key = walk.next_key()?;
    let ok = x
        .volume()
        .assert_valid(list.as_deref_mut(), level, volume_key, &mut || {
            walk.next_key()
        })?;
    rtn &= boolean(list.as_deref_mut(), key, C, s, 1, ok)?;

    /*  2 */
    // consistent internal cache
    let (breaks, conts) = (x.breaks().len(), x.conts().len());
    let consistent =
        (x.need_breaks() && (breaks == 0 && conts == 0)) || (!x.need_breaks() && breaks == conts);
    rtn &= boolean(list.as_deref_mut(), key, C, s, 2, consistent)?;

    /*  3 */
    // Bad BaseCurve Owner
    rtn &= boolean(
        list.as_deref_mut(),
        key,
        C,
        s,
        3,
        owns_curve && curve_owner == this,
    )?;

    /*  4 */
    // Bad BaseVolume Owner
    rtn &= boolean(
        list.as_deref_mut(),
        key,
        C,
        s,
        4,
        owns_volume && volume_linked,
    )?;

    /*  5 */
    // Bad BaseCurve context
    rtn &= boolean(list.as_deref_mut(), key, C, s, 5, owns_curve)?;

    /*  6 */
    // Bad BaseVolume context
    rtn &= boolean(list, key, C, s, 6, owns_volume)?;

    // all done
    Ok(rtn)
}
