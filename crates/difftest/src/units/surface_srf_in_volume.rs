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
//!
//! `CopyCtor` runs the native copy constructor `SmSrfInVolume(const SmSrfInVolume&)`
//! directly, not through `Copy`, and compares the copy's state, the original-versus-copy
//! equality, against `Surface::clone` of the object moved into a [`Surface`]. Its
//! fixtures (`fixtures/surface.srf_in_volume/copy_ctor_knots.json`) write raw
//! near-coincident knots back over the base surface after construction, so an owned and
//! a borrowed base surface take different upstream paths. The generator does not produce
//! this op. Observed before the port's ownership-aware `Clone`: the original 15
//! fixtures gave 7 passes and 8 failures (owner flags 0 and 2); the native copy keeps
//! `out[62] = 0.5` for the raw U pair and `out[118] = 0.5` for the raw V pair, where the
//! derived clone had fixed the borrowed base surface up to `0.49999999399`. After the
//! fix all 17 fixtures pass, including the plain-volume owner 0/1 controls, and the whole
//! unit ran 2149 cases: 2074 with values, 0 failures, 0 deferrals, maximum relative
//! error 0, and 75 platform skips — this runner's existing `ReadDb` skips (native
//! `istream` extraction differs between libc++ and libstdc++), none added here; the
//! canonical x86-64 CI run is authoritative. Owner 0 and 1 objects with a plain
//! `SmVolume` head are legal inputs: the base `SmVolume::Copy` returns `SM_ERR` without
//! setting the output pointer, so a copy that does not own the volume shares it; owner 2
//! and 3 are rejected (an owning copy would delete the original's volume), and so is the
//! constructor's own copy of a plain volume.
//!
//! Virtual `Copy` invokes the same copy constructor, so its plain-volume domain
//! also admits effective owner flags 0/1 and rejects owning results 2/3.
//! `fixtures/surface.srf_in_volume/copy_volume_domain.json` compares real copied
//! surface and volume-chain state, including raw knots and a non-identity next map.
//! Existing generated inputs retain their copyable-head sampling domain.

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
use solidcore::surface::cone::Cone;
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
    bss_of, default_form, gen_bss, gen_srf, gen_sub_dom, push_bss, srf, BssIn, InlineSrf, SrfIn,
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
    Bspline {
        bss: BssIn,
        /// Knots written back over the surface's own NURB after construction, undoing
        /// the constructor's knot fix-up (surface block kind `3`). Omitted when false,
        /// so existing case identities are unchanged.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        raw_knots: bool,
    },
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    /// `SmCylinder(o, x, y, r, start, end, h, swap, inside, FALSE)` with its NURB made.
    Cylinder {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        r: f64,
        start: f64,
        end: f64,
        h: f64,
        swap: bool,
        inside: bool,
    },
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
    /// The copy constructor `SmSrfInVolume(const SmSrfInVolume&)`, invoked directly,
    /// against Rust's `Surface::clone` of the same object.
    CopyCtor,
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
        SurfIn::Bspline { bss, raw_knots } => {
            v.push(if *raw_knots { 3.0 } else { 0.0 });
            push_bss(v, bss)?;
        }
        SurfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend_from_slice(o);
            v.extend_from_slice(n);
        }
        SurfIn::Cylinder {
            o,
            x,
            y,
            r,
            start,
            end,
            h,
            swap,
            inside,
        } => {
            v.push(2.0);
            v.extend_from_slice(o);
            v.extend_from_slice(x);
            v.extend_from_slice(y);
            v.extend([*r, *start, *end, *h, b(*swap), b(*inside)]);
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
        OpIn::CopyCtor => Op::CopyCtor,
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
        OpIn::SwapUv
        | OpIn::Copy
        | OpIn::CopyCtor
        | OpIn::MakeOwner
        | OpIn::Exact
        | OpIn::ReadDb { .. } => {}
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
        next = Some(
            Volume::with_maps(kind, m.orient.map(transform_of), next)
                .map_err(|e| HarnessError::new(e.to_string()))?,
        );
    }
    next.ok_or_else(|| harness("volume without maps"))
}

