//! Runner for unit `surface.surf_of_revolution_p3`: `SmSurfOfRevolution` part 3
//! (`Copy`, `Reparameterize`, `ConvertUVFromSTEPToNURBS`, `DropPointFast`, the
//! intersections with surfaces of revolution, cones and spheres about the same axis,
//! `RebuildSTEPFromNURBParameters`, `Transform`, `UpdateAnalyticalDomain` and
//! `ReadFromDB`), plus the inherited `SmBSplineSurface::Reverse` and `SwapUV`, which
//! reach the rebuild through its virtual (run through [`Surface`] on the Rust side).
//!
//! C++: `solidcore_ffi::units::surface_surf_of_revolution_p3` (upstream
//! `source/SMLib/src/SmSurfOfRevolution.cpp`). Rust:
//! `solidcore::surface::surf_of_revolution::p3`.
//!
//! A case is `surf` (the surface block of the `p1` runner) plus `op` and its
//! parameters. Both sides produce the same flat record (layout in the FFI module
//! docs), split into `build` (the construction status), `other_build` (for
//! `intersect`, the other surface's construction status), `status` (the op status)
//! and `out` (the rest). `read_db` has no construction status: its bytes are the
//! upstream `WriteToDB` record of `surf` (written by the C++ side when the case is
//! generated). Statuses are `0` (`SM_SUCCESS`) or the `SmStatus` (`InvalidInput` is
//! `1007`, `SM_ERR_INVALID_INPUT`); `-1` on the Rust side for an error upstream cannot
//! produce.
//!
//! Deferred cases: `ConvertUVFromSTEPToNURBS` without polar conversion and
//! `DropPointFast` with an inexact angle (`SmSurface::LocalPointSolve`,
//! `surface.surface_p5`); the intersections without approximation tolerance (the
//! bounding box's zone tolerance, `SmSurface::GetFace`, `surface.surface_p1`) or
//! reaching `ConvertUVFromNURBSToSTEP` without exact conversion (`SmSTEPSurface`,
//! `surface.surface_p1`); `UpdateAnalyticalDomain` reaching the latter; every
//! `IntersectWithCone` / `IntersectWithSphere` case (the Rust side cannot build the
//! `SmCone` / `SmSphere` operand: `surface.cone_p1`, `surface.sphere`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::database_io::{DatabaseReader, FileType, InputStream};
use solidcore_ffi::units::surface_surf_of_revolution_p3 as ffi;

use super::surface_surf_of_revolution_p1::{
    add, cross3, ffi_surf, flag, frame, gen_generator, gen_surf, normalize, param_of, put_curve,
    put_state, rust_surf, unit_vec, v3, BuildIn, GenIn, SurfIn, SC_ERR_ARG,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 900;

/// `surface.surf_of_revolution_p3` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// A placement `SmAxis2Placement(origin, x, y)` with an optional scale.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct FrameIn {
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default)]
    scale: Option<[f64; 3]>,
}

