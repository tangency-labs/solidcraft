//! Runner for unit `surface.unbend_volume`: the unbend volume `SmUnbendVolume`
//! (construction and repair, evaluation with and without compounding, bounding
//! boxes, the exact inverse and point solves, boundary and domain predicates,
//! copies, equality, database reads).
//!
//! C++: `solidcore_ffi::units::surface_unbend_volume` (upstream
//! `source/SMLib/src/SmUnbendVolume.cpp`). Rust: `solidcore::surface::unbend_volume`
//! through the [`Volume`] dispatch of `solidcore::surface::volume`.
//!
//! A case is an unbend (`unbend`: optional component-constructor arguments `ctor`,
//! the default constructor without; optional `next` transform `[origin, X, Y, Z]`
//! appended as an owned next map) and an `op` with its parameters. Both sides
//! produce the flat record documented in the ffi module; the runner reports its
//! first entry as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007`
//! for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the
//! rest as `out`.
//!
//! Deferred cases: `read_db`, whose base-class read `SmVolume::ReadFromDB` belongs to
//! unit `surface.volume_p2`, and `curve_in`, whose `SmCurve::HasPlaneCrossings` needs
//! `SmSurface::GlobalCurveIntersect` (unit `surface.surface_p8`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::unbend_volume::UnbendVolume;
use solidcore::surface::volume::{EvalOptions, Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::surface_unbend_volume::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

/// Generated cases per run (at least).
pub const MIN_GENERATED: usize = 1500;

/// The database version the stream reads use.
const DB_VERSION: u64 = 30;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

/// The component constructor's arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Ctor {
    c: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    neutral: f64,
    tol: f64,
}

/// An unbend volume: `ctor` (the default constructor when absent) and an optional
/// next transform `[origin, X, Y, Z]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnbendIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ctor: Option<Ctor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next: Option<[[f64; 3]; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Info {
        unbend: UnbendIn,
        types: Vec<i64>,
    },
    Eval {
        unbend: UnbendIn,
        p: [f64; 3],
        n: usize,
        comp: bool,
    },
    EvalSimple {
        unbend: UnbendIn,
        p: [f64; 3],
        n: usize,
    },
    BBox {
        unbend: UnbendIn,
        a: [f64; 3],
        b: [f64; 3],
        want_box: bool,
        want_pseudo: bool,
    },
    Trim {
        unbend: UnbendIn,
        a: [f64; 3],
        b: [f64; 3],
    },
    InvGuess {
        unbend: UnbendIn,
        p: [f64; 3],
    },
    Global {
        unbend: UnbendIn,
        p: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<[[f64; 3]; 2]>,
    },
    Local {
        unbend: UnbendIn,
        p: [f64; 3],
        g: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<[[f64; 3]; 2]>,
    },
    OnBoundary {
        unbend: UnbendIn,
        p: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<f64>,
    },
    Preds {
        unbend: UnbendIn,
        p: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
    },
    CurveIn {
        unbend: UnbendIn,
        a: [f64; 3],
        b: [f64; 3],
    },
    Copy {
        unbend: UnbendIn,
        simple: bool,
    },
    Equal {
        unbend: UnbendIn,
        other: UnbendIn,
        self_cmp: bool,
    },
    ReadDb {
        data: String,
    },
}

impl Inputs {
    /// The unbend every op but `read_db` runs on.
    fn unbend(&self) -> Option<&UnbendIn> {
        match self {
            Inputs::Info { unbend, .. }
            | Inputs::Eval { unbend, .. }
            | Inputs::EvalSimple { unbend, .. }
            | Inputs::BBox { unbend, .. }
            | Inputs::Trim { unbend, .. }
            | Inputs::InvGuess { unbend, .. }
            | Inputs::Global { unbend, .. }
            | Inputs::Local { unbend, .. }
            | Inputs::OnBoundary { unbend, .. }
            | Inputs::Preds { unbend, .. }
            | Inputs::CurveIn { unbend, .. }
            | Inputs::Copy { unbend, .. }
            | Inputs::Equal { unbend, .. } => Some(unbend),
            Inputs::ReadDb { .. } => None,
        }
    }
}

