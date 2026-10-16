//! Runner for unit `surface.volume_p1`: the `SmVolume` base class (compound
//! evaluation through orient and next maps, domain predicates, transforms, copies,
//! equality) and the chain-rule helpers `smvol_Lift*Derivative` / `smvol_ConcatEvals`.
//!
//! C++: `solidcore_ffi::units::surface_volume_p1` (upstream
//! `source/SMLib/src/SmVolume.cpp` on chains of plain `SmVolume`, `SmTransform`,
//! default `SmTwistVolume` and default `SmUnbendVolume` maps, each optionally
//! oriented by an `SmTransform`). Rust: `solidcore::surface::volume`.
//!
//! A case is a volume (`vol`: maps `{kind, t, orient}`, kind `0` plain, `1`
//! transform `t = [origin, X, Y, Z]`, `2` twist, `3` unbend) and an `op` with its
//! parameters, or a static op. Both sides produce the flat record documented in the
//! ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a
//! Rust error upstream cannot produce) and the rest as `out`.
//!
//! Deferred cases: ops reaching an unported callee (none since the unbend volume,
//! unit `surface.unbend_volume`, was wired). The twist and unbend maps are
//! default-constructed on both sides (twist period `0`, unbend neutral distance `0`:
//! some of their values are NaN or infinite).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::surface::volume::p1::{
    concat_evals, lift_first_derivative, lift_first_derivative_srf, lift_first_derivative_vol,
    lift_second_derivative, lift_second_derivative_srf, lift_second_derivative_vol,
    lift_third_derivative, lift_third_derivative_srf, lift_third_derivative_vol,
    FindPvIntersect,
};
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::unbend_volume::UnbendVolume;
use solidcore::surface::volume::{Discontinuities3d, EvalOptions, Volume, VolumeKind};
use solidcore::SolidError;
use solidcore_ffi::units::surface_volume_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

/// Generated cases per run (at least).
pub const MIN_GENERATED: usize = 2000;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

/// One map of a volume chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MapIn {
    /// `0` plain `SmVolume`, `1` `SmTransform`, `2` twist, `3` unbend.
    kind: u8,
    /// The transform `[origin, X, Y, Z]` (kind `1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<[[f64; 3]; 4]>,
    /// The orient transform `[origin, X, Y, Z]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orient: Option<[[f64; 3]; 4]>,
}

type Vol = Vec<MapIn>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Info {
        vol: Vol,
        types: Vec<i64>,
    },
    Bounded {
        vol: Vol,
    },
    Eval {
        vol: Vol,
        p: [f64; 3],
        n: usize,
        left: [bool; 3],
        nz: bool,
        comp: bool,
    },
    Map {
        vol: Vol,
        p: [f64; 3],
        n: usize,
        left: [bool; 3],
        nz: bool,
        comp: bool,
    },
    MapPoint {
        vol: Vol,
        p: [f64; 3],
        comp: bool,
    },
    Orient {
        vol: Vol,
        p: [f64; 3],
        n: usize,
    },
    OrientPoint {
        vol: Vol,
        p: [f64; 3],
    },
    OrientDir {
        vol: Vol,
        p: [f64; 3],
        d: [f64; 3],
        n: usize,
    },
    OrientVector {
        vol: Vol,
        p: [f64; 3],
        d: [f64; 3],
    },
    EvalSimple {
        vol: Vol,
        p: [f64; 3],
        n: usize,
    },
    EvalPointSimple {
        vol: Vol,
        p: [f64; 3],
    },
    Eval1stSimple {
        vol: Vol,
        p: [f64; 3],
        left: [bool; 3],
        nz: bool,
    },
    InvGuess {
        vol: Vol,
        p: [f64; 3],
    },
    SimplePreds {
        vol: Vol,
        p: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
    },
    PtInInSpace {
        vol: Vol,
        p: [f64; 3],
        comp: bool,
    },
    LineInParam {
        vol: Vol,
        a: [f64; 3],
        b: [f64; 3],
        comp: bool,
    },
    CurveInParam {
        vol: Vol,
        a: [f64; 3],
        b: [f64; 3],
        comp: bool,
    },
    CurveInInSpace {
        vol: Vol,
        a: [f64; 3],
        b: [f64; 3],
        comp: bool,
    },
    Transform {
        vol: Vol,
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    TransformSimple {
        vol: Vol,
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default)]
        scale: Option<[f64; 3]>,
    },
    AddNext {
        vol: Vol,
        other: Vol,
        copy: bool,
    },
    Copy {
        vol: Vol,
        simple: bool,
    },
    Equal {
        vol: Vol,
        other: Vol,
        #[serde(default)]
        self_cmp: bool,
    },
    MakeSimple {
        vol: Vol,
    },
    Refresh {
        vol: Vol,
    },
    TrimBox {
        vol: Vol,
        a: [f64; 3],
        b: [f64; 3],
        c: [f64; 3],
        d: [f64; 3],
    },
    HasDisc {
        vol: Vol,
    },
    PvEval {
        vol: Vol,
        p: [f64; 3],
        sq: f64,
        x: [f64; 3],
        jac: bool,
    },
    Lift {
        form: usize,
        order: usize,
        vecs: Vec<[f64; 3]>,
    },
    Concat {
        n: usize,
        a: Vec<[f64; 3]>,
        b: Vec<[f64; 3]>,
    },
    Disconts {},
}

