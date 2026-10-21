//! Runner for unit `boolean.api_brep`: the `SM_API` boolean entry points.
//!
//! C++: `solidcore_ffi::units::boolean_api_brep` (upstream
//! `source/SM_API/src/SmApiBrep.cpp`). Rust: `solidcore::boolean::api_brep`.
//!
//! A case is one API call (`api` plus its arguments). Operands are boxes
//! (`SmApiCreateBox`) or spheres (`SmApiCreateSphere`); those constructors stop at
//! `SmPrimitiveCreation` (unit `primitive.primitive_creation_p1`, not ported yet), so
//! the Rust side passes empty B-reps. A call that reaches the merge engine reads them:
//! it takes a mark (`SmNewMarkAndLock`), which input validation never does, so a Rust
//! call that advanced the kernel's mark counter is deferred whatever it returned. Both sides produce `status`
//! (`0` = `SM_SUCCESS`, else the `SmStatus`: `1007` `SM_ERR_INVALID_INPUT`; `-1` on the
//! Rust side for an error upstream cannot produce) and `record` (the op's outputs after
//! the status, see the FFI module).
//!
//! Compared: the input validation (`SM_BO_EXTRACT_SEPARATE` for the two-B-rep entry
//! points, short lists and operations other than union / intersection / difference /
//! merge for `SmApiMergeBreps`, which must leave the list untouched). Cases that pass
//! validation reach `SmMerge` and are deferred on the Rust side (`boolean.merge_p2`,
//! `primitive.primitive_creation_p1`, `topology.brep_p3`). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::boolean::api_brep as api;
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::context::MarkType;
use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::boolean_api_brep::{self as ffi, BrepSpec, Call, Flags};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 240;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;

/// `boolean.api_brep` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type P3 = [f64; 3];

/// An operand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrepIn {
    /// `SmApiCreateBox`.
    Box { origin: P3, size: P3 },
    /// `SmApiCreateSphere`.
    Sphere { center: P3, radius: f64 },
}

/// `SmApiBooleanWithOptions` flags; missing ones take the `SmMergeOptions` defaults.
#[allow(clippy::struct_excessive_bools)] // upstream's five SmBoolean arguments
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct FlagsIn {
    #[serde(default)]
    cookie_cutter: bool,
    #[serde(default)]
    imprinting: bool,
    #[serde(default = "yes")]
    boolean_post_process: bool,
    #[serde(default)]
    imprint_and_classify_faces: bool,
    #[serde(default)]
    keep_other_brep: bool,
}

fn yes() -> bool {
    true
}

/// One API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "api", rename_all = "snake_case")]
enum Inputs {
    Boolean {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
    },
    Union {
        a: BrepIn,
        b: BrepIn,
    },
    Difference {
        a: BrepIn,
        b: BrepIn,
    },
    Intersection {
        a: BrepIn,
        b: BrepIn,
    },
    Merge {
        a: BrepIn,
        b: BrepIn,
    },
    WithOptions {
        a: BrepIn,
        b: BrepIn,
        operation: i32,
        flags: FlagsIn,
    },
    MergeBreps {
        breps: Vec<BrepIn>,
        operation: i32,
    },
    Lists {
        breps1: Vec<BrepIn>,
        breps2: Vec<BrepIn>,
        operation: i32,
    },
    Trees {
        breps: Vec<BrepIn>,
        tree: Vec<i64>,
    },
}

fn spec(b: &BrepIn) -> BrepSpec {
    match *b {
        BrepIn::Box { origin, size } => BrepSpec::Box { origin, size },
        BrepIn::Sphere { center, radius } => BrepSpec::Sphere { center, radius },
    }
}

fn specs(l: &[BrepIn]) -> Vec<BrepSpec> {
    l.iter().map(spec).collect()
}

