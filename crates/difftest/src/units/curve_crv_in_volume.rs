//! Runner for unit `curve.crv_in_volume`: `SmCrvInVolume`, a curve mapped through a
//! volume.
//!
//! C++: `solidcore_ffi::units::curve_crv_in_volume` (upstream
//! `source/SMLib/src/SmCrvInVolume.cpp` on objects built from a B-spline or Hermite base
//! curve and an identity `SmTransform` volume; the break cache is loaded directly).
//! Rust: `solidcore::curve::crv_in_volume` (through the [`Curve`] dispatch where the
//! method is virtual upstream).
//!
//! A case is an optional object (`civ`: `curve` as in `curve.curve_p2`, `in_param`,
//! `copy`, `owner`, optional `cache` `{need, breaks, conts}`) and an `op` with its
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` int (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as `out`.
//!
//! The `clone` op runs the native copy constructor (`SmCrvInVolume(const
//! SmCrvInVolume&)`, shim op `CopyCtor`, not the virtual `Copy`) against the port's
//! `Curve::clone`: the object is moved into a [`Curve`], the kind's `Clone` dispatches
//! through `CurveKind` to `CrvInVolume`'s copy-constructor clone, and only the clone is
//! examined (the original is only compared with). Its record is `copy`'s (the copy's
//! state, then its `operator==` with the original). Upstream's copy constructor shares a
//! base curve the owner flag does not own and copies one it does; it normalizes the owner
//! flag to 0/1/2/3 and copies the break cache only when that cache is current. A shared
//! base keeps its raw knots, so the copy's `operator==` with the original is the sharp
//! comparison: a copy-constructor knot fix-up moves knots by ~1e-12, below the harness's
//! 1e-9 value tolerance.
//! `exact_volume_*` cases run `MakeExactBSplineIfPossible` on an object built from a
//! B-spline, Hermite, line or circle base curve and a chain of transform, bend, unbend
//! and twist maps (built in C++ and Rust from the same parameters), with the base curve
//! aligned with the head map's frame so the analytic branches of
//! `MakeExactBSpline1stOutCurve` run; the record is the exact curve's type, NURB,
//! natural interval and sampled points and tangents.
//!
//! Deferred cases: what reaches the unported parts: the curve classification of
//! uncached breaks (`intersect.curve_class_*`) and the unported volume kinds. Every
//! run has at least [`MIN_GENERATED`]
//! generated cases; generated inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_in_volume::{continuity_from_code, CrvInVolume};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::surface::bend_volume::BendVolume;
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::unbend_volume::UnbendVolume;
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;
use solidcore_ffi::units::curve_crv_in_volume::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `curve.crv_in_volume` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
    /// Knots written back over the curve's own NURB after construction, undoing the
    /// constructor's knot fix-up (curve block kind `2`). Omitted when false, so
    /// existing case identities are unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    raw_knots: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
}

/// A base curve of an `Exact` case: a curve block, a line or a circle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ExactCurveIn {
    Curve(CurveIn),
    /// `SmLine(p1, p2, 3)`.
    Line { p1: [f64; 3], p2: [f64; 3] },
    /// `SmCircle(c, x, y, [a0, a1] (degrees), r, 3)`.
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        a0: f64,
        a1: f64,
        r: f64,
    },
}

/// One map of a volume chain (as in `surface.srf_in_volume`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MapIn {
    /// `0` plain `SmVolume`, `1` `SmTransform`, `2` default twist, `3` default unbend,
    /// `4` default bend, `5` / `6` / `7` the twist / unbend / bend component constructors.
    kind: u8,
    /// The constructor arguments of kinds `5` to `7`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comp: Option<CompIn>,
    /// The transform `[origin, X, Y, Z]` (kind `1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<[[f64; 3]; 4]>,
    /// The orient transform `[origin, X, Y, Z]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orient: Option<[[f64; 3]; 4]>,
}

