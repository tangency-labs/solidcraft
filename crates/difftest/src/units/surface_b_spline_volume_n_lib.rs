//! Runner for unit `surface.b_spline_volume_n_lib`: the knot-plane continuities, the
//! discontinuity summary and the parameter-domain curve test of a B-spline volume.
//!
//! C++: `solidcore_ffi::units::surface_b_spline_volume_n_lib` (upstream
//! `source/SMLib/src/SmBSplineVolumeNLib.cpp` through `SmVolume*` on a B-spline volume
//! built from an `sm_AllocateNurbVolume` record, optionally oriented and compounded with
//! `SmTransform` next maps; parameter curves are real `SmBSplineCurve`, `SmLine`,
//! `SmEllipse` and `SmHermiteCurve` objects). Rust: `solidcore::surface::b_spline_volume_n_lib`
//! through `solidcore::surface::volume`.
//!
//! A case is a volume (`vol`, as in unit `surface.b_spline_volume`) and an `op` with its
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as `out`.
//!
//! Deferred: curve tests that get past the bounding boxes need
//! `SmCurve::CalculateTightBoundingBox` (non-line curves) or
//! `SmCurve::IsOnPositiveSideOfPlanes` (unit `curve.curve_p9`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::spiral::allocate_nurb_volume;
use solidcore::surface::b_spline_volume::{BSplineVolume, VolumeParam};
use solidcore::surface::volume::p1::{DirectionDiscontinuities, Discontinuities3d};
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::SolidError;
use solidcore_ffi::units::surface_b_spline_volume_n_lib::{self as ffi, Op};

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

