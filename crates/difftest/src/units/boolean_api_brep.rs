//! Runner for unit `boolean.api_brep`: the `SM_API` boolean entry points.
//!
//! C++: `solidcore_ffi::units::boolean_api_brep` (upstream
//! `source/SM_API/src/SmApiBrep.cpp`). Rust: `solidcore::boolean::api_brep`.
//!
//! A case is one API call (`api` plus its arguments). Operands are boxes
//! (`SmApiCreateBox`) or spheres (`SmApiCreateSphere`); fixture cases also use sheets
//! (`SmApiCreatePlane`) and the result of an `SmApiBoolean` of two operands. Both sides
//! build them with the same constructors (`primitive::api_primitives` and
//! `boolean::api_brep::boolean` on the Rust side) in the case's one kernel / context, at the default `Kernel::fixes` (SMLib behaviour). A
//! constructor that fails is a harness error on both sides (the shim's `SC_ERR_ARG`);
//! one that stops at an unported callee defers the case. Both sides produce `status` (`0` = `SM_SUCCESS`, else the `SmStatus`:
//! `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot
//! produce) and `record` (the op's outputs after the status, see the FFI module): on
//! success the result B-reps' face, edge, vertex and region counts and
//! `IsManifoldSolid`.
//!
//! Compared: the input validation (`SM_BO_EXTRACT_SEPARATE` for the two-B-rep entry
//! points, short lists and operations other than union / intersection / difference /
//! merge for `SmApiMergeBreps`, which must leave the list untouched) and the merges
//! themselves. A Rust call that stops at an unported callee is deferred under that
//! callee. Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::boolean::api_brep as api;
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::primitive::api_primitives as primitives;
use solidcore::topology::brep::{p1 as brep_p1, p2 as brep_p2};
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrepIn {
    /// `SmApiCreateBox`.
    Box { origin: P3, size: P3 },
    /// `SmApiCreateSphere`.
    Sphere { center: P3, radius: f64 },
    /// `SmApiCreatePlane` (`SmBrep*&` overload).
    Plane { origin: P3, size: [f64; 2] },
    /// `SmApiBoolean(a, b, operation)`'s result.
    Boolean {
        operation: i32,
        a: Box<BrepIn>,
        b: Box<BrepIn>,
    },
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
    match b {
        &BrepIn::Box { origin, size } => BrepSpec::Box { origin, size },
        &BrepIn::Sphere { center, radius } => BrepSpec::Sphere { center, radius },
        &BrepIn::Plane { origin, size } => BrepSpec::Plane { origin, size },
        BrepIn::Boolean { operation, a, b } => BrepSpec::Boolean {
            operation: *operation,
            a: Box::new(spec(a)),
            b: Box::new(spec(b)),
        },
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
    if ub_175(i)? {
        return Ok(Output::rejected(
            "UB-175: ~SmProtoTopologyManager reads a freed proto edge (proto vertices \
             collapsed below the 0.01 capture distance); not run",
        ));
    }
    match ffi::run(&to_call(i)) {
        Ok(o) => to_output(&o),
        Err(e) => Err(harness(format!("cpp: {e}"))),
    }
}

/// Whether the Boolean reaches UB-175 (`plan/upstream-issues.md`): the teardown of
/// `IntersectInsertRelate`'s proto topology manager reads a proto edge it already
/// freed, so the input is undefined in C++ (AGENTS.md section 4, step 5.6) and [`cpp`]
/// rejects it. Decided from the inputs and the Rust side only: the port replays the
/// teardown, goes on with its defined result and records `"UB-175"` on the kernel
/// ([`Kernel::upstream_ub_passed`]).
fn ub_175(i: &Inputs) -> Result<bool, HarnessError> {
    let mut k = Kernel::default();
    match rust_call_with(&mut k, i, &|_| {}) {
        Ok(_) | Err(Stop::Deferred(_)) => Ok(k.upstream_ub_passed.contains(&"UB-175")),
        Err(Stop::Harness(e)) => Err(e),
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

/// Why a Rust run stopped early: `Unported` defers the case; anything else is a
/// harness error, as the shim treats a failing operand constructor (`SC_ERR_ARG`).
enum Stop {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Stop {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn stop(what: &str, e: &SolidError) -> Stop {
    match Output::unported(e) {
        Some(d) => Stop::Deferred(Box::new(d)),
        None => Stop::Harness(harness(format!("rust: {what}: {e}"))),
    }
}

fn p3(p: P3) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

/// Builds an operand exactly as the shim does (`SmApiCreateBox`, `SmApiCreateSphere`,
/// `SmApiCreatePlane`, `SmApiBoolean`). Nested
/// operands are at most `ffi::MAX_DEPTH` deep (the FFI rejects deeper ones first).
fn build(k: &mut Kernel, b: &BrepIn) -> Result<Brep, Stop> {
    build_at(k, b, 0)
}

fn build_at(k: &mut Kernel, b: &BrepIn, depth: usize) -> Result<Brep, Stop> {
    if depth >= ffi::MAX_DEPTH {
        return Err(Stop::Harness(harness("operand nested too deep")));
    }
    let r = match b {
        &BrepIn::Box { origin, size } => {
            primitives::create_box(k, p3(origin), size[0], size[1], size[2])
        }
        &BrepIn::Sphere { center, radius } => primitives::create_sphere(k, p3(center), radius),
        &BrepIn::Plane { origin, size } => {
            primitives::create_plane(k, p3(origin), size[0], size[1])
        }
        BrepIn::Boolean { operation, a, b } => {
            let (a, b) = (build_at(k, a, depth + 1)?, build_at(k, b, depth + 1)?);
            api::boolean(k, a, b, op(*operation)?).map(|out| out.result)
        }
    };
    r.map_err(|e| stop("operand constructor", &e))
}

fn build_all(k: &mut Kernel, l: &[BrepIn]) -> Result<Vec<Brep>, Stop> {
    l.iter().map(|b| build(k, b)).collect()
}

/// A Rust error as a record (status only); deferred for `Unported`.
fn err_output(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| record(status(e), Vec::new()))
}

fn record(status: i64, record: Vec<i64>) -> Output {
    Output::new().int("status", status).ints("record", record)
}

fn count(n: usize) -> Result<i64, HarnessError> {
    i64::try_from(n).map_err(|_| harness("count overflow"))
}

/// `faces edges vertices regions manifold_solid` of a B-rep (the shim's `put_brep`).
fn put_brep(o: &mut Vec<i64>, b: &Brep) -> Result<(), Stop> {
    let regions = brep_p1::regions(b, None).map_err(|e| stop("GetRegions", &e))?;
    let manifold = brep_p2::is_manifold_solid(b).map_err(|e| stop("IsManifoldSolid", &e))?;
    o.push(count(b.face_list().len())?);
    o.push(count(b.edge_list().len())?);
    o.push(count(b.vertex_list().len())?);
    o.push(count(regions.len())?);
    o.push(i64::from(manifold));
    Ok(())
}

/// Whether upstream keeps the second operand of a successful two-B-rep boolean
/// (`source/SM_API/inc/SmApiBrep.h`; the shim's `keeps_other`).
fn keeps_other(operation: i32, keep_option: bool) -> bool {
    matches!(
        BooleanOperation::from_code(operation),
        Some(BooleanOperation::PartialMerge | BooleanOperation::ImprintClassify)
    ) || keep_option
}

/// The two-B-rep record `has is_first kept brep [brep]`. The Rust result is the first
/// operand modified in place ([`api::BooleanOutcome::result`]), so `is_first` is 1.
fn two_record(out: &api::BooleanOutcome, kept: bool) -> Result<Output, Stop> {
    let mut o = vec![1, 1, i64::from(kept)];
    put_brep(&mut o, &out.result)?;
    if kept {
        let other = out
            .other
            .as_ref()
            .ok_or_else(|| harness("rust: kept operand missing"))?;
        put_brep(&mut o, other)?;
    }
    Ok(record(0, o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    match rust_call(&mut k, i) {
        Ok(o) => Ok(o),
        Err(Stop::Deferred(o)) => Ok(*o),
        Err(Stop::Harness(e)) => Err(e),
    }
}

fn rust_call(k: &mut Kernel, i: &Inputs) -> Result<Output, Stop> {
    rust_call_with(k, i, &|_| {})
}

/// [`rust_call`], showing every error the API returned to `seen`.
fn rust_call_with(k: &mut Kernel, i: &Inputs, seen: &dyn Fn(&SolidError)) -> Result<Output, Stop> {
    let err_output = |e: &SolidError| {
        seen(e);
        err_output(e)
    };
    let two = |r: solidcore::Result<api::BooleanOutcome>, kept: bool| match r {
        Ok(out) => two_record(&out, kept),
        Err(e) => Ok(err_output(&e)),
    };
    let lone = |r: solidcore::Result<Brep>| {
        two(
            r.map(|result| api::BooleanOutcome {
                result,
                other: None,
            }),
            false,
        )
    };
    match i {
        Inputs::Boolean { a, b, operation } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            two(
                api::boolean(k, a, b, op(*operation)?),
                keeps_other(*operation, false),
            )
        }
        Inputs::Union { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_union(k, a, b))
        }
        Inputs::Difference { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_difference(k, a, b))
        }
        Inputs::Intersection { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_intersection(k, a, b))
        }
        Inputs::Merge { a, b } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            lone(api::boolean_merge(k, a, b))
        }
        Inputs::WithOptions {
            a,
            b,
            operation,
            flags,
        } => {
            let (a, b) = (build(k, a)?, build(k, b)?);
            two(
                api::boolean_with_options(k, a, b, op(*operation)?, options(*flags)),
                keeps_other(*operation, flags.keep_other_brep),
            )
        }
        Inputs::MergeBreps {
            breps: list,
            operation,
        } => {
            let mut v = build_all(k, list)?;
            match api::merge_breps(k, &mut v, op(*operation)?) {
                Ok(res) => {
                    let mut o = vec![count(v.len())?, 1];
                    put_brep(&mut o, &res)?;
                    Ok(record(0, o))
                }
                // Rejected: no result (`cleared`) and the list untouched.
                Err(e) if matches!(e, SolidError::InvalidInput { .. }) => {
                    seen(&e);
                    Ok(record(status(&e), vec![1, count(v.len())?]))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Lists {
            breps1,
            breps2,
            operation,
        } => {
            let b1 = build_all(k, breps1)?;
            let b2 = build_all(k, breps2)?;
            match api::boolean_lists(k, &b1, &[], &b2, &[], *operation) {
                Ok(res) => {
                    let mut o = vec![count(res.breps.len())?];
                    for b in &res.breps {
                        o.push(1);
                        put_brep(&mut o, b)?;
                    }
                    o.push(count(res.surfaces.len())?);
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
            }
        }
        Inputs::Trees { breps: list, tree } => {
            let mut v: Vec<Option<Brep>> = build_all(k, list)?.into_iter().map(Some).collect();
            match api::boolean_trees(k, &mut v, tree) {
                Ok(()) => {
                    let mut o = vec![count(v.len())?];
                    for b in &v {
                        o.push(i64::from(b.is_some()));
                        if let Some(b) = b {
                            put_brep(&mut o, b)?;
                        }
                    }
                    Ok(record(0, o))
                }
                Err(e) => Ok(err_output(&e)),
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
        // ---- compared: reach SmMerge ----------------------------------------------
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
