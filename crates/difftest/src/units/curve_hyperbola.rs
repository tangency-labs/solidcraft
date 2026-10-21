//! Runner for unit `curve.hyperbola`: hyperbolic arcs (`SmHyperbola`).
//!
//! C++: `solidcore_ffi::units::curve_hyperbola` (upstream
//! `source/SMLib/src/SmHyperbola.cpp`). Rust: `solidcore::curve::hyperbola`, reached
//! through the [`Curve`] dispatch where upstream calls a virtual.
//!
//! A case builds `this` with the constructor (which makes the NURB: `MakeNurb`) and
//! runs one `op` on it (`state`, `eval_step`, `step_inv`, `adjust`, `reverse`,
//! `transform`, `gps`, `eq`, `copy`), or reads a hyperbola from a database stream
//! (`read_db`). Both sides produce the same flat record `r` (see
//! `solidcore_ffi::units::curve_hyperbola`): the status (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `-1` on the Rust side for an error upstream cannot produce), the op's
//! outputs, and the state of the curve (placement, domain, axis lengths, the B-spline
//! base's flags, control points as stored and knots).
//!
//! Deferred: `gps` (`SmCurve::GlobalPointSolve`, unit `curve.curve_p1`) and `step_inv`
//! with classification of a parameter inside the domain (`SmCurve::EvaluateSTEPPoint`,
//! unit `curve.curve_p5`). Every run has at least [`MIN_GENERATED`] generated cases;
//! inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::curve::hyperbola::Hyperbola;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector3d::Vec3;
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::curve_hyperbola::{self as ffi, DbRecord, HyperbolaIn, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// Types asked with `IsKindOf` by generated `state` cases: the curve types, the
/// object base types and some others.
const KIND_TYPES: [i64; 12] = [
    14024, 14033, 14001, 14010, 14020, 14021, 14022, 14023, 14034, 15001, 16016, 0,
];

/// `curve.hyperbola` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct HypIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    a: f64,
    b: f64,
    #[serde(default = "three")]
    dim: u64,
}

fn three() -> u64 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordIn {
    head: [f64; 13],
    n: i64,
    p: i16,
    rat: i16,
    dim: i16,
    #[serde(default = "kt_default")]
    kt: u64,
    #[serde(default = "form_default")]
    form: u64,
    vals: Vec<f64>,
}

fn kt_default() -> u64 {
    1
}

fn form_default() -> u64 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    State {
        #[serde(default)]
        types: Vec<i64>,
    },
    EvalStep {
        t: f64,
        nd: usize,
    },
    StepInv {
        p: [f64; 3],
        #[serde(default)]
        classify: bool,
    },
    Adjust {
        ivl: [f64; 2],
    },
    Reverse {
        ivl: [f64; 2],
    },
    Transform {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<[f64; 3]>,
    },
    Gps {
        ivl: [f64; 2],
        sop: i32,
        p: [f64; 3],
        tol: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<f64>,
        #[serde(default = "req_all")]
        req: i32,
    },
    Eq {
        other: HypIn,
    },
    Copy,
    ReadDb {
        dim: u64,
        binary: bool,
        version: u64,
        record: RecordIn,
    },
}

fn req_all() -> i32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<HypIn>,
    #[serde(flatten)]
    op: OpIn,
}

fn hyp_ffi(h: &HypIn) -> HyperbolaIn {
    HyperbolaIn {
        center: h.center,
        x_axis: h.x,
        y_axis: h.y,
        domain: h.domain,
        semi_axis: h.a,
        semi_image_axis: h.b,
        dim: h.dim,
    }
}

fn record_ffi(r: &RecordIn) -> DbRecord {
    DbRecord {
        head: r.head,
        n: r.n,
        p: r.p,
        rat: r.rat,
        dim: r.dim,
        kt: r.kt,
        form: r.form,
        vals: r.vals.clone(),
    }
}