// ---- C++ side -----------------------------------------------------------------------

fn put3(o: &mut Vec<f64>, p: [f64; 3]) {
    o.extend(p);
}

fn unbend_block(o: &mut Vec<f64>, u: &UnbendIn) {
    match &u.ctor {
        Some(c) => {
            o.push(1.0);
            put3(o, c.c);
            put3(o, c.axis);
            put3(o, c.mid);
            o.push(c.neutral);
            o.push(c.tol);
        }
        None => o.push(0.0),
    }
    match &u.next {
        Some(n) => {
            o.push(1.0);
            for p in n {
                put3(o, *p);
            }
        }
        None => o.push(0.0),
    }
}

fn fl(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn put_dom(o: &mut Vec<f64>, dom: Option<&[[f64; 3]; 2]>) {
    match dom {
        Some([a, b]) => {
            o.push(1.0);
            put3(o, *a);
            put3(o, *b);
        }
        None => o.push(0.0),
    }
}

fn cpp_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    if let Some(u) = i.unbend() {
        unbend_block(&mut o, u);
    }
    let op = match i {
        Inputs::Info { types, .. } => {
            o.push(types.len() as f64);
            o.extend(types.iter().map(|&t| t as f64));
            Op::Info
        }
        Inputs::Eval { p, n, comp, .. } => {
            put3(&mut o, *p);
            o.push(*n as f64);
            o.push(fl(*comp));
            Op::Eval
        }
        Inputs::EvalSimple { p, n, .. } => {
            put3(&mut o, *p);
            o.push(*n as f64);
            Op::EvalSimple
        }
        Inputs::BBox {
            a,
            b,
            want_box,
            want_pseudo,
            ..
        } => {
            put3(&mut o, *a);
            put3(&mut o, *b);
            o.push(fl(*want_box));
            o.push(fl(*want_pseudo));
            Op::BBox
        }
        Inputs::Trim { a, b, .. } => {
            put3(&mut o, *a);
            put3(&mut o, *b);
            Op::Trim
        }
        Inputs::InvGuess { p, .. } => {
            put3(&mut o, *p);
            Op::InvGuess
        }
        Inputs::Global { p, dom, .. } => {
            put3(&mut o, *p);
            put_dom(&mut o, dom.as_ref());
            Op::Global
        }
        Inputs::Local { p, g, dom, .. } => {
            put3(&mut o, *p);
            put3(&mut o, *g);
            put_dom(&mut o, dom.as_ref());
            Op::Local
        }
        Inputs::OnBoundary { p, tol, .. } => {
            put3(&mut o, *p);
            match tol {
                Some(t) => o.extend([1.0, *t]),
                None => o.push(0.0),
            }
            Op::OnBoundary
        }
        Inputs::Preds { p, a, b, .. } => {
            put3(&mut o, *p);
            put3(&mut o, *a);
            put3(&mut o, *b);
            Op::Preds
        }
        Inputs::CurveIn { a, b, .. } => {
            put3(&mut o, *a);
            put3(&mut o, *b);
            Op::CurveIn
        }
        Inputs::Copy { simple, .. } => {
            o.push(fl(*simple));
            Op::Copy
        }
        Inputs::Equal {
            other, self_cmp, ..
        } => {
            unbend_block(&mut o, other);
            o.push(fl(*self_cmp));
            Op::Equal
        }
        Inputs::ReadDb { data } => {
            o.push(data.len() as f64);
            o.extend(data.bytes().map(f64::from));
            Op::ReadDb
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

fn transform_of(t: [[f64; 3]; 4]) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build(u: &UnbendIn) -> Result<Volume, SolidError> {
    let mut vol = match &u.ctor {
        Some(c) => UnbendVolume::create(v3(c.c), v3(c.axis), v3(c.mid), c.neutral, c.tol),
        None => Volume::new(VolumeKind::Unbend(UnbendVolume::new())),
    };
    if let Some(n) = u.next {
        vol.add_next_map(Volume::new(VolumeKind::Transform(transform_of(n))), 2)?;
    }
    Ok(vol)
}

fn unbend_of(vol: &Volume) -> Result<&UnbendVolume, HarnessError> {
    match vol.kind() {
        VolumeKind::Unbend(u) => Ok(u),
        _ => Err(harness("not an unbend volume")),
    }
}

/// The volume's handle in solutions (`this`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Me;

impl SolutionObject for Me {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Volume
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

fn put_state(o: &mut Vec<f64>, v: &Volume) -> Result<(), HarnessError> {
    let u = unbend_of(v)?;
    o.push(v.type_code() as f64);
    o.push(u.unbend_neutral_dist());
    o.push(u.singularity_tol());
    for m in [v.orient_map(), v.inv_orient_map()] {
        match m {
            Some(m) => {
                o.push(1.0);
                put_mat(o, m);
            }
            None => o.push(0.0),
        }
    }
    match v.next_map() {
        Some(n) => {
            o.push(1.0);
            o.push(n.type_code() as f64);
        }
        None => o.push(0.0),
    }
    Ok(())
}

fn put_box(o: &mut Vec<f64>, b: &Extent3d) {
    o.extend([b.u_min(), b.v_min(), b.w_min(), b.u_max(), b.v_max(), b.w_max()]);
}

fn put_pseudo(o: &mut Vec<f64>, pb: &PseudoBox) {
    for b in pb.basis() {
        pv(o, *b);
    }
    for ivl in pb.intervals() {
        o.extend([ivl.min(), ivl.max()]);
    }
}

fn put_solution(o: &mut Vec<f64>, s: &Solution<Me>) {
    o.push(f64::from(s.solution_type.code()));
    o.push(s.num_variables as f64);
    o.push(s.start.solution_value);
    o.extend(s.start.parameters.iter().take(3));
    o.push(s.num_objects as f64);
    o.push(fl(s.objects.first().copied().flatten() == Some(Me)));
}

fn str_codes(o: &mut Vec<f64>, s: &str) {
    o.extend(s.chars().map(|c| f64::from(u32::from(c))));
    o.push(-1.0);
}

fn ext(d: &[[f64; 3]; 2]) -> Extent3d {
    let [a, b] = *d;
    Extent3d::from_bounds(a[0], a[1], a[2], b[0], b[1], b[2])
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

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    let vol = match i.unbend() {
        Some(u) => Some(tri!(o, build(u))),
        None => None,
    };
    let vol = vol.as_ref();
    let need = || vol.ok_or_else(|| harness("op needs an unbend"));
    match i {
        Inputs::Info { types, .. } => {
            let v = need()?;
            let u = unbend_of(v)?;
            o.push(0.0);
            put_state(&mut o, v)?;
            o.push(v.type_code() as f64);
            str_codes(&mut o, v.type_string());
            str_codes(&mut o, v.class_string());
            o.extend(types.iter().map(|&t| fl(v.is_kind_of(t))));
            pv(&mut o, v.unbend_origin());
            pv(&mut o, v.unbend_axis());
            pv(&mut o, v.unbend_mid_dir());
            pv(&mut o, v.unbend_binormal());
            let dom = tri!(o, v.natural_param_domain());
            let bounded = tri!(o, v.is_bounded_simple());
            put_box(&mut o, &dom);
            o.push(fl(bounded));
            o.push(u.legal_min_u());
        }
        Inputs::Eval { p, n, comp, .. } => {
            let opts = EvalOptions {
                with_compounding: *comp,
                ..EvalOptions::default()
            };
            let d = tri!(o, need()?.evaluate(v3(*p), *n, &opts));
            o.push(0.0);
            for x in d {
                pv(&mut o, x);
            }
        }
        Inputs::EvalSimple { p, n, .. } => {
            let d = tri!(
                o,
                need()?.evaluate_simple(v3(*p), *n, &EvalOptions::default())
            );
            o.push(0.0);
            for x in d {
                pv(&mut o, x);
            }
        }
        Inputs::BBox {
            a,
            b,
            want_box,
            want_pseudo,
            ..
        } => {
            let v = need()?;
            let u = unbend_of(v)?;
            let param_box = ext(&[*a, *b]);
            let mut proj = Extent3d::new();
            let mut pseudo = PseudoBox::new();
            tri!(
                o,
                u.evaluate_bounding_box_simple(k, 
                    v,
                    &param_box,
                    want_box.then_some(&mut proj),
                    want_pseudo.then_some(&mut pseudo),
                )
            );
            o.push(0.0);
            if *want_box {
                put_box(&mut o, &proj);
            }
            if *want_pseudo {
                put_pseudo(&mut o, &pseudo);
            }
        }
        Inputs::Trim { a, b, .. } => {
            let mut trim = Extent3d::new();
            tri!(
                o,
                need()?.trim_param_bounding_box_simple(&ext(&[*a, *b]), &mut trim)
            );
            o.push(0.0);
            put_box(&mut o, &trim);
        }
        Inputs::InvGuess { p, .. } => {
            let g = tri!(o, need()?.inv_evaluate_guess_point_simple(v3(*p)));
            o.push(0.0);
            o.push(g.len() as f64);
            for x in g {
                pv(&mut o, x);
            }
        }
        Inputs::Global { p, dom, .. } => {
            let v = need()?;
            let u = unbend_of(v)?;
            let mut sols = SolutionArray::new();
            let d = dom.as_ref().map(ext);
            let found = tri!(
                o,
                u.global_point_solve_simple(v, v3(*p), &mut sols, d.as_ref(), Me)
            );
            o.push(0.0);
            o.push(fl(found));
            o.push(sols.len() as f64);
            for s in sols.as_slice() {
                put_solution(&mut o, s);
            }
        }
        Inputs::Local { p, dom, .. } => {
            let v = need()?;
            let u = unbend_of(v)?;
            let mut sol = Solution::new();
            let d = dom.as_ref().map(ext);
            let found = tri!(
                o,
                u.local_point_solve_simple(v, v3(*p), &mut sol, d.as_ref(), Me)
            );
            o.push(0.0);
            o.push(fl(found));
            put_solution(&mut o, &sol);
        }
        Inputs::OnBoundary { p, tol, .. } => {
            let u = unbend_of(need()?)?;
            let (r, [on_u, on_v, on_w]) = u.is_on_boundary_simple(v3(*p), *tol);
            o.push(0.0);
            o.extend([fl(r), fl(on_u), fl(on_v), fl(on_w)]);
        }
        Inputs::Preds { p, a, b, .. } => {
            let v = need()?;
            let in_point = tri!(o, v.is_point_in_param_domain_simple(v3(*p)));
            let in_line = tri!(o, v.is_line_in_param_domain_simple(v3(*a), v3(*b)));
            let bounded = tri!(o, v.is_bounded_simple());
            o.push(0.0);
            o.extend([fl(in_point), fl(in_line), fl(bounded)]);
        }
        Inputs::CurveIn { a, b, .. } => {
            let line = Curve::from(Line::from_points(v3(*a), v3(*b), 3));
            let in_curve = tri!(o, need()?.is_curve_in_param_domain_simple(&Kernel::default(), &line));
            o.push(0.0);
            o.push(fl(in_curve));
        }
        Inputs::Copy { simple, .. } => {
            let c = tri!(o, need()?.copy(*simple));
            o.push(0.0);
            put_state(&mut o, &c)?;
        }
        Inputs::Equal {
            other, self_cmp, ..
        } => {
            let v = need()?;
            let other = tri!(o, build(other));
            let eq = if *self_cmp {
                tri!(o, v.approx_eq(v))
            } else {
                tri!(o, v.approx_eq(&other))
            };
            o.push(0.0);
            o.push(fl(eq));
        }
        Inputs::ReadDb { data } => {
            let mut db = DatabaseReader::new();
            db.open_for_read(data.clone().into_bytes(), FileType::Ascii);
            let v = tri!(o, UnbendVolume::read_from_db(&mut db, DB_VERSION));
            o.push(0.0);
            put_state(&mut o, &v)?;
        }
    }
    Ok(to_output(&o))
}

// ---- generation ---------------------------------------------------------------------

fn coord(rng: &mut Rng, r: f64) -> [f64; 3] {
    rng.point3(-r, r)
}

const AXES: [[f64; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [-1.0, 0.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, -1.0],
];

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    match rng.below(10) {
        0 => [0.0; 3],
        1..=3 => rng.choose(&AXES).copied().unwrap_or([0.0, 0.0, 1.0]),
        _ => coord(rng, 3.0),
    }
}

fn gen_unbend(rng: &mut Rng) -> UnbendIn {
    let ctor = (!rng.chance(0.1)).then(|| {
        let axis = gen_dir(rng);
        let mid = match rng.below(8) {
            // parallel / anti-parallel to the axis
            0 => axis.map(|c| c * rng.uniform(-2.0, 2.0)),
            // exactly perpendicular unit axes
            1 => {
                let a = rng.below(3) as usize;
                let mut m = [0.0; 3];
                if let Some(c) = m.get_mut((a + 1) % 3) {
                    *c = 1.0;
                }
                m
            }
            _ => gen_dir(rng),
        };
        let neutral = match rng.below(10) {
            0 => 0.0,
            1 => rng.uniform(-1e-4, 1e-4),
            2 => rng.uniform(-1.0, -0.1),
            3 => rng.uniform(20.0, 200.0),
            _ => rng.uniform(0.2, 5.0),
        };
        let tol = match rng.below(7) {
            0 => 0.0,
            1 => 1e-5,
            2 => -0.5,
            3 => rng.uniform(0.0, 0.5),
            4 => 1e-6,
            _ => 0.01,
        };
        Ctor {
            c: coord(rng, 5.0),
            axis,
            mid,
            neutral,
            tol,
        }
    });
    let next = rng.chance(0.3).then(|| {
        [
            coord(rng, 5.0),
            coord(rng, 2.0),
            coord(rng, 2.0),
            coord(rng, 2.0),
        ]
    });
    UnbendIn { ctor, next }
}

/// A parameter point: random, on or near the singular axis, on the discontinuity
/// half plane, or on the singularity-tolerance boundary.
fn gen_point(rng: &mut Rng) -> [f64; 3] {
    match rng.below(12) {
        0 => [0.0; 3],
        1 => [0.0, 0.0, rng.uniform(-3.0, 3.0)],
        2 => [rng.uniform(-3.0, 0.0), 0.0, rng.uniform(-3.0, 3.0)],
        3 => [
            rng.uniform(-1e-2, 1e-2),
            rng.uniform(-1e-2, 1e-2),
            rng.uniform(-3.0, 3.0),
        ],
        4 => [
            rng.uniform(-3.0, 0.0),
            rng.uniform(-1e-2, 1e-2),
            rng.uniform(-3.0, 3.0),
        ],
        5 => [0.01, 0.0, 1.0],
        _ => coord(rng, 6.0),
    }
}

fn gen_box(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    const INF: f64 = 1_234_567.0;
    if rng.chance(0.08) {
        return ([-INF; 3], [INF; 3]);
    }
    let mut a = coord(rng, 4.0);
    let mut b = coord(rng, 4.0);
    for (x, y) in a.iter_mut().zip(b.iter_mut()) {
        if *x > *y && rng.chance(0.95) {
            core::mem::swap(x, y);
        }
    }
    match rng.below(10) {
        // away from the axis, in the u > 0 half space
        0..=2 => {
            a[0] = rng.uniform(0.1, 2.0);
            b[0] = a[0] + rng.uniform(0.1, 3.0);
        }
        // straddling the discontinuity half plane
        3 => {
            a[0] = rng.uniform(-4.0, -1.0);
            b[0] = rng.uniform(-0.9, -0.1);
            a[1] = rng.uniform(-2.0, -0.1);
            b[1] = rng.uniform(0.1, 2.0);
        }
        // a flat range (no edge lines possible)
        4 => {
            let k = rng.below(3) as usize;
            if let (Some(x), Some(y)) = (a.get(k).copied(), b.get_mut(k)) {
                *y = x;
            }
        }
        // one unbounded direction
        5 => {
            let k = rng.below(3) as usize;
            if let (Some(x), Some(y)) = (a.get_mut(k), b.get_mut(k)) {
                *x = -INF;
                *y = INF;
            }
        }
        // corners on the axis
        6 => {
            a[0] = 0.0;
            a[1] = 0.0;
        }
        _ => {}
    }
    (a, b)
}

/// A parameter segment: random, crossing the discontinuity half plane, through or
/// along the axis, crossing `v = 0` at positive `u`, or degenerate.
fn gen_line(rng: &mut Rng) -> ([f64; 3], [f64; 3]) {
    let z = |rng: &mut Rng| rng.uniform(-3.0, 3.0);
    match rng.below(10) {
        0 => {
            let x = rng.uniform(-3.0, -0.1);
            (
                [x, rng.uniform(-2.0, -0.1), z(rng)],
                [x, rng.uniform(0.1, 2.0), z(rng)],
            )
        }
        1 => {
            let x = rng.uniform(0.1, 3.0);
            (
                [x, rng.uniform(-2.0, -0.1), z(rng)],
                [x, rng.uniform(0.1, 2.0), z(rng)],
            )
        }
        2 => {
            let p = coord(rng, 3.0);
            (p, p.map(|c| -c))
        }
        3 => {
            let r = rng.uniform(0.0, 0.02);
            ([r, 0.0, z(rng)], [r, 0.0, z(rng)])
        }
        4 => {
            let p = coord(rng, 3.0);
            (p, p)
        }
        5 => ([rng.uniform(-3.0, 0.0), 0.0, z(rng)], coord(rng, 3.0)),
        _ => (coord(rng, 5.0), coord(rng, 5.0)),
    }
}

fn gen_n(rng: &mut Rng) -> usize {
    if rng.chance(0.05) {
        4 + rng.below(2) as usize
    } else {
        rng.below(4) as usize
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let unbend = gen_unbend(rng);
    let p = gen_point(rng);
    let dom = |rng: &mut Rng| {
        rng.chance(0.6).then(|| {
            let (a, b) = gen_box(rng);
            [a, b]
        })
    };
    match rng.below(24) {
        0 => Inputs::Info {
            unbend,
            types: vec![22010, 22001, 22008, 22011, 22006, 1, 11001],
        },
        1..=4 => Inputs::Eval {
            unbend,
            p,
            n: gen_n(rng),
            comp: rng.chance(0.8),
        },
        5..=7 => Inputs::EvalSimple {
            unbend,
            p,
            n: gen_n(rng),
        },
        8..=10 => {
            let (a, b) = gen_box(rng);
            let want_box = rng.chance(0.8);
            Inputs::BBox {
                unbend,
                a,
                b,
                want_box,
                want_pseudo: !want_box || rng.chance(0.5),
            }
        }
        11 => {
            let (a, b) = gen_box(rng);
            Inputs::Trim { unbend, a, b }
        }
        12 => Inputs::InvGuess {
            unbend,
            p: coord(rng, 6.0),
        },
        13 | 14 => Inputs::Global {
            unbend,
            p,
            dom: dom(rng),
        },
        15 => Inputs::Local {
            unbend,
            p,
            g: coord(rng, 3.0),
            dom: dom(rng),
        },
        16 | 17 => Inputs::OnBoundary {
            unbend,
            p,
            tol: match rng.below(6) {
                0 => None,
                1 => Some(0.0),
                2 => Some(-0.5),
                3 => Some(1.0),
                _ => Some(rng.uniform(0.0, 0.1)),
            },
        },
        18 | 19 => {
            let (a, b) = gen_line(rng);
            Inputs::Preds { unbend, p, a, b }
        }
        20 => {
            let (a, b) = gen_line(rng);
            Inputs::CurveIn { unbend, a, b }
        }
        21 => Inputs::Copy {
            unbend,
            simple: rng.chance(0.5),
        },
        _ => {
            let other = if rng.chance(0.4) {
                unbend.clone()
            } else if rng.chance(0.5) {
                // same frame and tolerance, other neutral distance
                let mut o = unbend.clone();
                if let Some(c) = o.ctor.as_mut() {
                    c.neutral += rng.uniform(-1.0, 1.0);
                }
                o
            } else {
                gen_unbend(rng)
            };
            Inputs::Equal {
                unbend,
                other,
                self_cmp: rng.chance(0.1),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.unbend_volume"
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
