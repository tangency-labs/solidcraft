//! The per-entity `AssertValid` level-0 helper cases (#1419) shared by the runners of
//! units `topology.vertex` (`SmVertex::CheckPointers`), `topology.loopuse`
//! (`SmLoopuse::IsLoopuseSideInSurface`) and `topology.edgeuse_p2`
//! (`SmEdgeuse::HasDogLegNearPole`, `SmEdgeuse::IsEdgeuseSideInSurface`).
//!
//! A case is `{"level0": <op>, "brep": <BrepIn>}`: the C++ shim builds the B-rep from the
//! B-rep block of unit `topology.vertexuse` and records the helper on every entity of the
//! kind in its traversal order; the Rust side builds the same B-rep itself
//! ([`super::super::topology_face_p1::own_brep`], arena ids in that traversal order) and
//! records the port the same way. `-2` marks a call the shim skips because upstream
//! dereferences null or reads out of bounds there (see the shims); the Rust side skips
//! the same calls on the same conditions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::topology::{Brep, EdgeuseId, LoopuseId, VertexId};
use solidcore::SolidError;
use solidcore_ffi::units::{topology_edgeuse_p2 as ffi_eu, topology_loopuse as ffi_lu};
use solidcore_ffi::units::topology_vertex as ffi_v;

use super::super::topology_vertexuse::{self as shared, BrepIn};
use super::{brep_block, FACE37_FILES, FILES};
use crate::{Case, HarnessError, Output, Rng};

/// A skipped call (see the module docs).
const SKIPPED: f64 = -2.0;

/// The helper a level-0 case records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum Level0Op {
    /// `SmVertex::CheckPointers` of every vertex.
    CheckPointers,
    /// `SmLoopuse::IsLoopuseSideInSurface` of every loop use (`-1`: NULL periodic flag).
    LoopuseSide { periodic_u: i64, periodic_v: i64 },
    /// `SmEdgeuse::HasDogLegNearPole` and `SmEdgeuse::IsEdgeuseSideInSurface` of every
    /// edge use.
    EdgeuseSide {
        smp_count: u64,
        periodic_u: i64,
        periodic_v: i64,
    },
}

/// A level-0 case's inputs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Level0In {
    level0: Level0Op,
    brep: BrepIn,
}

