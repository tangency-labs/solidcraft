//! Runner for unit `surface.twist_volume`: the twist volume `SmTwistVolume`
//! (construction and repair, evaluation with and without compounding, bounding
//! boxes, the exact inverse and point solves, domain predicates, copies, equality,
//! database reads).
//!
//! Ported from upstream `source/SMLib/src/SmTwistVolume.cpp`; checked against its golden.
//! Rust: `solidcore::surface::twist_volume`
//! through the [`Volume`] dispatch of `solidcore::surface::volume`.
//!
//! A case is a twist (`twist`: optional component-constructor arguments `ctor`, the
//! default constructor without; optional `next` transform `[origin, X, Y, Z]`
//! appended as an owned next map) and an `op` with its parameters. The Rust side produces
//! a flat record; the runner reports its first entry
//! as the `status` int (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest as
//! `out`.
//!
//! `read_db` reads the members and the base-class data (`SmVolume::ReadFromDB`, unit
//! `surface.volume_p2`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::kernel::Kernel;
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::line::Line;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::volume::{EvalOptions, Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::cases::contract;
use crate::runner::default_cases;
use crate::util::v3;
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
    o: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    period: f64,
    min: f64,
}

/// A twist volume: `ctor` (the default constructor when absent) and an optional next
/// transform `[origin, X, Y, Z]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TwistIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ctor: Option<Ctor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next: Option<[[f64; 3]; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Info {
        twist: TwistIn,
        types: Vec<i64>,
    },
    Eval {
        twist: TwistIn,
        p: [f64; 3],
        n: usize,
        comp: bool,
    },
    EvalSimple {
        twist: TwistIn,
        p: [f64; 3],
        n: usize,
    },
    BBox {
        twist: TwistIn,
        a: [f64; 3],
        b: [f64; 3],
        want_box: bool,
        want_pseudo: bool,
    },
    InvGuess {
        twist: TwistIn,
        p: [f64; 3],
    },
    Global {
        twist: TwistIn,
        p: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<[[f64; 3]; 2]>,
    },
    Local {
        twist: TwistIn,
        p: [f64; 3],
        g: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dom: Option<[[f64; 3]; 2]>,
    },
    Preds {
        twist: TwistIn,
        p: [f64; 3],
        a: [f64; 3],
        b: [f64; 3],
    },
    Copy {
        twist: TwistIn,
        simple: bool,
    },
    Equal {
        twist: TwistIn,
        other: TwistIn,
        self_cmp: bool,
    },
    ReadDb {
        data: String,
    },
}

// ---- C++ side -----------------------------------------------------------------------

