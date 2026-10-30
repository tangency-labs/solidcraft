//! Native cone/plane and cone/cone intersection adapters.
//!
//! Cone records: origin, x axis, y axis, bottom/top radii, height, start/end
//! angles (within `[-MAX_ANGLE, MAX_ANGLE]`, [`MAX_ANGLE`]), swap UV, inside out, cylinder class
//! (17 doubles). Plane records:
//! origin, x axis, y axis, STEP domain (13 doubles). Domain records contain
//! two rectangles as fractions of each natural NURB domain. The result stores
//! status, needs-general, full curve geometry and NURBS, UV nullness, types and
//! deviations. No native object or allocation crosses the boundary.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_cone_p3_intersect(
            op: i32,
            cone: *const f64,
            other: *const f64,
            domains: *const f64,
            tolerance: f64,
            has_tolerance: i32,
            same: i32,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Largest start or end angle magnitude (degrees) of a cone record, upstream's
/// documented range (`SmCone.h:205-206`). Native evaluation wraps angles one turn
/// per iteration (`SmExtent1d::PeriodicWrap`, upstream-issues HANG-1) and polar
/// set-up shifts them one turn per iteration (HANG-23), so the cost of every
/// evaluation grows with the angle: fuzzing found intersections of cones near
/// `3.8e8` degrees that did not finish in 20 s, and near `9.2e18` set-up never ends.
pub const MAX_ANGLE: f64 = 360.0;

fn cone_valid(v: &[f64]) -> bool {
    v.len() == 17
        && v.iter().all(|x| x.is_finite())
        && v.get(12..14)
            .unwrap_or(&[])
            .iter()
            .all(|x| x.abs() <= MAX_ANGLE)
        && v.get(14..)
            .unwrap_or(&[])
            .iter()
            .all(|x| *x == 0.0 || *x == 1.0)
}

