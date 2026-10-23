//! Runner for unit `surface.srf_in_volume`: `SmSrfInVolume`, a surface mapped through
//! a volume.
//!
//! C++: `solidcore_ffi::units::surface_srf_in_volume` (upstream
//! `source/SMLib/src/SmSrfInVolume.cpp` on objects built from a B-spline or plane base
//! surface and a chain of transform, twist, unbend or plain volumes).
//! Rust: `solidcore::surface::srf_in_volume`, through the [`Surface`] dispatch where the
//! method is virtual upstream.
//!
//! A case is an object (`siv`: `surface`, `volume` chain, `in_param`, `copy`, `owner`)
//! and an `op` with its parameters. Both sides produce the flat record documented in
//! the ffi module; the runner reports its first entry as the `status` int (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce) and the rest as `out`.
//!
//! `Exact` calls the existing kernel-aware implementation and compares the returned
//! surface's presence, concrete type and full NURB data, not merely its status.
//! Its existing FFI guard permits transform-only chains; bend, unbend and twist
//! exact-conversion implementations are not covered here. Volumes are otherwise
//! chains of transform, plain, default and component
//! twist / unbend / bend maps (the component constructors take a real axis, mid
//! direction, period or neutral distance and tolerance). B-spline volumes are not
//! built: they evaluate to `SM_ERR` on both sides (no `USE_VOLUMES` in the reference
//! build). `STEPInversion` and `ReadFromDB` run through `surface.volume_p2`; a
//! `ReadFromDB` case reads the text upstream's `WriteToDB` wrote for a generated object
//! (no plain volume). Hand-built inline cases cover `Evaluate` of orders (0, 0) with
//! `bOnlyUpperHalf == FALSE`, `Transform` on chains whose last map is not a
//! transform, and 60 `CreateMirrorSurface` cases (fixed seed). Every run has at least
//! [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper rejects are
//! redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::surface::bend_volume::BendVolume;
use solidcore::surface::plane::Plane;
use solidcore::surface::srf_in_volume::SrfInVolume;
use solidcore::surface::surface::p2::{Surface, SurfaceKind};
use solidcore::surface::twist_volume::TwistVolume;
use solidcore::surface::unbend_volume::UnbendVolume;
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore::SolidError;
use solidcore_ffi::units::surface_srf_in_volume::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::units::surface_surface_cache_p1::{
    bss_of, gen_bss, gen_srf, gen_sub_dom, push_bss, srf, BssIn, InlineSrf, SrfIn,
};
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `surface.srf_in_volume` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Bspline { bss: BssIn },
    Plane { o: [f64; 3], n: [f64; 3] },
}

/// One map of a volume chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MapIn {
    /// `0` plain `SmVolume`, `1` `SmTransform`, `2` default twist, `3` default unbend,
    /// `4` default bend, `5` / `6` / `7` the twist / unbend / bend component constructors.
    kind: u8,
    /// The constructor arguments of kinds `5` to `7`: `[origin or center, axis, mid
    /// direction]` and the two numbers (period and minimum, or neutral distance and
    /// singularity tolerance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    comp: Option<CompIn>,
    /// The transform `[origin, X, Y, Z]` (kind `1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    t: Option<[[f64; 3]; 4]>,
    /// The orient transform `[origin, X, Y, Z]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orient: Option<[[f64; 3]; 4]>,
}

/// The arguments of a component constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompIn {
    p: [f64; 3],
    axis: [f64; 3],
    mid: [f64; 3],
    a: f64,
    b: f64,
}