/// Whether `case` is a level-0 case.
pub(crate) fn is_level0(case: &Case) -> bool {
    case.inputs.get("level0").is_some()
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn periodic_of(p: i64) -> Option<bool> {
    (p >= 0).then_some(p == 1)
}

/// The C++ side of a level-0 case.
pub(crate) fn run_cpp(case: &Case) -> Result<Output, HarnessError> {
    let i: Level0In = case.parse()?;
    let path = shared::path_of(&i.brep)?;
    let block = brep_block(&i.brep)?;
    let r = match i.level0 {
        Level0Op::CheckPointers => ffi_v::check_pointers(&path, &block),
        Level0Op::LoopuseSide {
            periodic_u,
            periodic_v,
        } => ffi_lu::side_in_surface(&path, periodic_u as f64, periodic_v as f64, &block),
        Level0Op::EdgeuseSide {
            smp_count,
            periodic_u,
            periodic_v,
        } => ffi_eu::pole(
            &path,
            usize::try_from(smp_count).map_err(|_| harness("sample count"))?,
            periodic_u as f64,
            periodic_v as f64,
            &block,
        ),
    };
    let v = r.map_err(|e| harness(format!("c++ level-0 helper: {e}")))?;
    Ok(Output::new().values("out", v))
}

/// A Rust-side failure: an unported callee defers the case, anything else is the
/// harness's error.
fn fail(e: &SolidError) -> Result<Output, HarnessError> {
    Output::unported(e).ok_or_else(|| harness(format!("rust level-0 helper: {e}")))
}

/// Whether an edge use's edge has no curve while its face has a surface (upstream
/// dereferences the null curve there).
fn null_curve(b: &Brep, eu: EdgeuseId) -> Result<bool, SolidError> {
    let has_surface = match solidcore::topology::edgeuse::p1::face(b, eu)? {
        Some(f) => b.face(f)?.surface.is_some(),
        None => false,
    };
    let no_curve = match b.edgeuse(eu)?.edge {
        Some(e) => b.edge(e)?.curve.is_none(),
        None => false,
    };
    Ok(has_surface && no_curve)
}

fn rust_record(op: &Level0Op, b: &Brep, c: [usize; 10]) -> Result<Vec<f64>, SolidError> {
    let k = Kernel::default();
    let mut o = vec![0.0];
    let ids = |n: usize| (0..n).filter_map(|i| u32::try_from(i).ok());
    match op {
        Level0Op::CheckPointers => {
            o.push(c[9] as f64);
            for v in ids(c[9]) {
                o.push(flag(solidcore::topology::vertex::check_pointers(
                    b,
                    VertexId::from_index(v),
                )?));
            }
        }
        Level0Op::LoopuseSide {
            periodic_u,
            periodic_v,
        } => {
            o.push(c[4] as f64);
            for l in ids(c[4]) {
                let lu = LoopuseId::from_index(l);
                let mut guard = false;
                for eu in solidcore::topology::loopuse::edgeuses(b, lu, None, None)? {
                    guard |= null_curve(b, eu)?;
                }
                if guard {
                    o.extend([SKIPPED; 3]);
                    continue;
                }
                let r = solidcore::topology::loopuse::is_loopuse_side_in_surface(
                    &k,
                    b,
                    lu,
                    periodic_of(*periodic_u),
                    periodic_of(*periodic_v),
                )?;
                o.extend([
                    flag(r.in_surface),
                    flag(r.on_natural_boundary),
                    r.edgeuse_index as f64,
                ]);
            }
        }
        Level0Op::EdgeuseSide {
            smp_count,
            periodic_u,
            periodic_v,
        } => {
            use solidcore::topology::edgeuse::p2 as eu2;
            o.push(c[6] as f64);
            for e in ids(c[6]) {
                let eu = EdgeuseId::from_index(e);
                match eu2::has_dog_leg_near_pole(&k, b, eu) {
                    Ok(d) => o.extend([
                        flag(d.has_dog_leg),
                        d.index.map_or(-1.0, |x| x as f64),
                    ]),
                    // the intersector-form trim polygon of fewer than four points the
                    // shim skips (upstream reads `lNum - 4`)
                    Err(SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. })
                        if reason.contains("fewer than four control points") =>
                    {
                        o.extend([SKIPPED; 2]);
                    }
                    Err(e) => return Err(e),
                }
                if null_curve(b, eu)? {
                    o.extend([SKIPPED; 2]);
                    continue;
                }
                let r = eu2::is_edgeuse_side_in_surface(
                    &k,
                    b,
                    eu,
                    *smp_count,
                    periodic_of(*periodic_u),
                    periodic_of(*periodic_v),
                )?;
                o.extend([flag(r.in_surface), flag(r.on_natural_boundary)]);
            }
        }
    }
    Ok(o)
}

/// The Rust side of a level-0 case.
pub(crate) fn run_rust(case: &Case) -> Result<Output, HarnessError> {
    let i: Level0In = case.parse()?;
    if let Level0Op::EdgeuseSide {
        smp_count,
        periodic_u,
        periodic_v,
    } = i.level0
    {
        // an input the FFI refuses is a harness error on both sides (PR #502's
        // convention): the shim never runs it, so the Rust side must not either. Nightly
        // fuzz case topology.edgeuse_p2/fuzz00237 mutated `smp_count` to 2^31: the FFI
        // refuses more than `MAX_POLE_SAMPLES` for the C++ side, while the port samples
        // each edge use's trim curve `smp_count` times (as upstream) and ran past the fuzz
        // time limit. The checks see the same arguments and B-rep block as `run_cpp`.
        let path = shared::path_of(&i.brep)?;
        let block = brep_block(&i.brep)?;
        ffi_eu::validate_pole(
            &path,
            usize::try_from(smp_count).map_err(|_| harness("sample count"))?,
            periodic_u as f64,
            periodic_v as f64,
            &block,
        )
        .map_err(|e| harness(format!("level-0 helper: {e}")))?;
    }
    let (b, c) = match super::super::topology_face_p1::own_brep(&i.brep, false, false)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    match rust_record(&i.level0, &b, c) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(e) => fail(&e),
    }
}