/// The arguments of a component constructor: origin or center, axis, mid direction
/// and the two numbers (period and minimum, or neutral distance and singularity
/// tolerance).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompIn {
    p: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    a: f64,
    b: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheIn {
    need: bool,
    breaks: Vec<f64>,
    conts: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CivIn {
    curve: CurveIn,
    #[serde(default)]
    in_param: bool,
    #[serde(default)]
    copy: u64,
    #[serde(default)]
    owner: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache: Option<CacheIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        types: Vec<i64>,
    },
    Eval {
        t: f64,
        n: usize,
        left: bool,
        nonzero: bool,
    },
    EvalPoint {
        t: f64,
    },
    Knots {
        ivl: Option<[f64; 2]>,
        want: bool,
    },
    Mults {
        knots: Vec<f64>,
    },
    Nnk,
    Conts {
        angle: f64,
    },
    Equal {
        other: Option<CivIn>,
    },
    Copy,
    Clone,
    EditParam {
        a: f64,
        b: f64,
    },
    Reverse {
        a: f64,
        b: f64,
    },
    Trim {
        a: f64,
        b: f64,
        mode: i64,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    ReadDb {
        version: i64,
        text: String,
    },
    Exact {
        curve: ExactCurveIn,
        in_param: bool,
        volume: Vec<MapIn>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    civ: Option<CivIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            let kind = if bs.raw_knots { 2.0 } else { 0.0 };
            v.extend([kind, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
    }
}

fn civ_block(v: &mut Vec<f64>, c: &CivIn) {
    curve_block(v, &c.curve);
    v.extend([b(c.in_param), c.copy as f64, c.owner as f64]);
    match &c.cache {
        None => v.push(0.0),
        Some(k) => {
            v.extend([1.0, b(k.need), k.breaks.len() as f64]);
            v.extend_from_slice(&k.breaks);
            v.push(k.conts.len() as f64);
            v.extend(k.conts.iter().map(|&c| c as f64));
        }
    }
}

fn vol_block(v: &mut Vec<f64>, vol: &[MapIn]) {
    v.push(vol.len() as f64);
    for m in vol {
        v.push(f64::from(m.kind));
        if m.kind == 1 {
            for r in m.t.unwrap_or_default() {
                v.extend_from_slice(&r);
            }
        }
        if m.kind >= 5 {
            if let Some(c) = &m.comp {
                v.extend_from_slice(&c.p);
                v.extend_from_slice(&c.axis);
                v.extend_from_slice(&c.mid);
                v.extend([c.a, c.b]);
            }
        }
        match &m.orient {
            None => v.push(0.0),
            Some(o) => {
                v.push(1.0);
                for r in o {
                    v.extend_from_slice(r);
                }
            }
        }
    }
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Info { .. } => Op::Info,
        OpIn::Eval { .. } => Op::Eval,
        OpIn::EvalPoint { .. } => Op::EvalPoint,
        OpIn::Knots { .. } => Op::Knots,
        OpIn::Mults { .. } => Op::Mults,
        OpIn::Nnk => Op::Nnk,
        OpIn::Conts { .. } => Op::Conts,
        OpIn::Equal { .. } => Op::Equal,
        OpIn::Copy => Op::Copy,
        OpIn::Clone => Op::CopyCtor,
        OpIn::EditParam { .. } => Op::EditParam,
        OpIn::Reverse { .. } => Op::Reverse,
        OpIn::Trim { .. } => Op::Trim,
        OpIn::Transform { .. } => Op::Transform,
        OpIn::ReadDb { .. } => Op::ReadDb,
        OpIn::Exact { .. } => Op::Exact,
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op);
    let mut v = Vec::new();
    if !op.is_static() {
        let c = i
            .civ
            .as_ref()
            .ok_or_else(|| harness("object op without civ"))?;
        civ_block(&mut v, c);
    }
    match &i.op {
        OpIn::Info { types } => {
            v.push(types.len() as f64);
            v.extend(types.iter().map(|&t| t as f64));
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => v.extend([*t, *n as f64, b(*left), b(*nonzero)]),
        OpIn::EvalPoint { t } => v.push(*t),
        OpIn::Knots { ivl, want } => {
            let [x, y] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(ivl.is_some()), x, y, b(*want)]);
        }
        OpIn::Mults { knots } => {
            v.push(knots.len() as f64);
            v.extend_from_slice(knots);
        }
        OpIn::Nnk | OpIn::Copy | OpIn::Clone => {}
        OpIn::Conts { angle } => v.push(*angle),
        OpIn::Equal { other } => match other {
            None => v.push(1.0),
            Some(o) => {
                v.push(0.0);
                civ_block(&mut v, o);
            }
        },
        OpIn::EditParam { a, b: bb } | OpIn::Reverse { a, b: bb } => v.extend([*a, *bb]),
        OpIn::Trim { a, b: bb, mode } => v.extend([*a, *bb, *mode as f64]),
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.push(b(scale.is_some()));
            v.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::ReadDb { version, text } => {
            v.push(*version as f64);
            v.push(text.len() as f64);
            v.extend(text.bytes().map(f64::from));
        }
        OpIn::Exact {
            curve,
            in_param,
            volume,
        } => {
            match curve {
                ExactCurveIn::Curve(c) => curve_block(&mut v, c),
                ExactCurveIn::Line { p1, p2 } => {
                    v.push(2.0);
                    v.extend_from_slice(p1);
                    v.extend_from_slice(p2);
                }
                ExactCurveIn::Circle {
                    c,
                    x,
                    y,
                    a0,
                    a1,
                    r,
                } => {
                    v.push(3.0);
                    for p in [c, x, y] {
                        v.extend_from_slice(p);
                    }
                    v.extend([*a0, *a1, *r]);
                }
            }
            v.push(b(*in_param));
            vol_block(&mut v, volume);
        }
    }
    Ok((op, v))
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

// ---- Rust side --------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                bs.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            if bs.raw_knots {
                // undo the constructor's fix-up, as the shim does
                c.get_or_create_gw_nurb()
                    .map_err(|e| harness(format!("curve: {e}")))?
                    .knots_mut()
                    .copy_from_slice(&bs.u);
            }
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
    }
}

fn build(c: &CivIn) -> Result<CrvInVolume, HarnessError> {
    let mut civ = CrvInVolume::new(
        build_curve(&c.curve)?,
        c.in_param,
        Volume::new(VolumeKind::Transform(Transform::IDENTITY)),
        c.copy,
        c.owner,
    )
    .map_err(|e| HarnessError::new(e.to_string()))?;
    if let Some(k) = &c.cache {
        let conts = k
            .conts
            .iter()
            .map(|&x| continuity_from_code(x).ok_or_else(|| harness("bad continuity code")))
            .collect::<Result<Vec<_>, _>>()?;
        civ.set_breaks_cache(k.need, k.breaks.clone(), conts);
    }
    Ok(civ)
}