fn fl(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn to_output(flat: &[f64]) -> Output {
    let status = flat.first().copied().unwrap_or(f64::NAN);
    let rest = flat.get(1..).unwrap_or(&[]).to_vec();
    Output::new()
        .ints("status", vec![status as i64])
        .values("out", rest)
}

// ---- Rust side ----------------------------------------------------------------------

fn transform_of(t: [[f64; 3]; 4]) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build(t: &TwistIn) -> Result<Volume, SolidError> {
    let mut vol = match &t.ctor {
        Some(c) => TwistVolume::create(v3(c.o), v3(c.axis), v3(c.mid), c.period, c.min),
        None => Volume::new(VolumeKind::Twist(TwistVolume::new())),
    };
    if let Some(n) = t.next {
        vol.add_next_map(Volume::new(VolumeKind::Transform(transform_of(n))), 2)?;
    }
    Ok(vol)
}

fn twist_of(vol: &Volume) -> Result<&TwistVolume, HarnessError> {
    match vol.kind() {
        VolumeKind::Twist(t) => Ok(t),
        _ => Err(harness("not a twist volume")),
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
    let t = twist_of(v)?;
    o.push(v.type_code() as f64);
    o.push(t.twist_period_dist());
    o.push(t.period_min_dist());
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::upstream_status(e).is_some() => {
            f64::from(solidcore::error::upstream_status(e).unwrap_or(-1))
        }
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

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    let twist_in = match i {
        Inputs::Info { twist, .. }
        | Inputs::Eval { twist, .. }
        | Inputs::EvalSimple { twist, .. }
        | Inputs::BBox { twist, .. }
        | Inputs::InvGuess { twist, .. }
        | Inputs::Global { twist, .. }
        | Inputs::Local { twist, .. }
        | Inputs::Preds { twist, .. }
        | Inputs::Copy { twist, .. }
        | Inputs::Equal { twist, .. } => Some(twist),
        Inputs::ReadDb { .. } => None,
    };
    let vol = match twist_in {
        Some(t) => Some(tri!(o, build(t))),
        None => None,
    };
    let vol = vol.as_ref();
    let need = || vol.ok_or_else(|| harness("op needs a twist"));
    match i {
        Inputs::Info { types, .. } => {
            let v = need()?;
            o.push(0.0);
            put_state(&mut o, v)?;
            o.push(v.type_code() as f64);
            str_codes(&mut o, v.type_string());
            str_codes(&mut o, v.class_string());
            o.extend(types.iter().map(|&t| fl(v.is_kind_of(t))));
            pv(&mut o, v.twist_origin());
            pv(&mut o, v.twist_axis());
            pv(&mut o, v.twist_mid_dir());
            pv(&mut o, v.twist_binormal());
            let dom = tri!(o, v.natural_param_domain());
            let bounded = tri!(o, v.is_bounded_simple());
            put_box(&mut o, &dom);
            o.push(fl(bounded));
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
            let t = twist_of(need()?)?;
            let param_box = ext(&[*a, *b]);
            let mut proj = Extent3d::new();
            let mut pseudo = PseudoBox::new();
            tri!(
                o,
                t.evaluate_bounding_box_simple(
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
        Inputs::InvGuess { p, .. } => {
            let g = tri!(o, need()?.inv_evaluate_guess_point_simple(v3(*p)));
            o.push(0.0);
            o.push(g.len() as f64);
            for x in g {
                pv(&mut o, x);
            }
        }
        Inputs::Global { p, dom, .. } => {
            let t = twist_of(need()?)?;
            let mut sols = SolutionArray::new();
            let d = dom.as_ref().map(ext);
            let found = t.global_point_solve_simple(v3(*p), &mut sols, d.as_ref(), Me);
            o.push(0.0);
            o.push(fl(found));
            o.push(sols.len() as f64);
            for s in sols.as_slice() {
                put_solution(&mut o, s);
            }
        }
        Inputs::Local { p, dom, .. } => {
            let t = twist_of(need()?)?;
            let mut sol = Solution::new();
            let d = dom.as_ref().map(ext);
            let found = t.local_point_solve_simple(v3(*p), &mut sol, d.as_ref(), Me);
            o.push(0.0);
            o.push(fl(found));
            put_solution(&mut o, &sol);
        }
        Inputs::Preds { p, a, b, .. } => {
            let v = need()?;
            let line = Curve::from(Line::from_points(v3(*a), v3(*b), 3));
            let in_point = tri!(o, v.is_point_in_param_domain_simple(v3(*p)));
            let in_line = tri!(o, v.is_line_in_param_domain_simple(&Kernel::default(), v3(*a), v3(*b)));
            let in_curve = tri!(o, v.is_curve_in_param_domain_simple(&Kernel::default(), &line));
            let bounded = tri!(o, v.is_bounded_simple());
            o.push(0.0);
            o.extend([fl(in_point), fl(in_line), fl(in_curve), fl(bounded)]);
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
            let v = tri!(o, TwistVolume::read_from_db(&mut db, DB_VERSION));
            o.push(0.0);
            put_state(&mut o, &v)?;
        }
    }
    Ok(to_output(&o))
}

// ---- input contract -----------------------------------------------------------------

/// The largest derivative order.
const MAX_N: usize = 5;
/// The largest magnitude of a value.
const MAX_COORD: f64 = 1e8;
/// The longest `read_db` stream.
const MAX_BYTES: usize = 4096;
/// A box whose every corner coordinate is at least this large is unbounded.
const UNBOUNDED_FROM: f64 = 1111.0;

fn fin(v: f64) -> bool {
    contract::finite(v, MAX_COORD)
}

fn vecs(vs: &[[f64; 3]]) -> bool {
    vs.iter().all(|v| contract::vec3(v, MAX_COORD))
}

/// Whether a twist volume's constructor arguments and next map are finite within range.
fn twist_ok(t: &TwistIn) -> bool {
    t.ctor
        .as_ref()
        .is_none_or(|c| vecs(&[c.o, c.axis, c.mid]) && fin(c.period) && fin(c.min))
        && t.next.as_ref().is_none_or(|n| vecs(n))
}

/// Whether a case keeps the input contract ported from the reference's wrapper: finite
/// values within [`MAX_COORD`], up to 64 type codes in `[0, 2^30]`, derivative orders up to
/// [`MAX_N`], no `b_box` asking for the pseudo box alone of an unbounded box (upstream
/// writes the missing box through a null pointer) and streams up to [`MAX_BYTES`].
fn keeps_contract(i: &Inputs) -> bool {
    match i {
        Inputs::Info { twist, types } => {
            twist_ok(twist) && types.len() <= 64 && types.iter().all(|t| (0..=1 << 30).contains(t))
        }
        Inputs::Eval { twist, p, n, .. } | Inputs::EvalSimple { twist, p, n } => {
            twist_ok(twist) && vecs(&[*p]) && *n <= MAX_N
        }
        Inputs::BBox {
            twist,
            a,
            b,
            want_box,
            want_pseudo,
        } => {
            let unbounded = a.iter().chain(b).all(|c| c.abs() >= UNBOUNDED_FROM);
            twist_ok(twist) && vecs(&[*a, *b]) && !(*want_pseudo && !*want_box && unbounded)
        }
        Inputs::InvGuess { twist, p } => twist_ok(twist) && vecs(&[*p]),
        Inputs::Global { twist, p, dom } => {
            twist_ok(twist) && vecs(&[*p]) && dom.as_ref().is_none_or(|d| vecs(d))
        }
        Inputs::Local { twist, p, g, dom } => {
            twist_ok(twist) && vecs(&[*p, *g]) && dom.as_ref().is_none_or(|d| vecs(d))
        }
        Inputs::Preds { twist, p, a, b } => twist_ok(twist) && vecs(&[*p, *a, *b]),
        Inputs::Copy { twist, .. } => twist_ok(twist),
        Inputs::Equal { twist, other, .. } => twist_ok(twist) && twist_ok(other),
        Inputs::ReadDb { data } => data.len() <= MAX_BYTES,
    }
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

fn gen_twist(rng: &mut Rng) -> TwistIn {
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
        let period = match rng.below(10) {
            0 => 0.0,
            1 => rng.uniform(-1e-4, 1e-4),
            2 => rng.uniform(-1.0, -0.1),
            3 => rng.uniform(50.0, 500.0),
            _ => rng.uniform(-10.0, 10.0),
        };
        let min = match rng.below(6) {
            0 => 0.0,
            1 => 1e-5,
            2 => -0.5,
            3 => rng.uniform(0.0, 2.0),
            _ => 1e-3,
        };
        Ctor {
            o: coord(rng, 5.0),
            axis,
            mid,
            period,
            min,
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
    TwistIn { ctor, next }
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
        // a long w range: more than a turn for most periods
        0 => {
            a[2] = rng.uniform(-60.0, 0.0);
            b[2] = rng.uniform(0.0, 60.0);
        }
        // a flat w range
        1 => b[2] = a[2],
        // one unbounded direction
        2 => {
            let k = rng.below(3) as usize;
            if let (Some(x), Some(y)) = (a.get_mut(k), b.get_mut(k)) {
                *x = -INF;
                *y = INF;
            }
        }
        // corners on the axes
        3 => {
            a[0] = 0.0;
            a[1] = 0.0;
        }
        _ => {}
    }
    (a, b)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let twist = gen_twist(rng);
    let p = if rng.chance(0.1) {
        [0.0; 3]
    } else {
        coord(rng, 6.0)
    };
    let dom = |rng: &mut Rng| {
        rng.chance(0.6).then(|| {
            let (a, b) = gen_box(rng);
            [a, b]
        })
    };
    match rng.below(20) {
        0 => Inputs::Info {
            twist,
            types: vec![22011, 22001, 22008, 22010, 1, 11001],
        },
        1..=4 => Inputs::Eval {
            twist,
            p,
            n: if rng.chance(0.05) {
                4 + rng.below(2) as usize
            } else {
                rng.below(4) as usize
            },
            comp: rng.chance(0.8),
        },
        5..=7 => Inputs::EvalSimple {
            twist,
            p,
            n: if rng.chance(0.05) {
                4 + rng.below(2) as usize
            } else {
                rng.below(4) as usize
            },
        },
        8..=11 => {
            let (a, b) = gen_box(rng);
            let want_box = rng.chance(0.8);
            Inputs::BBox {
                twist,
                a,
                b,
                want_box,
                want_pseudo: !want_box || rng.chance(0.5),
            }
        }
        12 => Inputs::InvGuess { twist, p },
        13 | 14 => Inputs::Global {
            twist,
            p,
            dom: dom(rng),
        },
        15 => Inputs::Local {
            twist,
            p,
            g: coord(rng, 3.0),
            dom: dom(rng),
        },
        16 => Inputs::Preds {
            twist,
            p,
            a: coord(rng, 5.0),
            b: coord(rng, 5.0),
        },
        17 => Inputs::Copy {
            twist,
            simple: rng.chance(0.5),
        },
        _ => {
            let other = if rng.chance(0.4) {
                twist.clone()
            } else if rng.chance(0.5) {
                // same frame and minimum, other period
                let mut o = twist.clone();
                if let Some(c) = o.ctor.as_mut() {
                    c.period += rng.uniform(-1.0, 1.0);
                }
                o
            } else {
                gen_twist(rng)
            };
            Inputs::Equal {
                twist,
                other,
                self_cmp: rng.chance(0.1),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.twist_volume"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            if keeps_contract(&i) {
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