impl Inputs {
    fn vol(&self) -> Option<&Vol> {
        match self {
            Self::Info { vol, .. }
            | Self::Bounded { vol }
            | Self::Eval { vol, .. }
            | Self::Map { vol, .. }
            | Self::MapPoint { vol, .. }
            | Self::Orient { vol, .. }
            | Self::OrientPoint { vol, .. }
            | Self::OrientDir { vol, .. }
            | Self::OrientVector { vol, .. }
            | Self::EvalSimple { vol, .. }
            | Self::EvalPointSimple { vol, .. }
            | Self::Eval1stSimple { vol, .. }
            | Self::InvGuess { vol, .. }
            | Self::SimplePreds { vol, .. }
            | Self::PtInInSpace { vol, .. }
            | Self::LineInParam { vol, .. }
            | Self::CurveInParam { vol, .. }
            | Self::CurveInInSpace { vol, .. }
            | Self::Transform { vol, .. }
            | Self::TransformSimple { vol, .. }
            | Self::AddNext { vol, .. }
            | Self::Copy { vol, .. }
            | Self::Equal { vol, .. }
            | Self::MakeSimple { vol }
            | Self::Refresh { vol }
            | Self::TrimBox { vol, .. }
            | Self::HasDisc { vol }
            | Self::PvEval { vol, .. } => Some(vol),
            Self::Lift { .. } | Self::Concat { .. } | Self::Disconts {} => None,
        }
    }
}

// ---- C++ side -----------------------------------------------------------------------

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn push3(o: &mut Vec<f64>, p: [f64; 3]) {
    o.extend(p);
}