fn civ_of(c: &Curve) -> Result<&CrvInVolume, HarnessError> {
    match c.kind() {
        CurveKind::CrvInVolume(x) => Ok(x),
        _ => Err(harness("not a curve in volume")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    match c.kind() {
        CurveKind::BSpline(bs) => {
            o.extend([0.0, bs.dim() as f64]);
            match bs.gw_nurb() {
                None => o.push(-1.0),
                Some(cur) => {
                    o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
                    for p in cur.cpts() {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                    o.extend_from_slice(cur.knots());
                }
            }
        }
        CurveKind::Hermite(h) => {
            o.extend([1.0, h.dim() as f64]);
            let a = h.evaluate(0.0, 1, false);
            let e = h.evaluate(1.0, 1, false);
            for v in a.iter().take(2).chain(e.iter().take(2)) {
                pv(o, *v);
            }
        }
        // other kinds never are the base curve here
        _ => o.push(-1.0),
    }
}

fn put_cache(o: &mut Vec<f64>, c: &CrvInVolume) {
    o.push(b(c.need_breaks()));
    o.push(c.breaks().len() as f64);
    o.extend_from_slice(c.breaks());
    o.push(c.conts().len() as f64);
    o.extend(c.conts().iter().map(|&k| f64::from(k as i32)));
}

fn put_state(o: &mut Vec<f64>, c: &CrvInVolume) {
    o.push(c.owner_flag() as f64);
    o.push(b(c.in_param_space()));
    put_cache(o, c);
    put_curve(o, c.curve());
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

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    if let OpIn::ReadDb { version, text } = &i.op {
        let mut db = DatabaseReader::with_stream(
            FileType::Ascii,
            InputStream::new(text.as_bytes().to_vec()),
        );
        let c = match CrvInVolume::read_from_db(&Kernel::default(), &mut db, *version as u64) {
            Ok(c) => c,
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                // status, not made
                o.extend([status(&e), 0.0]);
                return Ok(to_output(&o));
            }
        };
        o.extend([0.0, 1.0]);
        put_state(&mut o, &c);
        return Ok(to_output(&o));
    }
    if let OpIn::Exact {
        curve,
        in_param,
        volume,
    } = &i.op
    {
        return rust_exact(curve, *in_param, volume);
    }
    let civ_in = i.civ.as_ref().ok_or_else(|| harness("object op without civ"))?;
    let mut civ = build(civ_in)?;
    let k = Kernel::default();
    match &i.op {
        OpIn::Info { types } => {
            let c = Curve::from(civ);
            let deg = tri!(o, c.degree());
            let ncp = tri!(o, c.number_control_points());
            let ivl = c.natural_interval();
            let x = civ_of(&c)?;
            o.push(0.0);
            o.extend([
                c.type_code() as f64,
                c.dim() as f64,
                deg as f64,
                ncp as f64,
                ivl.min(),
                ivl.max(),
                b(c.is_analytic()),
                b(c.is_bounded()),
                b(x.owns_curve()),
                b(x.owns_volume()),
            ]);
            o.extend(types.iter().map(|&t| b(c.is_kind_of(t))));
            put_state(&mut o, x);
        }
        OpIn::Eval {
            t,
            n,
            left,
            nonzero,
        } => {
            let c = Curve::from(civ);
            let v = tri!(o, c.evaluate(&k, *t, *n, *left, *nonzero));
            o.push(0.0);
            for p in v {
                pv(&mut o, p);
            }
        }
        OpIn::EvalPoint { t } => {
            let c = Curve::from(civ);
            let p = tri!(o, c.evaluate_point(&k, *t));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Knots { ivl, want } => {
            let iv = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            // the status, then what GetKnots wrote (also on a failing status)
            let w = tri!(o, civ.knots_with_status(&k, iv.as_ref()));
            match st_of(&w.status) {
                Ok(s) => o.push(s),
                Err(d) => return Ok(*d),
            }
            let (kn, m) = (w.knots, w.mults);
            o.push(kn.len() as f64);
            o.extend_from_slice(&kn);
            if *want {
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
            }
            put_cache(&mut o, &civ);
        }
        OpIn::Mults { knots } => {
            let m = tri!(o, civ.generate_mults(knots));
            o.push(0.0);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::Nnk => {
            let n = tri!(o, civ.number_natural_knots(&k));
            o.extend([0.0, n as f64]);
            put_cache(&mut o, &civ);
        }
        OpIn::Conts { angle } => {
            let (mn, cs) = tri!(o, civ.calculate_continuities(&k, *angle));
            o.extend([0.0, f64::from(mn as i32), cs.len() as f64]);
            o.extend(cs.iter().map(|&x| f64::from(x as i32)));
            put_cache(&mut o, &civ);
        }
        OpIn::Equal { other } => {
            let c = Curve::from(civ);
            let eq = match other {
                None => tri!(o, c.approx_eq(&k, &c)),
                Some(oc) => {
                    let d = Curve::from(build(oc)?);
                    tri!(o, c.approx_eq(&k, &d))
                }
            };
            o.extend([0.0, b(eq)]);
        }
        OpIn::Copy => {
            let c = Curve::from(civ);
            let d = tri!(o, c.copy(&Kernel::default()));
            o.push(0.0);
            put_state(&mut o, civ_of(&d)?);
            let eq = tri!(o, d.approx_eq(&k, &c));
            o.push(b(eq));
        }
        OpIn::Clone => {
            // the native copy constructor against the port's derived-dispatch
            // `Curve::clone` (the original is moved into a `Curve`; only the clone is
            // examined, so both sides start from the same object)
            let c = Curve::from(civ);
            let d = c.clone();
            o.push(0.0);
            put_state(&mut o, civ_of(&d)?);
            let eq = tri!(o, d.approx_eq(&k, &c));
            o.push(b(eq));
        }
        OpIn::EditParam { a, b: bb } => {
            let r = civ.edit_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &civ);
        }
        OpIn::Reverse { a, b: bb } => {
            let r =
                civ.reverse_parameterization(&Kernel::default(), &Extent1d::from_min_max(*a, *bb));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            if let Ok(nv) = r {
                o.extend([nv.min(), nv.max()]);
            }
            put_state(&mut o, &civ);
        }
        OpIn::Trim { a, b: bb, mode } => {
            let mode = match mode {
                0 => TrimMode::Notify,
                1 => TrimMode::Silent,
                _ => TrimMode::IntervalOnly,
            };
            let mut ivl = Extent1d::from_min_max(*a, *bb);
            let r = civ.trim(&k, &mut ivl, mode);
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            o.extend([ivl.min(), ivl.max()]);
            put_state(&mut o, &civ);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let r = civ.transform(&ax, scale.map(v3));
            o.push(match st_of(&r) {
                Ok(s) => s,
                Err(d) => return Ok(*d),
            });
            put_state(&mut o, &civ);
        }
        OpIn::ReadDb { .. } | OpIn::Exact { .. } => return Err(harness("static op")),
    }
    Ok(to_output(&o))
}

// ---- generation -------------------------------------------------------------------

fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational, 2D or with repeated interior knots).
fn gen_bspline(rng: &mut Rng) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(6) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.2);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.1, 10.0)
    };
    let mut u = vec![a; p + 1];
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    if inner >= 2 && p >= 2 && rng.chance(0.3) {
        if let Some(&v) = ks.first() {
            if let Some(x) = ks.get_mut(1) {
                *x = v;
            }
        }
    }
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let scale = if rng.chance(0.5) { 5.0 } else { 100.0 };
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.2;
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let z = if two_d { NOZ } else { z };
            if rational {
                [x * w, y * w, if two_d { z } else { z * w }, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    let dim = if two_d || rng.chance(0.15) { 2 } else { 3 };
    BSplineIn {
        dim,
        oob: rng.chance(0.15),
        cpts,
        p,
        u,
        raw_knots: false,
    }
}

fn gen_hermite(rng: &mut Rng) -> HermiteIn {
    let s = if rng.chance(0.5) { 3.0 } else { 50.0 };
    HermiteIn {
        dim: 3,
        p1: rp(rng, s),
        d1: rp(rng, s),
        p2: rp(rng, s),
        d2: rp(rng, s),
    }
}

fn gen_curve(rng: &mut Rng) -> CurveIn {
    if rng.chance(0.8) {
        CurveIn::Bspline(gen_bspline(rng))
    } else {
        CurveIn::Hermite(gen_hermite(rng))
    }
}

fn range_of(c: &CurveIn) -> (f64, f64) {
    match c {
        CurveIn::Bspline(bs) => (
            bs.u.first().copied().unwrap_or(0.0),
            bs.u.last().copied().unwrap_or(1.0),
        ),
        CurveIn::Hermite(_) => (0.0, 1.0),
    }
}

fn knots_of(c: &CurveIn) -> Vec<f64> {
    match c {
        CurveIn::Bspline(bs) => {
            let mut k = bs.u.clone();
            k.dedup();
            k
        }
        CurveIn::Hermite(_) => vec![0.0, 1.0],
    }
}

/// A parameter of `c`: interior, at a knot, at an end, or slightly outside.
fn gen_t(rng: &mut Rng, c: &CurveIn) -> f64 {
    let (a, bb) = range_of(c);
    match rng.below(10) {
        0 => a,
        1 => bb,
        2 => *rng.choose(&knots_of(c)).unwrap_or(&a),
        3 => a + (bb - a) * rng.uniform(-0.3, 1.3),
        _ => a + (bb - a) * rng.unit(),
    }
}

/// A random break cache: the curve knots plus a few extra breaks (sometimes mismatched
/// continuity counts, sometimes not current).
fn gen_cache(rng: &mut Rng, c: &CurveIn) -> CacheIn {
    let (a, bb) = range_of(c);
    let mut breaks = knots_of(c);
    for _ in 0..rng.below(4) {
        breaks.push(a + (bb - a) * rng.unit());
    }
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    if rng.chance(0.15) {
        // drop the first or last break
        if rng.chance(0.5) {
            breaks.pop();
        } else if !breaks.is_empty() {
            breaks.remove(0);
        }
    }
    if rng.chance(0.08) {
        breaks.truncate(rng.below(3) as usize);
    }
    let mut nc = breaks.len();
    if rng.chance(0.1) {
        nc = rng.below(breaks.len() as u64 + 2) as usize;
    }
    let conts = (0..nc)
        .map(|_| {
            if rng.chance(0.05) {
                0
            } else {
                1 + rng.below(13) as i64
            }
        })
        .collect();
    CacheIn {
        need: rng.chance(0.1),
        breaks,
        conts,
    }
}

fn gen_civ(rng: &mut Rng) -> CivIn {
    let curve = gen_curve(rng);
    let cache = if rng.chance(0.8) {
        Some(gen_cache(rng, &curve))
    } else {
        None
    };
    CivIn {
        in_param: rng.chance(0.5),
        copy: rng.below(4),
        owner: if rng.chance(0.85) {
            rng.below(4)
        } else {
            rng.below(8)
        },
        cache,
        curve,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let civ = gen_civ(rng);
    let c = &civ.curve;
    let (a, bb) = range_of(c);
    let op = match rng.below(26) {
        0 | 1 => OpIn::Info {
            types: vec![
                CrvInVolume::TYPE,
                14001,
                14033,
                14034,
                10001,
                rng.below(30000) as i64,
            ],
        },
        2 => OpIn::Eval {
            t: gen_t(rng, c),
            n: rng.below(6) as usize,
            left: rng.chance(0.5),
            nonzero: rng.chance(0.5),
        },
        3 => OpIn::EvalPoint { t: gen_t(rng, c) },
        4..=7 => {
            let ivl = if rng.chance(0.6) {
                let x = a + (bb - a) * rng.uniform(-0.2, 1.2);
                let y = a + (bb - a) * rng.uniform(-0.2, 1.2);
                Some([x.min(y), x.max(y)])
            } else {
                None
            };
            OpIn::Knots {
                ivl,
                want: rng.chance(0.7),
            }
        }
        8..=11 => {
            let mut knots: Vec<f64> = match &civ.cache {
                Some(k) if rng.chance(0.6) => k
                    .breaks
                    .iter()
                    .copied()
                    .filter(|_| rng.chance(0.7))
                    .collect(),
                _ => (0..rng.below(5)).map(|_| gen_t(rng, c)).collect(),
            };
            knots.sort_by(f64::total_cmp);
            if rng.chance(0.1) {
                knots.reverse();
            }
            OpIn::Mults { knots }
        }
        12 | 13 => OpIn::Nnk,
        14 | 15 => OpIn::Conts {
            angle: if rng.chance(0.5) {
                0.5
            } else {
                rng.uniform(0.0, 10.0)
            },
        },
        16 | 17 => OpIn::Equal {
            other: match rng.below(20) {
                0..=2 => None,
                3..=9 => {
                    let mut o = civ.clone();
                    o.in_param = !o.in_param;
                    Some(o)
                }
                10..=14 => Some(gen_civ(rng)),
                _ => Some(civ.clone()),
            },
        },
        18 | 19 => OpIn::Copy,
        20 => {
            let x = rng.uniform(-10.0, 10.0);
            OpIn::EditParam {
                a: x,
                b: x + rng.uniform(0.01, 20.0) * if rng.chance(0.1) { -1.0 } else { 1.0 },
            }
        }
        21 => OpIn::Reverse {
            a: a + (bb - a) * rng.uniform(0.0, 0.5),
            b: a + (bb - a) * rng.uniform(0.5, 1.0),
        },
        22 | 23 => {
            let x = a + (bb - a) * rng.uniform(-0.1, 0.6);
            let y = a + (bb - a) * rng.uniform(0.4, 1.1);
            OpIn::Trim {
                a: x,
                b: y,
                mode: rng.below(3) as i64,
            }
        }
        _ => {
            let identity = rng.chance(0.6);
            OpIn::Transform {
                origin: if identity { [0.0; 3] } else { rp(rng, 5.0) },
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                scale: if rng.chance(0.5) {
                    None
                } else if identity && rng.chance(0.5) {
                    Some([1.0, 1.0, 1.0])
                } else {
                    Some([2.0, 2.0, 2.0])
                },
            }
        }
    };
    Inputs { civ: Some(civ), op }
}

// ---- exact images through bend, unbend and twist volumes ----------------------------

fn transform_of(t: [[f64; 3]; 4]) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build_volume(v: &[MapIn]) -> Result<Volume, HarnessError> {
    let mut next: Option<Volume> = None;
    for m in v.iter().rev() {
        let kind = match m.kind {
            0 => VolumeKind::Base,
            1 => VolumeKind::Transform(transform_of(
                m.t.ok_or_else(|| harness("transform map without t"))?,
            )),
            2 => VolumeKind::Twist(TwistVolume::new()),
            3 => VolumeKind::Unbend(UnbendVolume::new()),
            4 => VolumeKind::Bend(BendVolume::new()),
            5..=7 => {
                // the component constructors build the volume with its own orient map
                let c = m
                    .comp
                    .as_ref()
                    .ok_or_else(|| harness("component map without comp"))?;
                let (p, axis, mid) = (v3(c.p), v3(c.axis), v3(c.mid));
                let mut vol = match m.kind {
                    5 => TwistVolume::create(p, axis, mid, c.a, c.b),
                    6 => UnbendVolume::create(p, axis, mid, c.a, c.b),
                    _ => BendVolume::create(p, axis, mid, c.a, c.b),
                };
                if let Some(n) = next.take() {
                    vol.add_next_map(n, 2)
                        .map_err(|e| harness(format!("chain: {e}")))?;
                }
                next = Some(vol);
                continue;
            }
            k => return Err(harness(format!("bad map kind {k}"))),
        };
        next = Some(
            Volume::with_maps(kind, m.orient.map(transform_of), next)
                .map_err(|e| HarnessError::new(e.to_string()))?,
        );
    }
    next.ok_or_else(|| harness("volume without maps"))
}

fn build_exact_curve(k: &Kernel, c: &ExactCurveIn) -> Result<Curve, HarnessError> {
    match c {
        ExactCurveIn::Curve(c) => build_curve(c),
        ExactCurveIn::Line { p1, p2 } => Ok(Curve::from(Line::from_points(v3(*p1), v3(*p2), 3))),
        ExactCurveIn::Circle {
            c,
            x,
            y,
            a0,
            a1,
            r,
        } => Ellipse::new_circle(
            k,
            v3(*c),
            v3(*x),
            v3(*y),
            Extent1d::from_min_max(*a0, *a1),
            *r,
            &EllipseOptions::default(),
        )
        .map(Curve::from)
        .map_err(|e| harness(format!("circle: {e}"))),
    }
}

/// The NURB `GetOrCreateGwNurbPointer` gives: `(had a NURB, the NURB)`; `None` for a
/// kind that is not an `SmBSplineCurve`.
fn exact_nurb(
    k: &Kernel,
    c: &Curve,
) -> solidcore::Result<Option<(bool, Option<solidcore::nurbs::crv_basic::Curve>)>> {
    Ok(match c.kind() {
        CurveKind::BSpline(bs) => Some((bs.gw_nurb().is_some(), bs.gw_nurb().cloned())),
        CurveKind::Line(l) => {
            let had = l.base().gw_nurb().is_some();
            Some((had, l.base_with_nurb()?.gw_nurb().cloned()))
        }
        CurveKind::Ellipse(e) => {
            let had = e.base().gw_nurb().is_some();
            if had {
                Some((true, e.base().gw_nurb().cloned()))
            } else {
                let mut e = e.clone();
                e.make_nurb(k)?;
                Some((false, e.base().gw_nurb().cloned()))
            }
        }
        _ => None,
    })
}

/// The exact record (see the ffi module docs).
fn put_exact_curve(o: &mut Vec<f64>, k: &Kernel, c: &Curve) -> solidcore::Result<()> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    match exact_nurb(k, c)? {
        Some((had, nurb)) => {
            o.push(b(had));
            match nurb {
                None => o.push(-1.0),
                Some(cur) => {
                    o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
                    for p in cur.cpts() {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                    o.extend_from_slice(cur.knots());
                }
            }
        }
        None => o.extend([0.0, -1.0]),
    }
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=4 {
        match c.evaluate(k, ivl.evaluate(0.25 * f64::from(i)), 1, true, true) {
            Ok(pn) => {
                o.push(0.0);
                for v in pn.iter().take(2) {
                    pv(o, *v);
                }
            }
            Err(e) => {
                if e.is_unported() {
                    return Err(e);
                }
                o.push(status(&e));
            }
        }
    }
    Ok(())
}

fn rust_exact(curve: &ExactCurveIn, in_param: bool, volume: &[MapIn]) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o: Vec<f64> = Vec::new();
    let civ = CrvInVolume::new(
        build_exact_curve(&k, curve)?,
        in_param,
        build_volume(volume)?,
        0,
        3,
    )
    .map_err(|e| HarnessError::new(e.to_string()))?;
    let exact = tri!(o, civ.make_exact_b_spline_if_possible_with(&k));
    o.push(0.0);
    o.push(b(exact.is_some()));
    if let Some(c) = exact {
        tri!(o, put_exact_curve(&mut o, &k, &c));
    }
    Ok(to_output(&o))
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(s: f64, a: [f64; 3]) -> [f64; 3] {
    [s * a[0], s * a[1], s * a[2]]
}

fn unit3(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    scale3(1.0 / l, a)
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

const IDENTITY_FRAME: [[f64; 3]; 4] = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// An orthonormal frame `(origin, mid, binormal, axis)`: axis-aligned or random.
fn gen_frame(rng: &mut Rng, scale: f64) -> [[f64; 3]; 4] {
    if rng.chance(0.3) {
        let mut f = IDENTITY_FRAME;
        f[0] = scale3(scale, rp(rng, 2.0));
        return f;
    }
    let axis = loop {
        let a = rp(rng, 1.0);
        if a.iter().map(|c| c * c).sum::<f64>() > 0.1 {
            break unit3(a);
        }
    };
    let mid = loop {
        let m = rp(rng, 1.0);
        let d = m[0] * axis[0] + m[1] * axis[1] + m[2] * axis[2];
        let m = add3(m, scale3(-d, axis));
        if m.iter().map(|c| c * c).sum::<f64>() > 0.1 {
            break unit3(m);
        }
    };
    [scale3(scale, rp(rng, 2.0)), mid, cross3(axis, mid), axis]
}

/// The point at frame coordinates `(a, b, c)`.
fn at_frame(f: &[[f64; 3]; 4], a: f64, b: f64, c: f64) -> [f64; 3] {
    add3(
        add3(f[0], scale3(a, f[1])),
        add3(scale3(b, f[2]), scale3(c, f[3])),
    )
}

/// A rigid (sometimes scaled or singular) transform map.
fn gen_transform(rng: &mut Rng, s: f64) -> [[f64; 3]; 4] {
    match rng.below(6) {
        0 => IDENTITY_FRAME,
        1 | 2 => {
            // rotation about z by a random angle, translated
            let a = rng.uniform(-3.2, 3.2);
            let (sn, c) = a.sin_cos();
            [scale3(s, rp(rng, 5.0)), [c, sn, 0.0], [-sn, c, 0.0], [0.0, 0.0, 1.0]]
        }
        3 => {
            let f = gen_frame(rng, s);
            [f[0], f[1], f[2], f[3]]
        }
        4 => {
            // scaled: not unit orthonormal (no exact image)
            let k = rng.uniform(0.1, 4.0);
            [scale3(s, rp(rng, 5.0)), [k, 0.0, 0.0], [0.0, k, 0.0], [0.0, 0.0, k]]
        }
        _ => [scale3(s, rp(rng, 5.0)), rp(rng, 2.0), rp(rng, 2.0), rp(rng, 2.0)],
    }
}

/// A straight base curve from `p` to `q`: a line, a degree 1 B-spline or a degree 2 or 3
/// B-spline with collinear control points (on a random parameter range).
fn straight(rng: &mut Rng, p: [f64; 3], q: [f64; 3]) -> ExactCurveIn {
    let deg = match rng.below(4) {
        0 => return ExactCurveIn::Line { p1: p, p2: q },
        1 => 1,
        2 => 2,
        _ => 3,
    };
    let (u0, u1) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let u0 = rng.uniform(-2.0, 2.0);
        (u0, u0 + rng.uniform(0.5, 3.0))
    };
    let mut cpts = Vec::new();
    for i in 0..=deg {
        let f = i as f64 / deg as f64;
        let x = add3(scale3(1.0 - f, p), scale3(f, q));
        cpts.push([x[0], x[1], x[2], NOW]);
    }
    let mut u = vec![u0; deg + 1];
    u.extend(vec![u1; deg + 1]);
    ExactCurveIn::Curve(CurveIn::Bspline(BSplineIn {
        raw_knots: false,
        dim: 3,
        oob: false,
        cpts,
        p: deg,
        u,
    }))
}

/// A curve in the plane at height `c` of frame `f` around the in-plane point `(x, y)`:
/// a circle, a planar cubic B-spline or a straight segment.
fn planar(rng: &mut Rng, f: &[[f64; 3]; 4], x: f64, y: f64, c: f64, s: f64) -> ExactCurveIn {
    match rng.below(3) {
        0 => {
            let (a0, a1) = span(rng, -170.0, 170.0);
            let t: f64 = rng.uniform(-3.0, 3.0);
            ExactCurveIn::Circle {
                c: at_frame(f, x, y, c),
                x: add3(scale3(t.cos(), f[1]), scale3(t.sin(), f[2])),
                y: add3(scale3(-t.sin(), f[1]), scale3(t.cos(), f[2])),
                a0,
                a1: a1.max(a0 + 1.0),
                r: rng.uniform(0.2, 1.5) * s,
            }
        }
        1 => {
            let cpts = (0..4)
                .map(|i| {
                    let q = at_frame(
                        f,
                        x + (f64::from(i) - 1.5) * 0.4 * s,
                        y + rng.uniform(-0.5, 0.5) * s,
                        c,
                    );
                    [q[0], q[1], q[2], NOW]
                })
                .collect();
            ExactCurveIn::Curve(CurveIn::Bspline(BSplineIn {
                raw_knots: false,
                dim: 3,
                oob: false,
                cpts,
                p: 3,
                u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            }))
        }
        _ => {
            let d = rng.uniform(0.2, 1.5) * s;
            let t: f64 = rng.uniform(-3.0, 3.0);
            straight(
                rng,
                at_frame(f, x, y, c),
                at_frame(f, x + d * t.cos(), y + d * t.sin(), c),
            )
        }
    }
}

/// Two increasing values in `[lo, hi]`.
fn span(rng: &mut Rng, lo: f64, hi: f64) -> (f64, f64) {
    let a = rng.uniform(lo, hi);
    let b = rng.uniform(lo, hi);
    (a.min(b), a.max(b))
}

/// A segment from `p` to `q` moved off its line at the end by a tolerance-limit tilt
/// (below or above the approximation tolerance) or reversed, by chance.
fn aligned(rng: &mut Rng, p: [f64; 3], q: [f64; 3], tilt_dir: [f64; 3]) -> ExactCurveIn {
    let (p, mut q) = if rng.chance(0.5) { (q, p) } else { (p, q) };
    if rng.chance(0.15) {
        let t = if rng.chance(0.5) { 1e-8 } else { 1e-5 };
        q = add3(q, scale3(t, tilt_dir));
    }
    straight(rng, p, q)
}

/// A curve in volume whose head map is a bend, unbend or twist volume and whose curve
/// is aligned with that volume's frame (in-space or parameter space), so the analytic
/// branches of `MakeExactBSpline1stOutCurve` run.
fn gen_exact_volume_case(rng: &mut Rng) -> OpIn {
    let scale = match rng.below(10) {
        0 => 1e-3,
        1 => 1e3,
        2 => 1e4,
        _ => 1.0,
    };
    let kind = match rng.below(20) {
        0 => 2,
        1 => 3,
        2 => 4,
        3..=7 => 5,
        8..=13 => 6,
        _ => 7,
    };
    let in_param = rng.chance(0.35);
    let frame = gen_frame(rng, scale);
    // neutral distance (bend, unbend) or period (twist); a tiny one is clamped upstream
    let a = match rng.below(12) {
        0 => 1e-7 * scale,
        1 if kind == 5 => -rng.uniform(1.0, 8.0) * scale,
        _ => rng.uniform(0.5, 4.0) * scale,
    };
    let bb = match rng.below(6) {
        0 => 0.0,
        1 => 0.9 * scale,
        _ => rng.uniform(1e-6, 0.2) * scale,
    };
    let head = if kind >= 5 {
        MapIn {
            kind,
            comp: Some(CompIn {
                p: frame[0],
                axis: frame[3],
                mid: frame[1],
                a,
                b: bb,
            }),
            t: None,
            orient: None,
        }
    } else {
        MapIn {
            kind,
            comp: None,
            t: None,
            orient: rng.chance(0.5).then_some(frame),
        }
    };
    // the input frame: in-space is the volume's frame, parameter space the identity
    let f = if in_param || kind < 5 { IDENTITY_FRAME } else { frame };
    let f = if kind < 5 && !in_param {
        head.orient.unwrap_or(f)
    } else {
        f
    };
    let n = a.abs().max(1e-3 * scale);
    let s = scale;
    let curve = match (kind, rng.below(7)) {
        // twist: curves in planes normal to the axis (height 0 is the zero angle)
        (2 | 5, 0..=4) => {
            let c = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) * s };
            let (x, y) = (rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s);
            if rng.chance(0.15) {
                // tilted out of the plane
                let t = if rng.chance(0.5) { 1e-8 } else { 1e-5 };
                aligned(
                    rng,
                    at_frame(&f, x, y, c),
                    add3(at_frame(&f, x + s, y, c), scale3(t, f[3])),
                    f[3],
                )
            } else {
                planar(rng, &f, x, y, c, s)
            }
        }
        // unbend: lines parallel to the axis
        (3 | 6, 0 | 1) => {
            let r = rng.uniform(0.3, 3.0) * s;
            let t: f64 = rng.uniform(-3.0, 3.0);
            let (z0, z1) = span(rng, -2.0 * s, 2.0 * s);
            let (x, y) = (r * t.cos(), r * t.sin());
            aligned(rng, at_frame(&f, x, y, z0), at_frame(&f, x, y, z1 + 0.1 * s), f[1])
        }
        // unbend: circles centred on the axis
        (3 | 6, 2 | 3) => {
            let (a0, a1) = span(rng, -175.0, 175.0);
            let t: f64 = rng.uniform(-3.0, 3.0);
            let flip = if rng.chance(0.3) { -1.0 } else { 1.0 };
            let c = at_frame(&f, 0.0, 0.0, rng.uniform(-2.0, 2.0) * s);
            let c = if rng.chance(0.15) {
                add3(c, scale3(if rng.chance(0.5) { 1e-8 } else { 1e-5 }, f[1]))
            } else {
                c
            };
            ExactCurveIn::Circle {
                c,
                x: add3(scale3(t.cos(), f[1]), scale3(t.sin(), f[2])),
                y: scale3(flip, add3(scale3(-t.sin(), f[1]), scale3(t.cos(), f[2]))),
                a0,
                a1: a1.max(a0 + 1.0),
                r: rng.uniform(0.3, 3.0) * s,
            }
        }
        // unbend: lines radiating from the axis
        (3 | 6, 4 | 5) => {
            let t: f64 = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) };
            let (r0, r1) = span(rng, 0.2 * s, 3.0 * s);
            let z = rng.uniform(-2.0, 2.0) * s;
            let tilt = add3(scale3(-t.sin(), f[1]), scale3(t.cos(), f[2]));
            aligned(
                rng,
                at_frame(&f, r0 * t.cos(), r0 * t.sin(), z),
                at_frame(&f, (r1 + 0.1 * s) * t.cos(), (r1 + 0.1 * s) * t.sin(), z),
                tilt,
            )
        }
        // bend: lines parallel to the axis
        (4 | 7, 0 | 1) => {
            let m = rng.uniform(0.2, 3.0) * s;
            let y = rng.uniform(-3.0, 3.0) * n;
            let (z0, z1) = span(rng, -2.0 * s, 2.0 * s);
            aligned(rng, at_frame(&f, m, y, z0), at_frame(&f, m, y, z1 + 0.1 * s), f[1])
        }
        // bend: lines parallel to the mid direction
        (4 | 7, 2 | 3) => {
            let y = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) * n };
            let (x0, x1) = span(rng, 0.2 * s, 3.0 * s);
            let z = rng.uniform(-2.0, 2.0) * s;
            aligned(rng, at_frame(&f, x0, y, z), at_frame(&f, x1 + 0.1 * s, y, z), f[3])
        }
        // bend: lines parallel to the binormal (circular arcs)
        (4 | 7, 4 | 5) => {
            let m = rng.uniform(0.2, 3.0) * s;
            let (y0, y1) = span(rng, -3.1 * n, 3.1 * n);
            let z = rng.uniform(-2.0, 2.0) * s;
            aligned(rng, at_frame(&f, m, y0, z), at_frame(&f, m, y1 + 0.01 * n, z), f[3])
        }
        // anything else: an unaligned segment (no exact image) or one leaving the domain
        _ => {
            let p = at_frame(&f, rng.uniform(-1.0, 3.0) * s, rng.uniform(-2.0, 2.0) * s, 0.0);
            let q = at_frame(&f, rng.uniform(-1.0, 3.0) * s, rng.uniform(-2.0, 2.0) * s, s);
            if rng.chance(0.3) {
                ExactCurveIn::Curve(CurveIn::Hermite(HermiteIn {
                    dim: 3,
                    p1: p,
                    d1: rp(rng, s),
                    p2: q,
                    d2: rp(rng, s),
                }))
            } else {
                straight(rng, p, q)
            }
        }
    };
    let mut volume = vec![head.clone()];
    match rng.below(6) {
        0 => volume.push(MapIn {
            kind: 1,
            comp: None,
            t: Some(gen_transform(rng, s)),
            orient: None,
        }),
        1 => volume.push(head),
        _ => {}
    }
    OpIn::Exact {
        curve,
        in_param,
        volume,
    }
}