/// Calls the native analytic intersection (`op` 0 cone/cone, 1 cone/plane,
/// 2 global cone/plane, 3 global cone/cone). `same` uses the exact same native cone pointer.
///
/// # Errors
/// Invalid records, malformed domains, or native adapter failures.
pub fn intersect(
    op: i32,
    cone: &[f64],
    other: &[f64],
    domains: &[f64; 8],
    tolerance: Option<f64>,
    same: bool,
) -> Result<Vec<f64>, FfiError> {
    let other_valid = if op == 0 || op == 3 {
        cone_valid(other)
    } else {
        other.len() == 13 && other.iter().all(|x| x.is_finite())
    };
    if !(0..=3).contains(&op)
        || !cone_valid(cone)
        || !other_valid
        || !domains.iter().all(|x| x.is_finite())
        || tolerance.is_some_and(|x| !x.is_finite())
        || (same && op != 0 && op != 3)
    {
        return Err(FfiError::InvalidArgument(
            "cone intersection records".into(),
        ));
    }
    // Coincident native cylinders dereference the optional tolerance unconditionally.
    // The shim guards that exact path after computing its geometry, rather than
    // rejecting all calls without a tolerance here.
    // The shim also rejects a STEP-to-NURBS conversion whose cone has no polar
    // curve after MakeNurb, at the virtual call that would dereference it.
    // It also replays IntersectWithCone/IntersectWithPlane up to the seam-line
    // intersections of sm_GetEllipseTrimIntervals and rejects the inputs on which
    // either fails, because upstream then reads an uninitialised parameter (UB-154).
    call_with_buffer(4096, |out, len| {
        // SAFETY: fixed record lengths and values checked above; output and length
        // are writable, and native-owned curves are destroyed before returning.
        unsafe {
            sys::sc_surface_cone_p3_intersect(
                op,
                cone.as_ptr(),
                other.as_ptr(),
                domains.as_ptr(),
                tolerance.unwrap_or(0.0),
                i32::from(tolerance.is_some()),
                i32::from(same),
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cone() -> Vec<f64> {
        vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 3.0, 0.0, 360.0, 0.0, 0.0, 0.0,
        ]
    }
    const DOMAINS: [f64; 8] = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0];

    #[test]
    fn rejects_malformed_record_before_entering_cpp() {
        assert!(matches!(
            intersect(0, &[], &cone(), &DOMAINS, None, false),
            Err(FfiError::InvalidArgument(_))
        ));
    }

    #[test]
    fn guards_only_default_tolerance_path_that_dereferences_null() {
        let a = cone();
        assert_eq!(
            intersect(0, &a, &a, &DOMAINS, None, false),
            Err(FfiError::Status(-1002))
        );
        // Identity returns before reading the optional tolerance.
        assert!(intersect(0, &a, &a, &DOMAINS, None, true).is_ok());
        let mut other = cone();
        // Same axis but disjoint heights: the native routine returns before the
        // coincident branch, so omitting the tolerance remains valid.
        if let Some(z) = other.get_mut(2) {
            *z = 10.0;
        }
        assert!(intersect(0, &a, &other, &DOMAINS, None, false).is_ok());
        // Nested radii likewise return before the null dereference.
        let mut nested = cone();
        for i in [9, 10] {
            if let Some(r) = nested.get_mut(i) {
                *r = 2.0;
            }
        }
        assert!(intersect(0, &a, &nested, &DOMAINS, None, false).is_ok());
    }

    #[test]
    fn rejects_only_angles_outside_the_documented_range() {
        for index in [12, 13] {
            for value in [
                MAX_ANGLE + 1e-9,
                -MAX_ANGLE - 1e-9,
                720.0,
                3.774_862_8e8,
                -9.223_372_036_854_776e18,
            ] {
                let mut a = cone();
                a[index] = value;
                assert!(matches!(
                    intersect(3, &cone(), &a, &DOMAINS, Some(1e-7), false),
                    Err(FfiError::InvalidArgument(_))
                ));
            }
        }
        // Native may still report a status for these; the wrapper must not refuse them.
        for (start, end) in [(-MAX_ANGLE, -270.0), (270.0, MAX_ANGLE), (-MAX_ANGLE, MAX_ANGLE)] {
            let mut other = cone();
            other[12] = start;
            other[13] = end;
            assert!(!matches!(
                intersect(3, &cone(), &other, &DOMAINS, Some(1e-7), false),
                Err(FfiError::InvalidArgument(_))
            ));
        }
    }

    fn plane() -> Vec<f64> {
        vec![
            0.0, 0.0, 1.5, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -4.0, -4.0, 4.0, 4.0,
        ]
    }

    #[test]
    fn rejects_nonfinite_values_and_invalid_flags_in_every_field() {
        for index in 0..17 {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut a = cone();
                a[index] = value;
                assert!(matches!(
                    intersect(0, &a, &cone(), &DOMAINS, None, false),
                    Err(FfiError::InvalidArgument(_))
                ));
                assert!(matches!(
                    intersect(0, &cone(), &a, &DOMAINS, None, false),
                    Err(FfiError::InvalidArgument(_))
                ));
            }
        }
        for index in 0..13 {
            let mut p = plane();
            p[index] = f64::NAN;
            assert!(matches!(
                intersect(1, &cone(), &p, &DOMAINS, None, false),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for index in 0..8 {
            let mut domains = DOMAINS;
            domains[index] = f64::INFINITY;
            assert!(matches!(
                intersect(1, &cone(), &plane(), &domains, None, false),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for index in 14..17 {
            for value in [-1.0, 0.5, 2.0] {
                let mut a = cone();
                a[index] = value;
                assert!(matches!(
                    intersect(0, &a, &cone(), &DOMAINS, None, false),
                    Err(FfiError::InvalidArgument(_))
                ));
            }
        }
        for op in [-1, 4] {
            assert!(matches!(
                intersect(op, &cone(), &plane(), &DOMAINS, None, false),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        for op in [1, 2] {
            assert!(matches!(
                intersect(op, &cone(), &plane(), &DOMAINS, None, true),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        assert!(matches!(
            intersect(0, &cone(), &cone(), &DOMAINS, Some(f64::NAN), false),
            Err(FfiError::InvalidArgument(_))
        ));
    }

    #[test]
    fn native_dispatch_flag_tolerance_and_domain_matrix() {
        for op in 0..=3 {
            for class in [0.0, 1.0] {
                for swap in [0.0, 1.0] {
                    for inside in [0.0, 1.0] {
                        for tolerance in
                            [None, Some(0.0), Some(1e-7), Some(-1e-7), Some(9876543.21)]
                        {
                            for subdomain in [false, true] {
                                let mut a = cone();
                                a[14] = swap;
                                a[15] = inside;
                                a[16] = class;
                                let mut b = if op == 0 || op == 3 { cone() } else { plane() };
                                if op == 0 || op == 3 {
                                    b[0] = 1.0;
                                }
                                let mut domains = DOMAINS;
                                if subdomain {
                                    domains[0] = 0.125;
                                    domains[2] = 0.875;
                                    domains[5] = 0.125;
                                    domains[7] = 0.875;
                                }
                                assert!(intersect(op, &a, &b, &domains, tolerance, false).is_ok(), "op={op} class={class} swap={swap} inside={inside} tol={tolerance:?} subdomain={subdomain}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn native_constructor_boundaries_return_without_dereferencing_failed_geometry() {
        for (index, value) in [
            (9, 0.0),
            (10, 0.0),
            (9, -1.0),
            (10, -1.0),
            (11, 0.0),
            (11, 1e-15),
            (11, -1.0),
            (13, 0.0),
            (13, -90.0),
            (12, -360.0),
            (3, 0.0),
            (7, 0.0),
            (3, 2.0),
        ] {
            let mut a = cone();
            a[index] = value;
            assert!(
                intersect(1, &a, &plane(), &DOMAINS, Some(1e-7), false).is_ok(),
                "constructor boundary index={index} value={value}"
            );
        }
        // A two-turn end angle is outside the documented range ([`MAX_ANGLE`]).
        let mut a = cone();
        a[13] = 720.0;
        assert!(matches!(
            intersect(1, &a, &plane(), &DOMAINS, Some(1e-7), false),
            Err(FfiError::InvalidArgument(_))
        ));
    }

    #[test]
    fn global_cylinder_target_uses_cone_analytic_guard() {
        let a = cone();
        let mut b = cone();
        b[16] = 1.0;
        // SmCylinder_TYPE reaches IntersectWithCone through the native global
        // switch (v1.0.1 SmCone.cpp:2377), so a coincident cylinder without a
        // tolerance takes the same guard as a cone target; with one it runs.
        assert!(matches!(
            intersect(3, &a, &b, &DOMAINS, None, false),
            Err(FfiError::Status(crate::SC_ERR_ARG))
        ));
        assert!(intersect(3, &a, &b, &DOMAINS, Some(1e-7), false).is_ok());
        // A disjoint cylinder needs no tolerance.
        b[0] = 10.0;
        assert!(intersect(3, &a, &b, &DOMAINS, None, false).is_ok());
    }

    #[test]
    fn guards_missing_polar_curve_only_when_conversion_is_reached() {
        let mut a = vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1e-300, 0.0, 1.5628, 1.5628,
            1.10780, 0.0, 360.0, 0.0, 1.0, 0.0,
        ];
        let mut b = vec![
            0.0, 0.5539, 0.5539, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.5628, 1.5628,
            1.10780, 0.0, 360.0, 0.0, 0.0, 0.0,
        ];
        let domains = [0.0, 0.0, 1.0, 1.0, 0.0, 0.125, 1.0, 0.875];
        for class in [0.0, 1.0] {
            a[16] = class;
            b[16] = class;
            for (first, second) in [(&a, &b), (&b, &a)] {
                assert_eq!(
                    intersect(0, first, second, &domains, Some(1e-7), false),
                    Err(FfiError::Status(crate::SC_ERR_ARG))
                );
            }
        }
        a[16] = 0.0;
        b[16] = 0.0;
        assert_eq!(
            intersect(3, &a, &b, &domains, Some(1e-7), false),
            Err(FfiError::Status(crate::SC_ERR_ARG))
        );
        // Missing polar state alone is not a reason to reject: these paths
        // return before requesting the undefined conversion.
        assert!(intersect(0, &a, &b, &domains, Some(1e-7), true).is_ok());
        b[1] = 10.0;
        assert!(intersect(0, &a, &b, &domains, Some(1e-7), false).is_ok());
    }

    #[test]
    fn guards_ub154_only_when_a_seam_line_intersection_fails() {
        let a = vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.335_032_448_789_221_8,
            1.335_032_448_789_221_8, 1.414_096_494_588_000_1, 0.0, 360.0, 1.0, 1.0, 0.0,
        ];
        let b = vec![
            0.0, 0.707_048_247_294_000_1, 0.707_048_247_294_000_1, 1.0, 0.0, 0.0, 0.0, 0.0,
            1.0, 1.335_032_448_789_221_8, 1.335_032_448_789_221_8, 1.414_096_494_588_000_1,
            0.0, 360.0, 0.0, 0.0, 0.0,
        ];
        let rejected = Err(FfiError::Status(crate::SC_ERR_ARG));
        // issue #276 fuzz00656: a reversed trim domain gives zero-length seams.
        let fuzz = [0.0, 0.0, -1e300, 1.0, 0.0, 0.0, 1.0, 1.0];
        for op in [0, 3] {
            assert_eq!(intersect(op, &a, &b, &fuzz, Some(1e-7), false), rejected);
        }
        // Seam length 1.414 * d against ZERO_VEC_ER's 1e-12: rejected just below,
        // compared just above, on either cone.
        for (d, ok) in [(7e-13, false), (8e-13, true), (1e-9, true)] {
            for domains in [
                [0.5, 0.0, 0.5 + d, 1.0, 0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0, 1.0, 0.0, 0.5, 1.0, 0.5 + d],
            ] {
                let r = intersect(0, &a, &b, &domains, Some(1e-7), false);
                assert_eq!(r.is_ok(), ok, "d={d} domains={domains:?} {r:?}");
            }
        }
        // Identity returns before the elliptical branch.
        assert!(intersect(0, &a, &a, &fuzz, Some(1e-7), true).is_ok());
    }
}