fn op_ffi(op: &OpIn) -> Op {
    match op {
        OpIn::State { types } => Op::State {
            types: types.clone(),
        },
        OpIn::EvalStep { t, nd } => Op::EvalStep { t: *t, nd: *nd },
        OpIn::StepInv { p, classify } => Op::StepInv {
            p: *p,
            classify: *classify,
        },
        OpIn::Adjust { ivl } => Op::Adjust { ivl: *ivl },
        OpIn::Reverse { ivl } => Op::Reverse { ivl: *ivl },
        OpIn::Transform { o, x, y, scale } => Op::Transform {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
        },
        OpIn::Gps {
            ivl,
            sop,
            p,
            tol,
            target,
            req,
        } => Op::Gps {
            ivl: *ivl,
            op: *sop,
            p: *p,
            tol: *tol,
            target: *target,
            req: *req,
        },
        OpIn::Eq { other } => Op::Eq {
            other: hyp_ffi(other),
        },
        OpIn::Copy => Op::Copy,
        OpIn::ReadDb {
            dim,
            binary,
            version,
            record,
        } => Op::ReadDb {
            dim: *dim,
            binary: *binary,
            version: *version,
            record: record_ffi(record),
        },
    }
}

fn run_ffi(i: &Inputs) -> Result<Vec<f64>, FfiError> {
    let this = i.this.as_ref().map(hyp_ffi);
    ffi::eval(this.as_ref(), &op_ffi(&i.op))
}

// ---- Rust side --------------------------------------------------------------------

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn build(h: &HypIn) -> Hyperbola {
    Hyperbola::new(
        v3(h.center),
        v3(h.x),
        v3(h.y),
        Extent1d::from_min_max(h.domain[0], h.domain[1]),
        h.a,
        h.b,
        h.dim,
    )
}

fn hyp_of(c: &Curve) -> Result<&Hyperbola, HarnessError> {
    match c.kind() {
        CurveKind::Hyperbola(h) => Ok(h),
        _ => Err(harness("curve is not a hyperbola")),
    }
}

fn state(o: &mut Vec<f64>, h: &Hyperbola) {
    let pos = h.position();
    pv(o, pos.origin());
    pv(o, pos.x_axis());
    pv(o, pos.y_axis());
    let d = h.step_interval();
    o.extend([d.min(), d.max(), h.semi_axis(), h.semi_image_axis()]);
    let b = h.base();
    o.extend([
        b.dim() as f64,
        flag(b.is_out_of_bounds_enabled()),
        flag(b.is_nurb_borrowed()),
        b.knot_type() as i64 as f64,
        b.form() as i64 as f64,
    ]);
    match b.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
}

fn chars(o: &mut Vec<f64>, s: &str) {
    o.extend(s.bytes().map(f64::from));
    o.push(-1.0);
}

/// `Ok(v)`, or the solid error out of the enclosing member op.
macro_rules! solid {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        }
    };
}

/// The outputs of a member op after the status, or the op's error.
fn member(k: &solidcore::kernel::Kernel, curve: &mut Curve, op: &OpIn) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let mut o = Vec::new();
    match op {
        OpIn::State { types } => {
            let h = hyp_of(curve)?;
            o.extend([
                flag(curve.is_bounded()),
                flag(curve.is_analytic()),
                flag(h.is_arc()),
            ]);
            let md = h.max_analytic_domain();
            let sd = h.step_interval();
            o.extend([
                md.min(),
                md.max(),
                sd.min(),
                sd.max(),
                curve.type_code() as f64,
            ]);
            chars(&mut o, curve.type_string());
            chars(&mut o, curve.class_string());
            for &t in types {
                o.push(flag(curve.is_kind_of(t)));
            }
        }
        OpIn::EvalStep { t, nd } => {
            for x in solid!(hyp_of(curve)?.evaluate_step(*t, *nd)) {
                pv(&mut o, x);
            }
        }
        OpIn::StepInv { p, classify } => {
            let (t, loc) = solid!(hyp_of(curve)?.step_inversion(v3(*p), *classify));
            o.push(t);
            if let Some(l) = loc {
                o.push(l as i64 as f64);
            }
        }
        OpIn::Adjust { ivl } => {
            let CurveKind::Hyperbola(h) = curve.kind_mut() else {
                return Err(harness("not a hyperbola"));
            };
            solid!(h.adjust_step_interval(&Extent1d::from_min_max(ivl[0], ivl[1]), None));
        }
        OpIn::Reverse { ivl } => {
            let r = solid!(curve.reverse_parameterization(&solidcore::kernel::Kernel::default(), &Extent1d::from_min_max(ivl[0], ivl[1])));
            o.extend([r.min(), r.max()]);
        }
        OpIn::Transform { o: po, x, y, scale } => {
            let mv = Axis2Placement::new(v3(*po), v3(*x), v3(*y));
            solid!(curve.transform(&solidcore::kernel::Kernel::default(), &mv, scale.map(v3)));
        }
        OpIn::Gps {
            ivl,
            sop,
            p,
            tol,
            target,
            req,
        } => {
            let sop = SolverOperation::from_code(*sop).ok_or_else(|| harness("bad sop"))?;
            let req = SolutionRequested::from_code(*req).ok_or_else(|| harness("bad req"))?;
            let CurveKind::Hyperbola(h) = curve.kind_mut() else {
                return Err(harness("not a hyperbola"));
            };
            let k = solidcore::kernel::Kernel::default();
            let sols = solid!(h.global_point_solve_step(
                &k,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                sop,
                v3(*p),
                *tol,
                *target,
                req,
                None,
            ));
            o.push(sols.len() as f64);
            for s in &sols {
                o.push(s.start.parameters[0]);
            }
        }
        OpIn::Eq { other } => {
            let other = Curve::from(build(other));
            o.push(flag(solid!(curve.approx_eq(k, &other))));
        }
        OpIn::Copy => {
            let c = solid!(curve.copy(&solidcore::kernel::Kernel::default()));
            o.push(flag(solid!(curve.approx_eq(k, &c))));
            state(&mut o, hyp_of(&c)?);
        }
        OpIn::ReadDb { .. } => return Err(harness("read_db is not a member op")),
    }
    Ok(Ok(o))
}