fn push_vol(o: &mut Vec<f64>, v: &Vol) -> Result<(), HarnessError> {
    o.push(v.len() as f64);
    for m in v {
        o.push(f64::from(m.kind));
        if m.kind == 1 {
            let t = m.t.ok_or_else(|| harness("transform map without t"))?;
            for p in t {
                push3(o, p);
            }
        }
        match m.orient {
            Some(t) => {
                o.push(1.0);
                for p in t {
                    push3(o, p);
                }
            }
            None => o.push(0.0),
        }
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    if let Some(v) = i.vol() {
        push_vol(&mut o, v)?;
    }
    let op = match i {
        Inputs::Info { types, .. } => {
            o.push(types.len() as f64);
            o.extend(types.iter().map(|&t| t as f64));
            Op::Info
        }
        Inputs::Bounded { .. } => Op::Bounded,
        Inputs::Eval {
            p,
            n,
            left,
            nz,
            comp,
            ..
        }
        | Inputs::Map {
            p,
            n,
            left,
            nz,
            comp,
            ..
        } => {
            push3(&mut o, *p);
            o.push(*n as f64);
            o.extend(left.map(b));
            o.extend([b(*nz), b(*comp)]);
            if matches!(i, Inputs::Eval { .. }) {
                Op::Eval
            } else {
                Op::Map
            }
        }
        Inputs::MapPoint { p, comp, .. } => {
            push3(&mut o, *p);
            o.push(b(*comp));
            Op::MapPoint
        }
        Inputs::Orient { p, n, .. } => {
            push3(&mut o, *p);
            o.push(*n as f64);
            Op::Orient
        }
        Inputs::OrientPoint { p, .. } => {
            push3(&mut o, *p);
            Op::OrientPoint
        }
        Inputs::OrientDir { p, d, n, .. } => {
            push3(&mut o, *p);
            push3(&mut o, *d);
            o.push(*n as f64);
            Op::OrientDir
        }
        Inputs::OrientVector { p, d, .. } => {
            push3(&mut o, *p);
            push3(&mut o, *d);
            Op::OrientVector
        }
        Inputs::EvalSimple { p, n, .. } => {
            push3(&mut o, *p);
            o.push(*n as f64);
            Op::EvalSimple
        }
        Inputs::EvalPointSimple { p, .. } => {
            push3(&mut o, *p);
            Op::EvalPointSimple
        }
        Inputs::Eval1stSimple { p, left, nz, .. } => {
            push3(&mut o, *p);
            o.extend(left.map(b));
            o.push(b(*nz));
            Op::Eval1stSimple
        }
        Inputs::InvGuess { p, .. } => {
            push3(&mut o, *p);
            Op::InvGuess
        }
        Inputs::SimplePreds { p, a, b: e, .. } => {
            push3(&mut o, *p);
            push3(&mut o, *a);
            push3(&mut o, *e);
            Op::SimplePreds
        }
        Inputs::PtInInSpace { p, comp, .. } => {
            push3(&mut o, *p);
            o.push(b(*comp));
            Op::PtInInSpace
        }
        Inputs::LineInParam { a, b: e, comp, .. }
        | Inputs::CurveInParam { a, b: e, comp, .. }
        | Inputs::CurveInInSpace { a, b: e, comp, .. } => {
            push3(&mut o, *a);
            push3(&mut o, *e);
            o.push(b(*comp));
            match i {
                Inputs::LineInParam { .. } => Op::LineInParam,
                Inputs::CurveInParam { .. } => Op::CurveInParam,
                _ => Op::CurveInInSpace,
            }
        }
        Inputs::Transform {
            o: org, x, y, scale, ..
        }
        | Inputs::TransformSimple {
            o: org, x, y, scale, ..
        } => {
            push3(&mut o, *org);
            push3(&mut o, *x);
            push3(&mut o, *y);
            o.push(b(scale.is_some()));
            push3(&mut o, scale.unwrap_or([1.0, 1.0, 1.0]));
            if matches!(i, Inputs::Transform { .. }) {
                Op::Transform
            } else {
                Op::TransformSimple
            }
        }
        Inputs::AddNext { other, copy, .. } => {
            push_vol(&mut o, other)?;
            o.push(b(*copy));
            Op::AddNext
        }
        Inputs::Copy { simple, .. } => {
            o.push(b(*simple));
            Op::Copy
        }
        Inputs::Equal {
            other, self_cmp, ..
        } => {
            push_vol(&mut o, other)?;
            o.push(b(*self_cmp));
            Op::Equal
        }
        Inputs::MakeSimple { .. } => Op::MakeSimple,
        Inputs::Refresh { .. } => Op::Refresh,
        Inputs::TrimBox {
            a, b: e, c, d, ..
        } => {
            for p in [a, e, c, d] {
                push3(&mut o, *p);
            }
            Op::TrimBox
        }
        Inputs::HasDisc { .. } => Op::HasDisc,
        Inputs::PvEval { p, sq, x, jac, .. } => {
            push3(&mut o, *p);
            o.push(*sq);
            push3(&mut o, *x);
            o.push(b(*jac));
            Op::PvEval
        }
        Inputs::Lift { form, order, vecs } => {
            o.extend([*form as f64, *order as f64]);
            for p in vecs {
                push3(&mut o, *p);
            }
            Op::Lift
        }
        Inputs::Concat { n, a, b: e } => {
            o.push(*n as f64);
            for p in a.iter().chain(e) {
                push3(&mut o, *p);
            }
            Op::Concat
        }
        Inputs::Disconts {} => Op::Disconts,
    };
    Ok((op, o))
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    match ffi::eval(op, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn transform_of(t: [[f64; 3]; 4]) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build(v: &Vol) -> Result<Volume, HarnessError> {
    let mut next: Option<Volume> = None;
    for m in v.iter().rev() {
        let kind = match m.kind {
            0 => VolumeKind::Base,
            1 => VolumeKind::Transform(transform_of(
                m.t.ok_or_else(|| harness("transform map without t"))?,
            )),
            2 => VolumeKind::Twist(TwistVolume::new()),
            3 => VolumeKind::Unbend(UnbendVolume::new()),
            k => return Err(harness(format!("bad map kind {k}"))),
        };
        next = Some(Volume::with_maps(kind, m.orient.map(transform_of), next));
    }
    next.ok_or_else(|| harness("volume without maps"))
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn pvs(o: &mut Vec<f64>, vs: impl IntoIterator<Item = Vec3>) {
    for v in vs {
        pv(o, v);
    }
}

fn put_mat(o: &mut Vec<f64>, t: &Transform) {
    for row in t.matrix() {
        o.extend(row);
    }
}

fn put_state(o: &mut Vec<f64>, v: &Volume) {
    let mut maps = vec![v];
    while let Some(n) = maps.last().and_then(|m| m.next_map()) {
        maps.push(n);
    }
    o.push(maps.len() as f64);
    for m in maps {
        o.push(m.type_code() as f64);
        if let VolumeKind::Transform(t) = m.kind() {
            put_mat(o, t);
        }
        match m.orient_map() {
            Some(t) => {
                o.push(1.0);
                put_mat(o, t);
            }
            None => o.push(0.0),
        }
        match m.inv_orient_map() {
            Some(t) => {
                o.push(1.0);
                put_mat(o, t);
            }
            None => o.push(0.0),
        }
    }
}

fn cont(c: ContinuityType) -> f64 {
    f64::from(c as i32)
}

fn put_disc(o: &mut Vec<f64>, d: &Discontinuities3d) {
    o.extend([b(d.has_discontinuities), cont(d.min_continuity)]);
    for dir in [&d.u, &d.v, &d.w] {
        o.extend([
            b(dir.unaligned),
            cont(dir.min_cont),
            dir.params.len() as f64,
            dir.conts.len() as f64,
        ]);
    }
}

fn str_codes(o: &mut Vec<f64>, s: &str) {
    o.extend(s.chars().map(|c| f64::from(u32::from(c))));
    o.push(-1.0);
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// Evaluates `r`: `Ok(v)`, or returns the deferral / status record.
macro_rules! tri {
    ($o:ident, $r:expr) => {
        match $r {
            Ok(v) => v,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                $o.push(status(&e));
                return Ok(to_output(&$o));
            }
        }
    };
}

/// The status of a result whose failure still has outputs: `Ok(0)` / `Ok(code)`, or
/// `Err(deferred)`.
fn st_of<T>(r: &solidcore::Result<T>) -> Result<f64, Box<Output>> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(d) => Err(Box::new(d)),
            None => Ok(status(e)),
        },
    }
}

fn line(a: [f64; 3], e: [f64; 3]) -> Curve {
    Curve::from(Line::from_points(v3(a), v3(e), 3))
}

fn take<const N: usize>(it: &mut impl Iterator<Item = Vec3>) -> Result<[Vec3; N], HarnessError> {
    let mut out = [Vec3::ZERO; N];
    for slot in &mut out {
        *slot = it.next().ok_or_else(|| harness("lift input too short"))?;
    }
    Ok(out)
}

fn rust_lift(form: usize, order: usize, vecs: &[[f64; 3]]) -> Result<Vec<Vec3>, HarnessError> {
    let mut it = vecs.iter().map(|&p| v3(p));
    let it = &mut it;
    Ok(match (form, order) {
        (0, 1) => {
            let [c] = take::<1>(it)?;
            vec![lift_first_derivative(c, take(it)?)]
        }
        (0, 2) => vec![lift_second_derivative(take(it)?, take(it)?, take(it)?)],
        (0, _) => vec![lift_third_derivative(
            take(it)?,
            take(it)?,
            take(it)?,
            take(it)?,
        )],
        (1, 1) => lift_first_derivative_srf(take(it)?, take(it)?).to_vec(),
        (1, 2) => {
            let s1 = take(it)?;
            let s2 = take(it)?;
            lift_second_derivative_srf(s1, s2, take(it)?, take(it)?).to_vec()
        }
        (1, _) => {
            let s1 = take(it)?;
            let s2 = take(it)?;
            let s3 = take(it)?;
            let d1 = take(it)?;
            let d2 = take(it)?;
            lift_third_derivative_srf(s1, s2, s3, d1, d2, take(it)?).to_vec()
        }
        (_, 1) => lift_first_derivative_vol(take(it)?, take(it)?).to_vec(),
        (_, 2) => {
            let f1 = take(it)?;
            let f2 = take(it)?;
            lift_second_derivative_vol(f1, f2, take(it)?, take(it)?).to_vec()
        }
        _ => {
            let f1 = take(it)?;
            let f2 = take(it)?;
            let f3 = take(it)?;
            let d1 = take(it)?;
            let d2 = take(it)?;
            lift_third_derivative_vol(f1, f2, f3, d1, d2, take(it)?).to_vec()
        }
    })
}

fn opts(left: [bool; 3], nz: bool, comp: bool) -> EvalOptions {
    EvalOptions {
        from_left: left,
        non_zero_tangents: nz,
        do_zero_sampling: true,
        with_compounding: comp,
    }
}

#[allow(clippy::too_many_lines)]
fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::Lift { form, order, vecs } => {
            o.push(0.0);
            pvs(&mut o, rust_lift(*form, *order, vecs)?);
            return Ok(to_output(&o));
        }
        Inputs::Concat { n, a, b: e } => {
            let a: Vec<Vec3> = a.iter().map(|&p| v3(p)).collect();
            let e: Vec<Vec3> = e.iter().map(|&p| v3(p)).collect();
            let c = tri!(o, concat_evals(*n, &a, &e));
            o.push(0.0);
            pvs(&mut o, c);
            return Ok(to_output(&o));
        }
        Inputs::Disconts {} => {
            o.push(0.0);
            let mut d = Discontinuities3d::new();
            put_disc(&mut o, &d);
            d.set_unaligned();
            put_disc(&mut o, &d);
            d.init();
            put_disc(&mut o, &d);
            return Ok(to_output(&o));
        }
        _ => {}
    }
    let vol = i.vol().ok_or_else(|| harness("volume op without vol"))?;
    let mut v = build(vol)?;
    match i {
        Inputs::Info { types, .. } => {
            o.push(0.0);
            o.push(v.type_code() as f64);
            str_codes(&mut o, v.type_string());
            str_codes(&mut o, v.class_string());
            for &t in types {
                o.push(b(v.is_kind_of(t)));
            }
            o.push(v.last_map().map_or(-1.0, |m| m.type_code() as f64));
            pv(&mut o, v.orient_origin());
            pv(&mut o, v.orient_x_axis());
            pv(&mut o, v.orient_y_axis());
            pv(&mut o, v.orient_z_axis());
            put_state(&mut o, &v);
        }
        Inputs::Bounded { .. } => {
            let simple = tri!(o, v.is_bounded_simple());
            let bounded = tri!(o, v.is_bounded());
            let d = tri!(o, v.natural_param_domain());
            o.extend([0.0, b(simple), b(bounded)]);
            pv(&mut o, d.min());
            pv(&mut o, d.max());
        }
        Inputs::Eval {
            p,
            n,
            left,
            nz,
            comp,
            ..
        } => {
            let d = tri!(o, v.evaluate(v3(*p), *n, &opts(*left, *nz, *comp)));
            o.push(0.0);
            pvs(&mut o, d);
        }
        Inputs::Map {
            p,
            n,
            left,
            nz,
            comp,
            ..
        } => {
            let d = tri!(o, v.map(v3(*p), *n, &opts(*left, *nz, *comp)));
            o.push(0.0);
            pvs(&mut o, d);
        }
        Inputs::MapPoint { p, comp, .. } => {
            let q = tri!(o, v.map_point(v3(*p), *comp));
            o.push(0.0);
            pv(&mut o, q);
        }
        Inputs::Orient { p, n, .. } => {
            let d = tri!(o, v.orient(v3(*p), *n, [true; 3], true, true));
            o.push(0.0);
            pvs(&mut o, d);
        }
        Inputs::OrientPoint { p, .. } => {
            let q = tri!(o, v.orient_point(v3(*p)));
            o.push(0.0);
            pv(&mut o, q);
        }
        Inputs::OrientDir { p, d, n, .. } => {
            let r = tri!(o, v.orient_directional_derivs(v3(*p), v3(*d), *n, [true; 3]));
            o.push(0.0);
            pvs(&mut o, r.into_iter().take(n + 1));
        }
        Inputs::OrientVector { p, d, .. } => {
            let r = tri!(o, v.orient_vector(v3(*p), v3(*d), [true; 3]));
            o.push(0.0);
            pv(&mut o, r);
        }
        Inputs::EvalSimple { p, n, .. } => {
            let d = tri!(o, v.evaluate_simple(v3(*p), *n, &EvalOptions::default()));
            o.push(0.0);
            pvs(&mut o, d);
        }
        Inputs::EvalPointSimple { p, .. } => {
            let q = tri!(o, v.evaluate_point_simple(v3(*p)));
            o.push(0.0);
            pv(&mut o, q);
        }
        Inputs::Eval1stSimple { p, left, nz, .. } => {
            let d = tri!(o, v.evaluate_1st_derivatives_simple(v3(*p), *left, *nz));
            o.push(0.0);
            pvs(&mut o, d);
        }
        Inputs::InvGuess { p, .. } => {
            let g = tri!(o, v.inv_evaluate_guess_point_simple(v3(*p)));
            o.push(0.0);
            o.push(g.len() as f64);
            pvs(&mut o, g);
        }
        Inputs::SimplePreds { p, a, b: e, .. } => {
            let pt = tri!(o, v.is_point_in_param_domain_simple(v3(*p)));
            let ln = tri!(o, v.is_line_in_param_domain_simple(v3(*a), v3(*e)));
            let cv = tri!(o, v.is_curve_in_param_domain_simple(&Kernel::default(), &line(*a, *e)));
            o.extend([0.0, b(pt), b(ln), b(cv)]);
        }
        Inputs::PtInInSpace { p, comp, .. } => {
            let (is_in, q) = tri!(o, v.is_point_in_in_space_domain(v3(*p), *comp));
            o.extend([0.0, b(is_in)]);
            pv(&mut o, q);
        }
        Inputs::LineInParam { a, b: e, comp, .. } => {
            let r = tri!(o, v.is_line_in_param_domain(&Kernel::default(), v3(*a), v3(*e), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::CurveInParam { a, b: e, comp, .. } => {
            let r = tri!(o, v.is_curve_in_param_domain(&Kernel::default(), &line(*a, *e), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::CurveInInSpace { a, b: e, comp, .. } => {
            let r = tri!(o, v.is_curve_in_in_space_domain(&Kernel::default(), &line(*a, *e), *comp));
            o.extend([0.0, b(r)]);
        }
        Inputs::Transform {
            o: org, x, y, scale, ..
        }
        | Inputs::TransformSimple {
            o: org, x, y, scale, ..
        } => {
            let ax = Axis2Placement::new(v3(*org), v3(*x), v3(*y));
            let r = if matches!(i, Inputs::Transform { .. }) {
                v.transform(&ax, scale.map(v3))
            } else {
                v.transform_simple(&ax, scale.map(v3))
            };
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &v);
        }
        Inputs::AddNext { other, copy, .. } => {
            let other = build(other)?;
            let r = v.add_next_map(other, if *copy { 1 } else { 2 });
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &v);
        }
        Inputs::Copy { simple, .. } => {
            let c = tri!(o, v.copy(*simple));
            o.push(0.0);
            put_state(&mut o, &c);
        }
        Inputs::Equal {
            other, self_cmp, ..
        } => {
            let other = build(other)?;
            let r = if *self_cmp {
                tri!(o, v.approx_eq(&v))
            } else {
                tri!(o, v.approx_eq(&other))
            };
            o.extend([0.0, b(r)]);
        }
        Inputs::MakeSimple { .. } => {
            v.make_map_simple();
            o.push(0.0);
            put_state(&mut o, &v);
        }
        Inputs::Refresh { .. } => {
            let r = v.refresh_inv_orient_map();
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &v);
        }
        Inputs::TrimBox {
            a, b: e, c, d, ..
        } => {
            let bx = Extent3d::from_points(v3(*a), v3(*e));
            let mut trim = Extent3d::from_points(v3(*c), v3(*d));
            let r = v.trim_param_bounding_box_simple(&bx, &mut trim);
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(dd) => return Ok(*dd),
            });
            pv(&mut o, trim.min());
            pv(&mut o, trim.max());
        }
        Inputs::HasDisc { .. } => {
            let mut d = Discontinuities3d::new();
            d.set_unaligned();
            let r = tri!(o, v.has_discontinuities_simple(k, Some(&mut d), true));
            o.extend([0.0, b(r)]);
            put_disc(&mut o, &d);
        }
        Inputs::PvEval {
            p, sq, x, jac, ..
        } => {
            let mut e = FindPvIntersect::new(v3(*p), &v, *sq);
            let mut f = [0.0; 3];
            let mut m = Matrix::new(3, 3);
            let found = tri!(o, e.evaluate(x, &mut f, if *jac { Some(&mut m) } else { None }));
            o.extend([0.0, b(found)]);
            o.extend(f);
            if *jac {
                for r in 0..3 {
                    for c in 0..3 {
                        o.push(m.at(r, c).map_err(|e| harness(e.to_string()))?);
                    }
                }
            }
        }
        Inputs::Lift { .. } | Inputs::Concat { .. } | Inputs::Disconts {} => {}
    }
    Ok(to_output(&o))
}

