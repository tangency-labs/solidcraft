//! Safe native fixtures and operations for `topology.brep_p5`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p5.cpp`. Upstream: `SmBrep.cpp`.
//! An export is the shared `topology.vertexuse` raw graph. Operation results are
//! `record_len record* raw_graph`; `CombineTarget` returns `status xyz tolerance`
//! only and certifies the target stage, not the topology mutation.

use crate::{call_with_buffer, FfiError};

/// Native operation codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Export the unmodified fixture graph.
    Export = 0,
    /// `CombineCoincidentVertices`, including output and topology.
    Combine = 1,
    /// Target-stage comparison against the actual native vertex merge result.
    CombineTarget = 2,
    /// `DeleteTopologicalVertex`.
    DeleteTopologicalVertex = 3,
    /// `FindAndSetInfiniteRegion`.
    FindAndSetInfiniteRegion = 4,
    /// `GlueVertices`.
    GlueVertices = 5,
    /// `MakeEdgeInFace` (defined early refusal branches).
    MakeEdgeInFace = 6,
    /// `MakeManifold`.
    MakeManifold = 7,
    /// `RemoveTopologicalEdgesAndVertices`.
    RemoveTopologicalEdgesAndVertices = 8,
    /// `RemoveFaces`.
    RemoveFaces = 9,
    /// `StitchAndOrient`.
    StitchAndOrient = 10,
    /// `SqueezeFace`.
    SqueezeFace = 11,
    /// `Transform`.
    Transform = 12,
    /// `CreateWireEdgeFromCurve` on a real non-normalized `SmLine`.
    CreateWireEdgeFromCurve = 13,
    /// Real topology-change callback scripts (supporting healer integration).
    TrackNotifications = 15,
}