/// The other operand of `intersect`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OtherIn {
    /// Another surface of revolution; `None` is the same surface.
    Revolution {
        #[serde(default)]
        surf: Option<Box<SurfIn>>,
    },
    Cone {
        frame: [[f64; 3]; 3],
        r0: f64,
        r1: f64,
        h: f64,
    },
    Sphere {
        frame: [[f64; 3]; 3],
        r: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Copy {},
    Reparam {
        dom: [f64; 4],
    },
    ToNurbs {
        u: f64,
        v: f64,
    },
    Drop {
        /// `None`: the natural domain.
        #[serde(default)]
        dom: Option<[f64; 4]>,
        operation: i32,
        point: [f64; 3],
        #[serde(default)]
        vec: Option<[f64; 3]>,
        tol: f64,
        #[serde(default)]
        target: Option<f64>,
        requested: i32,
    },
    Intersect {
        other: OtherIn,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        other_dom: Option<[f64; 4]>,
        #[serde(default)]
        tol: Option<f64>,
    },
    Rebuild {
        #[serde(default)]
        edit: Option<FrameIn>,
    },
    Transform {
        frame: FrameIn,
    },
    Update {
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    /// The inherited `SmBSplineSurface::Reverse` (Rust: [`Surface::reverse`]).
    Reverse {
        param: u8,
    },
    /// The inherited `SmBSplineSurface::SwapUV` (Rust: [`Surface::swap_uv`]).
    Swap {},
    /// `bytes` absent in a fixture: filled with the C++ `WriteToDB` record of `surf`
    /// when the cases are loaded.
    ReadDb {
        binary: bool,
        version: u32,
        #[serde(default)]
        bytes: Option<Vec<u8>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surf: SurfIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_dom(d: Option<[f64; 4]>) -> ffi::Dom {
    d.map_or(ffi::Dom::Natural, ffi::Dom::Explicit)
}

fn ffi_frame(f: &FrameIn) -> ffi::Frame {
    ffi::Frame {
        origin: f.origin,
        x: f.x,
        y: f.y,
        scale: f.scale,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let s = ffi_surf(&i.surf)?;
    let flat = if let OpIn::ReadDb {
        binary,
        version,
        bytes,
    } = &i.op
    {
        let bytes = bytes.as_ref().ok_or_else(|| harness("read_db: no bytes"))?;
        ffi::read_db(&s, *binary, *version, bytes)
    } else {
        let op = match &i.op {
            OpIn::Copy {} => ffi::Op::Copy,
            OpIn::Reparam { dom } => ffi::Op::Reparam { dom: *dom },
            OpIn::ToNurbs { u, v } => ffi::Op::ToNurbs { u: *u, v: *v },
            OpIn::Drop {
                dom,
                operation,
                point,
                vec,
                tol,
                target,
                requested,
            } => ffi::Op::Drop {
                dom: ffi_dom(*dom),
                operation: *operation,
                point: *point,
                vec: *vec,
                tol: *tol,
                target: *target,
                requested: *requested,
            },
            OpIn::Intersect {
                other,
                dom,
                other_dom,
                tol,
            } => ffi::Op::Intersect {
                other: match other {
                    OtherIn::Revolution { surf } => ffi::Other::Revolution(match surf {
                        Some(o) => Some(Box::new(ffi_surf(o)?)),
                        None => None,
                    }),
                    OtherIn::Cone { frame, r0, r1, h } => ffi::Other::Cone {
                        frame: *frame,
                        r0: *r0,
                        r1: *r1,
                        h: *h,
                    },
                    OtherIn::Sphere { frame, r } => ffi::Other::Sphere {
                        frame: *frame,
                        r: *r,
                    },
                },
                dom: ffi_dom(*dom),
                other_dom: ffi_dom(*other_dom),
                tol: *tol,
            },
            OpIn::Rebuild { edit } => ffi::Op::Rebuild {
                edit: edit.as_ref().map(ffi_frame),
            },
            OpIn::Transform { frame } => ffi::Op::Transform {
                frame: ffi_frame(frame),
            },
            OpIn::Update { dom } => ffi::Op::Update {
                dom: ffi_dom(*dom),
            },
            OpIn::Reverse { param } => ffi::Op::Reverse { param: *param },
            OpIn::Swap {} => ffi::Op::Swap,
            OpIn::ReadDb { .. } => return Err(harness("unreachable")),
        };
        ffi::surface_op(&s, &op)
    }
    .map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Splits a record into `build`, `other_build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut rest = flat;
    if !matches!(i.op, OpIn::ReadDb { .. }) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("empty record"))?;
        out = out.values("build", vec![*b]);
        rest = r;
        if *b != 0.0 {
            if !rest.is_empty() {
                return Err(harness("record continues after a failed build"));
            }
            return Ok(out);
        }
    }
    if matches!(i.op, OpIn::Intersect { .. }) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("record has no other build status"))?;
        out = out.values("other_build", vec![*b]);
        rest = r;
        if *b != 0.0 {
            if !rest.is_empty() {
                return Err(harness("record continues after a failed other build"));
            }
            return Ok(out);
        }
    }
    let (s, r) = rest
        .split_first()
        .ok_or_else(|| harness("record has no op status"))?;
    out = out.values("status", vec![*s]);
    Ok(out.values("out", r.to_vec()))
}

// ---- Rust side ------------------------------------------------------------------------

/// The `SmStatus` of a Rust error: the upstream code, `1007` (`SM_ERR_INVALID_INPUT`)
/// for `InvalidInput` (e.g. `SmBSplineCurve::Trim` of an interval shorter than
/// `SM_EFF_ZERO`, reached by `UpdateAnalyticalDomain`), else `-1`.
fn status(e: &solidcore::SolidError) -> f64 {
    match e {
        solidcore::SolidError::InvalidInput { .. } => 1007.0,
        _ => super::surface_surf_of_revolution_p1::status(e),
    }
}

/// The surface named by the drop's solutions: a surface with the given natural domain.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfHandle(Extent2d);

impl SolutionObject for SurfHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        Some(self.0)
    }
}

fn dom_of(s: &SurfOfRevolution, d: Option<[f64; 4]>) -> Extent2d {
    d.map_or_else(
        || s.base().natural_uv_domain(),
        |[a, b, c, e]| Extent2d::from_bounds(a, b, c, e),
    )
}

fn frame_of(f: &FrameIn) -> (Axis2Placement, Option<Vec3>) {
    (
        Axis2Placement::new(v3(f.origin), v3(f.x), v3(f.y)),
        f.scale.map(v3),
    )
}

/// The surface of revolution a [`Surface`] holds.
fn revolution(s: &Surface) -> solidcore::Result<&SurfOfRevolution> {
    match s.kind() {
        SurfaceKind::SurfOfRevolution(r) => Ok(r.as_ref()),
        _ => Err(solidcore::SolidError::sm_err("difftest: not a surface of revolution")),
    }
}

/// Pushes `0` and the state of `s`.
fn ok_state(o: &mut Vec<f64>, s: &SurfOfRevolution) -> solidcore::Result<()> {
    o.push(0.0);
    put_state(o, s)
}

/// Runs the op on the Rust surface, appending its status and outputs (for
/// `intersect`, the other surface's construction status first).
fn rust_op(
    k: &Kernel,
    s: &SurfOfRevolution,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<solidcore::Result<()>, HarnessError> {
    let r: solidcore::Result<()> = match op {
        OpIn::Copy {} => {
            let c = s.copy();
            ok_state(o, &c).and_then(|()| {
                o.push(flag(c.approx_eq(k, s)?));
                Ok(())
            })
        }
        OpIn::Reparam { dom } => {
            let mut c = s.clone();
            let [a, b, cc, d] = *dom;
            c.reparameterize(k, &Extent2d::from_bounds(a, b, cc, d))
                .and_then(|()| ok_state(o, &c))
        }
        OpIn::ToNurbs { u, v } => s
            .convert_uv_from_step_to_nurbs(k, Point2::new(*u, *v))
            .map(|p| o.extend([0.0, p.x, p.y])),
        OpIn::Drop {
            dom,
            operation,
            point,
            vec,
            tol,
            target,
            requested,
        } => {
            let operation = SolverOperation::from_code(*operation)
                .ok_or_else(|| harness("drop: operation"))?;
            let requested = SolutionRequested::from_code(*requested)
                .ok_or_else(|| harness("drop: requested"))?;
            let me = SurfHandle(s.base().natural_uv_domain());
            s.drop_point_fast(
                k,
                &dom_of(s, *dom),
                operation,
                v3(*point),
                vec.map(v3),
                *tol,
                *target,
                requested,
                Some(me),
            )
            .map(|sols| {
                o.extend([0.0, sols.len() as f64]);
                for sol in sols.as_slice() {
                    o.extend([
                        f64::from(sol.solution_type as u8),
                        sol.num_objects as f64,
                        sol.num_variables as f64,
                        flag(sol.objects[0] == Some(me)),
                        sol.start.solution_value,
                        sol.start.parameters[0],
                        sol.start.parameters[1],
                    ]);
                }
            })
        }
        OpIn::Intersect {
            other,
            dom,
            other_dom,
            tol,
        } => {
            let built;
            let other_s: &SurfOfRevolution = match other {
                OtherIn::Revolution { surf: None } => s,
                OtherIn::Revolution { surf: Some(os) } => match rust_surf(k, os)? {
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(e) => {
                        o.push(status(&e));
                        return Ok(Ok(()));
                    }
                    Ok(b) => {
                        built = b;
                        &built
                    }
                },
                OtherIn::Cone { .. } => {
                    return Ok(Err(solidcore::SolidError::unported(
                        "SmCone::CreateCanonical",
                        "surface.cone_p1",
                    )))
                }
                OtherIn::Sphere { .. } => {
                    return Ok(Err(solidcore::SolidError::unported(
                        "SmSphere::CreateCanonical",
                        "surface.sphere",
                    )))
                }
            };
            o.push(0.0);
            s.intersect_with_surf_of_revolution(
                k,
                &dom_of(s, *dom),
                other_s,
                &dom_of(other_s, *other_dom),
                *tol,
            )
            .map(|(more, x)| {
                o.extend([0.0, flag(more), x.curves_3d.len() as f64]);
                for c in &x.curves_3d {
                    put_curve(o, Some(c));
                }
                o.push(x.curve_types.len() as f64);
                o.extend(x.curve_types.iter().map(|t| f64::from(*t as u8)));
                o.push(x.deviations.len() as f64);
                o.extend_from_slice(&x.deviations);
                let null1 = x.surface1_uv_curves.iter().filter(|c| c.is_none()).count();
                let null2 = x.surface2_uv_curves.iter().filter(|c| c.is_none()).count();
                o.extend([
                    x.surface1_uv_curves.len() as f64,
                    x.surface2_uv_curves.len() as f64,
                    null1 as f64,
                    null2 as f64,
                ]);
            })
        }
        OpIn::Rebuild { edit } => {
            let mut c = s.clone();
            if let Some(f) = edit {
                let (rf, scale) = frame_of(f);
                if let Err(e) = c.base_mut().transform(&rf, scale) {
                    if e.is_unported() {
                        return Ok(Err(e));
                    }
                    o.push(SC_ERR_ARG);
                    return Ok(Ok(()));
                }
            }
            c.rebuild_step_from_nurb_parameters(k)
                .and_then(|()| ok_state(o, &c))
        }
        OpIn::Transform { frame } => {
            let mut c = s.clone();
            let (rf, scale) = frame_of(frame);
            c.transform(k, &rf, scale).and_then(|()| ok_state(o, &c))
        }
        OpIn::Update { dom } => {
            let mut c = s.clone();
            let d = dom_of(&c, *dom);
            c.update_analytical_domain(k, &d)
                .and_then(|()| ok_state(o, &c))
        }
        OpIn::Reverse { param } => {
            let mut c = Surface::from(s.clone());
            c.reverse(k, param_of(*param))
                .and_then(|()| ok_state(o, revolution(&c)?))
        }
        OpIn::Swap {} => {
            let mut c = Surface::from(s.clone());
            c.swap_uv(k).and_then(|()| ok_state(o, revolution(&c)?))
        }
        OpIn::ReadDb { .. } => return Err(harness("unreachable")),
    };
    Ok(r)
}

fn rust_read(k: &Kernel, binary: bool, version: u32, bytes: &[u8]) -> Result<Output, HarnessError> {
    let ft = if binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let mut db = DatabaseReader::with_stream(ft, InputStream::new(bytes.to_vec()));
    let mut o = Vec::new();
    match SurfOfRevolution::read_from_db(k, &mut db, u64::from(version)) {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(s) => {
            if let Err(e) = ok_state(&mut o, &s) {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                return Err(harness(format!("state: {e}")));
            }
        }
    }
    Ok(Output::new()
        .values("status", vec![o.first().copied().unwrap_or(-1.0)])
        .values("out", o.get(1..).unwrap_or(&[]).to_vec()))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    if let OpIn::ReadDb {
        binary,
        version,
        bytes,
    } = &i.op
    {
        let bytes = bytes.as_ref().ok_or_else(|| harness("read_db: no bytes"))?;
        return rust_read(&k, *binary, *version, bytes);
    }
    let mut o = Vec::new();
    match rust_surf(&k, &i.surf)? {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(surf) => {
            o.push(0.0);
            let at = o.len();
            if let Err(e) = rust_op(&k, &surf, &i.op, &mut o)? {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                // keep the other surface's construction status of an intersection
                let keep = if matches!(i.op, OpIn::Intersect { .. }) {
                    at + 1
                } else {
                    at
                };
                o.truncate(keep);
                o.push(status(&e));
            }
        }
    }
    to_output(i, &o)
}

// ---- generation -----------------------------------------------------------------------

/// Scales every coordinate of the surface block by `f` (huge and tiny models).
pub(crate) fn scale_surf(s: &mut SurfIn, f: f64) {
    let sc = |p: &mut [f64; 3]| {
        for c in p.iter_mut() {
            *c *= f;
        }
    };
    match &mut s.gen {
        GenIn::BSpline(c) => c.cpts.iter_mut().for_each(sc),
        GenIn::Line { a, b } => {
            sc(a);
            sc(b);
        }
        GenIn::Circle { c, r, .. } => {
            sc(c);
            *r *= f;
        }
    }
    match &mut s.build {
        BuildIn::Canonical { point, .. } => sc(point),
        BuildIn::Ctor { origin, .. } => sc(origin),
    }
}

/// A sub-domain of `d` (fractions of each side), or `None` (the natural domain).
pub(crate) fn sub_dom(rng: &mut Rng, d: &Extent2d) -> Option<[f64; 4]> {
    if rng.chance(0.5) {
        return None;
    }
    let a = d.evaluate(rng.uniform(0.0, 0.45), rng.uniform(0.0, 0.45));
    let b = d.evaluate(rng.uniform(0.55, 1.0), rng.uniform(0.55, 1.0));
    Some([a.x, a.y, b.x, b.y])
}

fn rand_frame(rng: &mut Rng) -> FrameIn {
    let z = unit_vec(rng);
    let t = unit_vec(rng);
    let y = normalize(cross3(z, t));
    let x = cross3(y, z);
    let origin = if rng.chance(0.3) {
        [0.0; 3]
    } else {
        rng.point3(-5.0, 5.0)
    };
    let scale = match rng.below(5) {
        0 | 1 => None,
        2 => {
            let s = rng.uniform(0.2, 5.0);
            Some([s, s, s])
        }
        3 => Some([1.0, 1.0, 1.0]),
        _ => Some([rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]),
    };
    if rng.chance(0.1) {
        // the identity frame
        return FrameIn {
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            scale,
        };
    }
    FrameIn {
        origin,
        x,
        y,
        scale,
    }
}

/// Another surface of revolution about the same axis as `s` (when `coaxial`), with a
/// planar generator.
pub(crate) fn other_surf(rng: &mut Rng, s: &SurfOfRevolution, coaxial: bool) -> SurfIn {
    let p = s.position();
    let (o, x, y, z) = (p.origin(), p.x_axis(), p.y_axis(), p.z_axis());
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let (mut o, x, y, mut z) = (arr(o), arr(x), arr(y), arr(z));
    if !coaxial {
        match rng.below(2) {
            0 => o = add(o, normalize(cross3(z, unit_vec(rng))), rng.uniform(1e-4, 1.0)),
            _ => z = normalize(add(z, unit_vec(rng), rng.uniform(1e-3, 0.5))),
        }
    }
    // rotate the X axis about the axis, maybe flip the axis
    let ang = rng.uniform(-180.0, 180.0).to_radians();
    let (c, sn) = (ang.cos(), ang.sin());
    let x2 = add([x[0] * c, x[1] * c, x[2] * c], y, sn);
    let flip = rng.chance(0.3);
    let z2 = if flip { [-z[0], -z[1], -z[2]] } else { z };
    let x2 = if coaxial {
        normalize(x2)
    } else {
        normalize(cross3(cross3(z2, x2), z2))
    };
    let y2 = cross3(z2, x2);
    let f = (o, x2, y2, z2);
    let gen = gen_generator(rng, &f, true);
    let build = if rng.chance(0.5) {
        BuildIn::Canonical { point: o, axis: z2 }
    } else {
        let [v0, v1] = match &gen {
            GenIn::BSpline(c) => [
                c.u.first().copied().unwrap_or(0.0),
                c.u.last().copied().unwrap_or(1.0),
            ],
            GenIn::Line { .. } => [0.0, 1.0],
            GenIn::Circle { d, .. } => *d,
        };
        let a = rng.uniform(-180.0, 180.0);
        BuildIn::Ctor {
            origin: o,
            x: x2,
            y: y2,
            domain: [a, v0, (a + rng.uniform(20.0, 360.0)).min(360.0), v1],
        }
    };
    SurfIn {
        gen,
        build,
        oob: false,
        swap: rng.chance(0.4),
    }
}

/// A coaxial cylinder or cone sharing a radius with the generator of `s` at some
/// height: a line generator at a radius the surface reaches (crossing intersections).
pub(crate) fn crossing_surf(rng: &mut Rng, s: &SurfOfRevolution, k: &Kernel) -> Option<SurfIn> {
    let p = s.position();
    let (o, x, z) = (p.origin(), p.x_axis(), p.z_axis());
    let sd = s.step_uv_domain();
    let q = s
        .evaluate_step_point(k, Point2::new(sd.min().x, sd.evaluate(0.5, rng.unit()).y))
        .ok()?;
    let h = (q - o).dot(z);
    let r = ((q - o) - h * z).length();
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let base = |rr: f64, hh: f64| arr(o + rr * x + hh * z);
    let tilt = if rng.chance(0.5) { 0.0 } else { rng.uniform(-0.5, 0.5) };
    let a = base(r - tilt, h - 1.5);
    let b = base(r + tilt, h + 1.5);
    Some(SurfIn {
        gen: GenIn::Line { a, b },
        build: BuildIn::Canonical {
            point: arr(o),
            axis: arr(if rng.chance(0.3) { -z } else { z }),
        },
        oob: false,
        swap: rng.chance(0.4),
    })
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let op_kind = rng.below(22);
    // the drops and intersections mostly get planar generators (the others only test
    // the early exits)
    let want_planar = (4..=13).contains(&op_kind) && rng.chance(0.8);
    let mut s = gen_surf(rng);
    for _ in 0..6 {
        let planar = rust_surf(&k, &s)
            .ok()
            .and_then(Result::ok)
            .is_some_and(|b| b.planar_generator());
        if planar || !want_planar {
            break;
        }
        s = gen_surf(rng);
    }
    if rng.chance(0.12) {
        let f = if rng.chance(0.5) { 1e-3 } else { 1e3 };
        scale_surf(&mut s, f);
    }
    // the Rust surface sizes the parameters (both sides get the same numbers)
    let built = rust_surf(&k, &s).ok()?.ok();
    let nd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), |b| {
            b.base().natural_uv_domain()
        });
    let sd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0), |b| {
            b.step_uv_domain()
        });
    let size = built
        .as_ref()
        .and_then(|b| b.evaluate_step_point(&k, sd.evaluate(0.5, 0.5)).ok())
        .map_or(1.0, |p| 1.0 + p.length());
    let pick = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        match rng.below(10) {
            0 => lo,
            1 => hi,
            2 => lo - rng.uniform(0.0, 0.5) * (hi - lo),
            _ => lo + rng.unit() * (hi - lo),
        }
    };
    let op = match op_kind {
        0 => OpIn::Copy {},
        1 => {
            let a = rng.uniform(-5.0, 5.0);
            let b = rng.uniform(-5.0, 5.0);
            OpIn::Reparam {
                dom: [a, b, a + rng.uniform(0.1, 10.0), b + rng.uniform(0.1, 10.0)],
            }
        }
        2 | 3 => OpIn::ToNurbs {
            u: pick(rng, sd.min().x, sd.max().x),
            v: pick(rng, sd.min().y, sd.max().y),
        },
        4..=8 => {
            let b = built.as_ref()?;
            let point = match rng.below(5) {
                // near the surface
                0..=2 => {
                    let uv = Point2::new(
                        sd.min().x + rng.unit() * (sd.max().x - sd.min().x),
                        sd.min().y + rng.unit() * (sd.max().y - sd.min().y),
                    );
                    let p = b.evaluate_step_point(&k, uv).ok()?;
                    let d = rng.point3(-0.3, 0.3);
                    let e = if rng.chance(0.3) { 0.0 } else { size * 0.1 };
                    [p.x + e * d[0], p.y + e * d[1], p.z + e * d[2]]
                }
                // on the axis
                3 => {
                    let o = b.position().origin();
                    let z = b.position().z_axis();
                    let t = rng.uniform(-2.0, 2.0) * size;
                    [o.x + t * z.x, o.y + t * z.y, o.z + t * z.z]
                }
                _ => {
                    let p = rng.point3(-5.0, 5.0);
                    [p[0] * size, p[1] * size, p[2] * size]
                }
            };
            OpIn::Drop {
                dom: sub_dom(rng, &nd),
                operation: *rng.choose(&[3, 3, 3, 0, 5, 4])?,
                point,
                vec: rng.chance(0.3).then(|| unit_vec(rng)),
                tol: *rng.choose(&[1e-9, 1e-6, 1e-4, 1e-2])? * size,
                target: rng.chance(0.15).then(|| rng.uniform(0.0, 2.0) * size),
                requested: i32::from(!rng.chance(0.3)),
            }
        }
        9..=13 => {
            let b = built.as_ref()?;
            let other = match rng.below(12) {
                0..=4 => OtherIn::Revolution {
                    surf: Some(Box::new(other_surf(rng, b, true))),
                },
                5..=7 => OtherIn::Revolution {
                    surf: Some(Box::new(crossing_surf(rng, b, &k)?)),
                },
                8 => OtherIn::Revolution {
                    surf: Some(Box::new(other_surf(rng, b, false))),
                },
                9 => OtherIn::Revolution { surf: None },
                10 => {
                    let f = frame(rng);
                    OtherIn::Cone {
                        frame: [f.0, f.1, f.2],
                        r0: rng.uniform(0.5, 3.0),
                        r1: rng.uniform(0.0, 3.0),
                        h: rng.uniform(0.5, 4.0),
                    }
                }
                _ => {
                    let f = frame(rng);
                    OtherIn::Sphere {
                        frame: [f.0, f.1, f.2],
                        r: rng.uniform(0.5, 3.0),
                    }
                }
            };
            let other_dom = match &other {
                OtherIn::Revolution { surf: Some(o) } => rust_surf(&k, o)
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|os| sub_dom(rng, &os.base().natural_uv_domain())),
                _ => None,
            };
            OpIn::Intersect {
                other,
                dom: sub_dom(rng, &nd),
                other_dom,
                tol: if rng.chance(0.12) {
                    None
                } else {
                    Some(*rng.choose(&[1e-7, 1e-6, 1e-4, 1e-3])? * size)
                },
            }
        }
        14 | 15 => {
            let edit = match rng.below(4) {
                0 | 1 => None,
                _ => Some(rand_frame(rng)),
            };
            OpIn::Rebuild { edit }
        }
        16 | 17 => OpIn::Transform {
            frame: rand_frame(rng),
        },
        18 => OpIn::Update {
            dom: sub_dom(rng, &nd),
        },
        20 => OpIn::Reverse {
            param: rng.below(2) as u8,
        },
        21 => OpIn::Swap {},
        _ => {
            let binary = rng.chance(0.5);
            let version = *rng.choose(&[30, 33])?;
            let bytes = ffi::write_db_bytes(&ffi_surf(&s).ok()?, binary, version).ok()??;
            OpIn::ReadDb {
                binary,
                version,
                bytes: Some(bytes),
            }
        }
    };
    Some(Inputs { surf: s, op })
}

/// Fills the bytes of a fixture `read_db` case with the C++ `WriteToDB` record.
fn fill_read_bytes(case: &mut Case) -> Result<(), HarnessError> {
    let mut i: Inputs = case.parse()?;
    let OpIn::ReadDb {
        binary,
        version,
        bytes,
    } = &mut i.op
    else {
        return Ok(());
    };
    if bytes.is_some() {
        return Ok(());
    }
    let s = ffi_surf(&i.surf)?;
    *bytes = Some(
        ffi::write_db_bytes(&s, *binary, *version)
            .map_err(|e| harness(e.to_string()))?
            .ok_or_else(|| harness(format!("case {}: WriteToDB failed", case.name)))?,
    );
    case.inputs = serde_json::to_value(i).map_err(|e| harness(e.to_string()))?;
    Ok(())
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surf_of_revolution_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            if let Some(i) = gen_case(rng) {
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
        for case in &mut cases {
            fill_read_bytes(case)?;
        }
        Ok(cases)
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

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Copy {} => "copy",
            OpIn::Reparam { .. } => "reparam",
            OpIn::ToNurbs { .. } => "to_nurbs",
            OpIn::Drop { .. } => "drop",
            OpIn::Intersect { .. } => "intersect",
            OpIn::Rebuild { .. } => "rebuild",
            OpIn::Transform { .. } => "transform",
            OpIn::Update { .. } => "update",
            OpIn::ReadDb { .. } => "read_db",
            OpIn::Reverse { .. } => "reverse",
            OpIn::Swap {} => "swap",
        }
    }

    /// The generators reach a successful run of every op on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..900u64 {
            let mut rng = Rng::new(0x5eed_3000 + seed);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                let o = out.values.get("out").cloned().unwrap_or_default();
                // the solution / curve counts and the "more" flag of the successes
                let detail = match (&i.op, st) {
                    (OpIn::Drop { .. }, Some(0.0)) => {
                        let n = o.first().copied().unwrap_or(0.0);
                        if n > 0.0 {
                            " found".to_string()
                        } else {
                            " none".to_string()
                        }
                    }
                    (OpIn::Intersect { .. }, Some(0.0)) => {
                        let more = o.first().copied().unwrap_or(0.0);
                        let n = o.get(1).copied().unwrap_or(0.0);
                        format!(" more:{more} curves:{}", n.min(3.0))
                    }
                    _ => String::new(),
                };
                format!("status:{st:?}{detail}")
            };
            *tally.entry((op_name(&i.op).to_string(), kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for op in [
            "copy",
            "reparam",
            "to_nurbs",
            "drop",
            "intersect",
            "rebuild",
            "transform",
            "update",
            "read_db",
            "reverse",
            "swap",
        ] {
            let ok = tally
                .iter()
                .any(|((o, k), _)| o == op && k.starts_with("status:Some(0.0)"));
            assert!(ok, "{op} never succeeds");
        }
        let count = |op: &str, k: &str| -> usize {
            tally
                .iter()
                .filter(|((o, kk), _)| o == op && kk.contains(k))
                .map(|(_, n)| n)
                .sum()
        };
        assert!(count("drop", " found") > 50, "drops rarely find solutions");
        assert!(
            count("intersect", "more:0 curves:1") + count("intersect", "more:0 curves:2") + count("intersect", "more:0 curves:3") > 40,
            "intersections rarely give curves"
        );
    }
}