/// The Rust record, or `Err(output)` for a deferred case.
fn rust_record(k: &solidcore::kernel::Kernel, i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    let mut o = Vec::new();

    if let OpIn::ReadDb {
        dim,
        binary,
        version,
        record,
    } = &i.op
    {
        let bytes = ffi::db_bytes(*binary, *version, &record_ffi(record))
            .map_err(|e| harness(format!("record: {e}")))?;
        let mut db = DatabaseReader::new();
        db.open_for_read(
            bytes,
            if *binary {
                FileType::Binary
            } else {
                FileType::Ascii
            },
        );
        match Curve::read_from_db(&solidcore::kernel::Kernel::default(), Hyperbola::TYPE, &mut db, *dim, *version) {
            Ok(c) => {
                o.extend([0.0, 1.0]);
                state(&mut o, hyp_of(&c)?);
            }
            Err(e) => match Output::unported(&e) {
                Some(d) => return Ok(Err(d)),
                None => o.push(status_code(&e)),
            },
        }
        return Ok(Ok(o));
    }

    let this = i.this.as_ref().ok_or_else(|| harness("member op without this"))?;
    let mut curve = Curve::from(build(this));
    match member(k, &mut curve, &i.op)? {
        Ok(out) => {
            o.push(0.0);
            o.extend(out);
        }
        Err(e) => match Output::unported(&e) {
            Some(d) => return Ok(Err(d)),
            None => o.push(status_code(&e)),
        },
    }
    state(&mut o, hyp_of(&curve)?);
    Ok(Ok(o))
}

// ---- generation -------------------------------------------------------------------

fn r3(rng: &mut Rng, lo: f64, hi: f64) -> [f64; 3] {
    rng.point3(lo, hi)
}

fn gen_axis(rng: &mut Rng) -> [f64; 3] {
    match rng.below(20) {
        0 => [0.0, 0.0, 0.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0, 0.0, 1.0],
        3 => [0.0, -1.0, 0.0],
        _ => r3(rng, -2.0, 2.0),
    }
}

fn gen_domain(rng: &mut Rng) -> [f64; 2] {
    match rng.below(12) {
        0 => {
            let a = rng.uniform(-5.0, 5.0);
            [a, a]
        }
        1 => {
            let a = rng.uniform(-3.0, 3.0);
            [a, a + rng.uniform(1e-9, 1e-5)]
        }
        2 => [rng.uniform(-8.0, -5.0), rng.uniform(5.0, 8.0)],
        3 => [-1.0, 1.0],
        4 => [0.0, rng.uniform(0.1, 4.0)],
        5 => [rng.uniform(-4.0, 0.0), 0.0],
        _ => [rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0)],
    }
}

