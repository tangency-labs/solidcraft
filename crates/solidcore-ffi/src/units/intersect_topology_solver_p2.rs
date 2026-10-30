//! FFI for the topology p2 traversal using the validated p1 fixture protocol.
//!
//! Upstream: `source/SMLib/src/SmTopologySolver.cpp:3024` (`LocalSolve`).
//! The C++ entry runs real `SolveTrees`/`SolveTwoTrees`, which invoke `LocalSolve`.
use super::intersect_topology_solver_p1::{validate, Op};
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_intersect_topology_solver_p2_empty_lists(results: *mut i64) -> i32;
        pub fn sc_intersect_topology_solver_p2_edge_vertex_operand() -> i32;
        pub fn sc_intersect_topology_solver_p2_static(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_intersect_topology_solver_p2_static_faces(
            input: *const f64,
            n_in: i64,
            srf: *const f64,
            n_srf: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_intersect_topology_solver_p2_local(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_intersect_topology_solver_p2_point_storage(counts: *mut i64) -> i32;
        pub fn sc_intersect_topology_solver_p2_trees(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Runs topology-tree traversal on a p1-protocol tree record.
///
/// # Errors
/// Invalid counts, flags, objects or geometry are rejected by the shared validator;
/// C++ setup errors remain harness errors.
pub fn eval_trees(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(Op::Trees, input)?;
    call_with_buffer(1024, |out, len| {
        // SAFETY: the complete record is validated by the same parser as the
        // delegated C++ reference; output capacity is enforced by the shim.
        unsafe {
            sys::sc_intersect_topology_solver_p2_trees(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// C++ object-list counts for external and managed point nodes, respectively.
/// Used to verify the fixture storage convention independently of the port.
///
/// # Errors
/// C++ setup failures.
pub fn point_storage_counts() -> Result<[i64; 2], FfiError> {
    let mut counts = [0; 2];
    // SAFETY: the fixed-size shim writes exactly two i64 entries.
    let status = unsafe { sys::sc_intersect_topology_solver_p2_point_storage(counts.as_mut_ptr()) };
    crate::check(status)?;
    Ok(counts)
}

/// Native solve status and solution count for each order of an empty-list tree.
///
/// # Errors
/// C++ setup failures.
pub fn empty_list_results() -> Result<[i64; 4], FfiError> {
    let mut results = [0; 4];
    // SAFETY: the shim writes exactly four i64 entries to the provided array.
    let status = unsafe { sys::sc_intersect_topology_solver_p2_empty_lists(results.as_mut_ptr()) };
    crate::check(status)?;
    Ok(results)
}

/// Runs a validated local-solver fixture (including p2's `CurveVertex` class).
///
/// # Errors
/// Invalid records or C++ setup failures.
pub fn eval_local(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(Op::Local, input)?;
    call_with_buffer(1024, |out, len| {
        // SAFETY: the complete p1-protocol local record is validated above;
        // the delegated reference enforces output capacity.
        unsafe {
            sys::sc_intersect_topology_solver_p2_local(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs bounded static topology entrypoints on shell-vertex B-reps or empty shapes.
/// Header: entry, operation, request, tolerance, best, n1, n2, query xyz, vertices xyz;
/// the final two flags select swapped arguments and inclusion without a shape limit.
///
/// # Errors
/// Malformed/nonfinite/unbounded input, or a reference construction failure.
pub fn eval_static(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_static(input)?;
    call_with_buffer(1024, |out, len| {
        // SAFETY: the complete fixed header, bounded counts and finite payload are
        // validated above; the reference copies only within the supplied capacity.
        unsafe {
            sys::sc_intersect_topology_solver_p2_static(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// An analytic face surface of [`eval_static_faces`], built as the `ssi_corpus` shim
/// builds it (`harness/cpp/shim/sc_analytic_surface.h`; angles in degrees).
#[derive(Debug, Clone, PartialEq)]
pub enum FaceSurface {
    /// The entry's default plane (`z = 0` for the first face, `z = 1` for the second,
    /// domain `(0, 0, 1, 1)`).
    Plane,
    /// `SmCylinder(origin + z h0, x, y, radius, angle0, angle1, h1 - h0)`.
    Cylinder {
        /// Origin of the axis.
        origin: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Radius.
        radius: f64,
        /// Angles.
        angle: [f64; 2],
        /// Heights.
        height: [f64; 2],
    },
    /// `SmCone::CreateCanonical` of the apex-based cone `radius(h) = h tan(half_angle)`.
    Cone {
        /// Apex.
        apex: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Half angle.
        half_angle_deg: f64,
        /// Heights from the apex.
        height: [f64; 2],
    },
    /// `SmSphere(center, x, y, SmExtent2d(lon0, lat0, lon1, lat1), radius)`.
    Sphere {
        /// Centre.
        center: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Radius.
        radius: f64,
        /// Longitudes.
        lon: [f64; 2],
        /// Latitudes.
        lat: [f64; 2],
    },
    /// `SmTorus(center, x, y, SmExtent2d(major0, minor0, major1, minor1), R, r)`.
    Torus {
        /// Centre.
        center: [f64; 3],
        /// X axis.
        x: [f64; 3],
        /// Y axis.
        y: [f64; 3],
        /// Major radius.
        major_radius: f64,
        /// Minor radius.
        minor_radius: f64,
        /// Major angles.
        major: [f64; 2],
        /// Minor angles.
        minor: [f64; 2],
    },
}

impl FaceSurface {
    /// The shim's block (`0`, or the kind `4..7` and its values), after rejecting
    /// non-finite values and magnitudes above `1e9`: the checks of the `ssi_corpus`
    /// FFI for the same constructors (finiteness only). A shape the constructors cannot
    /// build (`SmCone::CreateCanonical` failing) is a shim argument error, and a C++
    /// crash fails only its case (fresh process).
    fn encode(&self, o: &mut Vec<f64>) -> Result<(), FfiError> {
        let invalid = || FfiError::InvalidArgument("unsafe topology p2 face surface".into());
        let start = o.len();
        match self {
            FaceSurface::Plane => {
                o.push(0.0);
            }
            FaceSurface::Cylinder {
                origin,
                x,
                y,
                radius,
                angle,
                height,
            } => {
                o.push(4.0);
                o.extend(origin.iter().chain(x).chain(y));
                o.push(*radius);
                o.extend(angle.iter().chain(height));
            }
            FaceSurface::Cone {
                apex,
                x,
                y,
                half_angle_deg,
                height,
            } => {
                o.push(5.0);
                o.extend(apex.iter().chain(x).chain(y));
                o.push(*half_angle_deg);
                o.extend(height);
            }
            FaceSurface::Sphere {
                center,
                x,
                y,
                radius,
                lon,
                lat,
            } => {
                o.push(6.0);
                o.extend(center.iter().chain(x).chain(y));
                o.push(*radius);
                o.extend(lon.iter().chain(lat));
            }
            FaceSurface::Torus {
                center,
                x,
                y,
                major_radius,
                minor_radius,
                major,
                minor,
            } => {
                o.push(7.0);
                o.extend(center.iter().chain(x).chain(y));
                o.extend([*major_radius, *minor_radius]);
                o.extend(major.iter().chain(minor));
            }
        }
        let finite = o
            .get(start..)
            .is_some_and(|v| v.iter().all(|x| x.is_finite() && x.abs() <= 1e9));
        if finite {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

/// [`eval_static`] for the face entries (5..8) with the two faces on `faces` instead
/// of the default planes, each face on its surface's `GetNaturalUVDomain()` (the
/// default plane on `(0, 0, 1, 1)`), run in a fresh process
/// ([`crate::isolate::fresh_process`]) so that a crash in the C++ fails only its case.
///
/// # Errors
/// As [`eval_static`]; an entry other than 5..8 or a face surface its checks reject.
pub fn eval_static_faces(input: &[f64], faces: &[FaceSurface; 2]) -> Result<Vec<f64>, FfiError> {
    validate_static(input)?;
    if !input.first().is_some_and(|e| (5.0..=8.0).contains(e)) {
        return Err(FfiError::InvalidArgument(
            "topology p2 static faces: not a face entry".into(),
        ));
    }
    let mut srf = Vec::new();
    for f in faces {
        f.encode(&mut srf)?;
    }
    crate::isolate::fresh_process(|| {
        call_with_buffer(1024, |out, len| {
            // SAFETY: the static record is validated above and the face blocks are
            // complete, finite and bounded; the reference copies only within the
            // supplied capacity.
            unsafe {
                sys::sc_intersect_topology_solver_p2_static_faces(
                    input.as_ptr(),
                    input.len() as i64,
                    srf.as_ptr(),
                    srf.len() as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut *len,
                )
            }
        })
    })
}

/// The checks of [`eval_static`].
fn validate_static(input: &[f64]) -> Result<(), FfiError> {
    let invalid = || FfiError::InvalidArgument("unsafe topology p2 static fixture".into());
    let Some(&[entry, operation, request, tolerance, best, n1, n2, _, _, _]) = input.get(..10)
    else {
        return Err(invalid());
    };
    if input.iter().any(|v| !v.is_finite())
        || entry.fract() != 0.0
        || !(0.0..=8.0).contains(&entry)
        || operation.fract() != 0.0
        || !(0.0..=18.0).contains(&operation)
        || request.fract() != 0.0
        || !(0.0..=3.0).contains(&request)
        || !(0.0..=1e9).contains(&tolerance)
        || best.abs() > 1e30
        || [n1, n2]
            .iter()
            .any(|n| n.fract() != 0.0 || !(1.0..=64.0).contains(n))
        || input
            .get(7..)
            .is_none_or(|v| v.iter().any(|x| x.abs() > 1e9))
    {
        return Err(invalid());
    }
    let count = n1 as usize + n2 as usize;
    if input.len() != 12 + 3 * count
        || input
            .get(input.len().saturating_sub(2)..)
            .is_none_or(|flags| flags.iter().any(|v| *v != 0.0 && *v != 1.0))
    {
        return Err(invalid());
    }
    Ok(())
}

/// Whether the native vertex selected as a curve by `EdgeVertex` is a curve.
/// This runtime-class probe avoids invoking the upstream undefined cast.
///
/// # Errors
/// Reference fixture construction failure.
pub fn edge_vertex_operand_is_curve() -> Result<bool, FfiError> {
    // SAFETY: the native helper constructs and frees its own fixed-size fixture
    // and performs a runtime type query, never the invalid cast.
    match unsafe { sys::sc_intersect_topology_solver_p2_edge_vertex_operand() } {
        0 => Ok(false),
        1 => Ok(true),
        code => Err(FfiError::Status(code)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpp_edge_vertex_selected_operand_is_not_a_curve() {
        assert!(!edge_vertex_operand_is_curve().unwrap());
    }

    #[test]
    fn static_protocol_rejects_unbounded_or_malformed_records() {
        let valid = [
            0.0, 3.0, 1.0, 1e-5, 1e30, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            0.0,
        ];
        for (index, value) in [
            (0, 9.0),
            (1, 19.0),
            (2, 4.0),
            (5, 1e9),
            (5, 1.5),
            (6, -1.0),
            (7, f64::NAN),
            (16, 0.5),
        ] {
            let mut malformed = valid;
            malformed[index] = value;
            assert!(matches!(
                eval_static(&malformed),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        assert!(matches!(
            eval_static(&valid[..17]),
            Err(FfiError::InvalidArgument(_))
        ));
    }
    #[test]
    fn cpp_face_fixtures_have_valid_setup_and_complete() {
        for entry in 5..=8 {
            for included in [0.0, 1.0] {
                let input = [
                    f64::from(entry),
                    3.0,
                    1.0,
                    1e-5,
                    1e30,
                    1.0,
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    0.0,
                    included,
                ];
                // A normal operation error is distinct from failure to construct
                // the reference fixture, which must fail this test.
                let output = eval_static(&input).unwrap();
                assert!(!output.is_empty());
                if included == 0.0 {
                    assert_eq!(output[0], 0.0);
                }
            }
        }
    }
    #[test]
    fn cpp_empty_object_lists_succeed_in_both_orders() {
        assert_eq!(empty_list_results().unwrap(), [0, 0, 0, 0]);
    }

    #[test]
    fn cpp_distinguishes_external_and_managed_point_nodes() {
        assert_eq!(point_storage_counts().unwrap(), [0, 1]);
    }
}