impl Op {
    /// Parses a protocol operation.
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Export,
            1 => Self::Combine,
            2 => Self::CombineTarget,
            3 => Self::DeleteTopologicalVertex,
            4 => Self::FindAndSetInfiniteRegion,
            5 => Self::GlueVertices,
            6 => Self::MakeEdgeInFace,
            7 => Self::MakeManifold,
            8 => Self::RemoveTopologicalEdgesAndVertices,
            9 => Self::RemoveFaces,
            10 => Self::StitchAndOrient,
            11 => Self::SqueezeFace,
            12 => Self::Transform,
            13 => Self::CreateWireEdgeFromCurve,
            15 => Self::TrackNotifications,
            _ => return None,
        })
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p5_eval(
            op: i32,
            fixture: *const f64,
            nf: i64,
            args: *const f64,
            na: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn invalid(reason: &str) -> FfiError {
    FfiError::InvalidArgument(reason.into())
}

fn flag(x: f64) -> bool {
    x == 0.0 || x == 1.0
}

fn integer(x: f64) -> bool {
    x.fract() == 0.0 && x >= -1.0 && x < i64::MAX as f64
}

fn raw(op: Op, fixture: &[f64; 11], args: &[f64; 22]) -> Result<Vec<f64>, FfiError> {
    raw_code(op as i32, fixture, args)
}

fn raw_code(op: i32, fixture: &[f64; 11], args: &[f64; 22]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1024, |out, len| {
        // SAFETY: fixed-size inputs are validated by eval; C++ constructs its own
        // complete graph. Output storage and length remain live during this call.
        unsafe {
            sys::sc_topology_brep_p5_eval(
                op,
                fixture.as_ptr(),
                11,
                args.as_ptr(),
                22,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Evaluates a native operation on a complete native fixture.
///
/// `fixture`: kind (0 paired wire stars, 1 upstream wire network, 2 plane face),
/// two xyz positions, two raw zone tolerances, two incident-edge counts (0..4).
/// `args`: editing, first/second/aux selectors (-1 is null), mode (0..2), raw
/// stitching tolerance, placement origin/X/Y, have-scale, scale xyz, scale-tols,
/// flag, allow-spine-edges. Selectors use the shared raw graph's traversal order.
///
/// Guards refuse null merge inputs and isolated aliased target stages (native
/// use-after-free), the unsafe
/// non-refusal null-geometry insertion branch, and a null face on enabled squeeze.
/// Negative raw tolerances remain valid and convert through native zone-tol types.
pub fn eval(op: Op, fixture: &[f64; 11], args: &[f64; 22]) -> Result<Vec<f64>, FfiError> {
    if !fixture.iter().chain(args).all(|x| x.is_finite())
        || !matches!(fixture[0], 0.0 | 1.0 | 2.0)
        || fixture[9..]
            .iter()
            .any(|x| x.fract() != 0.0 || !(0.0..=4.0).contains(x))
        || [args[0], args[15], args[19], args[20], args[21]]
            .into_iter()
            .any(|x| !flag(x))
        || args[1..4].iter().any(|x| !integer(*x))
        || args[4].fract() != 0.0
        || !(0.0..=2.0).contains(&args[4])
    {
        return Err(invalid("fixture or operation layout"));
    }
    // SetCanonical stores all three vectors before it can fail. Zero/parallel
    // axes therefore leave defined native state; retain these inputs as well.
    if op == Op::Export {
        return raw(op, fixture, args);
    }
    let graph = raw(Op::Export, fixture, args)?;
    if graph.first() == Some(&-999.0) {
        return Ok(graph);
    }
    let counts =
        super::topology_vertexuse::counts(&graph).ok_or_else(|| invalid("fixture graph"))?;
    let select = |x: f64, n: usize| (x >= 0.0 && n != 0).then(|| x as usize % n);
    let v1 = select(args[1], counts[9]);
    let v2 = select(args[2], counts[9]);
    let face = select(args[1], counts[3]);
    if matches!(
        op,
        Op::Combine | Op::CombineTarget | Op::GlueVertices | Op::TrackNotifications
    ) && (v1.is_none() || v2.is_none())
        && !(op == Op::GlueVertices && v1 == v2)
    {
        return Err(invalid("null merge vertices"));
    }
    if op == Op::CombineTarget && v1 == v2 {
        // Native combine deletes an isolated aliased survivor, which the target
        // stage then reads. Inspect the actual selected native vertex: fixture
        // selectors can name star leaves as well as centers.
        let isolated = raw_code(14, fixture, args)?;
        if isolated.first() != Some(&0.0) {
            return Err(invalid("target stage reads a deleted isolated alias"));
        }
    }
    if op == Op::CombineTarget {
        // This stage's oracle is limited to shell-vertex branches: its final
        // point/tol is defined before DeleteVertex and does not run interval repair.
        if fixture[0] != 0.0 || (fixture[9] != 0.0 && fixture[10] != 0.0) {
            return Err(invalid(
                "target-stage fixture must have an isolated component",
            ));
        }
    }
    if op == Op::MakeEdgeInFace && args[0] != 0.0 && face.is_some() {
        return Err(invalid("insertion fixture has no 3D curve"));
    }
    if op == Op::SqueezeFace && args[0] != 0.0 && face.is_none() {
        return Err(invalid("null enabled squeeze face"));
    }
    raw(op, fixture, args)
}

#[cfg(all(test, feature = "cpp"))]
mod tests {
    use super::*;

    #[test]
    fn guards_refuse_isolated_target_aliases_and_missing_geometry() {
        let f = [0., 0., 0., 0., 1., 0., 0., 1e-5, 1e-5, 0., 0.];
        let mut a = [0.; 22];
        assert!(eval(Op::CombineTarget, &f, &a).is_err());
        // The whole combine is defined: native deletes the alias and succeeds.
        assert_eq!(eval(Op::Combine, &f, &a).unwrap().get(1), Some(&0.));
        a[0] = 1.;
        assert!(eval(Op::SqueezeFace, &f, &a).is_err());
        let mut face = f;
        face[0] = 2.;
        assert!(eval(Op::MakeEdgeInFace, &face, &a).is_err());
    }

    #[test]
    fn defined_targets_and_degenerate_placements_are_retained() {
        let f = [0., 0., 0., 0., 1., 0., 0., -1e-5, -2e-5, 0., 0.];
        let mut a = [0.; 22];
        a[0] = 1.;
        a[2] = 1.;
        assert_eq!(
            eval(Op::CombineTarget, &f, &a).unwrap(),
            vec![0., 0., 0., 0., 2.]
        );
        // SetCanonical stores its inputs and has a defined zero-axis fallback;
        // constructor failure is not uninitialized state or a reason to refuse it.
        assert!(eval(Op::Transform, &f, &a).is_ok());
    }
    #[test]
    fn valid_glue_aliases_are_retained_before_notification() {
        let f = [0., 0., 0., 0., 1., 0., 0., 1e-5, 1e-5, 1., 1.];
        let mut a = [0.; 22];
        // Equal actual vertex pointers are valid even with editing disabled.
        let output = eval(Op::GlueVertices, &f, &a).unwrap();
        assert_eq!(output.get(1), Some(&0.));
        // Equal null pointers take the same native early return.
        a[1] = -1.;
        a[2] = -1.;
        let output = eval(Op::GlueVertices, &f, &a).unwrap();
        assert_eq!(output.get(1), Some(&0.));
    }
}