fn gen_len(rng: &mut Rng) -> f64 {
    match rng.below(25) {
        0 => 0.0,
        1 => -rng.uniform(0.1, 3.0),
        2 => rng.uniform(1e-6, 1e-3),
        3 => rng.uniform(100.0, 1e4),
        _ => rng.uniform(0.05, 10.0),
    }
}

fn gen_hyp(rng: &mut Rng) -> HypIn {
    let (x, y) = if rng.chance(0.6) {
        // orthonormal frame from a random rotation
        let a = rng.uniform(0.0, std::f64::consts::TAU);
        let b = rng.uniform(-1.5, 1.5);
        let (sa, ca) = a.sin_cos();
        let (sb, cb) = b.sin_cos();
        let x = [ca * cb, sa * cb, sb];
        let z0 = [-sa, ca, 0.0];
        (x, z0)
    } else {
        (gen_axis(rng), gen_axis(rng))
    };
    HypIn {
        center: if rng.chance(0.2) {
            [0.0; 3]
        } else {
            r3(rng, -20.0, 20.0)
        },
        x,
        y,
        domain: gen_domain(rng),
        a: gen_len(rng),
        b: gen_len(rng),
        dim: if rng.chance(0.2) { 2 } else { 3 },
    }
}

/// A point on the analytic curve of `h` at `s` (no clamping), computed like upstream.
fn curve_point(h: &HypIn, s: f64) -> [f64; 3] {
    let hy = build(h);
    let pos = hy.position();
    let p = pos.origin() + (h.a * s.cosh()) * pos.x_axis() + (h.b * s.sinh()) * pos.y_axis();
    [p.x, p.y, p.z]
}

fn gen_record(rng: &mut Rng, h: &HypIn) -> RecordIn {
    let mut head = [0.0; 13];
    head[..3].copy_from_slice(&h.center);
    head[3..6].copy_from_slice(&h.x);
    head[6..9].copy_from_slice(&h.y);
    head[9] = h.domain[0];
    head[10] = h.domain[1];
    head[11] = h.a;
    head[12] = h.b;
    if rng.chance(0.5) {
        // the hyperbola's own NURB as WriteToDB would write it
        if let Some(cur) = build(h).base().gw_nurb() {
            let rat = i16::from(cur.cpts().iter().any(|c| c.w != -f64::MAX));
            let three_d = cur.cpts().iter().all(|c| c.z != f64::MAX);
            let mut vals = Vec::new();
            for c in cur.cpts() {
                vals.extend([c.x, c.y]);
                if three_d {
                    vals.push(c.z);
                }
                if rat != 0 {
                    vals.push(c.w);
                }
            }
            vals.extend_from_slice(cur.knots());
            return RecordIn {
                head,
                n: cur.n() as i64,
                p: cur.degree() as i16,
                rat,
                dim: if three_d { 3 } else { 2 },
                kt: 1,
                form: 4,
                vals,
            };
        }
    }
    let n = 1 + rng.below(5) as i64;
    let p = 1 + rng.below(n as u64) as i16;
    let rat = i16::from(rng.chance(0.5));
    let dim: i16 = if rng.chance(0.3) { 2 } else { 3 };
    let per = 2 + usize::from(dim != 2) + usize::from(rat != 0);
    let mut vals = Vec::new();
    for _ in 0..=n {
        for k in 0..per {
            if rat != 0 && k == per - 1 {
                vals.push(rng.uniform(0.2, 3.0));
            } else {
                vals.push(rng.uniform(-10.0, 10.0));
            }
        }
    }
    let nk = (n + i64::from(p) + 2) as usize;
    let mut knots: Vec<f64> = (0..nk).map(|_| rng.uniform(-2.0, 2.0)).collect();
    knots.sort_by(f64::total_cmp);
    vals.extend(knots);
    RecordIn {
        head,
        n,
        p,
        rat,
        dim,
        kt: rng.below(5),
        form: rng.below(7),
        vals,
    }
}