/// The level-0 cases of `unit` for `ops`: every upstream file (with and without rebuilt
/// UV trim curves), the files with holed faces, and `n_gen` generated B-reps (fixed seed).
pub(crate) fn cases(
    unit: &str,
    ops: &[Level0Op],
    seed: u64,
    n_gen: usize,
) -> Result<Vec<Case>, HarnessError> {
    let mut v = Vec::new();
    let push = |v: &mut Vec<Case>, name: String, brep: BrepIn, op: &Level0Op| {
        let inputs = Level0In {
            level0: op.clone(),
            brep,
        };
        let inputs: Value = serde_json::to_value(inputs).map_err(|e| harness(e.to_string()))?;
        let index = v.len();
        v.push(Case {
            unit: unit.to_string(),
            name,
            source: crate::CaseSource::Fixture {
                path: "built-in: AssertValid level-0 helper cases".to_string(),
                index,
                upstream_ref: Some("tests/prog_test (the pt_TestFiles B-reps)".to_string()),
            },
            inputs,
        });
        Ok::<(), HarnessError>(())
    };
    for file in FILES.iter().chain(FACE37_FILES.iter()) {
        for rebuild in [false, true] {
            for (j, op) in ops.iter().enumerate() {
                let brep = BrepIn::File {
                    file: (*file).to_string(),
                    rebuild,
                };
                push(
                    &mut v,
                    format!("level0_{file}_{}_op{j}", u8::from(rebuild)),
                    brep,
                    op,
                )?;
            }
        }
    }
    let mut rng = Rng::new(seed);
    for g in 0..n_gen {
        let brep = shared::gen_brep(&mut rng);
        let op = ops
            .get(g % ops.len().max(1))
            .ok_or_else(|| harness("no level-0 ops"))?;
        push(&mut v, format!("level0_gen_{g:03}"), brep, op)?;
    }
    for (j, op) in ops.iter().enumerate() {
        push(&mut v, format!("level0_network_op{j}"), BrepIn::Network, op)?;
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edgeuse_side_case(smp_count: u64) -> Case {
        Case {
            unit: "topology.edgeuse_p2".into(),
            name: "t".into(),
            source: crate::CaseSource::Inline,
            inputs: serde_json::json!({
                "brep": {"kind": "file", "file": "surf_sphere150.smb", "rebuild": true},
                "level0": {
                    "op": "edgeuse_side", "periodic_u": -1, "periodic_v": -1,
                    "smp_count": smp_count
                }
            }),
        }
    }

    /// Nightly fuzz case `topology.edgeuse_p2/fuzz00237` (run 41040730716): an
    /// `smp_count` of 2^31. The FFI refuses more than `MAX_POLE_SAMPLES` samples for the
    /// C++ side, and the Rust side must be refused the same way instead of sampling every
    /// edge use's trim curve 2^31 times.
    #[test]
    fn huge_sample_count_is_refused_on_both_sides() {
        let case = edgeuse_side_case(2_147_483_648);
        let cpp = run_cpp(&case).unwrap_err().to_string();
        let rust = run_rust(&case).unwrap_err().to_string();
        assert_eq!(cpp, "c++ level-0 helper: invalid argument: too many samples");
        assert_eq!(rust, cpp.trim_start_matches("c++ "));
    }

    /// The largest `smp_count` the FFI accepts still runs on both sides.
    #[test]
    fn max_sample_count_runs_on_both_sides() {
        let case = edgeuse_side_case(ffi_eu::MAX_POLE_SAMPLES as u64);
        let cpp = run_cpp(&case).unwrap();
        let rust = run_rust(&case).unwrap();
        assert!(crate::compare(&cpp, &rust, &[]).mismatches.is_empty());
    }
}