fn to_call(i: &Inputs) -> Call {
    match i {
        Inputs::Boolean { a, b, operation } => Call::Boolean {
            a: spec(a),
            b: spec(b),
            operation: *operation,
        },
        Inputs::Union { a, b } => Call::Union {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Difference { a, b } => Call::Difference {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Intersection { a, b } => Call::Intersection {
            a: spec(a),
            b: spec(b),
        },
        Inputs::Merge { a, b } => Call::Merge {
            a: spec(a),
            b: spec(b),
        },
        Inputs::WithOptions {
            a,
            b,
            operation,
            flags,
        } => Call::WithOptions {
            a: spec(a),
            b: spec(b),
            operation: *operation,
            flags: Flags {
                cookie_cutter: flags.cookie_cutter,
                imprinting: flags.imprinting,
                boolean_post_process: flags.boolean_post_process,
                imprint_and_classify_faces: flags.imprint_and_classify_faces,
                keep_other_brep: flags.keep_other_brep,
            },
        },
        Inputs::MergeBreps { breps, operation } => Call::MergeBreps {
            breps: specs(breps),
            operation: *operation,
        },
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => Call::Lists {
            breps1: specs(breps1),
            breps2: specs(breps2),
            operation: *operation,
        },
        Inputs::Trees { breps, tree } => Call::Trees {
            breps: specs(breps),
            tree: tree.clone(),
        },
    }
}

// ---- outputs ------------------------------------------------------------------------

fn to_output(o: &[f64]) -> Result<Output, HarnessError> {
    let (&status, rest) = o.split_first().ok_or_else(|| harness("empty record"))?;
    let ints = rest
        .iter()
        .map(|&x| {
            if x.fract() == 0.0 && x.abs() < 1e15 {
                Ok(x as i64)
            } else {
                Err(harness(format!("non-integer record entry {x}")))
            }
        })
        .collect::<Result<Vec<i64>, _>>()?;
    Ok(Output::new()
        .int("status", status as i64)
        .ints("record", ints))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match ffi::run(&to_call(i)) {
        Ok(o) => to_output(&o),
        Err(e) => Err(harness(format!("cpp: {e}"))),
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

fn op(code: i32) -> Result<BooleanOperation, HarnessError> {
    BooleanOperation::from_code(code).ok_or_else(|| harness(format!("operation {code}")))
}

fn options(f: FlagsIn) -> MergeOptions {
    MergeOptions::new(
        f.cookie_cutter,
        f.imprinting,
        f.boolean_post_process,
        f.imprint_and_classify_faces,
        f.keep_other_brep,
    )
}

/// Empty stand-ins for the operands (their constructors are not ported).
fn breps(n: usize) -> Vec<Brep> {
    (0..n).map(|_| Brep::new()).collect()
}

/// A Rust error as a record (status only); deferred for `Unported`.
fn err_output(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| {
        Output::new()
            .int("status", status(e))
            .ints("record", Vec::<i64>::new())
    })
}

/// The deferral of a call that reached the merge engine, which reads the operands.
fn stand_in_operands() -> Output {
    // UNPORTED(primitive.primitive_creation_p1): SmPrimitiveCreation::CreateBox / CreateSphere
    Output::deferred(
        "primitive.primitive_creation_p1",
        "SmPrimitiveCreation::CreateBox / CreateSphere (stand-in operands)",
    )
}

/// The record of a successful call cannot be produced yet (every success path goes
/// through `SmMerge`).
fn unexpected_success() -> Result<Output, HarnessError> {
    Err(harness("rust: unexpected success before SmMerge is ported"))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let next_mark = k.context.current_mark(MarkType::NoMark);
    let out = rust_call(&mut k, i);
    // (the stand-ins only stand in for operands nothing reads, whatever the call did)
    if k.context.current_mark(MarkType::NoMark) == next_mark {
        out
    } else {
        Ok(stand_in_operands())
    }
}

fn rust_call(k: &mut Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let two = |r: solidcore::Result<()>| match r {
        Ok(()) => unexpected_success(),
        Err(e) => Ok(err_output(&e)),
    };
    match i {
        Inputs::Boolean { operation, .. } => {
            two(api::boolean(k, Brep::new(), Brep::new(), op(*operation)?).map(|_| ()))
        }
        Inputs::Union { .. } => {
            two(api::boolean_union(k, Brep::new(), Brep::new()).map(|_| ()))
        }
        Inputs::Difference { .. } => {
            two(api::boolean_difference(k, Brep::new(), Brep::new()).map(|_| ()))
        }
        Inputs::Intersection { .. } => {
            two(api::boolean_intersection(k, Brep::new(), Brep::new()).map(|_| ()))
        }
        Inputs::Merge { .. } => {
            two(api::boolean_merge(k, Brep::new(), Brep::new()).map(|_| ()))
        }
        Inputs::WithOptions {
            operation, flags, ..
        } => two(api::boolean_with_options(
            k,
            Brep::new(),
            Brep::new(),
            op(*operation)?,
            options(*flags),
        )
        .map(|_| ())),
        Inputs::MergeBreps {
            breps: list,
            operation,
        } => {
            let mut v = breps(list.len());
            match api::merge_breps(k, &mut v, op(*operation)?) {
                Ok(_) => unexpected_success(),
                Err(e) if e.is_unported() => Ok(err_output(&e)),
                Err(e) => {
                    // Rejected: no result (`cleared`) and the list untouched.
                    let len = i64::try_from(v.len()).map_err(|_| harness("list too long"))?;
                    Ok(Output::new()
                        .int("status", status(&e))
                        .ints("record", vec![1, len]))
                }
            }
        }
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => two(api::boolean_lists(
            k,
            &breps(breps1.len()),
            &[],
            &breps(breps2.len()),
            &[],
            *operation,
        )
        .map(|_| ())),
        Inputs::Trees { breps: list, tree } => {
            let mut v = breps(list.len()).into_iter().map(Some).collect();
            match api::boolean_trees(k, &mut v, tree) {
                Ok(()) if v.is_empty() => {
                    Ok(Output::new().int("status", 0).ints("record", vec![0]))
                }
                // The output record reads each resulting operand (its counts and
                // IsManifoldSolid, topology.brep_p2), and the Rust operands are empty
                // stand-ins: the box and sphere constructors stop at
                // SmBrep::SmBrep, so stand-ins are never inspected for a fake value.
                // UNPORTED(topology.brep_p3): SmBrep::SmBrep
                Ok(()) => Ok(Output::deferred(
                    "topology.brep_p3",
                    "SmBrep::SmBrep (stand-in operands)",
                )),
                Err(error) => Ok(err_output(&error)),
            }
        }
    }
}

// ---- generators ---------------------------------------------------------------------

/// A length scale: mostly moderate, sometimes tiny or huge. Tiny stops at `1e-3`:
/// upstream `SmApiCreateSphere` takes tens of seconds at radius `~5e-5`.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 10f64.powi(-(rng.range_i64(2, 3) as i32)),
        1 => 10f64.powi(rng.range_i64(2, 4) as i32),
        _ => rng.uniform(0.5, 20.0),
    }
}

/// An operand at scale `s` near `center`: a box or a sphere.
fn gen_brep(rng: &mut Rng, s: f64, center: P3) -> BrepIn {
    let jitter = rng.point3(-0.6 * s, 0.6 * s);
    let c = [
        center[0] + jitter[0],
        center[1] + jitter[1],
        center[2] + jitter[2],
    ];
    if rng.chance(0.65) {
        let size = [
            s * rng.uniform(0.3, 1.5),
            s * rng.uniform(0.3, 1.5),
            s * rng.uniform(0.3, 1.5),
        ];
        // Sometimes snap the corner onto the grid of the scale, so faces coincide.
        let origin = if rng.chance(0.2) {
            [center[0], center[1], center[2]]
        } else {
            c
        };
        BrepIn::Box { origin, size }
    } else {
        let r = s * rng.uniform(0.3, 1.0);
        BrepIn::Sphere {
            center: c,
            radius: if rng.chance(0.1) { -r } else { r },
        }
    }
}

fn gen_list(rng: &mut Rng, n: usize, s: f64) -> Vec<BrepIn> {
    let c = rng.point3(-2.0 * s, 2.0 * s);
    (0..n).map(|_| gen_brep(rng, s, c)).collect()
}

/// Operations other than union / intersection / difference / merge.
const NON_KERNEL_OPS: [i32; 7] = [0, 4, 6, 7, 8, 9, 10];
/// Operations the two-B-rep merge is exercised with (all but `SM_BO_EXTRACT_SEPARATE`).
const TWO_OPS: [i32; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10];

fn gen_flags(rng: &mut Rng) -> FlagsIn {
    FlagsIn {
        cookie_cutter: rng.chance(0.3),
        imprinting: rng.chance(0.2),
        boolean_post_process: rng.chance(0.7),
        imprint_and_classify_faces: rng.chance(0.2),
        keep_other_brep: rng.chance(0.3),
    }
}

fn pick(rng: &mut Rng, ops: &[i32]) -> i32 {
    rng.choose(ops).copied().unwrap_or(1)
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let two = |rng: &mut Rng| {
        let c = rng.point3(-2.0 * s, 2.0 * s);
        (gen_brep(rng, s, c), gen_brep(rng, s, c))
    };
    match rng.below(20) {
        // ---- compared: input validation --------------------------------------------
        0..=2 => {
            let (a, b) = two(rng);
            Inputs::Boolean { a, b, operation: 9 }
        }
        3 | 4 => {
            let (a, b) = two(rng);
            Inputs::WithOptions {
                a,
                b,
                operation: 9,
                flags: gen_flags(rng),
            }
        }
        5 | 6 => {
            let n = rng.below(2) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            }
        }
        7..=9 => {
            let n = 2 + rng.below(4) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &NON_KERNEL_OPS),
            }
        }
        // ---- deferred: reach SmMerge -----------------------------------------------
        10 | 11 => {
            let (a, b) = two(rng);
            Inputs::Boolean {
                a,
                b,
                operation: pick(rng, &TWO_OPS),
            }
        }
        12 => {
            let (a, b) = two(rng);
            match rng.below(4) {
                0 => Inputs::Union { a, b },
                1 => Inputs::Difference { a, b },
                2 => Inputs::Intersection { a, b },
                _ => Inputs::Merge { a, b },
            }
        }
        13 | 14 => {
            let (a, b) = two(rng);
            Inputs::WithOptions {
                a,
                b,
                operation: pick(rng, &[1, 2, 3, 5]),
                flags: gen_flags(rng),
            }
        }
        15 | 16 => {
            let n = 2 + rng.below(2) as usize;
            Inputs::MergeBreps {
                breps: gen_list(rng, n, s),
                operation: pick(rng, &[1, 2, 3, 5]),
            }
        }
        17 | 18 => {
            let n1 = rng.below(3) as usize;
            let n2 = rng.below(3) as usize;
            Inputs::Lists {
                breps1: gen_list(rng, n1, s),
                breps2: gen_list(rng, n2, s),
                operation: rng.range_i64(-1, 5) as i32,
            }
        }
        _ => {
            let n = 2 + rng.below(2) as usize;
            let mut tree = vec![0, 1, -rng.range_i64(1, 4)];
            if n == 3 {
                tree.extend([2, -rng.range_i64(1, 4)]);
            }
            Inputs::Trees {
                breps: gen_list(rng, n, s),
                tree,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "boolean.api_brep"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