fn exact_case(name: String, op: OpIn) -> Result<Option<Case>, HarnessError> {
    let i = Inputs { civ: None, op };
    let Ok((op, input)) = cpp_input(&i) else {
        return Ok(None);
    };
    if ffi::validate(op, &input).is_err() {
        return Ok(None);
    }
    Ok(Some(Case {
        unit: "curve.crv_in_volume".into(),
        name,
        source: CaseSource::Inline,
        inputs: serde_json::to_value(&i).map_err(|e| harness(e.to_string()))?,
    }))
}

/// Generated `MakeExactBSplineIfPossible` cases on bend, unbend and twist volumes the
/// ffi wrapper accepts, from a fixed seed, after hand-built ones.
fn exact_volume_cases() -> Result<Vec<Case>, HarnessError> {
    const WANT: usize = 400;
    let mut cases = Vec::new();
    // hand-built (axis z, mid direction x at the origin): bend arcs over almost a full
    // turn (both orientations) and lines along the axis and the mid direction, unbend
    // circles about the axis, radial lines and axis lines, a zero-angle and a left-hand
    // twist
    let comp = |kind: u8, a: f64| MapIn {
        kind,
        comp: Some(CompIn {
            p: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            mid: [1.0, 0.0, 0.0],
            a,
            b: 1e-3,
        }),
        t: None,
        orient: None,
    };
    let line = |p1: [f64; 3], p2: [f64; 3]| ExactCurveIn::Line { p1, p2 };
    let bpi = core::f64::consts::PI * (1.0 - 1e-7);
    let circle = |c: [f64; 3], y: [f64; 3], a0: f64, a1: f64, r: f64| ExactCurveIn::Circle {
        c,
        x: [1.0, 0.0, 0.0],
        y,
        a0,
        a1,
        r,
    };
    let hand = [
        (comp(7, 1.0), line([1.5, -bpi, 0.5], [1.5, bpi, 0.5])),
        (comp(7, 1.0), line([1.5, bpi, 0.5], [1.5, -bpi, 0.5])),
        (comp(7, 1.0), line([1.5, -1.0, 0.5], [1.5, 1.0, 0.5])),
        (comp(7, 1.0), line([1.0, 0.3, -1.0], [1.0, 0.3, 1.0])),
        (comp(7, 1.0), line([0.5, 0.2, 0.5], [2.0, 0.2, 0.5])),
        (comp(6, 1.0), circle([0.0, 0.0, 0.5], [0.0, 1.0, 0.0], -179.0, 179.0, 1.5)),
        (comp(6, 1.0), circle([0.0, 0.0, 0.5], [0.0, -1.0, 0.0], -90.0, 45.0, 1.5)),
        (comp(6, 1.0), line([0.5 * 0.7f64.cos(), 0.5 * 0.7f64.sin(), 0.2], [2.0 * 0.7f64.cos(), 2.0 * 0.7f64.sin(), 0.2])),
        (comp(6, 1.0), line([1.0, 0.5, -1.0], [1.0, 0.5, 1.0])),
        (comp(5, 2.0), line([-1.0, 0.5, 0.0], [1.0, -0.5, 0.0])),
        (comp(5, -3.0), circle([0.3, 0.2, 0.75], [0.0, 1.0, 0.0], 0.0, 270.0, 1.0)),
        (comp(5, 2.0), circle([0.3, 0.2, 1.25], [0.0, -1.0, 0.0], -45.0, 300.0, 0.5)),
    ];
    for (n, (map, curve)) in hand.into_iter().enumerate() {
        for in_param in [false, true] {
            let op = OpIn::Exact {
                curve: curve.clone(),
                in_param,
                volume: vec![map.clone()],
            };
            let name = format!("exact_volume_hand_{n}_{}", u8::from(in_param));
            let c = exact_case(name.clone(), op)?
                .ok_or_else(|| harness(format!("{name}: rejected by the ffi wrapper")))?;
            cases.push(c);
        }
    }
    let hand_count = cases.len();
    let mut rng = Rng::new(0x5eed_c1e4);
    for _ in 0..20_000 {
        if cases.len() >= hand_count + WANT {
            break;
        }
        let op = gen_exact_volume_case(&mut rng);
        let name = format!("exact_volume_{}", cases.len() - hand_count);
        if let Some(c) = exact_case(name, op)? {
            cases.push(c);
        }
    }
    Ok(cases)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "curve.crv_in_volume"
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
        let mut cases = default_cases(self, &ctx)?;
        cases.extend(exact_volume_cases()?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
