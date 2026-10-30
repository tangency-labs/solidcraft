//! The volume `AssertValid` checks (`SmVolume` and its derived classes), reached from
//! `SmCrvInVolume::AssertValid` (test 1, "Compounding Volume").
//!
//! Upstream: `source/SMLib/src/SmVolume.cpp`, `SmTransform.cpp`, `SmBSplineVolume.cpp`,
//! `SmBendVolume.cpp`, `SmUnbendVolume.cpp`, `SmTwistVolume.cpp`.
//!
//! | Rust                    | Upstream                                              |
//! |-------------------------|-------------------------------------------------------|
//! | [`Volume::assert_valid`] | the virtual `AssertValid` of every volume class      |
//! | `volume_assert_valid` (private) | `SmVolume::AssertValid`                       |
//! | `frame_assert_valid` (private) | `SmBendVolume::AssertValid`, `SmUnbendVolume::AssertValid`, `SmTwistVolume::AssertValid` |
//!
//! Deviations: the release `SM_ASSERT_*_REPORT` macros; every volume test is
//! `SM_LEVEL_0`, so the level only selects whether the base classes run
//! (`SM_LEVEL_GIVEN` skips `SmAObject::AssertValid`, which has no failing test) and is
//! passed down. Orient maps are [`Transform`](crate::surface::transform::Transform)
//! values with no next or orient maps of their own, so their nested check
//! (`SmTransform::AssertValid` of a simple map) and the simple-map tests 3..8 always
//! pass (they still run, in native order). Upstream builds without `USE_VOLUMES`, so
//! `SmBSplineVolume::PassesValidityCheck` returns `SM_ERR`, which the report reads as
//! true; the port's B-spline volume always holds its NURBS record (test 0). The
//! next-map recursion is bounded at [`MAX_VOLUME_CHAIN`] maps
//! ([`SolidError::InvalidInput`] past it; upstream has no bound). Contained objects
//! report under the keys `contained_key` hands out, in traversal order.

use super::{Volume, VolumeKind};
use crate::error::{Result, SolidError};
use crate::geom::a_object;
use crate::geom::assert_array::labels::{
    BEND_VOLUME_TYPE, B_SPLINE_VOLUME_TYPE, TWIST_VOLUME_TYPE, UNBEND_VOLUME_TYPE, VOLUME_TYPE,
};
use crate::geom::assert_array::{AssertArray, AssertReport, ObjectKey, ReportSpec};
use crate::geom::tol::{tol3d, ZONE_TOL_3D};
use crate::math::{EFF_ZERO, UNDEF_DOUBLE};

/// The longest next-map chain [`Volume::assert_valid`] walks (upstream: none).
pub const MAX_VOLUME_CHAIN: usize = 256;

/// `SM_LEVEL_GIVEN`.
const LEVEL_GIVEN: u8 = 4;

const OP: &str = "surface::volume::assert_valid";

/// `SM_ASSERT_BOOLEAN_REPORT` / `SM_ASSERT_VALUE_REPORT` (release) of an
/// `SM_LEVEL_0` test `test` in the `AssertValid` of class `class`, run by the
/// object `key` whose class name is `class_string`.
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

impl Volume {
    /// `AssertValid(pAList, eTestLevel, eWalkTree)` (virtual): the checks of this
    /// volume's class and of `SmVolume`, reporting failures about `key` into `list`.
    /// A next map reports under the key `contained_key` returns for it. Returns
    /// `true` when every check passed.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a next-map chain longer than
    /// [`MAX_VOLUME_CHAIN`]; those of `contained_key` and of [`AssertReport::new`].
    pub fn assert_valid(
        &self,
        list: Option<&mut AssertArray>,
        level: u8,
        key: ObjectKey,
        contained_key: &mut dyn FnMut() -> Result<ObjectKey>,
    ) -> Result<bool> {
        self.assert_valid_at(list, level, key, contained_key, 0)
    }

