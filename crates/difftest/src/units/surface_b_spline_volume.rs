//! Runner for unit `surface.b_spline_volume`: the `SmBSplineVolume` simple map
//! (accessors, copies, equality, the `USE_VOLUMES`-less evaluations, bounding boxes,
//! inversion guesses, transforms) and the `SmVolume` base members dispatching to it.
//!
//! C++: `solidcore_ffi::units::surface_b_spline_volume` (upstream
//! `source/SMLib/src/SmBSplineVolume.cpp` on a B-spline volume built from an
//! `sm_AllocateNurbVolume` record, optionally oriented and compounded with
//! `SmTransform` next maps). Rust: `solidcore::surface::b_spline_volume` through
//! `solidcore::surface::volume`.
//!
//! A case is a volume (`vol`: `hi` highest control point indices, `deg` degrees,
//! `knots` per direction, `cpts` `[x, y, z, w]` with `w` index fastest, optional
//! `orient` and `next` transforms `[origin, X, Y, Z]`) and an `op` with its
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream
//! cannot produce) and the rest as `out`.
//!
//! No case is deferred. The NLib overrides (`HasDiscontinuitiesSimple`,
//! `CalculateContinuitiesSimple`, `IsCurveInParamDomainSimple`) are compared by the
//! runner of unit `surface.b_spline_volume_n_lib`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::spiral::allocate_nurb_volume;
use solidcore::surface::b_spline_volume::{BSplineVolume, VolumeParam, VolumeParams};
use solidcore::surface::volume::{EvalOptions, Volume, VolumeKind};
use solidcore::SolidError;
use solidcore_ffi::units::surface_b_spline_volume::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

/// Generated cases per run (at least).
pub const MIN_GENERATED: usize = 2000;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type Tf = [[f64; 3]; 4];

/// A B-spline volume with its orient map and next maps.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VolIn {
    hi: [usize; 3],
    deg: [usize; 3],
    knots: [Vec<f64>; 3],
    cpts: Vec<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orient: Option<Tf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    next: Vec<Tf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Info {
        vol: VolIn,
        types: Vec<i64>,
        ivl: [f64; 2],
    },
    Copy {
        vol: VolIn,
        simple: bool,
    },
    Equal {
        vol: VolIn,
        mode: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<VolIn>,
    },
    Eval {
        vol: VolIn,
        p: [f64; 3],
        n: usize,
        comp: bool,
    },
    #[serde(rename = "bbox")]
    BBox {
        vol: VolIn,
        a: [f64; 3],
        b: [f64; 3],
        want_box: bool,
        want_pb: bool,
    },
    Iso {
        vol: VolIn,
        kind: usize,
        p1: f64,
        p2: f64,
    },
    InvGuess {
        vol: VolIn,
        p: [f64; 3],
    },
    Transform {
        vol: VolIn,
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<[f64; 3]>,
        simple: bool,
    },
    Preds {
        vol: VolIn,
        p: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
        c: [f64; 3],
        d: [f64; 3],
    },
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

fn push_tf(o: &mut Vec<f64>, t: &Tf) {
    for p in t {
        push3(o, *p);
    }
}

fn push_vol(o: &mut Vec<f64>, v: &VolIn) {
    o.extend(v.hi.map(|x| x as f64));
    o.extend(v.deg.map(|x| x as f64));
    for k in &v.knots {
        o.extend(k);
    }
    for c in &v.cpts {
        o.extend(c);
    }
    match &v.orient {
        Some(t) => {
            o.push(1.0);
            push_tf(o, t);
        }
        None => o.push(0.0),
    }
    o.push(v.next.len() as f64);
    for t in &v.next {
        push_tf(o, t);
    }
}

