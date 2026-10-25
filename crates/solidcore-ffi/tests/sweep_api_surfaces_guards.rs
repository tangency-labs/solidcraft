//! Wrapper-only sweep guards; no C++ reference build is required.
#![allow(unsafe_code)] // FFI wrapper and recording shim, as in the FFI library.
#![cfg(not(feature = "cpp"))]

use solidcore_ffi::{call_with_buffer, FfiError};
use std::cell::RefCell;

#[allow(dead_code)]
#[path = "../src/units/sweep_api_surfaces.rs"]
mod sweep_api_surfaces;
#[allow(dead_code)]
#[path = "../src/validate.rs"]
mod validate;

use sweep_api_surfaces::{run, Call, CurveSpec};

thread_local! {
    static DISPATCHES: RefCell<Vec<Vec<f64>>> = const { RefCell::new(Vec::new()) };
}

#[no_mangle]
unsafe extern "C" fn sc_sweep_api_surfaces_run(
    input: *const f64,
    n_in: i64,
    _out: *mut f64,
    _cap: i64,
    out_len: *mut i64,
) -> i32 {
    // SAFETY: run supplies a valid input slice and writable output length.
    let program = unsafe { std::slice::from_raw_parts(input, n_in as usize) };
    DISPATCHES.with_borrow_mut(|calls| calls.push(program.to_vec()));
    unsafe { *out_len = 0 };
    solidcore_ffi::SC_OK
}

fn line() -> CurveSpec {
    CurveSpec::LineSegment {
        start: [0.0; 3],
        end: [1.0, 0.0, 0.0],
    }
}

fn circle() -> CurveSpec {
    CurveSpec::AnalyticCircle {
        center: [0.0; 3],
        radius: 1.0,
    }
}

fn extrude(curve: CurveSpec) -> Call {
    Call::Extrude {
        curve,
        vector: [0.0, 0.0, 1.0],
    }
}

fn assert_rejected(call: &Call) {
    DISPATCHES.with_borrow_mut(Vec::clear);
    assert!(matches!(run(call), Err(FfiError::InvalidArgument(_))));
    DISPATCHES.with_borrow(|calls| assert!(calls.is_empty()));
}

#[test]
fn extrude_real_line_dispatches_exact_payload() {
    DISPATCHES.with_borrow_mut(Vec::clear);
    assert_eq!(run(&extrude(line())), Ok(Vec::new()));
    DISPATCHES.with_borrow(|calls| {
        assert_eq!(
            calls.as_slice(),
            &[vec![1.0, 5.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]]
        );
    });
}

#[test]
fn extrude_real_circle_dispatches_exact_payload() {
    DISPATCHES.with_borrow_mut(Vec::clear);
    assert_eq!(run(&extrude(circle())), Ok(Vec::new()));
    DISPATCHES.with_borrow(|calls| {
        assert_eq!(
            calls.as_slice(),
            &[vec![1.0, 6.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]]
        );
    });
}

#[test]
fn null_curve_is_rejected_before_dispatch() {
    assert_rejected(&extrude(CurveSpec::Null));
}

#[test]
fn nonfinite_coordinates_are_rejected_before_dispatch() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut point = [0.0; 3];
            point[axis] = value;
            assert_rejected(&extrude(CurveSpec::LineSegment {
                start: point,
                end: [1.0, 0.0, 0.0],
            }));
            assert_rejected(&extrude(CurveSpec::LineSegment {
                start: [0.0; 3],
                end: point,
            }));
            assert_rejected(&extrude(CurveSpec::AnalyticCircle {
                center: point,
                radius: 1.0,
            }));
            for curve in [line(), circle()] {
                assert_rejected(&Call::Extrude {
                    curve,
                    vector: point,
                });
            }
        }
    }
}

#[test]
fn invalid_circle_radii_are_rejected_before_dispatch() {
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_rejected(&extrude(CurveSpec::AnalyticCircle {
            center: [0.0; 3],
            radius,
        }));
    }
}

#[test]
fn other_bspline_needs_still_reject_subclasses() {
    let bspline = CurveSpec::ApiCircle {
        center: [0.0; 3],
        radius: 1.0,
    };
    for curve in [line(), circle()] {
        // The unchanged rejection paths keep their exact pre-existing messages.
        let expected = if matches!(curve, CurveSpec::LineSegment { .. }) {
            "line where upstream takes an SmBSplineCurve"
        } else {
            "analytic circle where upstream takes an SmBSplineCurve"
        };
        for call in [
            Call::Sweep {
                curve: curve.clone(),
                path: bspline.clone(),
            },
            Call::Sweep {
                curve: bspline.clone(),
                path: curve.clone(),
            },
            Call::Ruled {
                curve1: curve.clone(),
                curve2: bspline.clone(),
            },
            Call::Ruled {
                curve1: bspline.clone(),
                curve2: curve.clone(),
            },
            Call::Revolution {
                curve: curve.clone(),
                origin: [0.0; 3],
                axis: [0.0, 0.0, 1.0],
                angle_deg: 90.0,
            },
            Call::Skin {
                profiles: vec![curve],
            },
        ] {
            let err = run(&call).unwrap_err();
            assert!(
                matches!(&err, FfiError::InvalidArgument(message) if message.as_str() == expected),
                "{err:?}"
            );
            DISPATCHES.with_borrow(|calls| assert!(calls.is_empty()));
        }
    }
}

#[test]
fn any_curve_need_still_accepts_subclasses() {
    DISPATCHES.with_borrow_mut(Vec::clear);
    assert_eq!(
        run(&Call::SkinPrimitive {
            counts: vec![2],
            curves: vec![line(), circle()],
            degree: 1,
            cap_ends: false,
        }),
        Ok(Vec::new())
    );
    DISPATCHES.with_borrow(|calls| assert_eq!(calls.len(), 1));
}
