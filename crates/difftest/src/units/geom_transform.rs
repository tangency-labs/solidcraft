//! Runner for unit `geom.transform`: 4x4 linear transforms (the simplest volume map).
//!
//! C++: `solidcore_ffi::units::geom_transform` (upstream `SmTransform`,
//! `source/SMLib/inc/SmTransform.h`, `source/SMLib/src/SmTransform.cpp`).
//! Rust: `solidcore::geom::transform`.
//!
//! Every case builds transform `a` (and `b` for binary operations) by a construction
//! `kind` from eight vectors `v` and an optional-Z flag `zf`, then runs one operation
//! `op` with vectors `w`, flag `flag`, integer `n` and (for `read_db`) the database
//! text `data`. The result `r` is the operation's values with booleans as
//! `1.0`/`0.0` and statuses as their upstream codes (`SM_ERR` = 1001,
//! `SM_ERR_INVALID_INPUT` = 1007). `read_db` reads a whole transform volume (its
//! base data through `surface.volume_p2`) and compares the matrix.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::surface::volume::{Discontinuities3d, Volume, VolumeKind};
use solidcore::math::{EFF_ZERO, PI};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::geom_transform::{self as ffi, Built, Kind, Op, Operands};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i32 = 1007;

/// `geom.transform` runner.
pub struct Runner;

/// Construction names used in `inputs.a.kind` / `inputs.b.kind`.
const KINDS: [(&str, Kind); 8] = [
    ("identity", Kind::Identity),
    ("canonical", Kind::Canonical),
    ("setters", Kind::Setters),
    ("axis2", Kind::Axis2),
    ("copy", Kind::Copy),
    ("copy_virtual", Kind::CopyVirtual),
    ("concat", Kind::Concat),
    ("reset", Kind::Reset),
];

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 22] = [
    ("state", Op::State),
    ("transform_vector", Op::TransformVector),
    ("inv_point", Op::InvPoint),
    ("inv_vector", Op::InvVector),
    ("concat", Op::Concat),
    ("scale", Op::Scale),
    ("transform_simple", Op::TransformSimple),
    ("invert", Op::Invert),
    ("evaluate", Op::Evaluate),
    ("guess", Op::Guess),
    ("global_solve", Op::GlobalSolve),
    ("local_solve", Op::LocalSolve),
    ("eq", Op::Eq),
    ("eq_self", Op::EqSelf),
    ("is_kind_of", Op::IsKindOf),
    ("bbox", Op::Bbox),
    ("read_db", Op::ReadDb),
    ("has_disconts", Op::HasDisconts),
    ("type_strings", Op::TypeStrings),
    ("init", Op::Init),
    ("set_a2p", Op::SetA2p),
    ("set_canonical", Op::SetCanonical),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransformIn {
    kind: String,
    #[serde(default)]
    v: [[f64; 3]; 8],
    #[serde(default)]
    zf: bool,
}

impl Default for TransformIn {
    fn default() -> Self {
        Self {
            kind: "identity".into(),
            v: [[0.0; 3]; 8],
            zf: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: TransformIn,
    #[serde(default)]
    b: TransformIn,
    #[serde(default)]
    w: [[f64; 3]; 5],
    #[serde(default)]
    flag: bool,
    #[serde(default)]
    n: i64,
    #[serde(default)]
    data: String,
    #[serde(default)]
    binary: bool,
}

/// The volume handle stored in solutions (`m_apObjects[0] = this`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct ThisVolume;

impl SolutionObject for ThisVolume {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Volume
    }
}

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Result<T, HarnessError> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, x)| *x)
        .ok_or_else(|| HarnessError::new(format!("unknown name {name}")))
}

fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => f64::from(SM_ERR_INVALID_INPUT),
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn normalise(e: &SolidError) -> String {
    format!("rust_error: {e}")
}

/// The database bytes of a case: `data` as text, or (`binary`) as whitespace
/// separated doubles written little-endian.
fn db_bytes(i: &Inputs) -> Result<Vec<u8>, HarnessError> {
    if !i.binary {
        return Ok(i.data.clone().into_bytes());
    }
    let mut out = Vec::new();
    for tok in i.data.split_whitespace() {
        let v: f64 = tok
            .parse()
            .map_err(|_| HarnessError::new(format!("bad binary double {tok}")))?;
        out.extend(v.to_le_bytes());
    }
    Ok(out)
}