fn cpp_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    let op = match i {
        Inputs::Info { vol, types, ivl } => {
            push_vol(&mut o, vol);
            o.push(types.len() as f64);
            o.extend(types.iter().map(|&t| t as f64));
            o.extend(ivl);
            Op::Info
        }
        Inputs::Copy { vol, simple } => {
            push_vol(&mut o, vol);
            o.push(b(*simple));
            Op::Copy
        }
        Inputs::Equal { vol, mode, other } => {
            push_vol(&mut o, vol);
            o.push(*mode as f64);
            if *mode == 2 {
                if let Some(other) = other {
                    push_vol(&mut o, other);
                }
            }
            Op::Equal
        }
        Inputs::Eval { vol, p, n, comp } => {
            push_vol(&mut o, vol);
            push3(&mut o, *p);
            o.push(*n as f64);
            o.push(b(*comp));
            Op::Eval
        }
        Inputs::BBox {
            vol,
            a,
            b: e,
            want_box,
            want_pb,
        } => {
            push_vol(&mut o, vol);
            push3(&mut o, *a);
            push3(&mut o, *e);
            o.push(b(*want_box));
            o.push(b(*want_pb));
            Op::BBox
        }
        Inputs::Iso { vol, kind, p1, p2 } => {
            push_vol(&mut o, vol);
            o.extend([*kind as f64, *p1, *p2]);
            Op::Iso
        }
        Inputs::InvGuess { vol, p } => {
            push_vol(&mut o, vol);
            push3(&mut o, *p);
            Op::InvGuess
        }
        Inputs::Transform {
            vol,
            o: org,
            x,
            y,
            scale,
            simple,
        } => {
            push_vol(&mut o, vol);
            push3(&mut o, *org);
            push3(&mut o, *x);
            push3(&mut o, *y);
            o.push(b(scale.is_some()));
            push3(&mut o, scale.unwrap_or([1.0; 3]));
            o.push(b(*simple));
            Op::Transform
        }
        Inputs::Preds {
            vol,
            p,
            a,
            b: e,
            c,
            d,
        } => {
            push_vol(&mut o, vol);
            for v in [p, a, e, c, d] {
                push3(&mut o, *v);
            }
            Op::Preds
        }
    };
    (op, o)
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i);
    match ffi::eval(op, &input) {
        Ok(flat) => Ok(to_output(&flat)),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn transform_of(t: Tf) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build(v: &VolIn) -> Result<Volume, HarnessError> {
    let [l, m, n] = v.hi;
    let [p, q, r] = v.deg;
    let mut rec = allocate_nurb_volume(v.hi, v.deg, [l + p + 1, m + q + 1, n + r + 1])
        .map_err(|e| harness(e.to_string()))?;
    for (d, k) in v.knots.iter().enumerate() {
        let dst = rec.knots_mut(d);
        if dst.len() != k.len() {
            return Err(harness("knot count mismatch"));
        }
        dst.copy_from_slice(k);
    }
    if rec.cpts().len() != v.cpts.len() {
        return Err(harness("control point count mismatch"));
    }
    for (dst, c) in rec.cpts_mut().iter_mut().zip(&v.cpts) {
        let [x, y, z, w] = *c;
        *dst = CPoint::new(x, y, z, w);
    }
    let mut next: Option<Volume> = None;
    for t in v.next.iter().rev() {
        next = Some(Volume::with_maps(
            VolumeKind::Transform(transform_of(*t)),
            None,
            next,
        ));
    }
    Ok(Volume::with_maps(
        VolumeKind::BSpline(BSplineVolume::new(rec)),
        v.orient.map(transform_of),
        next,
    ))
}

fn bsv(v: &Volume) -> Result<&BSplineVolume, HarnessError> {
    match v.kind() {
        VolumeKind::BSpline(b) => Ok(b),
        _ => Err(harness("not a B-spline volume")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_mat(o: &mut Vec<f64>, t: &Transform) {
    for row in t.matrix() {
        o.extend(row);
    }
}

fn put_nurb(o: &mut Vec<f64>, b: &BSplineVolume) {
    let rec = b.nurb();
    for d in 0..3 {
        o.extend(rec.knots(d));
    }
    for c in rec.cpts() {
        o.extend([c.x, c.y, c.z, c.w]);
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
        match m.kind() {
            VolumeKind::Transform(t) => put_mat(o, t),
            VolumeKind::BSpline(b) => put_nurb(o, b),
            _ => {}
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

fn put_knots(o: &mut Vec<f64>, r: solidcore::Result<(Vec<f64>, Vec<usize>)>) {
    match r {
        Ok((k, m)) => {
            o.push(0.0);
            o.push(k.len() as f64);
            o.extend(k);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|&x| x as f64));
        }
        Err(e) => {
            o.push(status(&e));
            o.extend([0.0, 0.0]);
        }
    }
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

/// The status of a result: `Ok(0)` / `Ok(code)`, or `Err(deferred)`.
fn st_of<T>(r: &solidcore::Result<T>) -> Result<f64, Box<Output>> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => match Output::unported(e) {
            Some(d) => Err(Box::new(d)),
            None => Ok(status(e)),
        },
    }
}

macro_rules! st {
    ($r:expr) => {
        match st_of(&$r) {
            Ok(s) => s,
            Err(d) => return Ok(*d),
        }
    };
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::Info { vol, types, ivl } => {
            let v = build(vol)?;
            let bs = bsv(&v)?;
            o.push(0.0);
            o.push(v.type_code() as f64);
            for &t in types {
                o.push(b(v.is_kind_of(t)));
            }
            let bounded_simple = tri!(o, v.is_bounded_simple());
            let bounded = tri!(o, v.is_bounded());
            o.extend([b(bounded_simple), b(bounded)]);
            for p in VolumeParam::ALL_VALUES {
                o.push(bs.degree(p) as f64);
            }
            let d = tri!(o, v.natural_param_domain());
            pv(&mut o, d.min());
            pv(&mut o, d.max());
            for p in VolumeParam::ALL_VALUES {
                put_knots(&mut o, bs.knots(p, None));
            }
            let e = Extent1d::from_min_max(ivl[0], ivl[1]);
            for p in [VolumeParam::U, VolumeParam::V, VolumeParam::W] {
                put_knots(&mut o, bs.knots(p, Some(&e)));
            }
            put_state(&mut o, &v);
        }
        Inputs::Copy { vol, simple } => {
            let v = build(vol)?;
            let c = tri!(o, v.copy(*simple));
            o.push(0.0);
            put_state(&mut o, &c);
        }
        Inputs::Equal { vol, mode, other } => {
            let v = build(vol)?;
            let eq = match mode {
                0 => tri!(o, v.approx_eq(&v)),
                1 => {
                    let c = tri!(o, v.copy(false));
                    tri!(o, v.approx_eq(&c))
                }
                _ => {
                    let other = build(other.as_ref().ok_or_else(|| harness("no other"))?)?;
                    tri!(o, v.approx_eq(&other))
                }
            };
            o.extend([0.0, b(eq)]);
        }
        Inputs::Eval { vol, p, n, comp } => {
            let v = build(vol)?;
            let opts = EvalOptions {
                with_compounding: *comp,
                ..EvalOptions::default()
            };
            o.push(0.0);
            o.push(st!(v.evaluate_simple(v3(*p), *n, &EvalOptions::default())));
            o.push(st!(v.evaluate(v3(*p), *n, &opts)));
            o.push(st!(v.map_point(v3(*p), *comp)));
        }
        Inputs::BBox {
            vol,
            a,
            b: e,
            want_box,
            want_pb,
        } => {
            let v = build(vol)?;
            let bs = bsv(&v)?;
            let pbox = Extent3d::from_points(v3(*a), v3(*e));
            let mut proj = Extent3d::new();
            let mut pb = PseudoBox::new();
            let r = bs.evaluate_bounding_box_simple(
                &pbox,
                None,
                want_box.then_some(&mut proj),
                want_pb.then_some(&mut pb),
            );
            o.push(st!(r));
            if *want_box {
                pv(&mut o, proj.min());
                pv(&mut o, proj.max());
            }
            if *want_pb {
                for v in pb.basis() {
                    pv(&mut o, *v);
                }
                for k in 0..3 {
                    let ivl = pb.interval(k).ok_or_else(|| harness("interval"))?;
                    o.extend([ivl.min(), ivl.max()]);
                }
            }
        }
        Inputs::Iso { vol, kind, p1, p2 } => {
            let v = build(vol)?;
            let bs = bsv(&v)?;
            let kinds = [
                VolumeParams::XyIn,
                VolumeParams::XzIn,
                VolumeParams::YzIn,
                VolumeParams::Uv,
                VolumeParams::Uw,
                VolumeParams::Vw,
                VolumeParams::Unknown,
                VolumeParams::All,
                VolumeParams::None,
            ];
            let k = *kinds.get(*kind).ok_or_else(|| harness("iso kind"))?;
            let r = bs.evaluate_iso_parametric_curve_simple(k, *p1, *p2, 1.0e-3, None);
            o.push(st!(r));
            o.push(b(r.is_ok()));
        }
        Inputs::InvGuess { vol, p } => {
            let v = build(vol)?;
            let g = tri!(o, v.inv_evaluate_guess_point_simple(v3(*p)));
            o.push(0.0);
            o.push(g.len() as f64);
            for q in g {
                pv(&mut o, q);
            }
        }
        Inputs::Transform {
            vol,
            o: org,
            x,
            y,
            scale,
            simple,
        } => {
            let mut v = build(vol)?;
            let ax = Axis2Placement::new(v3(*org), v3(*x), v3(*y));
            let sc = scale.map(v3);
            let r = if *simple {
                v.transform_simple(&ax, sc)
            } else {
                v.transform(&ax, sc)
            };
            o.push(st!(r));
            put_state(&mut o, &v);
        }
        Inputs::Preds {
            vol,
            p,
            a,
            b: e,
            c,
            d,
        } => {
            let v = build(vol)?;
            let in_pt = tri!(o, v.is_point_in_param_domain_simple(v3(*p)));
            let in_line = tri!(o, v.is_line_in_param_domain_simple(v3(*a), v3(*e)));
            let pbox = Extent3d::from_points(v3(*a), v3(*e));
            let mut trim = Extent3d::from_points(v3(*c), v3(*d));
            let r = v.trim_param_bounding_box_simple(&pbox, &mut trim);
            o.extend([0.0, b(in_pt), b(in_line)]);
            o.push(st!(r));
            pv(&mut o, trim.min());
            pv(&mut o, trim.max());
        }
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

/// A transform `[origin, X, Y, Z]`: rigid, scaled, degenerate or identity.
fn gen_transform(rng: &mut Rng) -> Tf {
    match rng.below(6) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1 => {
            let a = rng.uniform(-3.2, 3.2);
            let (s, c) = a.sin_cos();
            [coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
        }
        2 => {
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

/// A knot vector of `count` knots for degree `deg`: clamped or not, uniform or
/// random, sometimes with near-coincident knots (fixed up on construction).
fn gen_knots(rng: &mut Rng, count: usize, deg: usize) -> Vec<f64> {
    let lo = if rng.chance(0.3) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let span = if rng.chance(0.05) {
        0.0
    } else {
        rng.uniform(0.01, 10.0)
    };
    let mut k: Vec<f64> = if rng.below(4) == 0 {
        (0..count)
            .map(|i| lo + span * i as f64 / (count.max(2) - 1) as f64)
            .collect()
    } else {
        let mut v: Vec<f64> = (0..count)
            .map(|_| lo + span * rng.uniform(0.0, 1.0))
            .collect();
        v.sort_by(f64::total_cmp);
        v
    };
    if rng.chance(0.6) && count >= 2 * (deg + 1) {
        // clamp the ends
        let first = k.first().copied().unwrap_or(lo);
        let last = k.last().copied().unwrap_or(lo);
        for x in k.iter_mut().take(deg + 1) {
            *x = first;
        }
        for x in k.iter_mut().rev().take(deg + 1) {
            *x = last;
        }
    }
    if rng.chance(0.2) && count >= 3 {
        // a knot within a hair of its neighbour (the fixup spreads or merges it)
        let i = 1 + rng.below(count as u64 - 2) as usize;
        let eps = *[1e-13, 1e-9, 5e-9, 1e-8]
            .get(rng.below(4) as usize)
            .unwrap_or(&1e-9);
        if let (Some(&prev), Some(&next)) = (k.get(i - 1), k.get(i + 1)) {
            let val = (prev + eps).min(next);
            if let Some(x) = k.get_mut(i) {
                *x = val;
            }
        }
    }
    k
}

fn gen_vol(rng: &mut Rng) -> VolIn {
    let hi = [
        rng.below(ffi::MAX_CP as u64) as usize,
        rng.below(4) as usize,
        rng.below(4) as usize,
    ];
    let hi = if rng.chance(0.5) {
        [hi[1], hi[0], hi[2]]
    } else {
        hi
    };
    let deg = hi.map(|h| (rng.below(h as u64 + 1) as usize).min(ffi::MAX_DEGREE));
    let ([h0, h1, h2], [d0, d1, d2]) = (hi, deg);
    let knots = [
        gen_knots(rng, h0 + d0 + 2, d0),
        gen_knots(rng, h1 + d1 + 2, d1),
        gen_knots(rng, h2 + d2 + 2, d2),
    ];
    let count = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
    let degenerate = rng.chance(0.05);
    let rational = rng.chance(0.4);
    let base = coord(rng, 3.0);
    let cpts = (0..count)
        .map(|_| {
            let p = if degenerate { base } else { coord(rng, 10.0) };
            let w = if rational { rng.uniform(0.2, 3.0) } else { 1.0 };
            if rational && rng.chance(0.5) {
                [p[0] * w, p[1] * w, p[2] * w, w]
            } else {
                [p[0], p[1], p[2], w]
            }
        })
        .collect();
    let orient = rng.chance(0.4).then(|| gen_transform(rng));
    let n_next = if rng.chance(0.6) {
        0
    } else {
        1 + rng.below(ffi::MAX_NEXT as u64) as usize
    };
    let next = (0..n_next).map(|_| gen_transform(rng)).collect();
    VolIn {
        hi,
        deg,
        knots,
        cpts,
        orient,
        next,
    }
}

/// A parameter point: inside the domain, at a corner, or anywhere.
fn gen_param(rng: &mut Rng, v: &VolIn) -> [f64; 3] {
    let ends = |k: &Vec<f64>| {
        (
            k.first().copied().unwrap_or(0.0),
            k.last().copied().unwrap_or(0.0),
        )
    };
    let e = [ends(&v.knots[0]), ends(&v.knots[1]), ends(&v.knots[2])];
    match rng.below(3) {
        0 => e.map(|(a, z)| a + (z - a) * rng.uniform(0.0, 1.0)),
        1 => e.map(|(a, z)| if rng.chance(0.5) { a } else { z }),
        _ => coord(rng, 8.0),
    }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let vol = gen_vol(rng);
    match rng.below(10) {
        0 => {
            let a = rng.uniform(-6.0, 6.0);
            let z = a + rng.uniform(-2.0, 8.0);
            Inputs::Info {
                vol,
                types: vec![22001, 22004, 22008, 22011, 12060, 1],
                ivl: [a, z],
            }
        }
        1 => Inputs::Copy {
            vol,
            simple: rng.chance(0.5),
        },
        2 => {
            let mode = rng.below(3) as usize;
            let other = (mode == 2).then(|| {
                if rng.chance(0.5) {
                    vol.clone()
                } else {
                    gen_vol(rng)
                }
            });
            Inputs::Equal { vol, mode, other }
        }
        3 => {
            let p = gen_param(rng, &vol);
            Inputs::Eval {
                vol,
                p,
                n: rng.below(4) as usize,
                comp: rng.chance(0.5),
            }
        }
        4 | 5 => {
            let (a, b) = if rng.chance(0.7) {
                (gen_param(rng, &vol), gen_param(rng, &vol))
            } else {
                // the whole domain, possibly nudged outside
                let lo = vol.knots.clone().map(|k| k.first().copied().unwrap_or(0.0));
                let hi = vol.knots.clone().map(|k| k.last().copied().unwrap_or(0.0));
                let nudge = *[0.0, 1e-13, -1e-11, 1e-6]
                    .get(rng.below(4) as usize)
                    .unwrap_or(&0.0);
                (lo.map(|x| x - nudge), hi.map(|x| x + nudge))
            };
            Inputs::BBox {
                vol,
                a,
                b,
                want_box: rng.chance(0.8),
                want_pb: rng.chance(0.8),
            }
        }
        6 => Inputs::Iso {
            vol,
            kind: rng.below(9) as usize,
            p1: rng.uniform(-2.0, 2.0),
            p2: rng.uniform(-2.0, 2.0),
        },
        7 => {
            let p = if rng.chance(0.3) {
                // exactly a control point (ties possible)
                let i = rng.below(vol.cpts.len() as u64) as usize;
                let [x, y, z, w] = vol.cpts.get(i).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
                [x / w, y / w, z / w]
            } else {
                coord(rng, 12.0)
            };
            Inputs::InvGuess { vol, p }
        }
        8 => {
            let (o, x, y) = if rng.chance(0.15) {
                ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            } else {
                let a = rng.uniform(-3.2, 3.2);
                let (s, c) = a.sin_cos();
                (coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0])
            };
            let scale = match rng.below(3) {
                0 => None,
                1 => Some([1.0; 3]),
                _ => Some([
                    rng.uniform(0.2, 3.0),
                    rng.uniform(0.2, 3.0),
                    rng.uniform(0.2, 3.0),
                ]),
            };
            Inputs::Transform {
                vol,
                o,
                x,
                y,
                scale,
                simple: rng.chance(0.4),
            }
        }
        _ => {
            let p = gen_param(rng, &vol);
            let a = gen_param(rng, &vol);
            let b = gen_param(rng, &vol);
            Inputs::Preds {
                vol,
                p,
                a,
                b,
                c: coord(rng, 5.0),
                d: coord(rng, 5.0),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.b_spline_volume"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let (op, input) = cpp_input(&i);
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
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generator reaches the success and failure paths of every op.
    #[test]
    fn generated_cases_cover_both_outcomes() {
        let mut rng = Rng::new(7);
        let mut seen: BTreeMap<(String, bool), usize> = BTreeMap::new();
        for _ in 0..3000 {
            let i = gen_case(&mut rng);
            let (op, input) = cpp_input(&i);
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            let flat = ffi::eval(op, &input).unwrap();
            let ok = flat.first() == Some(&0.0);
            *seen.entry((format!("{op:?}"), ok)).or_default() += 1;
        }
        for op in ["Info", "Copy", "Equal", "BBox", "InvGuess", "Transform", "Preds"] {
            assert!(seen.contains_key(&(op.to_string(), true)), "{op} never succeeds: {seen:?}");
        }
        for op in ["BBox", "Iso", "Transform"] {
            assert!(seen.contains_key(&(op.to_string(), false)), "{op} never fails: {seen:?}");
        }
        eprintln!("{seen:?}");
    }
}