fn build_surface(k: &Kernel, s: &SurfIn) -> Result<Surface, HarnessError> {
    match s {
        SurfIn::Bspline { bss, raw_knots } => {
            let mut b = bss_of(bss)?;
            if *raw_knots {
                // undo the constructor's fix-up, as the shim does
                let raw = srf(&bss.srf)?;
                let nurb = b
                    .get_or_create_gw_nurb()
                    .map_err(|e| harness(format!("surface: {e}")))?;
                nurb.u_knots_mut().copy_from_slice(&raw.u);
                nurb.v_knots_mut().copy_from_slice(&raw.v);
            }
            Ok(Surface::from(b))
        }
        SurfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SurfIn::Cylinder {
            o,
            x,
            y,
            r,
            start,
            end,
            h,
            swap,
            inside,
        } => {
            let mut c = Cone::new_cylinder_bounded(
                v3(*o),
                v3(*x),
                v3(*y),
                *r,
                *start,
                *end,
                *h,
                *swap,
                *inside,
                false,
            );
            // (the shim ignores MakeNurb's status: the ffi wrapper admits only inputs it
            // succeeds on)
            c.make_nurb(k)
                .map_err(|e| harness(format!("cylinder MakeNurb: {e}")))?;
            Ok(Surface::from(c))
        }
    }
}