    fn assert_valid_at(
        &self,
        list: Option<&mut AssertArray>,
        level: u8,
        key: ObjectKey,
        contained_key: &mut dyn FnMut() -> Result<ObjectKey>,
        depth: usize,
    ) -> Result<bool> {
        if depth > MAX_VOLUME_CHAIN {
            return Err(SolidError::invalid_input(
                OP,
                "volume next-map chain exceeds the validity walk bound",
            ));
        }
        let mut list = list;

        // check base class
        let mut rtn =
            volume_assert_valid(self, list.as_deref_mut(), level, key, contained_key, depth)?;

        let s = self.class_string();
        match &self.kind {
            VolumeKind::Base | VolumeKind::Transform(_) => {}
            VolumeKind::BSpline(_) => {
                // run SmBSplineVolume PassesValidityCheck checks (SM_ERR without USE_VOLUMES)
                rtn &= report(
                    list.as_deref_mut(),
                    key,
                    B_SPLINE_VOLUME_TYPE,
                    s,
                    1,
                    true,
                    UNDEF_DOUBLE,
                    UNDEF_DOUBLE,
                )?;
                // check for nonNULL m_pNurb pointer
                rtn &= report(
                    list.as_deref_mut(),
                    key,
                    B_SPLINE_VOLUME_TYPE,
                    s,
                    0,
                    true,
                    UNDEF_DOUBLE,
                    UNDEF_DOUBLE,
                )?;
            }
            VolumeKind::Bend(b) => {
                let frame = Frame {
                    axis: self.bend_axis(),
                    mid_dir: self.bend_mid_dir(),
                    binormal: self.bend_binormal(),
                };
                rtn &= frame_assert_valid(&frame, list.as_deref_mut(), key, BEND_VOLUME_TYPE, s)?;
                rtn &= neutral_checks(
                    b.bend_neutral_dist(),
                    b.singularity_tol(),
                    list,
                    key,
                    BEND_VOLUME_TYPE,
                    s,
                )?;
            }
            VolumeKind::Unbend(u) => {
                let frame = Frame {
                    axis: self.unbend_axis(),
                    mid_dir: self.unbend_mid_dir(),
                    binormal: self.unbend_binormal(),
                };
                rtn &= frame_assert_valid(&frame, list.as_deref_mut(), key, UNBEND_VOLUME_TYPE, s)?;
                rtn &= neutral_checks(
                    u.unbend_neutral_dist(),
                    u.singularity_tol(),
                    list,
                    key,
                    UNBEND_VOLUME_TYPE,
                    s,
                )?;
            }
            VolumeKind::Twist(t) => {
                let frame = Frame {
                    axis: self.twist_axis(),
                    mid_dir: self.twist_mid_dir(),
                    binormal: self.twist_binormal(),
                };
                rtn &= frame_assert_valid(&frame, list.as_deref_mut(), key, TWIST_VOLUME_TYPE, s)?;
                // Twist PeriodDistance abs value has to be greater than PeriodMinDist
                let period = t.twist_period_dist();
                rtn &= report(
                    list,
                    key,
                    TWIST_VOLUME_TYPE,
                    s,
                    5,
                    period.abs() >= t.period_min_dist(),
                    EFF_ZERO,
                    period,
                )?;
            }
        }

        // all done
        Ok(rtn)
    }
}