// ---- generation ---------------------------------------------------------------------

fn coord(rng: &mut Rng, scale: f64) -> [f64; 3] {
    let p = rng.point3(-scale, scale);
    // sometimes integral coordinates (exact arithmetic paths)
    if rng.chance(0.15) {
        p.map(f64::round)
    } else {
        p
    }
}

/// A transform `[origin, X, Y, Z]`: rigid, scaled, sheared, degenerate or identity.
fn gen_transform(rng: &mut Rng) -> [[f64; 3]; 4] {
    match rng.below(8) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1 => {
            // rotation about z by a random angle, translated
            let a = rng.uniform(-3.2, 3.2);
            let (s, c) = a.sin_cos();
            [coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
        }
        2 => {
            // singular: a zero or repeated axis
            let x = coord(rng, 2.0);
            [coord(rng, 5.0), x, x, coord(rng, 2.0)]
        }
        3 => {
            let s = rng.uniform(0.1, 4.0);
            [coord(rng, 5.0), [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
        }
        _ => [coord(rng, 5.0), coord(rng, 2.0), coord(rng, 2.0), coord(rng, 2.0)],
    }
}

fn gen_map(rng: &mut Rng, allow_unported: bool) -> MapIn {
    let roll = rng.below(20);
    let kind = if roll < 15 {
        1
    } else if roll < 18 || !allow_unported {
        0
    } else if roll == 18 {
        2
    } else {
        3
    };
    MapIn {
        kind,
        t: (kind == 1).then(|| gen_transform(rng)),
        orient: rng.chance(0.5).then(|| gen_transform(rng)),
    }
}

fn gen_vol(rng: &mut Rng) -> Vol {
    let n = 1 + rng.below(4) as usize;
    // unported kinds only now and then (they defer)
    let allow = rng.chance(0.1);
    (0..n).map(|_| gen_map(rng, allow)).collect()
}

fn gen_left(rng: &mut Rng) -> [bool; 3] {
    [rng.chance(0.5), rng.chance(0.5), rng.chance(0.5)]
}

fn gen_n(rng: &mut Rng) -> usize {
    // mostly 0..=3, sometimes the unsupported 4 and 5
    if rng.chance(0.1) {
        4 + rng.below(2) as usize
    } else {
        rng.below(4) as usize
    }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let vol = gen_vol(rng);
    let p = coord(rng, 10.0);
    match rng.below(31) {
        0 => Inputs::Info {
            vol,
            types: vec![22001, 22008, 22011, 22010, 12060, 11001, 1, 22004],
        },
        1 => Inputs::Bounded { vol },
        2..=5 => Inputs::Eval {
            vol,
            p,
            n: gen_n(rng),
            left: gen_left(rng),
            nz: rng.chance(0.5),
            comp: rng.chance(0.8),
        },
        6..=9 => Inputs::Map {
            vol,
            p,
            n: gen_n(rng),
            left: gen_left(rng),
            nz: rng.chance(0.5),
            comp: rng.chance(0.8),
        },
        10 => Inputs::MapPoint {
            vol,
            p,
            comp: rng.chance(0.8),
        },
        11 => Inputs::Orient {
            vol,
            p,
            n: rng.below(6) as usize,
        },
        12 => Inputs::OrientPoint { vol, p },
        13 => Inputs::OrientDir {
            vol,
            p,
            d: if rng.chance(0.1) {
                [0.0; 3]
            } else {
                coord(rng, 2.0)
            },
            n: gen_n(rng),
        },
        14 => Inputs::OrientVector {
            vol,
            p,
            d: coord(rng, 2.0),
        },
        15 => Inputs::EvalSimple {
            vol,
            p,
            n: rng.below(6) as usize,
        },
        16 => Inputs::EvalPointSimple { vol, p },
        17 => Inputs::Eval1stSimple {
            vol,
            p,
            left: gen_left(rng),
            nz: rng.chance(0.5),
        },
        18 => Inputs::InvGuess { vol, p },
        19 => Inputs::SimplePreds {
            vol,
            p,
            a: coord(rng, 10.0),
            b: coord(rng, 10.0),
        },
        20 => Inputs::PtInInSpace {
            vol,
            p,
            comp: rng.chance(0.5),
        },
        21 => Inputs::LineInParam {
            vol,
            a: p,
            b: coord(rng, 10.0),
            comp: rng.chance(0.8),
        },
        22 => Inputs::CurveInParam {
            vol,
            a: p,
            b: coord(rng, 10.0),
            comp: rng.chance(0.8),
        },
        23 => Inputs::CurveInInSpace {
            vol,
            a: p,
            b: coord(rng, 10.0),
            comp: rng.chance(0.8),
        },
        24 | 25 => {
            let identity = rng.chance(0.15);
            let (o, x, y) = if identity {
                ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                (coord(rng, 5.0), coord(rng, 2.0), coord(rng, 2.0))
            };
            let scale = rng.chance(0.4).then(|| {
                if rng.chance(0.3) {
                    [1.0, 1.0, 1.0]
                } else {
                    [rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)]
                }
            });
            if rng.chance(0.75) {
                Inputs::Transform {
                    vol,
                    o,
                    x,
                    y,
                    scale,
                }
            } else {
                Inputs::TransformSimple {
                    vol,
                    o,
                    x,
                    y,
                    scale,
                }
            }
        }
        26 => Inputs::AddNext {
            vol,
            other: gen_vol(rng),
            copy: rng.chance(0.5),
        },
        27 => Inputs::Copy {
            vol,
            simple: rng.chance(0.5),
        },
        28 => {
            let other = if rng.chance(0.4) {
                vol.clone()
            } else {
                gen_vol(rng)
            };
            Inputs::Equal {
                vol,
                other,
                self_cmp: rng.chance(0.1),
            }
        }
        29 => match rng.below(4) {
            0 => Inputs::MakeSimple { vol },
            1 => Inputs::HasDisc { vol },
            2 => {
                let mut vol = vol;
                if let Some(head) = vol.first_mut() {
                    head.orient.get_or_insert_with(|| gen_transform(rng));
                }
                Inputs::Refresh { vol }
            }
            _ => Inputs::TrimBox {
                vol,
                a: coord(rng, 5.0),
                b: coord(rng, 5.0),
                c: coord(rng, 5.0),
                d: coord(rng, 5.0),
            },
        },
        _ => match rng.below(4) {
            0 => {
                let n = rng.below(4) as usize;
                let size = (n + 1) * (n + 1) * (n + 1);
                Inputs::Concat {
                    n,
                    a: (0..size).map(|_| coord(rng, 3.0)).collect(),
                    b: (0..size).map(|_| coord(rng, 3.0)).collect(),
                }
            }
            1 => {
                let form = rng.below(3) as usize;
                let order = 1 + rng.below(3) as usize;
                let first: usize = match form {
                    0 => order,
                    1 => [2, 3, 4].iter().take(order).sum(),
                    _ => [3, 6, 10].iter().take(order).sum(),
                };
                let second: usize = [3, 6, 10].iter().take(order).sum();
                Inputs::Lift {
                    form,
                    order,
                    vecs: (0..first + second).map(|_| coord(rng, 3.0)).collect(),
                }
            }
            _ => Inputs::PvEval {
                vol,
                p,
                sq: if rng.chance(0.3) { 1e6 } else { 1e-20 },
                x: coord(rng, 5.0),
                jac: rng.chance(0.7),
            },
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.volume_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_ok() {
                return serde_json::to_value(i).ok();
            }
        }
        None
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
let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}