fn gen_ivl(rng: &mut Rng, h: &HypIn) -> [f64; 2] {
    match rng.below(6) {
        0 => h.domain,
        1 => [h.domain[0] + 1e-12, h.domain[1] - 1e-12],
        2 => [h.domain[1], h.domain[0]],
        _ => gen_domain(rng),
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let h = gen_hyp(rng);
    let op = match rng.below(20) {
        0 | 1 => OpIn::State {
            types: KIND_TYPES.to_vec(),
        },
        2..=5 => OpIn::EvalStep {
            t: if rng.chance(0.8) {
                rng.uniform(-6.0, 6.0)
            } else {
                *rng.choose(&h.domain).unwrap_or(&0.0)
            },
            nd: if rng.chance(0.9) {
                rng.below(4) as usize
            } else {
                4 + rng.below(3) as usize
            },
        },
        6..=8 => {
            let p = if rng.chance(0.6) {
                curve_point(&h, rng.uniform(-6.0, 6.0))
            } else {
                r3(rng, -30.0, 30.0)
            };
            OpIn::StepInv {
                p,
                classify: rng.chance(0.4),
            }
        }
        9 | 10 => OpIn::Adjust {
            ivl: gen_ivl(rng, &h),
        },
        11 | 12 => OpIn::Reverse {
            ivl: if rng.chance(0.5) {
                h.domain
            } else {
                gen_domain(rng)
            },
        },
        13 | 14 => {
            let identity = rng.chance(0.15);
            let s = rng.uniform(0.2, 4.0);
            OpIn::Transform {
                o: if identity {
                    [0.0; 3]
                } else {
                    r3(rng, -10.0, 10.0)
                },
                x: if identity {
                    [1.0, 0.0, 0.0]
                } else {
                    gen_axis(rng)
                },
                y: if identity {
                    [0.0, 1.0, 0.0]
                } else {
                    gen_axis(rng)
                },
                scale: match rng.below(5) {
                    0 => None,
                    1 => Some([1.0, 1.0, 1.0]),
                    2 => Some([s, s * 1.5, s]),
                    3 => Some([s, s * (1.0 + 1e-13), s]),
                    _ => Some([s, s, s]),
                },
            }
        }
        15 => OpIn::Gps {
            ivl: gen_ivl(rng, &h),
            sop: *rng.choose(&[0, 3, 4, 5]).unwrap_or(&3),
            p: r3(rng, -10.0, 10.0),
            tol: 1e-6,
            target: None,
            req: 1,
        },
        16 | 17 => {
            let mut other = h;
            match rng.below(6) {
                0 => {}
                1 => other.a += rng.uniform(-1e-9, 1e-9),
                2 => other.b += 1e-3,
                3 => other.domain[1] += 1e-11,
                4 => other.center[0] += 1e-4,
                _ => other = gen_hyp(rng),
            }
            OpIn::Eq { other }
        }
        18 => OpIn::Copy,
        _ => OpIn::ReadDb {
            dim: if rng.chance(0.25) { 2 } else { 3 },
            binary: rng.chance(0.5),
            version: if rng.chance(0.8) { 33 } else { 32 },
            record: gen_record(rng, &h),
        },
    };
    let this = match op {
        OpIn::ReadDb { .. } => None,
        _ => Some(h),
    };
    Inputs { this, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.hyperbola"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let r = run_ffi(&i).map_err(|e| harness(format!("ffi: {e}")))?;
        Ok(Output::new().values("r", r))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &solidcore::Kernel::default();
        let i: Inputs = case.parse()?;
        Ok(match rust_record(k, &i)? {
            Ok(r) => Output::new().values("r", r),
            Err(deferred) => deferred,
        })
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // redraw inputs the ffi wrapper rejects
        for _ in 0..64 {
            let i = gen_inputs(rng);
            let this = i.this.as_ref().map(hyp_ffi);
            let ok = match &i.op {
                // upstream's GlobalPointSolve crashes on a curve without NURB
                OpIn::Gps { ivl, .. } => i.this.as_ref().is_some_and(|h| {
                    let mut hy = build(h);
                    hy.adjust_step_interval(&Extent1d::from_min_max(ivl[0], ivl[1]), None)
                        .is_err()
                        || hy.base().gw_nurb().is_some()
                }),
                OpIn::ReadDb {
                    binary,
                    version,
                    record,
                    ..
                } => ffi::db_bytes(*binary, *version, &record_ffi(record)).is_ok(),
                _ => this.is_some(),
            };
            if ok {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }
}