/// `SmVolume::AssertValid`.
fn volume_assert_valid(
    v: &Volume,
    list: Option<&mut AssertArray>,
    level: u8,
    key: ObjectKey,
    contained_key: &mut dyn FnMut() -> Result<ObjectKey>,
    depth: usize,
) -> Result<bool> {
    const C: i64 = VOLUME_TYPE;
    let mut list = list;
    let s = v.class_string();

    // check base class validity
    let mut rtn = true;

    // call the base class AssertValid
    rtn &= if level == LEVEL_GIVEN {
        true
    } else {
        a_object::assert_valid(false)
    };

    // check NextMap
    let ok = match v.next_map.as_deref() {
        None => true,
        Some(next) => {
            let next_key = contained_key()?;
            next.assert_valid_at(
                list.as_deref_mut(),
                level,
                next_key,
                contained_key,
                depth + 1,
            )?
        }
    };
    rtn &= ok;
    report(
        list.as_deref_mut(),
        key,
        C,
        s,
        0,
        rtn,
        UNDEF_DOUBLE,
        UNDEF_DOUBLE,
    )?;

    // check OrientMap (a simple SmTransform: its SmVolume check passes)
    rtn &= true;
    report(
        list.as_deref_mut(),
        key,
        C,
        s,
        1,
        rtn,
        UNDEF_DOUBLE,
        UNDEF_DOUBLE,
    )?;

    // check InvOrientMap (likewise)
    rtn &= true;
    report(
        list.as_deref_mut(),
        key,
        C,
        s,
        2,
        rtn,
        UNDEF_DOUBLE,
        UNDEF_DOUBLE,
    )?;

    // simple OrientMap check - Orient maps are simple, i.e. they have no Next, Orient, or InvOrient maps
    // simple InvOrientMap check - likewise
    for test in 3..=8 {
        report(
            list.as_deref_mut(),
            key,
            C,
            s,
            test,
            true,
            UNDEF_DOUBLE,
            UNDEF_DOUBLE,
        )?;
    }

    // all done
    Ok(rtn)
}

/// The in-space frame of a bend, unbend or twist volume (`Get*Axis`, `Get*MidDir`,
/// `Get*Binormal`).
struct Frame {
    axis: crate::math::Vec3,
    mid_dir: crate::math::Vec3,
    binormal: crate::math::Vec3,
}

/// Tests 0..4 of `SmBendVolume`, `SmUnbendVolume` and `SmTwistVolume::AssertValid`:
/// unit axis, mid direction and binormal, and an orthonormal right-handed basis.
fn frame_assert_valid(
    f: &Frame,
    list: Option<&mut AssertArray>,
    key: ObjectKey,
    class: i64,
    s: &'static str,
) -> Result<bool> {
    let mut list = list;
    let mut rtn = true;

    // Axis, MidDir and Binormal are unit vectors
    for (test, v) in [(0, f.axis), (1, f.mid_dir), (2, f.binormal)] {
        let dev = (v.dot(v) - 1.0).abs();
        rtn &= report(
            list.as_deref_mut(),
            key,
            class,
            s,
            test,
            dev < EFF_ZERO,
            EFF_ZERO,
            dev,
        )?;
    }

    // basis triple product - should be +1
    let triple = f.mid_dir.triple_product(f.binormal, f.axis);

    // basis vectors have to be mutually orthogonal
    let ortho = triple.abs() - 1.0;
    rtn &= report(
        list.as_deref_mut(),
        key,
        class,
        s,
        3,
        ortho < EFF_ZERO,
        EFF_ZERO,
        ortho,
    )?;

    // basis vectors have to be right handed
    let right = triple - 1.0;
    rtn &= report(list, key, class, s, 4, right < EFF_ZERO, EFF_ZERO, right)?;
    Ok(rtn)
}

/// Tests 5 and 6 of `SmBendVolume` / `SmUnbendVolume::AssertValid`: the neutral
/// distance is at least the singularity tolerance, which is at least
/// `SM_ZONE_TOL_3D`.
fn neutral_checks(
    neutral_dist: f64,
    singularity_tol: f64,
    list: Option<&mut AssertArray>,
    key: ObjectKey,
    class: i64,
    s: &'static str,
) -> Result<bool> {
    let mut list = list;
    let mut rtn = report(
        list.as_deref_mut(),
        key,
        class,
        s,
        5,
        neutral_dist >= singularity_tol,
        EFF_ZERO,
        neutral_dist,
    )?;
    rtn &= report(
        list,
        key,
        class,
        s,
        6,
        singularity_tol >= ZONE_TOL_3D,
        EFF_ZERO,
        singularity_tol,
    )?;
    Ok(rtn)
}