/// A parameter-space curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    Bspline {
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
    Line {
        p: [f64; 3],
        v: [f64; 3],
        d: [f64; 2],
    },
    Ellipse {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: [f64; 2],
    },
    Hermite {
        p1: [f64; 3],
        d1: [f64; 3],
        p2: [f64; 3],
        d2: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Conts {
        vol: VolIn,
        e: usize,
        geom: bool,
    },
    HasDisc {
        vol: VolIn,
        given: bool,
        geom: bool,
    },
    CurveIn {
        vol: VolIn,
        which: usize,
        comp: bool,
        curve: CurveIn,
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

fn push_tf(o: &mut Vec<f64>, t: &Tf) {
    for p in t {
        o.extend(p);
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

fn push_curve(o: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Bspline { p, cpts, knots } => {
            o.extend([0.0, cpts.len() as f64 - 1.0, *p as f64]);
            for c in cpts {
                o.extend(c);
            }
            o.extend(knots);
        }
        CurveIn::Line { p, v, d } => {
            o.push(1.0);
            o.extend(p);
            o.extend(v);
            o.extend(d);
        }
        CurveIn::Ellipse { c, x, y, d, r } => {
            o.push(2.0);
            o.extend(c);
            o.extend(x);
            o.extend(y);
            o.extend(d);
            o.extend(r);
        }
        CurveIn::Hermite { p1, d1, p2, d2 } => {
            o.push(3.0);
            for v in [p1, d1, p2, d2] {
                o.extend(v);
            }
        }
    }
}

fn cpp_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    let op = match i {
        Inputs::Conts { vol, e, geom } => {
            push_vol(&mut o, vol);
            o.extend([*e as f64, b(*geom)]);
            Op::Conts
        }
        Inputs::HasDisc { vol, given, geom } => {
            push_vol(&mut o, vol);
            o.extend([b(*given), b(*geom)]);
            Op::HasDisc
        }
        Inputs::CurveIn {
            vol,
            which,
            comp,
            curve,
        } => {
            push_vol(&mut o, vol);
            o.extend([*which as f64, b(*comp)]);
            push_curve(&mut o, curve);
            Op::CurveIn
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

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

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

fn build_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Bspline { p, cpts, knots } => {
            let n = cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
            let mut cur = allocate_nurb_curve(n, *p, n + p + 1)
                .map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(cpts) {
                *d = CPoint::new(s[0], s[1], s[2], s[3]);
            }
            let dst = cur.knots_mut();
            if dst.len() != knots.len() {
                return Err(harness("curve knot count mismatch"));
            }
            dst.copy_from_slice(knots);
            Ok(Curve::from(BSplineCurve::from_nurb(cur, 3, false)))
        }
        CurveIn::Line { p, v, d } => Ok(Curve::from(Line::bounded(
            v3(*p),
            v3(*v),
            &Extent1d::from_min_max(d[0], d[1]),
            &BoundedLineOptions::default(),
        ))),
        CurveIn::Ellipse { c, x, y, d, r } => Ellipse::new(
            k,
            v3(*c),
            v3(*x),
            v3(*y),
            Extent1d::from_min_max(d[0], d[1]),
            r[0],
            r[1],
            &EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: false,
            },
        )
        .map(Curve::from)
        .map_err(|e| harness(format!("ellipse: {e}"))),
        CurveIn::Hermite { p1, d1, p2, d2 } => Ok(Curve::from(HermiteCurve::new(
            v3(*p1),
            v3(*d1),
            v3(*p2),
            v3(*d2),
            3,
        ))),
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

fn cont(c: ContinuityType) -> f64 {
    f64::from(c as i32)
}

fn put_dir(o: &mut Vec<f64>, d: &DirectionDiscontinuities) {
    o.push(b(d.unaligned));
    o.push(cont(d.min_cont));
    o.push(d.params.len() as f64);
    o.extend(&d.params);
    o.push(d.conts.len() as f64);
    o.extend(d.conts.iter().map(|&c| cont(c)));
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::Conts { vol, e, geom } => {
            let v = build(vol)?;
            let param = VolumeParam::from_code(*e).ok_or_else(|| harness("bad param"))?;
            let mut d = DirectionDiscontinuities {
                unaligned: false,
                min_cont: ContinuityType::G1,
                params: vec![-7.0, 9.0],
                conts: vec![ContinuityType::C0],
            };
            match v.calculate_continuities_simple(k, param, &mut d, *geom) {
                Ok(()) => o.push(0.0),
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Ok(d);
                    }
                    o.push(status(&e));
                }
            }
            put_dir(&mut o, &d);
        }
        Inputs::HasDisc { vol, given, geom } => {
            let v = build(vol)?;
            let mut d = Discontinuities3d::new();
            d.set_unaligned();
            let r = match v.has_discontinuities_simple(k, given.then_some(&mut d), *geom) {
                Ok(r) => r,
                Err(e) => {
                    return Output::unported(&e)
                        .ok_or_else(|| harness(format!("unexpected error: {e}")))
                }
            };
            o.extend([0.0, b(r)]);
            if *given {
                o.push(b(d.has_discontinuities));
                o.push(cont(d.min_continuity));
                put_dir(&mut o, &d.u);
                put_dir(&mut o, &d.v);
                put_dir(&mut o, &d.w);
            }
        }
        Inputs::CurveIn {
            vol,
            which,
            comp,
            curve,
        } => {
            let k = Kernel::default();
            let v = build(vol)?;
            let c = build_curve(&k, curve)?;
            let r = match which {
                0 => v.is_curve_in_param_domain_simple(&k, &c),
                1 => v.is_curve_in_param_domain(&k, &c, *comp),
                _ => v.is_curve_in_in_space_domain(&k, &c, *comp),
            };
            let r = match r {
                Ok(r) => r,
                Err(e) => {
                    return Output::unported(&e)
                        .ok_or_else(|| harness(format!("unexpected error: {e}")))
                }
            };
            o.extend([0.0, b(r)]);
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

/// A transform `[origin, X, Y, Z]`: identity, rotation, scaling or general.
fn gen_transform(rng: &mut Rng) -> Tf {
    match rng.below(4) {
        0 => [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        1 => {
            let a = rng.uniform(-3.2, 3.2);
            let (s, c) = a.sin_cos();
            [coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]
        }
        2 => {
            let s = rng.uniform(0.1, 4.0);
            [coord(rng, 5.0), [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
        }
        _ => [coord(rng, 5.0), coord(rng, 2.0), coord(rng, 2.0), coord(rng, 2.0)],
    }
}

/// A knot vector of `count` knots for degree `deg` over `[lo, lo + span]`: random
/// distinct values with random multiplicities (interior up to `deg + 1`), clamped or
/// not, sometimes all equal or with near-coincident knots.
fn gen_knots(rng: &mut Rng, count: usize, deg: usize, lo: f64, span: f64) -> Vec<f64> {
    if rng.chance(0.03) {
        // a single distinct knot
        return vec![lo; count];
    }
    let mut k: Vec<f64> = Vec::with_capacity(count);
    let clamp = rng.chance(0.6) && count >= 2 * (deg + 1);
    let mut vals: Vec<f64> = (0..count).map(|_| lo + span * rng.uniform(0.0, 1.0)).collect();
    if rng.chance(0.3) {
        // uniform values
        vals = (0..count)
            .map(|i| lo + span * i as f64 / (count.max(2) - 1) as f64)
            .collect();
    }
    vals.sort_by(f64::total_cmp);
    let mut i = 0;
    while k.len() < count {
        let v = vals.get(i).copied().unwrap_or(lo + span);
        i += 1;
        let rep = 1 + rng.below(deg as u64 + 2) as usize;
        for _ in 0..rep.min(count - k.len()) {
            k.push(v);
        }
    }
    k.sort_by(f64::total_cmp);
    if clamp {
        let first = lo;
        let last = lo + span;
        for x in k.iter_mut().take(deg + 1) {
            *x = first;
        }
        for x in k.iter_mut().rev().take(deg + 1) {
            *x = last;
        }
        k.sort_by(f64::total_cmp);
    }
    if rng.chance(0.15) && count >= 3 {
        // a knot within a hair of its neighbour (the fixup spreads or merges it)
        let i = 1 + rng.below(count as u64 - 2) as usize;
        let eps = *[1e-13, 1e-9, 5e-9, 1e-8]
            .get(rng.below(4) as usize)
            .unwrap_or(&1e-9);
        if let (Some(&prev), Some(&next)) = (k.get(i - 1), k.get(i + 1)) {
            let val = (prev + eps * span.max(1.0)).min(next);
            if let Some(x) = k.get_mut(i) {
                *x = val;
            }
        }
    }
    k
}

/// The parameter range scale of a case: ordinary, tiny or huge.
fn gen_scale(rng: &mut Rng) -> (f64, f64) {
    match rng.below(10) {
        0 => (rng.uniform(-1e-6, 1e-6), rng.uniform(1e-9, 1e-6)),
        1 => (rng.uniform(-1e6, 1e6), rng.uniform(1e3, 1e7)),
        2 => (0.0, 1.0),
        _ => (
            if rng.chance(0.3) {
                0.0
            } else {
                rng.uniform(-5.0, 5.0)
            },
            rng.uniform(0.01, 10.0),
        ),
    }
}

fn gen_vol(rng: &mut Rng) -> VolIn {
    let hi = [0, 1, 2].map(|_| rng.below(ffi::MAX_CP as u64) as usize);
    let deg = hi.map(|h| (rng.below(h as u64 + 1) as usize).min(ffi::MAX_DEGREE));
    let (lo, span) = gen_scale(rng);
    let [(h0, p0), (h1, p1), (h2, p2)] = [(hi[0], deg[0]), (hi[1], deg[1]), (hi[2], deg[2])];
    let knots = [(h0, p0), (h1, p1), (h2, p2)].map(|(h, p)| {
        if rng.chance(0.7) {
            gen_knots(rng, h + p + 2, p, lo, span)
        } else {
            let (lo, span) = gen_scale(rng);
            gen_knots(rng, h + p + 2, p, lo, span)
        }
    });
    let count = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
    let cpts = (0..count)
        .map(|_| {
            let p = coord(rng, 10.0);
            [p[0], p[1], p[2], 1.0]
        })
        .collect();
    let orient = rng.chance(0.3).then(|| gen_transform(rng));
    let n_next = if rng.chance(0.7) {
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

/// The natural parameter box of `v`: `(lo, hi)` per direction.
fn domain(v: &VolIn) -> [(f64, f64); 3] {
    v.knots.clone().map(|k| {
        (
            k.first().copied().unwrap_or(0.0),
            k.last().copied().unwrap_or(0.0),
        )
    })
}

/// A parameter point: inside the domain, on a face or corner, just outside or
/// inside a face (by about `SM_EFF_ZERO` or more), or far away.
fn gen_point(rng: &mut Rng, v: &VolIn) -> [f64; 3] {
    let ends = domain(v);
    let mut p = ends.map(|(a, z)| a + (z - a) * rng.uniform(0.0, 1.0));
    match rng.below(6) {
        0 | 1 => {}
        2 => {
            let d = rng.below(3) as usize;
            let face = rng.chance(0.5);
            if let (Some(x), Some(&(a, z))) = (p.get_mut(d), ends.get(d)) {
                *x = if face { a } else { z };
            }
        }
        3 => {
            let d = rng.below(3) as usize;
            let eps = *[1e-13, 5e-12, 1e-11, 1e-9, 1e-6, 0.1]
                .get(rng.below(6) as usize)
                .unwrap_or(&1e-11);
            let out = if rng.chance(0.5) { eps } else { -eps };
            let low = rng.chance(0.5);
            if let (Some(x), Some(&(a, z))) = (p.get_mut(d), ends.get(d)) {
                *x = if low { a - out } else { z + out };
            }
        }
        4 => {
            p = ends.map(|(a, z)| if rng.chance(0.5) { a } else { z });
        }
        _ => {
            let span = ends
                .iter()
                .map(|(a, z)| z - a)
                .fold(1.0, f64::max)
                .min(1e6);
            p = ends.map(|(a, _)| a + span * rng.uniform(-3.0, 4.0));
        }
    }
    p
}

fn unit_axes(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let a = rng.uniform(-3.2, 3.2);
    let bb = rng.uniform(-1.5, 1.5);
    let (sa, ca) = a.sin_cos();
    let (sb, cb) = bb.sin_cos();
    let x = [ca * cb, sa * cb, sb];
    let y = [-sa, ca, 0.0];
    (x, y)
}

fn gen_curve(rng: &mut Rng, v: &VolIn) -> CurveIn {
    let size = domain(v)
        .iter()
        .map(|(a, z)| z - a)
        .fold(0.0, f64::max)
        .max(1e-9);
    match rng.below(4) {
        0 => {
            let a = gen_point(rng, v);
            let e = gen_point(rng, v);
            let mut dir = [e[0] - a[0], e[1] - a[1], e[2] - a[2]];
            if dir.iter().all(|&c| c == 0.0) {
                dir = [size, 0.0, 0.0];
            }
            CurveIn::Line {
                p: a,
                v: dir,
                d: [0.0, 1.0],
            }
        }
        1 => {
            let n = 1 + rng.below(6) as usize;
            let p = 1 + rng.below(n.min(3) as u64) as usize;
            let rational = rng.chance(0.3);
            let cpts = (0..=n)
                .map(|_| {
                    let q = gen_point(rng, v);
                    let w = if rational { rng.uniform(0.3, 3.0) } else { 1.0 };
                    [q[0] * w, q[1] * w, q[2] * w, w]
                })
                .collect();
            let (a, z) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                let a = rng.uniform(-10.0, 10.0);
                (a, a + rng.uniform(0.5, 20.0))
            };
            let mut knots = vec![a; p + 1];
            let mut inner: Vec<f64> = (0..n - p).map(|_| a + (z - a) * rng.uniform(0.05, 0.95)).collect();
            inner.sort_by(f64::total_cmp);
            knots.extend(inner);
            knots.extend(vec![z; p + 1]);
            CurveIn::Bspline { p, cpts, knots }
        }
        2 => {
            let c = gen_point(rng, v);
            let (x, y) = unit_axes(rng);
            let r = [
                size * rng.uniform(0.01, 1.2),
                size * rng.uniform(0.01, 1.2),
            ];
            let d0 = rng.uniform(0.0, 360.0);
            let d1 = d0 + rng.uniform(5.0, 360.0);
            CurveIn::Ellipse {
                c,
                x,
                y,
                d: [d0, d1],
                r,
            }
        }
        _ => {
            let p1 = gen_point(rng, v);
            let p2 = gen_point(rng, v);
            let s = size * rng.uniform(0.0, 2.0);
            let d1 = rng.point3(-s, s);
            let d2 = rng.point3(-s, s);
            CurveIn::Hermite { p1, d1, p2, d2 }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let vol = gen_vol(rng);
    match rng.below(10) {
        0..=3 => Inputs::Conts {
            e: if rng.chance(0.75) {
                3 + rng.below(3) as usize
            } else {
                rng.below(9) as usize
            },
            geom: rng.chance(0.4),
            vol,
        },
        4 | 5 => Inputs::HasDisc {
            given: rng.chance(0.7),
            geom: rng.chance(0.5),
            vol,
        },
        _ => {
            let curve = gen_curve(rng, &vol);
            Inputs::CurveIn {
                which: if rng.chance(0.6) {
                    0
                } else {
                    1 + rng.below(2) as usize
                },
                comp: rng.chance(0.5),
                curve,
                vol,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.b_spline_volume_n_lib"
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
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generator reaches every op and outcome that matters.
    #[test]
    fn generated_cases_cover_outcomes() {
        let mut rng = Rng::new(11);
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for _ in 0..3000 {
            let i = gen_case(&mut rng);
            let (op, input) = cpp_input(&i);
            if ffi::validate(op, &input).is_err() {
                *seen.entry("invalid".into()).or_default() += 1;
                continue;
            }
            let flat = ffi::eval(op, &input).unwrap();
            let key = match op {
                Op::CurveIn => format!("{op:?} {}", flat.get(1).copied().unwrap_or(-1.0)),
                _ => format!("{op:?} {}", flat.first().copied().unwrap_or(-1.0)),
            };
            *seen.entry(key).or_default() += 1;
        }
        eprintln!("{seen:?}");
        for k in ["Conts 0", "Conts 1001", "HasDisc 0", "CurveIn 0", "CurveIn 1"] {
            assert!(seen.contains_key(k), "{k} never seen: {seen:?}");
        }
    }
}