fn build(k: &Kernel, c: &SivIn) -> Result<SrfInVolume, HarnessError> {
    SrfInVolume::new(
        k,
        build_surface(k, &c.surface)?,
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

/// Rejects `CopyCtor` object blocks whose native copy constructor would copy a domain
/// upstream leaves undefined: a cylinder base (the ffi wrapper admits it for the
/// evaluation ops only) and a plain `SmVolume` head that the constructor copies or that
/// the copy constructor's result owns (the base `Copy` fails without setting the output
/// pointer, so an owning result would delete the original's volume; an owner-1 result
/// only shares it). Mirrors the ffi wrapper's guards, so the Rust side never reaches its
/// clone on an input the shim must not see.
fn copy_ctor_defined(c: &SivIn) -> Result<(), HarnessError> {
    if matches!(c.surface, SurfIn::Cylinder { .. }) {
        return Err(harness("copy constructor: cylinder base is undefined"));
    }
    if c.volume.first().is_some_and(|m| m.kind == 0) {
        // the constructor itself copies the volume for copy flags 2 and 3
        if c.copy == 2 || c.copy == 3 {
            return Err(harness(
                "copy constructor: the constructor copies a plain volume",
            ));
        }
        // the result owns the volume only for owner flags 2 and 3
        if matches!(c.owner | c.copy, 2 | 3) {
            return Err(harness(
                "copy constructor: the copy owns a plain volume upstream cannot copy",
            ));
        }
    }
    Ok(())
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
        // an SmCylinder is an SmBSplineSurface: its NURB
        SurfaceKind::Cone(c) => put_bss(o, c.rev().base()),
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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
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
            let max = tri!(o, s.max_analytic_domain(&k,));
            o.push(0.0);
            o.extend([
                s.type_code() as f64,
                s.degree(SurfParamType::U) as f64,
                s.degree(SurfParamType::V) as f64,
            ]);
            put_dom(
                &mut o,
                &s.natural_uv_domain_with_nurb(&k)
                    .map_err(|e| HarnessError::new(e.to_string()))?,
            );
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
        OpIn::CopyCtor => {
            copy_ctor_defined(&i.siv)?;
            let c = siv_of(&s)?;
            // the Rust copy constructor: `Surface::clone` of the object moved into a
            // `Surface` (not `SrfInVolume::copy`)
            let copy = s.clone();
            let cc = siv_of(&copy)?;
            o.push(0.0);
            put_state(&mut o, cc);
            o.push(b(c.approx_eq(&k, cc).map_err(|e| {
                harness(format!("copy constructor equality: {e}"))
            })?));
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
            let ns = build_surface(&k, surface)?;
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
                // what the result evaluates to (the shim's `put_exact_eval`)
                let d = tri!(o, exact.natural_uv_domain_with_nurb(&k));
                let c = [d.u_min(), d.v_min(), d.u_max(), d.v_max()];
                o.extend(c);
                let finite = c.iter().all(|x| x.is_finite() && x.abs() < 1.0e12);
                o.push(b(finite));
                if finite {
                    let (iu, iv) = (d.u_interval(), d.v_interval());
                    for i in 0..=2 {
                        for j in 0..=2 {
                            let uv = Point2::new(
                                iu.evaluate(0.5 * f64::from(i)),
                                iv.evaluate(0.5 * f64::from(j)),
                            );
                            let p = exact.evaluate_point(&k, uv);
                            st!(o, p);
                            if let Ok(p) = p {
                                pv(&mut o, p);
                            }
                            let n = exact.evaluate_normal(&k, uv, Side::Left, Side::Left);
                            st!(o, n);
                            if let Ok(n) = n {
                                pv(&mut o, n);
                            }
                        }
                    }
                }
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
        (
            SurfIn::Bspline {
                bss: gen_bss(rng, s),
                raw_knots: false,
            },
            dom,
        )
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
        _ => [
            coord(rng, 5.0),
            coord(rng, 2.0),
            coord(rng, 2.0),
            coord(rng, 2.0),
        ],
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
        _ => (
            1 + rng.below(3) as usize,
            1 + rng.below(3) as usize,
            rng.chance(0.5),
        ),
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
            let (a, e) = if v_dir {
                (dom[1], dom[3])
            } else {
                (dom[0], dom[2])
            };
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
            let (a, e) = if v_dir {
                (dom[1], dom[3])
            } else {
                (dom[0], dom[2])
            };
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

// ---- exact images through bend, unbend and twist volumes ----------------------------

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

/// An orthonormal frame `(origin, mid, binormal, axis)`: axis-aligned or random.
fn gen_frame(rng: &mut Rng, scale: f64) -> [[f64; 3]; 4] {
    if rng.chance(0.3) {
        return [
            scale3(scale, coord(rng, 2.0)),
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
    }
    let axis = loop {
        let a = coord(rng, 1.0);
        if a.iter().map(|c| c * c).sum::<f64>() > 0.1 {
            break unit3(a);
        }
    };
    let mid = loop {
        let m = coord(rng, 1.0);
        let d = m[0] * axis[0] + m[1] * axis[1] + m[2] * axis[2];
        let m = add3(m, scale3(-d, axis));
        if m.iter().map(|c| c * c).sum::<f64>() > 0.1 {
            break unit3(m);
        }
    };
    [scale3(scale, coord(rng, 2.0)), mid, cross3(axis, mid), axis]
}

/// The point at frame coordinates `(a, b, c)`.
fn at_frame(f: &[[f64; 3]; 4], a: f64, b: f64, c: f64) -> [f64; 3] {
    add3(
        add3(f[0], scale3(a, f[1])),
        add3(scale3(b, f[2]), scale3(c, f[3])),
    )
}

/// The bilinear B-spline patch with corners `p[i][j]` (i along u, j along v).
fn patch(rng: &mut Rng, p: [[[f64; 3]; 2]; 2]) -> SurfIn {
    let (u0, v0) = if rng.chance(0.5) {
        (0.0, 0.0)
    } else {
        (rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0))
    };
    let (u1, v1) = (u0 + rng.uniform(0.5, 3.0), v0 + rng.uniform(0.5, 3.0));
    let row = |r: [[f64; 3]; 2]| r.iter().map(|q| [q[0], q[1], q[2], 1.0]).collect();
    SurfIn::Bspline {
        raw_knots: false,
        bss: BssIn {
            oob: false,
            form: default_form(),
            srf: SrfIn::Inline(InlineSrf {
                net: vec![row(p[0]), row(p[1])],
                p: 1,
                q: 1,
                u: vec![u0, u0, u1, u1],
                v: vec![v0, v0, v1, v1],
            }),
        },
    }
}

/// Two increasing values in `[lo, hi]`.
fn span(rng: &mut Rng, lo: f64, hi: f64) -> (f64, f64) {
    let a = rng.uniform(lo, hi);
    let b = rng.uniform(lo, hi);
    (a.min(b), a.max(b))
}

/// A patch over a random sub-rectangle of `x_range` by `y_range` in frame coordinates, `put(x, y)`
/// placing a point; reversed orientation, a tolerance-limit tilt (below or above the
/// approximation tolerance) or a degenerate (zero-width) side by chance.
fn frame_patch(
    rng: &mut Rng,
    x_range: (f64, f64),
    y_range: (f64, f64),
    tilt_dir: [f64; 3],
    put: &dyn Fn(f64, f64) -> [f64; 3],
) -> SurfIn {
    let (x0, x1) = span(rng, x_range.0, x_range.1);
    let (y0, y1) = span(rng, y_range.0, y_range.1);
    let x1 = if rng.chance(0.05) { x0 } else { x1 };
    let mut p = [[put(x0, y0), put(x0, y1)], [put(x1, y0), put(x1, y1)]];
    if rng.chance(0.15) {
        let t = if rng.chance(0.5) { 1e-8 } else { 1e-5 };
        p[1][1] = add3(p[1][1], scale3(t, tilt_dir));
    }
    if rng.chance(0.5) {
        p.swap(0, 1);
    }
    patch(rng, p)
}

/// A surface in volume whose head map is a bend, unbend or twist volume and whose
/// surface is aligned with that volume's frame (in-space or parameter space), so the
/// analytic branches of `MakeExactBSpline1stOutSurface` run; then `Exact`.
fn gen_exact_volume_case(rng: &mut Rng) -> Inputs {
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
    let b = match rng.below(6) {
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
                b,
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
    let f = if in_param || kind < 5 {
        [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    } else {
        frame
    };
    let f = if kind < 5 && !in_param {
        head.orient.unwrap_or(f)
    } else {
        f
    };
    let n = a.abs().max(1e-3 * scale);
    let s = scale;
    let surface = match (kind, rng.below(6)) {
        // twist: planes normal to the axis (height 0 is the zero angle)
        (2 | 5, 0..=3) => {
            let c = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) * s };
            frame_patch(rng, (-2.0 * s, 2.0 * s), (-2.0 * s, 2.0 * s), f[3], &|x, y| {
                at_frame(&f, x, y, c)
            })
        }
        // unbend: cylinders about the axis
        (3 | 6, 0 | 1) => {
            let (st, en) = span(rng, -170.0, 170.0);
            SurfIn::Cylinder {
                o: at_frame(&f, 0.0, 0.0, rng.uniform(-2.0, 2.0) * s),
                x: f[1],
                y: f[2],
                r: rng.uniform(0.3, 3.0) * s,
                start: st,
                end: en.max(st + 1.0),
                h: rng.uniform(0.2, 3.0) * s,
                swap: rng.chance(0.3),
                inside: rng.chance(0.5),
            }
        }
        // unbend: planes through the axis
        (3 | 6, 2) => {
            let t: f64 = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) };
            frame_patch(rng, (0.2 * s, 3.0 * s), (-2.0 * s, 2.0 * s), f[3], &|r, z| {
                at_frame(&f, r * t.cos(), r * t.sin(), z)
            })
        }
        // unbend: planes normal to the axis
        (3 | 6, 3) => {
            let c = rng.uniform(-2.0, 2.0) * s;
            frame_patch(rng, (0.2 * s, 3.0 * s), (-2.0 * s, 2.0 * s), f[3], &|x, y| {
                at_frame(&f, x, y, c)
            })
        }
        // bend: planes normal to the axis, the mid direction or the binormal
        (4 | 7, 0 | 1) => {
            let c = rng.uniform(-2.0, 2.0) * s;
            frame_patch(rng, (0.2 * s, 3.0 * s), (-3.0 * n, 3.0 * n), f[3], &|x, y| {
                at_frame(&f, x, y, c)
            })
        }
        (4 | 7, 2 | 3) => {
            let m = rng.uniform(0.2, 3.0) * s;
            frame_patch(rng, (-3.0 * n, 3.0 * n), (-2.0 * s, 2.0 * s), f[1], &|y, z| {
                at_frame(&f, m, y, z)
            })
        }
        (4 | 7, 4) => {
            let y0 = if rng.chance(0.2) { 0.0 } else { rng.uniform(-3.0, 3.0) * n };
            frame_patch(rng, (0.2 * s, 3.0 * s), (-2.0 * s, 2.0 * s), f[2], &|x, z| {
                at_frame(&f, x, y0, z)
            })
        }
        // anything else: an unaligned patch (no exact image) or one crossing the domain
        _ => {
            let c0 = rng.uniform(-2.0, 2.0) * s;
            frame_patch(rng, (-s, 3.0 * s), (-2.0 * s, 2.0 * s), f[3], &|x, y| {
                at_frame(&f, x, y, c0 + 0.3 * x)
            })
        }
    };
    let mut volume = vec![head.clone()];
    match rng.below(6) {
        0 => volume.push(MapIn {
            kind: 1,
            comp: None,
            t: Some(gen_transform(rng)),
            orient: None,
        }),
        1 => volume.push(head),
        _ => {}
    }
    Inputs {
        siv: SivIn {
            surface,
            volume,
            in_param,
            copy: 0,
            owner: 3,
        },
        op: OpIn::Exact,
    }
}

/// Generated `MakeExactBSplineIfPossible` cases on bend, unbend and twist volumes the
/// ffi wrapper accepts, from a fixed seed.
fn exact_volume_cases() -> Result<Vec<Case>, HarnessError> {
    const WANT: usize = 400;
    let mut rng = Rng::new(0x5eed_be4d);
    let mut cases = Vec::new();
    // hand-built: the bend plane bumps at angles -PI..PI (both orientations), an
    // unbend cylinder over almost a full turn, a zero-angle and a left-hand twist
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
    let rect = |x: [f64; 2], y: [f64; 2], z: f64, flip: bool| {
        let mut p = [
            [[x[0], y[0], z], [x[0], y[1], z]],
            [[x[1], y[0], z], [x[1], y[1], z]],
        ];
        if flip {
            p.swap(0, 1);
        }
        let row = |r: [[f64; 3]; 2]| r.iter().map(|q| [q[0], q[1], q[2], 1.0]).collect();
        SurfIn::Bspline {
            raw_knots: false,
            bss: BssIn {
                oob: false,
                form: default_form(),
                srf: SrfIn::Inline(InlineSrf {
                    net: vec![row(p[0]), row(p[1])],
                    p: 1,
                    q: 1,
                    u: vec![0.0, 0.0, 1.0, 1.0],
                    v: vec![0.0, 0.0, 1.0, 1.0],
                }),
            },
        }
    };
    let b = core::f64::consts::PI * (1.0 - 1e-7);
    let hand = [
        (comp(7, 1.0), rect([0.5, 2.0], [-b, b], 0.5, false)),
        (comp(7, 1.0), rect([0.5, 2.0], [-b, b], 0.5, true)),
        (comp(7, 1.0), rect([0.5, 2.0], [-1.0, 1.0], 0.5, false)),
        (
            comp(6, 1.0),
            SurfIn::Cylinder {
                o: [0.0, 0.0, -1.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                r: 1.5,
                start: -179.0,
                end: 179.0,
                h: 2.0,
                swap: false,
                inside: false,
            },
        ),
        (comp(5, 2.0), rect([-1.0, 1.0], [-1.0, 1.0], 0.0, false)),
        (comp(5, -3.0), rect([-1.0, 1.0], [-1.0, 1.0], 0.75, true)),
    ];
    for (n, (map, surface)) in hand.into_iter().enumerate() {
        for in_param in [false, true] {
            let i = Inputs {
                siv: SivIn {
                    surface: surface.clone(),
                    volume: vec![map.clone()],
                    in_param,
                    copy: 0,
                    owner: 3,
                },
                op: OpIn::Exact,
            };
            let (op, input) = cpp_input(&i)?;
            ffi::validate(op, &input).map_err(|e| harness(format!("{e:?}")))?;
            cases.push(Case {
                unit: "surface.srf_in_volume".into(),
                name: format!("exact_volume_hand_{n}_{}", u8::from(in_param)),
                source: CaseSource::Inline,
                inputs: serde_json::to_value(&i).map_err(|e| harness(e.to_string()))?,
            });
        }
    }
    let hand_count = cases.len();
    for _ in 0..20_000 {
        if cases.len() >= hand_count + WANT {
            break;
        }
        let i = gen_exact_volume_case(&mut rng);
        let Ok((op, input)) = cpp_input(&i) else {
            continue;
        };
        if ffi::validate(op, &input).is_err() {
            continue;
        }
        cases.push(Case {
            unit: "surface.srf_in_volume".into(),
            name: format!("exact_volume_{}", cases.len() - hand_count),
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
                    name: format!(
                        "inline_{name}_{}_{op_name}",
                        if in_param { "param" } else { "in" }
                    ),
                    source: CaseSource::Inline,
                    inputs: serde_json::to_value(inputs)
                        .map_err(|e| harness(format!("inline case: {e}")))?,
                });
            }
        }
    }
    out.extend(cylinder_cases()?);
    Ok(out)
}

/// `Info` (and point evaluation) on a surface in volume over an `SmCylinder` base: the
/// object forwards `ConvertUVFromSTEPToNURBS` / `ConvertUVFromNURBSToSTEP` (and the STEP
/// domain) to the analytic base, whose conversions are not the identity (STEP u in
/// degrees over [start, end], NURBS u over the rational circle's knots).
fn cylinder_cases() -> Result<Vec<Case>, HarnessError> {
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
    let cylinders = [
        ("full", 0.0, 360.0, false, false),
        ("arc", 30.0, 200.0, false, false),
        ("arc_swap", 30.0, 200.0, true, false),
        ("arc_inside", -90.0, 120.0, false, true),
    ];
    let mut out = Vec::new();
    for (name, start, end, swap, inside) in cylinders {
        let siv = SivIn {
            surface: SurfIn::Cylinder {
                o: [0.5, -1.0, 2.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                r: 2.5,
                start,
                end,
                h: 4.0,
                swap,
                inside,
            },
            volume: vec![transform.clone()],
            in_param: false,
            copy: 0,
            owner: 3,
        };
        // a STEP (degrees, height) point and a NURBS-range point, as the ops convert both
        let ops = [
            (
                "info_step",
                OpIn::Info {
                    uv: [0.5 * (start + end), 1.5],
                    types: vec![],
                },
            ),
            (
                "info_unit",
                OpIn::Info {
                    uv: [0.375, 0.625],
                    types: vec![],
                },
            ),
            ("eval_point", OpIn::EvalPoint { uv: [0.375, 0.625] }),
        ];
        for (op_name, op) in ops {
            let inputs = Inputs {
                siv: siv.clone(),
                op,
            };
            let (o, input) = cpp_input(&inputs)?;
            ffi::validate(o, &input).map_err(|e| harness(format!("cylinder case: {e}")))?;
            out.push(Case {
                unit: "surface.srf_in_volume".to_string(),
                name: format!("inline_cylinder_{name}_{op_name}"),
                source: CaseSource::Inline,
                inputs: serde_json::to_value(inputs)
                    .map_err(|e| harness(format!("cylinder case: {e}")))?,
            });
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
            // Keep generated cases in their established copyable-head domain;
            // the newly admitted plain-volume Copy domain has named fixtures.
            if matches!(i.op, OpIn::Copy) && i.siv.volume.first().is_some_and(|map| map.kind == 0) {
                continue;
            }
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
        cases.extend(exact_volume_cases()?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn skipped_platform(&self, case: &Case) -> Option<String> {
        let i: Inputs = case.parse().ok()?;
        if matches!(i.op, OpIn::ReadDb { .. }) {
            Some("libc++ istream extraction differs from libstdc++".into())
        } else {
            None
        }
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
            data.get(25..33),
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
    fn exact_volume_hand_cases_build_analytic_images() {
        let want = [
            Plane::TYPE,
            Plane::TYPE,
            Plane::TYPE,
            Plane::TYPE,
            BSplineSurface::TYPE,
            BSplineSurface::TYPE,
        ];
        let cases: Vec<Case> = exact_volume_cases()
            .unwrap()
            .into_iter()
            .filter(|c| c.name.starts_with("exact_volume_hand_"))
            .collect();
        assert_eq!(cases.len(), 2 * want.len());
        for c in cases {
            let input: Inputs = c.parse().unwrap();
            let native = cpp(&input).unwrap();
            let port = rust(&input).unwrap();
            let n: usize = c.name["exact_volume_hand_".len()..][..1].parse().unwrap();
            let out = &port.values["out"];
            assert_eq!(out.first(), Some(&1.0), "{}: no exact image", c.name);
            assert_eq!(out.get(1), Some(&(want[n] as f64)), "{}", c.name);
            let diff = crate::compare::compare(&native, &port, &[]);
            assert!(diff.pass(), "{}: {diff:?}", c.name);
            assert!(diff.values_compared > 40, "{}: {diff:?}", c.name);
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