fn ffi_built(t: &TransformIn) -> Result<Built, HarnessError> {
    Ok(Built {
        kind: lookup(&KINDS, &t.kind)?,
        v: t.v,
        zf: t.zf,
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let a = ffi_built(&i.a)?;
    let b = ffi_built(&i.b)?;
    let x = Operands {
        w: i.w,
        flag: i.flag,
        n: i.n,
        data: db_bytes(i)?,
        file_type: i32::from(i.binary),
    };
    match ffi::op(op, &a, &b, &x) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// Build a transform as the shim does.
fn build(t: &TransformIn) -> Result<Transform, HarnessError> {
    let kind = lookup(&KINDS, &t.kind)?;
    let [v0, v1, v2, v3, v4, v5, v6, v7] = t.v.map(Vec3::from);
    let z3 = t.zf.then_some(v3);
    let z7 = t.zf.then_some(v7);
    let setters = || {
        let mut s = Transform::IDENTITY;
        s.set_to_x_axis(v1);
        s.set_to_y_axis(v2);
        s.set_to_z_axis(v3);
        s.set_to_disp(v0);
        s
    };
    Ok(match kind {
        Kind::Identity => Transform::default(),
        // Both copies are `Copy`.
        Kind::Canonical | Kind::Copy | Kind::CopyVirtual => Transform::new(v0, v1, v2, z3),
        Kind::Setters => setters(),
        Kind::Axis2 => {
            let mut s = Transform::IDENTITY;
            s.set_axis2_placement(&Axis2Placement::new(v0, v1, v2));
            s
        }
        Kind::Concat => {
            let mut s = Transform::new(v0, v1, v2, z3);
            s.concat_transform(&Transform::new(v4, v5, v6, z7));
            s
        }
        Kind::Reset => {
            let mut s = setters();
            s.set_canonical(v4, v5, v6, z7);
            s
        }
    })
}

fn put_v(out: &mut Vec<f64>, v: Vec3) {
    out.extend(v.to_array());
}

fn put_mat(out: &mut Vec<f64>, t: &Transform) {
    out.extend(t.matrix().iter().flatten());
}

fn put_status<T>(out: &mut Vec<f64>, r: &solidcore::Result<T>) {
    out.push(match r {
        Ok(_) => 0.0,
        Err(e) => status_code(e),
    });
}

fn object_code(o: Option<ThisVolume>) -> f64 {
    if o.is_some() {
        1.0
    } else {
        0.0
    }
}

fn str_codes(out: &mut Vec<f64>, s: &str) {
    out.extend(s.bytes().map(f64::from));
}

fn rust_values(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let mut a = build(&i.a)?;
    let mut out = Vec::new();
    let [w0, w1, w2, w3, _] = i.w.map(Vec3::from);
    match op {
        Op::State => {
            put_mat(&mut out, &a);
            put_v(&mut out, a.to_origin());
            put_v(&mut out, a.to_x_axis());
            put_v(&mut out, a.to_y_axis());
            put_v(&mut out, a.to_z_axis());
            put_v(&mut out, a.to_disp());
            let d = a.natural_param_domain();
            out.extend([d.u_min(), d.v_min(), d.w_min(), d.u_max(), d.v_max(), d.w_max()]);
            out.push(flag(a.is_bounded_simple()));
            out.push(flag(a.is_curve_in_param_domain_simple()));
            out.push(flag(a.has_discontinuities_simple(None)));
            out.push(a.type_code() as f64);
        }
        Op::TransformVector => put_v(&mut out, a.transform_vector(w0)),
        Op::InvPoint | Op::InvVector => {
            let r = if op == Op::InvPoint {
                a.inv_transform_point(w0)
            } else {
                a.inv_transform_vector(w0)
            };
            put_status(&mut out, &r);
            if let Ok(p) = r {
                put_v(&mut out, p);
            }
        }
        Op::Concat => {
            let b = build(&i.b)?;
            put_mat(&mut out, &b);
            a.concat_transform(&b);
            out.push(0.0);
            put_mat(&mut out, &a);
        }
        Op::Scale => {
            a.scale_simple(w0, i.flag.then_some(w1));
            out.push(0.0);
            put_mat(&mut out, &a);
        }
        Op::TransformSimple => {
            a.transform_simple(&Axis2Placement::new(w0, w1, w2), i.flag.then_some(w3));
            out.push(0.0);
            put_mat(&mut out, &a);
        }
        Op::Invert => {
            let r = a.invert_simple();
            put_status(&mut out, &r);
            put_mat(&mut out, &a);
        }
        Op::Evaluate => {
            let n = usize::try_from(i.n).map_err(|_| HarnessError::new("negative n"))?;
            let r = a.evaluate_simple(w0, n);
            put_status(&mut out, &r);
            if let Ok(d) = r {
                for x in d {
                    put_v(&mut out, x);
                }
            }
        }
        Op::Guess => {
            let r = a.inv_evaluate_guess_point_simple(w0);
            put_status(&mut out, &r);
            if let Ok(g) = r {
                out.push(g.len() as f64);
                for p in g.iter().take(1) {
                    put_v(&mut out, *p);
                }
            }
        }
        Op::GlobalSolve => {
            // the pre-fill is harness data: same bound as the FFI guard
            if !(0..=4).contains(&i.n) {
                return Err(HarnessError::new("pre-filled solutions"));
            }
            let mut sols: SolutionArray<ThisVolume> = SolutionArray::new();
            for k in 0..i.n {
                let mut s = Solution::new();
                s.end.solution_value = 7.0 + k as f64;
                s.start.parameters[3] = 3.0 + k as f64;
                sols.add(&s);
            }
            let r = a.global_point_solve_simple(w0, &mut sols, ThisVolume);
            put_status(&mut out, &r);
            out.push(flag(r.is_ok()));
            out.push(sols.len() as f64);
            for s in sols.as_slice() {
                out.push(f64::from(s.solution_type as i32));
                out.push(s.num_variables as f64);
                out.push(s.start.solution_value);
                out.extend(s.start.parameters.iter().take(4));
                out.push(s.end.solution_value);
                out.push(s.num_objects as f64);
                out.push(object_code(s.objects[0]));
            }
        }
        Op::LocalSolve => {
            let r = a.local_point_solve_simple(w0, ThisVolume);
            put_status(&mut out, &r);
            if let Ok(s) = r {
                out.push(1.0);
                out.push(f64::from(s.solution_type as i32));
                out.push(s.num_variables as f64);
                out.push(s.start.solution_value);
                out.extend(s.start.parameters.iter().take(4));
                out.push(s.num_objects as f64);
                out.push(object_code(s.objects[0]));
            }
        }
        Op::Eq => {
            let b = build(&i.b)?;
            put_mat(&mut out, &b);
            out.push(flag(a.approx_eq(&b)));
        }
        Op::EqSelf => {
            let c = a;
            out.push(flag(a.approx_eq(&c)));
        }
        Op::IsKindOf => out.push(flag(a.is_kind_of(i.n))),
        Op::Bbox => {
            let bx = Extent3d::from_bounds(w0.x, w0.y, w0.z, w1.x, w1.y, w1.z);
            // the shim passes a default param pseudo box whenever an output is requested
            let param_pb = PseudoBox::new();
            let mut proj_box = Extent3d::new();
            let mut proj_pb = PseudoBox::new();
            let r = a.evaluate_bounding_box_simple(
                &bx,
                (i.n != 0).then_some(&param_pb),
                (i.n == 1).then_some(&mut proj_box),
                (i.n == 2).then_some(&mut proj_pb),
                |p| Volume::new(VolumeKind::Transform(a)).evaluate_point(p, true),
            );
            match r {
                Ok(()) => {
                    out.push(0.0);
                    if i.n == 1 {
                        let b = proj_box;
                        out.extend([b.u_min(), b.v_min(), b.w_min(), b.u_max(), b.v_max(), b.w_max()]);
                    }
                }
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::ReadDb => {
            let ft = if i.binary {
                FileType::Binary
            } else {
                FileType::Ascii
            };
            let mut db = DatabaseReader::new();
            db.open_for_read(db_bytes(i)?, ft);
            match Transform::read_from_db(&mut db, 30) {
                Ok(v) => {
                    let VolumeKind::Transform(t) = v.kind() else {
                        return Err(HarnessError::new("read_db did not make a transform"));
                    };
                    out.push(0.0);
                    put_mat(&mut out, t);
                }
                Err(e) => return Ok(Err(e)),
            }
        }
        Op::HasDisconts => {
            let mut disconts = Discontinuities3d::new();
            out.push(flag(a.has_discontinuities_simple(Some(&mut disconts))));
        }
        Op::TypeStrings => {
            out.push(a.type_code() as f64);
            str_codes(&mut out, a.type_string());
            out.push(-1.0);
            str_codes(&mut out, a.class_string());
        }
        Op::Init => {
            a.init();
            put_mat(&mut out, &a);
        }
        Op::SetA2p => {
            a.set_axis2_placement(&Axis2Placement::new(w0, w1, w2));
            put_mat(&mut out, &a);
        }
        Op::SetCanonical => {
            a.set_canonical(w0, w1, w2, i.flag.then_some(w3));
            put_mat(&mut out, &a);
        }
    }
    Ok(Ok(out))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_values(i)? {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
    }
}

// ---- generation ---------------------------------------------------------------

fn unit(rng: &mut Rng) -> [f64; 3] {
    let z = rng.uniform(-1.0, 1.0);
    let t = rng.uniform(0.0, 2.0 * PI);
    let r = (1.0 - z * z).sqrt();
    [r * t.cos(), r * t.sin(), z]
}

fn scale(v: [f64; 3], k: f64) -> [f64; 3] {
    [v[0] * k, v[1] * k, v[2] * k]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn coord(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.7 {
        0.0
    } else if r < 0.8 {
        rng.range_i64(-5, 5) as f64
    } else if r < 0.9 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(2, 7) as i32)
    } else {
        rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(3, 12) as i32))
    }
}

fn point(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

/// An axis: unit, scaled, axis-aligned, tiny (around the `SM_EFF_ZERO` singular
/// tests), zero or arbitrary.
fn direction(rng: &mut Rng) -> [f64; 3] {
    let r = rng.unit();
    if r < 0.35 {
        unit(rng)
    } else if r < 0.5 {
        scale(unit(rng), rng.uniform(0.01, 100.0))
    } else if r < 0.65 {
        let mut v = [0.0; 3];
        if let Some(slot) = v.get_mut(rng.below(3) as usize) {
            *slot = if rng.chance(0.5) { 1.0 } else { -1.0 } * rng.uniform(0.5, 3.0);
        }
        v
    } else if r < 0.7 {
        scale(unit(rng), EFF_ZERO * rng.uniform(0.3, 3.0))
    } else if r < 0.73 {
        [0.0; 3]
    } else {
        point(rng)
    }
}

/// A second axis for `x`: perpendicular, nearly parallel (near-singular), parallel
/// or independent.
fn partner(rng: &mut Rng, x: [f64; 3]) -> [f64; 3] {
    let r = rng.unit();
    let p = cross(x, unit(rng));
    let plen = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    if r < 0.4 && plen > 0.0 {
        scale(p, rng.uniform(0.1, 5.0) / plen)
    } else if r < 0.5 {
        let eps = 10f64.powi(-(rng.range_i64(6, 15) as i32)) * rng.uniform(-3.0, 3.0);
        add(x, scale(unit(rng), eps))
    } else if r < 0.58 {
        scale(x, rng.uniform(-3.0, 3.0))
    } else {
        direction(rng)
    }
}

fn gen_transform(rng: &mut Rng) -> TransformIn {
    let idx = rng.below(KINDS.len() as u64) as usize;
    let (name, _) = KINDS.get(idx).copied().unwrap_or(("identity", Kind::Identity));
    let x = direction(rng);
    let y = partner(rng, x);
    let z = if rng.chance(0.5) {
        cross(x, y)
    } else {
        partner(rng, x)
    };
    let x2 = direction(rng);
    let y2 = partner(rng, x2);
    let z2 = partner(rng, y2);
    TransformIn {
        kind: name.to_string(),
        v: [point(rng), x, y, z, point(rng), x2, y2, z2],
        zf: rng.chance(0.5),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    // read_db cases come from fixtures only.
    let ops: Vec<(&str, Op)> = OPS
        .iter()
        .copied()
        .filter(|(_, o)| *o != Op::ReadDb)
        .collect();
    let idx = rng.below(ops.len() as u64) as usize;
    let (name, op) = ops.get(idx).copied().unwrap_or(("state", Op::State));
    let a = gen_transform(rng);
    let mut b = gen_transform(rng);
    if op == Op::Eq && rng.chance(0.5) {
        b = a.clone();
        let eps = if rng.chance(0.3) {
            0.0
        } else {
            10f64.powi(-(rng.range_i64(8, 15) as i32))
        };
        let (vi, ci) = (rng.below(8) as usize, rng.below(3) as usize);
        if let Some(c) = b.v.get_mut(vi).and_then(|v| v.get_mut(ci)) {
            *c += eps * rng.uniform(-1.0, 1.0);
        }
    }
    let x = direction(rng);
    let y = partner(rng, x);
    let mut w = [point(rng), x, y, direction(rng), point(rng)];
    if op == Op::Scale && rng.chance(0.3) {
        w[0] = [rng.uniform(-3.0, 3.0); 3];
    }
    let n = match op {
        Op::Evaluate => rng.range_i64(0, 7),
        Op::GlobalSolve => rng.range_i64(0, 3),
        Op::IsKindOf => *rng
            .choose(&[22008, 22001, 11001, 22004, 0, 12020])
            .unwrap_or(&22008),
        Op::Bbox => rng.range_i64(0, 2),
        _ => 0,
    };
    if op == Op::Bbox {
        let lo = point(rng);
        let hi = add(lo, [rng.uniform(0.0, 5.0), rng.uniform(0.0, 5.0), rng.uniform(0.0, 5.0)]);
        w[0] = lo;
        w[1] = hi;
    }
    Inputs {
        op: name.to_string(),
        a,
        b,
        w,
        flag: rng.chance(0.5),
        n,
        data: String::new(),
        binary: false,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.transform"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
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