type Vol = Vec<MapIn>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SivIn {
    surface: SurfIn,
    volume: Vol,
    #[serde(default)]
    in_param: bool,
    #[serde(default)]
    copy: u64,
    #[serde(default)]
    owner: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Info {
        uv: [f64; 2],
        types: Vec<i64>,
    },
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        ul: bool,
        vl: bool,
        oh: bool,
        nz: bool,
    },
    EvalSimple {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        ul: bool,
        vl: bool,
        oh: bool,
    },
    EvalPoint {
        uv: [f64; 2],
    },
    Normal {
        uv: [f64; 2],
        ul: bool,
        vl: bool,
    },
    Knots {
        v_dir: bool,
        ivl: Option<[f64; 2]>,
    },
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
    },
    Reverse {
        v_dir: bool,
    },
    SwapUv,
    Trim {
        dom: [f64; 4],
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Split {
        v_dir: bool,
        t: f64,
    },
    Copy,
    Equal {
        other: Option<SivIn>,
    },
    SetOwner {
        flag: u64,
    },
    MakeOwner,
    SetBase {
        surface: SurfIn,
        in_param: bool,
        copy: bool,
        owns: bool,
    },
    SetVolume {
        volume: Vol,
        copy: bool,
        owns: bool,
    },
    Mirror {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
    },
    Exact,
    /// `SmSrfInVolume::ReadFromDB` of the text upstream's `WriteToDB` wrote for `siv`
    /// (the object is only the text's source: neither side builds it).
    ReadDb {
        version: u64,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    siv: SivIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ input --------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn surf_block(v: &mut Vec<f64>, s: &SurfIn) -> Result<(), HarnessError> {
    match s {
        SurfIn::Bspline { bss } => {
            v.push(0.0);
            push_bss(v, bss)?;
        }
        SurfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend_from_slice(o);
            v.extend_from_slice(n);
        }
    }
    Ok(())
}

fn vol_block(v: &mut Vec<f64>, vol: &Vol) {
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

fn siv_block(v: &mut Vec<f64>, c: &SivIn) -> Result<(), HarnessError> {
    surf_block(v, &c.surface)?;
    vol_block(v, &c.volume);
    v.extend([b(c.in_param), c.copy as f64, c.owner as f64]);
    Ok(())
}

fn op_of(op: &OpIn) -> Op {
    match op {
        OpIn::Info { .. } => Op::Info,
        OpIn::Eval { .. } => Op::Eval,
        OpIn::EvalSimple { .. } => Op::EvalSimple,
        OpIn::EvalPoint { .. } => Op::EvalPoint,
        OpIn::Normal { .. } => Op::Normal,
        OpIn::Knots { .. } => Op::Knots,
        OpIn::StepInv { .. } => Op::StepInv,
        OpIn::Reverse { .. } => Op::Reverse,
        OpIn::SwapUv => Op::SwapUv,
        OpIn::Trim { .. } => Op::Trim,
        OpIn::Transform { .. } => Op::Transform,
        OpIn::Split { .. } => Op::Split,
        OpIn::Copy => Op::Copy,
        OpIn::Equal { .. } => Op::Equal,
        OpIn::SetOwner { .. } => Op::SetOwner,
        OpIn::MakeOwner => Op::MakeOwner,
        OpIn::SetBase { .. } => Op::SetBase,
        OpIn::SetVolume { .. } => Op::SetVolume,
        OpIn::Mirror { .. } => Op::Mirror,
        OpIn::Exact => Op::Exact,
        OpIn::ReadDb { .. } => Op::ReadDb,
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let op = op_of(&i.op);
    let mut v = Vec::new();
    if let OpIn::ReadDb { version, bytes } = &i.op {
        v.extend([*version as f64, bytes.len() as f64]);
        v.extend(bytes.iter().map(|&x| f64::from(x)));
        return Ok((op, v));
    }
    siv_block(&mut v, &i.siv)?;
    match &i.op {
        OpIn::Info { uv, types } => {
            v.extend_from_slice(uv);
            v.push(types.len() as f64);
            v.extend(types.iter().map(|&t| t as f64));
        }
        OpIn::Eval {
            uv,
            hu,
            hv,
            ul,
            vl,
            oh,
            nz,
        } => {
            v.extend_from_slice(uv);
            v.extend([*hu as f64, *hv as f64, b(*ul), b(*vl), b(*oh), b(*nz)]);
        }
        OpIn::EvalSimple {
            uv,
            hu,
            hv,
            ul,
            vl,
            oh,
        } => {
            v.extend_from_slice(uv);
            v.extend([*hu as f64, *hv as f64, b(*ul), b(*vl), b(*oh)]);
        }
        OpIn::EvalPoint { uv } => v.extend_from_slice(uv),
        OpIn::Normal { uv, ul, vl } => {
            v.extend_from_slice(uv);
            v.extend([b(*ul), b(*vl)]);
        }
        OpIn::Knots { v_dir, ivl } => {
            let [x, y] = ivl.unwrap_or([0.0, 0.0]);
            v.extend([b(*v_dir), b(ivl.is_some()), x, y]);
        }
        OpIn::StepInv { dom, p, tol } => {
            v.extend_from_slice(dom);
            v.extend_from_slice(p);
            v.push(*tol);
        }
        OpIn::Reverse { v_dir } => v.push(b(*v_dir)),
        OpIn::SwapUv | OpIn::Copy | OpIn::MakeOwner | OpIn::Exact | OpIn::ReadDb { .. } => {}
        OpIn::Trim { dom } => v.extend_from_slice(dom),
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
        OpIn::Split { v_dir, t } => v.extend([b(*v_dir), *t]),
        OpIn::Equal { other } => match other {
            None => v.push(1.0),
            Some(o) => {
                v.push(0.0);
                siv_block(&mut v, o)?;
            }
        },
        OpIn::SetOwner { flag } => v.push(*flag as f64),
        OpIn::SetBase {
            surface,
            in_param,
            copy,
            owns,
        } => {
            surf_block(&mut v, surface)?;
            v.extend([b(*in_param), b(*copy), b(*owns)]);
        }
        OpIn::SetVolume { volume, copy, owns } => {
            vol_block(&mut v, volume);
            v.extend([b(*copy), b(*owns)]);
        }
        OpIn::Mirror { origin, x, y } => {
            v.extend_from_slice(origin);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
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

fn transform_of(t: [[f64; 3]; 4]) -> Transform {
    let [o, x, y, z] = t;
    Transform::new(v3(o), v3(x), v3(y), Some(v3(z)))
}

fn build_volume(v: &Vol) -> Result<Volume, HarnessError> {
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
        next = Some(Volume::with_maps(kind, m.orient.map(transform_of), next).map_err(|e| HarnessError::new(e.to_string()))?);
    }
    next.ok_or_else(|| harness("volume without maps"))
}

fn build_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    match s {
        SurfIn::Bspline { bss } => Ok(Surface::from(bss_of(bss)?)),
        SurfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
    }
}

fn build(k: &Kernel, c: &SivIn) -> Result<SrfInVolume, HarnessError> {
    SrfInVolume::new(
        k,
        build_surface(&c.surface)?,
        c.in_param,
        build_volume(&c.volume)?,
        c.copy,
        c.owner,
    )
    .map_err(|e| harness(format!("build: {e}")))
}

fn siv_of(s: &Surface) -> Result<&SrfInVolume, HarnessError> {
    match s.kind() {
        SurfaceKind::SrfInVolume(x) => Ok(x),
        _ => Err(harness("not a surface in volume")),
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn put_bss(o: &mut Vec<f64>, s: &BSplineSurface) {
    match s.gw_nurb() {
        None => o.push(0.0),
        Some(sur) => {
            o.push(1.0);
            let ix = sur.indices();
            o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
            for row in sur.rows() {
                for p in row {
                    o.extend([p.x, p.y, p.z, p.w]);
                }
            }
            o.extend_from_slice(sur.u_knots());
            o.extend_from_slice(sur.v_knots());
        }
    }
}

fn put_surface(o: &mut Vec<f64>, s: &Surface) {
    o.push(s.type_code() as f64);
    match s.kind() {
        SurfaceKind::BSpline(bs) => put_bss(o, bs),
        // the plane's NURB is made on demand upstream, also by const methods (the shim makes
        // it before dumping)
        SurfaceKind::Plane(p) => match p.base_with_nurb() {
            Ok(b) => put_bss(o, &b),
            Err(_) => o.push(0.0),
        },
        _ => o.push(0.0),
    }
}

fn put_mat(o: &mut Vec<f64>, t: &Transform) {
    for row in t.matrix() {
        o.extend(row);
    }
}

fn put_volume(o: &mut Vec<f64>, v: &Volume) {
    let mut maps = vec![v];
    while let Some(n) = maps.last().and_then(|m| m.next_map()) {
        maps.push(n);
    }
    o.push(maps.len() as f64);
    for m in maps {
        o.push(m.type_code() as f64);
        match m.kind() {
            VolumeKind::Transform(t) => put_mat(o, t),
            VolumeKind::Twist(t) => o.extend([t.twist_period_dist(), t.period_min_dist()]),
            VolumeKind::Unbend(u) => o.extend([u.unbend_neutral_dist(), u.singularity_tol()]),
            VolumeKind::Bend(bd) => o.extend([bd.bend_neutral_dist(), bd.singularity_tol()]),
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

fn put_state(o: &mut Vec<f64>, c: &SrfInVolume) {
    o.push(c.owner_flag() as f64);
    o.push(b(c.in_param_space()));
    put_surface(o, c.surface());
    put_volume(o, c.volume());
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

/// Pushes the status of `$r` or returns the deferral.
macro_rules! st {
    ($o:ident, $r:expr) => {
        $o.push(match st_of(&$r) {
            Ok(s) => s,
            Err(d) => return Ok(*d),
        })
    };
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

fn param(v_dir: bool) -> SurfParamType {
    if v_dir {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

fn pt(uv: [f64; 2]) -> Point2 {
    Point2::new(uv[0], uv[1])
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

/// The defined entries of the matrix `d[u][v]`: all for unequal orders or a full
/// matrix, else `u + v <= n`.
fn put_derivs(o: &mut Vec<f64>, d: &[Vec<Vec3>], hu: usize, hv: usize, upper: bool) {
    for i in 0..=hu {
        for j in 0..=hv {
            if hu != hv || !upper || i + j <= hu {
                pv(
                    o,
                    d.get(i)
                        .and_then(|r| r.get(j))
                        .copied()
                        .unwrap_or(Vec3::UNDEF),
                );
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    let k = Kernel::default();
    if let OpIn::ReadDb { version, bytes } = &i.op {
        let mut db = DatabaseReader::with_stream(FileType::Ascii, InputStream::new(bytes.clone()));
        match SrfInVolume::read_from_db(&k, &mut db, *version) {
            Ok(c) => {
                o.extend([0.0, 1.0]);
                put_state(&mut o, &c);
            }
            Err(e) => {
                if let Some(d) = Output::unported(&e) {
                    return Ok(d);
                }
                // status, not made
                o.extend([status(&e), 0.0]);
            }
        }
        return Ok(to_output(&o));
    }
    let mut s = Surface::from(build(&k, &i.siv)?);
    match &i.op {
        OpIn::Info { uv, types } => {
            let c = siv_of(&s)?;
            let step = s.step_uv_domain();
            let max = tri!(o, s.max_analytic_domain(&k, ));
            o.push(0.0);
            o.extend([
                s.type_code() as f64,
                s.degree(SurfParamType::U) as f64,
                s.degree(SurfParamType::V) as f64,
            ]);
            put_dom(&mut o, &s.natural_uv_domain());
            put_dom(&mut o, &step);
            put_dom(&mut o, &max);
            o.extend([b(c.owns_surface()), b(c.owns_volume())]);
            let a = tri!(o, s.convert_uv_from_step_to_nurbs(&k, pt(*uv)));
            let e = tri!(o, s.convert_uv_from_nurbs_to_step(&k, pt(*uv)));
            o.extend([0.0, a.x, a.y, 0.0, e.x, e.y]);
            for &t in types {
                o.push(b(tri!(o, s.is_kind_of(t))));
            }
            put_state(&mut o, c);
        }
        OpIn::Eval {
            uv,
            hu,
            hv,
            ul,
            vl,
            oh,
            nz,
        } => {
            let flags = EvaluateFlags {
                u_side: side(*ul),
                v_side: side(*vl),
                only_upper_half: *oh,
                non_zero_tangents: *nz,
            };
            let d = tri!(o, s.evaluate(&k, pt(*uv), *hu, *hv, flags));
            o.push(0.0);
            put_derivs(&mut o, &d, *hu, *hv, *oh);
        }
        OpIn::EvalSimple {
            uv,
            hu,
            hv,
            ul,
            vl,
            oh,
        } => {
            let d = tri!(
                o,
                siv_of(&s)?.evaluate_simple(&k, pt(*uv), *hu, *hv, side(*ul), side(*vl), *oh)
            );
            o.push(0.0);
            put_derivs(&mut o, &d, *hu, *hv, *oh);
        }
        OpIn::EvalPoint { uv } => {
            let p = tri!(o, s.evaluate_point(&k, pt(*uv)));
            o.push(0.0);
            pv(&mut o, p);
        }
        OpIn::Normal { uv, ul, vl } => {
            let n = tri!(o, s.evaluate_normal(&k, pt(*uv), side(*ul), side(*vl)));
            o.push(0.0);
            pv(&mut o, n);
        }
        OpIn::Knots { v_dir, ivl } => {
            let iv = ivl.map(|[x, y]| Extent1d::from_min_max(x, y));
            let (k, m) = tri!(o, s.knots(param(*v_dir), iv.as_ref()));
            o.push(0.0);
            o.push(k.len() as f64);
            o.extend_from_slice(&k);
            o.push(m.len() as f64);
            o.extend(m.iter().map(|&x| x as f64));
        }
        OpIn::StepInv { dom, p, tol } => {
            let (uv, loc) = tri!(o, s.step_inversion(&k, &dom_of(*dom), v3(*p), *tol, None));
            o.extend([0.0, uv.x, uv.y, f64::from(loc as i32)]);
        }
        OpIn::Reverse { v_dir } => {
            let r = s.reverse(&k, param(*v_dir));
            st!(o, r);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::SwapUv => {
            let r = s.swap_uv(&k);
            st!(o, r);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::Trim { dom } => {
            let mut d = dom_of(*dom);
            let r = s.trim_with_domain(&k, &mut d);
            st!(o, r);
            put_dom(&mut o, &d);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let r = s.transform(&k, &ax, scale.map(v3));
            st!(o, r);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::Split { v_dir, t } => {
            let (l, r) = tri!(o, s.split_at(&k, *t, param(*v_dir)));
            o.push(0.0);
            put_state(&mut o, siv_of(&l)?);
            put_state(&mut o, siv_of(&r)?);
        }
        OpIn::Copy => {
            let c = tri!(o, s.copy(&k));
            o.push(0.0);
            put_state(&mut o, siv_of(&c)?);
        }
        OpIn::Equal { other } => {
            let c = siv_of(&s)?;
            let eq = match other {
                None => tri!(o, c.approx_eq(&k, c)),
                Some(oc) => tri!(o, c.approx_eq(&k, &build(&k, oc)?)),
            };
            o.extend([0.0, b(eq)]);
        }
        OpIn::SetOwner { flag } => {
            if let SurfaceKind::SrfInVolume(c) = s.kind_mut() {
                c.set_owner_flag(*flag);
            }
            o.push(0.0);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::MakeOwner => {
            let r = match s.kind_mut() {
                SurfaceKind::SrfInVolume(c) => c.make_owner(&k),
                _ => return Err(harness("not a surface in volume")),
            };
            st!(o, r);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::SetBase {
            surface,
            in_param,
            copy,
            owns,
        } => {
            let ns = build_surface(surface)?;
            if let SurfaceKind::SrfInVolume(c) = s.kind_mut() {
                tri!(o, c.set_base_surface(&k, ns, *in_param, *copy, *owns));
            }
            o.push(0.0);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::SetVolume { volume, copy, owns } => {
            let nv = build_volume(volume)?;
            if let SurfaceKind::SrfInVolume(c) = s.kind_mut() {
                tri!(o, c.set_volume(nv, *copy, *owns));
            }
            o.push(0.0);
            put_state(&mut o, siv_of(&s)?);
        }
        OpIn::Mirror { origin, x, y } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let m = tri!(o, siv_of(&s)?.create_mirror_surface(&k, &ax));
            o.push(0.0);
            put_state(&mut o, &m);
        }
        OpIn::Exact => {
            let exact = tri!(o, siv_of(&s)?.make_exact_b_spline_if_possible_with(&k));
            o.push(0.0);
            o.push(b(exact.is_some()));
            if let Some(exact) = exact {
                put_surface(&mut o, &exact);
            }
        }
        OpIn::ReadDb { .. } => return Err(harness("static op")),
    }
    Ok(to_output(&o))
}

// ---- generation -------------------------------------------------------------------

fn coord(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A surface and the parameter domain to draw parameters from.
fn gen_surface(rng: &mut Rng) -> (SurfIn, [f64; 4]) {
    if rng.chance(0.15) {
        let n = coord(rng, 1.0);
        let n = if n.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
            [0.0, 0.0, 1.0]
        } else {
            n
        };
        (
            SurfIn::Plane {
                o: coord(rng, 3.0),
                n,
            },
            [-2.0, -2.0, 2.0, 2.0],
        )
    } else {
        let s: InlineSrf = gen_srf(rng, 4);
        let dom = [
            s.u.first().copied().unwrap_or(0.0),
            s.v.first().copied().unwrap_or(0.0),
            s.u.last().copied().unwrap_or(1.0),
            s.v.last().copied().unwrap_or(1.0),
        ];
        (SurfIn::Bspline { bss: gen_bss(rng, s) }, dom)
    }
}

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
            let s = rng.uniform(0.1, 4.0);
            [coord(rng, 5.0), [s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
        }
        3 => {
            // singular: a repeated axis
            let x = coord(rng, 2.0);
            [coord(rng, 5.0), x, x, coord(rng, 2.0)]
        }
        _ => [coord(rng, 5.0), coord(rng, 2.0), coord(rng, 2.0), coord(rng, 2.0)],
    }
}

/// The arguments of a component constructor: a real axis, a mid direction (sometimes
/// not perpendicular to the axis, which upstream repairs), a period or neutral distance
/// of useful size and a small tolerance.
fn gen_comp(rng: &mut Rng, kind: u8) -> CompIn {
    let axis = if rng.chance(0.4) {
        [0.0, 0.0, 1.0]
    } else {
        let a = coord(rng, 1.0);
        if a.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
            [0.0, 1.0, 0.0]
        } else {
            a
        }
    };
    let mid = if rng.chance(0.4) {
        [1.0, 0.0, 0.0]
    } else {
        let m = coord(rng, 1.0);
        if m.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
            [1.0, 0.0, 0.0]
        } else {
            m
        }
    };
    let (a, b) = if kind == 5 {
        // twist period and minimum period distance
        (rng.uniform(0.5, 8.0), rng.uniform(1e-6, 0.3))
    } else {
        // neutral distance and singularity tolerance
        (rng.uniform(0.2, 6.0), rng.uniform(1e-6, 0.3))
    };
    CompIn {
        p: coord(rng, 3.0),
        axis,
        mid,
        a,
        b,
    }
}

fn gen_vol(rng: &mut Rng) -> Vol {
    let n = 1 + rng.below(3) as usize;
    (0..n)
        .map(|_| {
            let kind = match rng.below(60) {
                0..=27 => 1,
                28 | 29 => 0,
                30 => 2,
                31 => 3,
                32 => 4,
                33..=40 => 5,
                41..=48 => 6,
                _ => 7,
            };
            let component = kind >= 5;
            MapIn {
                kind,
                comp: component.then(|| gen_comp(rng, kind)),
                t: (kind == 1).then(|| gen_transform(rng)),
                // a component constructor builds its own orient map
                orient: (!component && rng.chance(0.4)).then(|| gen_transform(rng)),
            }
        })
        .collect()
}

fn gen_siv(rng: &mut Rng) -> (SivIn, [f64; 4]) {
    let (surface, dom) = gen_surface(rng);
    (
        SivIn {
            surface,
            volume: gen_vol(rng),
            in_param: rng.chance(0.5),
            copy: rng.below(4),
            owner: if rng.chance(0.7) { 3 } else { rng.below(4) },
        },
        dom,
    )
}

/// A parameter pair near `d`: interior, at a corner or edge, or slightly outside.
fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let one = |rng: &mut Rng, a: f64, bb: f64| match rng.below(10) {
        0 => a,
        1 => bb,
        2 => a + (bb - a) * rng.uniform(-0.2, 1.2),
        _ => a + (bb - a) * rng.unit(),
    };
    [one(rng, d[0], d[2]), one(rng, d[1], d[3])]
}

fn gen_orders(rng: &mut Rng) -> (usize, usize, bool) {
    match rng.below(20) {
        0..=11 => {
            let n = rng.below(4) as usize;
            (n, n, true)
        }
        12 => (0, 0, false),
        13..=14 => (1, 1, false),
        15..=17 => {
            let n = 1 + rng.below(3) as usize;
            if rng.chance(0.5) {
                (n, 0, rng.chance(0.5))
            } else {
                (0, n, rng.chance(0.5))
            }
        }
        _ => (1 + rng.below(3) as usize, 1 + rng.below(3) as usize, rng.chance(0.5)),
    }
}

fn gen_placement(rng: &mut Rng) -> ([f64; 3], [f64; 3], [f64; 3]) {
    if rng.chance(0.4) {
        ([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        let a = rng.uniform(-3.2, 3.2);
        let (s, c) = a.sin_cos();
        (coord(rng, 5.0), [c, s, 0.0], [-s, c, 0.0])
    }
}

/// A database record upstream's `WriteToDB` wrote for a random object (no plain volume:
/// reading one fills a null volume).
fn gen_read_db(rng: &mut Rng) -> Option<Inputs> {
    let (siv, _) = gen_siv(rng);
    let version = *rng.choose(&[30u64, 33, 40])?;
    let mut v = Vec::new();
    siv_block(&mut v, &siv).ok()?;
    v.push(version as f64);
    let out = ffi::eval(Op::WriteDb, &v).ok()?;
    let (&status, bytes) = out.split_first()?;
    if status != 0.0 {
        return None;
    }
    let bytes: Vec<u8> = bytes.iter().map(|&x| x as u8).collect();
    Some(Inputs {
        siv,
        op: OpIn::ReadDb { version, bytes },
    })
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.04) {
        if let Some(i) = gen_read_db(rng) {
            return i;
        }
    }
    let (siv, dom) = gen_siv(rng);
    let uv = gen_uv(rng, dom);
    let op = match rng.below(100) {
        0..=3 => OpIn::Info {
            uv,
            types: vec![
                SrfInVolume::TYPE,
                15001,
                15002,
                15011,
                15045,
                rng.below(30000) as i64,
            ],
        },
        4..=25 => {
            let (hu, hv, oh) = gen_orders(rng);
            OpIn::Eval {
                uv,
                hu,
                hv,
                ul: rng.chance(0.5),
                vl: rng.chance(0.5),
                oh,
                nz: rng.chance(0.7),
            }
        }
        26..=29 => {
            let (hu, hv, oh) = gen_orders(rng);
            OpIn::EvalSimple {
                uv,
                hu,
                hv,
                ul: rng.chance(0.5),
                vl: rng.chance(0.5),
                oh,
            }
        }
        30..=35 => OpIn::EvalPoint { uv },
        36..=43 => OpIn::Normal {
            uv,
            ul: rng.chance(0.5),
            vl: rng.chance(0.5),
        },
        44..=49 => {
            let v_dir = rng.chance(0.5);
            let (a, e) = if v_dir { (dom[1], dom[3]) } else { (dom[0], dom[2]) };
            let ivl = rng.chance(0.5).then(|| {
                let x = a + (e - a) * rng.uniform(-0.2, 1.2);
                let y = a + (e - a) * rng.uniform(-0.2, 1.2);
                [x.min(y), x.max(y)]
            });
            OpIn::Knots { v_dir, ivl }
        }
        50..=52 => OpIn::StepInv {
            dom,
            p: coord(rng, 5.0),
            tol: 1e-6,
        },
        53..=56 => OpIn::Reverse {
            v_dir: rng.chance(0.5),
        },
        57..=59 => OpIn::SwapUv,
        60..=64 => OpIn::Trim {
            dom: gen_sub_dom(rng, dom),
        },
        65..=70 => {
            let (origin, x, y) = gen_placement(rng);
            OpIn::Transform {
                origin,
                x,
                y,
                scale: match rng.below(3) {
                    0 => None,
                    1 => Some([1.0, 1.0, 1.0]),
                    _ => Some([2.0, 2.0, 2.0]),
                },
            }
        }
        71..=76 => {
            let v_dir = rng.chance(0.5);
            let (a, e) = if v_dir { (dom[1], dom[3]) } else { (dom[0], dom[2]) };
            OpIn::Split {
                v_dir,
                t: a + (e - a) * rng.uniform(0.05, 0.95),
            }
        }
        77..=81 => OpIn::Copy,
        82..=84 => OpIn::Equal {
            other: match rng.below(4) {
                0 => None,
                1 => Some(siv.clone()),
                _ => Some(gen_siv(rng).0),
            },
        },
        85..=87 => OpIn::SetOwner { flag: rng.below(4) },
        88..=90 => OpIn::MakeOwner,
        91..=93 => OpIn::SetBase {
            surface: gen_surface(rng).0,
            in_param: rng.chance(0.5),
            copy: rng.chance(0.5),
            owns: rng.chance(0.5),
        },
        94..=96 => OpIn::SetVolume {
            volume: gen_vol(rng),
            copy: rng.chance(0.5),
            owns: rng.chance(0.5),
        },
        97 | 98 => {
            let (origin, x, y) = gen_placement(rng);
            OpIn::Mirror { origin, x, y }
        }
        _ => OpIn::Exact,
    };
    Inputs { siv, op }
}

/// Hand-built cases for what the generator reaches only by chance: `Evaluate` of a
/// point (orders 0, 0) with `bOnlyUpperHalf == FALSE`, which upstream completes with
/// the second-derivative branch, and `Transform` on a chain whose last map is not an
/// `SmTransform` (upstream appends a copied transform).
/// Generated `CreateMirrorSurface` cases the ffi wrapper accepts (the generator draws
/// the op rarely and the wrapper takes transform chains only), from a fixed seed.
fn mirror_cases() -> Result<Vec<Case>, HarnessError> {
    const WANT: usize = 60;
    let mut rng = Rng::new(0x5eed_4d17);
    let mut cases = Vec::new();
    for _ in 0..200_000 {
        if cases.len() >= WANT {
            break;
        }
        let mut i = gen_case(&mut rng);
        let (origin, x, y) = gen_placement(&mut rng);
        i.op = OpIn::Mirror { origin, x, y };
        let Ok((op, input)) = cpp_input(&i) else {
            continue;
        };
        if ffi::validate(op, &input).is_err() {
            continue;
        }
        cases.push(Case {
            unit: "surface.srf_in_volume".into(),
            name: format!("mirror_{}", cases.len()),
            source: CaseSource::Inline,
            inputs: serde_json::to_value(&i).map_err(|e| harness(e.to_string()))?,
        });
    }
    Ok(cases)
}

fn inline_cases() -> Result<Vec<Case>, HarnessError> {
    let plane = SurfIn::Plane {
        o: [0.0, 0.0, 0.0],
        n: [0.0, 0.0, 1.0],
    };
    let comp = |kind: u8, a: f64| MapIn {
        kind,
        comp: Some(CompIn {
            p: [0.5, -0.25, 0.0],
            axis: [0.0, 0.0, 1.0],
            mid: [1.0, 0.0, 0.0],
            a,
            b: 1e-3,
        }),
        t: None,
        orient: None,
    };
    let transform = MapIn {
        kind: 1,
        comp: None,
        t: Some([
            [1.0, 2.0, 3.0],
            [0.0, 1.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ]),
        orient: None,
    };
    let chains: Vec<(&str, Vol)> = vec![
        ("twist", vec![comp(5, 4.0)]),
        ("bend", vec![comp(7, 2.0)]),
        ("unbend", vec![comp(6, 3.0)]),
        ("twist_transform", vec![comp(5, 4.0), transform.clone()]),
        ("transform_bend", vec![transform.clone(), comp(7, 2.0)]),
        (
            "bend_twist_unbend",
            vec![comp(7, 2.0), comp(5, 4.0), comp(6, 3.0)],
        ),
    ];
    let place = || OpIn::Transform {
        origin: [1.0, -2.0, 0.5],
        x: [0.0, 1.0, 0.0],
        y: [-1.0, 0.0, 0.0],
        scale: Some([2.0, 2.0, 2.0]),
    };
    let mut out = Vec::new();
    for (name, vol) in chains {
        for in_param in [false, true] {
            let siv = SivIn {
                surface: plane.clone(),
                volume: vol.clone(),
                in_param,
                copy: 0,
                owner: 3,
            };
            let ops = [
                (
                    "eval00_all",
                    OpIn::Eval {
                        uv: [0.3, 0.7],
                        hu: 0,
                        hv: 0,
                        ul: true,
                        vl: true,
                        oh: false,
                        nz: false,
                    },
                ),
                (
                    "eval00_all_nz",
                    OpIn::Eval {
                        uv: [0.3, 0.7],
                        hu: 0,
                        hv: 0,
                        ul: false,
                        vl: true,
                        oh: false,
                        nz: true,
                    },
                ),
                ("transform", place()),
            ];
            for (op_name, op) in ops {
                let inputs = Inputs {
                    siv: siv.clone(),
                    op,
                };
                let (o, input) = cpp_input(&inputs)?;
                ffi::validate(o, &input).map_err(|e| harness(format!("inline case: {e}")))?;
                out.push(Case {
                    unit: "surface.srf_in_volume".to_string(),
                    name: format!("inline_{name}_{}_{op_name}", if in_param { "param" } else { "in" }),
                    source: CaseSource::Inline,
                    inputs: serde_json::to_value(inputs)
                        .map_err(|e| harness(format!("inline case: {e}")))?,
                });
            }
        }
    }
    Ok(out)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.srf_in_volume"
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
        cases.extend(inline_cases()?);
        cases.extend(mirror_cases()?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

// `srf` is part of the shared helpers: keep the import used by the fixtures path.
#[allow(dead_code)]
fn _srf_in(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    srf(s)
}

#[cfg(all(test, feature = "cpp"))]
mod tests {
    use super::*;

    fn assert_exact_result(name: &str, output: &Output) -> Result<(), HarnessError> {
        assert!(output.error.is_none(), "{name}: {:?}", output.error);
        assert!(output.deferred.is_none(), "{name}: {:?}", output.deferred);
        assert!(output.rejected.is_none(), "{name}: {:?}", output.rejected);
        assert_eq!(
            output.ints.get("status").map(Vec::as_slice),
            Some([0].as_slice()),
            "{name}"
        );
        let data = output
            .values
            .get("out")
            .ok_or_else(|| harness("missing exact result"))?;
        if name.contains("_null_") {
            // A successful null result is an observed output, not an empty pass.
            assert_eq!(data, &[0.0], "{name}");
            return Ok(());
        }

        assert_eq!(data.first(), Some(&1.0), "{name}: exact surface missing");
        assert_eq!(
            data.get(2),
            Some(&1.0),
            "{name}: returned surface has no NURB"
        );
        if name.starts_with("exact_plane_") {
            assert_eq!(
                data.get(1),
                Some(&(Plane::TYPE as f64)),
                "{name}: plane was sliced"
            );
            assert!(data.len() > 9, "{name}: plane geometry missing");
            return Ok(());
        }

        assert_eq!(data.get(1), Some(&(BSplineSurface::TYPE as f64)), "{name}");
        assert_eq!(
            data.get(3..9),
            Some([1.0, 1.0, 1.0, 1.0, 3.0, 3.0].as_slice()),
            "{name}"
        );
        // Independent homogeneous control nets: translation acts on w, and the
        // noncommuting chain must run in its original order, without dehomogenizing.
        let expected = if name.starts_with("exact_identity_") {
            [
                1.0, 2.0, 3.0, 1.0, 0.0, 4.0, 2.0, 2.0, 6.0, -2.0, 4.0, 2.0, 3.0, 6.0, 9.0, 3.0,
            ]
        } else if name.starts_with("exact_rigid_") {
            [
                0.0, -2.0, 8.0, 1.0, 0.0, -6.0, 12.0, 2.0, 6.0, 0.0, 14.0, 2.0, 0.0, -6.0, 24.0,
                3.0,
            ]
        } else {
            assert!(name.starts_with("exact_chain_"), "{name}");
            [
                -3.0, -5.0, -1.0, 1.0, -6.0, -6.0, -4.0, 2.0, 0.0, -8.0, 2.0, 2.0, -9.0, -15.0,
                -3.0, 3.0,
            ]
        };
        assert_eq!(
            data.get(9..25),
            Some(expected.as_slice()),
            "{name}: transformed control net"
        );
        assert_eq!(
            data.get(25..),
            Some([-2.0, -2.0, 3.0, 3.0, 4.0, 4.0, 6.0, 6.0].as_slice()),
            "{name}: knots"
        );
        Ok(())
    }

    #[test]
    fn exact_fixtures_compare_returned_geometry_and_nullness() {
        let text = include_str!("../../../../fixtures/surface.srf_in_volume/exact_results.json");
        let fixtures: Value = serde_json::from_str(text).unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 16);
        for fixture in cases {
            let name = fixture["name"].as_str().unwrap();
            let input: Inputs = serde_json::from_value(fixture["inputs"].clone()).unwrap();
            let native = cpp(&input).unwrap();
            let port = rust(&input).unwrap();
            assert_exact_result(name, &native).unwrap();
            assert_exact_result(name, &port).unwrap();
            let diff = crate::compare::compare(&native, &port, &[]);
            assert!(diff.pass(), "{name}: {diff:?}");
            assert!(diff.values_compared > 0, "{name}: empty comparison");
        }
    }

    #[test]
    fn exact_former_deferrals_compare_observed_results() {
        for index in [450, 602, 608, 1982] {
            let seed = CaseContext::seed_for("surface.srf_in_volume", index);
            let mut rng = Rng::new(seed);
            let input: Inputs = serde_json::from_value(Runner.generate(&mut rng).unwrap()).unwrap();
            assert!(matches!(input.op, OpIn::Exact), "gen{index:04}");
            let native = cpp(&input).unwrap();
            let port = rust(&input).unwrap();
            assert!(
                port.deferred.is_none(),
                "gen{index:04}: {:?}",
                port.deferred
            );
            assert_eq!(port.ints["status"], [0], "gen{index:04}");
            let diff = crate::compare::compare(&native, &port, &[]);
            assert!(diff.pass(), "gen{index:04}: {diff:?}");
            assert!(diff.values_compared > 0, "gen{index:04}: empty comparison");
        }
    }
}
