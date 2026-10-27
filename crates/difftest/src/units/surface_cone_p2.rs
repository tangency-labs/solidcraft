//! Runner for unit `surface.cone_p2`: `SmCone::CreateOffsetSurface`,
//! `SmCone::IsNurbSurfaceCone`, `SmCone::RebuildSTEPFromNURBParameters` and the
//! file-static `sm_GetEllipseTrimIntervals`.
//!
//! C++: `solidcore_ffi::units::surface_cone_p2` (upstream `source/SMLib/src/SmCone.cpp`).
//! Rust: `solidcore::surface::cone::p2`.
//!
//! A case is a `cone` (built as in `surface.cone_p1`: real `SmCone` / `SmCylinder`
//! objects through `CreateCanonical` or the constructors with `MakeNurb`) and an `op`.
//! Both sides produce the same flat record (layout in the FFI module docs), split into
//! `build` (the construction status), `status` (the op status or boolean) and `out`
//! (the rest: the result's or the edited cone's full state, or the intervals).
//! Statuses are raw `SmStatus` values (`1000` = `SM_SUCCESS`); `-1` on the Rust side
//! for an error upstream cannot produce; `-1002` (`SC_ERR_ARG`) where the shim refuses
//! an input upstream would read uninitialised values or crash on (the Rust side applies
//! the same test with the Rust calls).
//!
//! NURB edits (`recognize`, `rebuild`) move, rescale (homogeneously) or flatten control
//! points; the rotation of a move is computed here once and given to both sides.
//! Trim domains are fractions of the cone's natural domain (`Evaluate` on each side).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p1::line_plane_intersect;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::PI;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::cone::{ellipse_trim_intervals, Cone};
use solidcore::surface::surface::p2::{CONE_TYPE, CYLINDER_TYPE};
use solidcore::SolidError;
use solidcore_ffi::units::surface_cone_p2 as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

/// `SM_SUCCESS`.
const SUCCESS: f64 = 1000.0;
/// `SM_ERR`.
const SM_ERR: f64 = 1001.0;
/// `SM_ERR_INVALID_INPUT` (what [`SolidError::InvalidInput`] stands for).
const SM_ERR_INVALID_INPUT: f64 = 1007.0;
/// `SC_ERR_ARG` (`harness/cpp/shim/sc_shim.h`): the shim refused the input.
const SC_ERR_ARG: f64 = -1002.0;

/// `surface.cone_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ModeIn {
    CanonicalSemi {
        bot: f64,
        semi: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<f64>,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    Canonical {
        bot: f64,
        top: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    CylCanonical {
        radius: f64,
    },
    ConeUnbounded {
        base: f64,
        semi: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
    ConeBounded {
        bot: f64,
        top: f64,
        start: f64,
        end: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
        #[serde(default)]
        make_nurb_gen: bool,
    },
    CylBounded {
        radius: f64,
        start: f64,
        end: f64,
        height: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
        #[serde(default)]
        make_nurb_gen: bool,
    },
    CylUnbounded {
        radius: f64,
        #[serde(default)]
        swap: bool,
        #[serde(default)]
        inside: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConeIn {
    mode: ModeIn,
    origin: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    #[serde(default)]
    oob: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum EditIn {
    None,
    /// Rotation by `angle` degrees about `axis`, then translation by `t`, of the last
    /// generator-direction row.
    MoveLast {
        axis: [f64; 3],
        angle: f64,
        t: [f64; 3],
    },
    ScaleLast {
        f: f64,
    },
    MoveAll {
        axis: [f64; 3],
        angle: f64,
        t: [f64; 3],
    },
    Flatten,
    /// Scale X/Y on the last generator row without changing Z or the weight.
    ScaleLastXy {
        f: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OpIn {
    Offset {
        distance: f64,
        tol: f64,
    },
    Recognize {
        edit: EditIn,
        scale: f64,
    },
    Rebuild {
        edit: EditIn,
    },
    Trim {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        rx: f64,
        ry: f64,
        start: f64,
        end: f64,
        /// Fractions of the natural domain: `Evaluate(f0, f1)`, `Evaluate(f2, f3)`.
        dom: [f64; 4],
        tol: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    cone: ConeIn,
    op: OpIn,
}

/// The rotation matrix (row-major) of `angle` degrees about `axis`; the identity for a
/// zero axis.
fn rotation(axis: [f64; 3], angle: f64) -> [f64; 9] {
    let [ax, ay, az] = axis;
    let l = (ax * ax + ay * ay + az * az).sqrt();
    if l <= 0.0 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let (x, y, z) = (ax / l, ay / l, az / l);
    let a = angle * PI / 180.0;
    let (s, c) = a.sin_cos();
    let t = 1.0 - c;
    [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
    ]
}

fn ffi_edit(e: &EditIn) -> ffi::Edit {
    match *e {
        EditIn::None => ffi::Edit::None,
        EditIn::MoveLast { axis, angle, t } => ffi::Edit::MoveLast {
            r: rotation(axis, angle),
            t,
        },
        EditIn::ScaleLast { f } => ffi::Edit::ScaleLast { f },
        EditIn::MoveAll { axis, angle, t } => ffi::Edit::MoveAll {
            r: rotation(axis, angle),
            t,
        },
        EditIn::Flatten => ffi::Edit::Flatten,
        EditIn::ScaleLastXy { f } => ffi::Edit::ScaleLastXy { f },
    }
}

fn ffi_mode(m: &ModeIn) -> ffi::Mode {
    match *m {
        ModeIn::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        } => ffi::Mode::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        },
        ModeIn::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        } => ffi::Mode::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        },
        ModeIn::CylCanonical { radius } => ffi::Mode::CylCanonical { radius },
        ModeIn::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => ffi::Mode::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        },
        ModeIn::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => ffi::Mode::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        },
        ModeIn::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => ffi::Mode::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        },
        ModeIn::CylUnbounded {
            radius,
            swap,
            inside,
        } => ffi::Mode::CylUnbounded {
            radius,
            swap,
            inside,
        },
    }
}

fn ffi_cone(c: &ConeIn) -> ffi::Build {
    ffi::Build {
        mode: ffi_mode(&c.mode),
        origin: c.origin,
        x: c.x,
        y: c.y,
        oob: c.oob,
    }
}

// ---- C++ side -----------------------------------------------------------------------

/// The trim domain of the shim: corner points at the fractions of the natural domain.
/// The C++ side computes it on its own cone; the record carries the fractions.
fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = match &i.op {
        OpIn::Offset { distance, tol } => ffi::Op::Offset {
            distance: *distance,
            tol: *tol,
        },
        OpIn::Recognize { edit, scale } => ffi::Op::Recognize {
            edit: ffi_edit(edit),
            scale: *scale,
        },
        OpIn::Rebuild { edit } => ffi::Op::Rebuild {
            edit: ffi_edit(edit),
        },
        OpIn::Trim {
            origin,
            x,
            y,
            rx,
            ry,
            start,
            end,
            dom,
            tol,
        } => ffi::Op::Trim {
            origin: *origin,
            x: *x,
            y: *y,
            rx: *rx,
            ry: *ry,
            start: *start,
            end: *end,
            dom: *dom,
            tol: *tol,
        },
    };
    let flat = ffi::cone_op(&ffi_cone(&i.cone), &op).map_err(|e| harness(e.to_string()))?;
    to_output(&flat)
}

/// Splits a record into `build`, `status` and `out`.
fn to_output(flat: &[f64]) -> Result<Output, HarnessError> {
    let (b, rest) = flat.split_first().ok_or_else(|| harness("empty record"))?;
    let out = Output::new().values("build", vec![*b]);
    if *b != SUCCESS {
        if !rest.is_empty() {
            return Err(harness("record continues after a failed build"));
        }
        return Ok(out);
    }
    let (s, rest) = rest
        .split_first()
        .ok_or_else(|| harness("record has no op status"))?;
    Ok(out.values("status", vec![*s]).values("out", rest.to_vec()))
}

// ---- Rust side ----------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn v3(p: [f64; 3]) -> Vec3 {
    let [x, y, z] = p;
    Vec3::new(x, y, z)
}

fn push_v(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The `SmStatus` of a Rust error (`InvalidInput` is `SM_ERR_INVALID_INPUT`); `-1` for
/// one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        _ => -1.0,
    }
}

/// Builds the cone (the `surface.cone_p1` constructions).
fn rust_cone(k: &Kernel, c: &ConeIn) -> solidcore::Result<Cone> {
    let (o, x, y) = (v3(c.origin), v3(c.x), v3(c.y));
    let a2p = Axis2Placement::new(o, x, y);
    let mut cone = match c.mode {
        ModeIn::CanonicalSemi {
            bot,
            semi,
            height,
            swap,
            inside,
        } => Cone::create_canonical_with_semi_angle(k, &a2p, bot, semi, height, swap, inside)?,
        ModeIn::Canonical {
            bot,
            top,
            height,
            swap,
            inside,
        } => Cone::create_canonical(k, &a2p, bot, top, height, swap, inside)?,
        ModeIn::CylCanonical { radius } => Cone::create_canonical_cylinder(k, &a2p, radius)?,
        ModeIn::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => {
            let mut c = Cone::new_unbounded(k, o, x, y, base, semi, swap, inside)?;
            c.make_nurb(k)?;
            c
        }
        ModeIn::ConeBounded {
            bot,
            top,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => {
            let mut c = Cone::new_bounded(
                o,
                x,
                y,
                bot,
                top,
                start,
                end,
                height,
                swap,
                inside,
                make_nurb_gen,
            );
            c.make_nurb(k)?;
            c
        }
        ModeIn::CylBounded {
            radius,
            start,
            end,
            height,
            swap,
            inside,
            make_nurb_gen,
        } => {
            let mut c = Cone::new_cylinder_bounded(
                o,
                x,
                y,
                radius,
                start,
                end,
                height,
                swap,
                inside,
                make_nurb_gen,
            );
            c.make_nurb(k)?;
            c
        }
        ModeIn::CylUnbounded {
            radius,
            swap,
            inside,
        } => {
            let mut c = Cone::new_cylinder_unbounded(k, o, x, y, radius, swap, inside)?;
            c.make_nurb(k)?;
            c
        }
    };
    cone.set_out_of_bounds_enabled(c.oob);
    Ok(cone)
}

fn step_interval(c: &Curve) -> Extent1d {
    match c.kind() {
        CurveKind::Ellipse(e) => e.step_interval(),
        CurveKind::Line(l) => l.step_interval(),
        CurveKind::Hyperbola(h) => h.step_interval(),
        CurveKind::Parabola(p) => p.step_interval(),
        _ => c.natural_interval(),
    }
}

fn nurb_of(c: &Curve) -> Option<&NurbCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb(),
        CurveKind::Ellipse(e) => e.base().gw_nurb(),
        CurveKind::Line(l) => l.base().gw_nurb(),
        CurveKind::Hyperbola(h) => h.base().gw_nurb(),
        CurveKind::Parabola(p) => p.base().gw_nurb(),
        _ => None,
    }
}

fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    o.push(flag(c.is_some()));
    let Some(c) = c else { return };
    o.push(c.type_code() as f64);
    let ivl = step_interval(c);
    o.extend([ivl.min(), ivl.max()]);
    let cur = nurb_of(c);
    o.push(flag(cur.is_some()));
    let Some(cur) = cur else { return };
    o.push(cur.degree() as f64);
    o.push(cur.cpts().len() as f64);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.push(cur.knots().len() as f64);
    o.extend_from_slice(cur.knots());
}

/// The state (layout in the FFI module docs).
fn put_state(k: &Kernel, o: &mut Vec<f64>, s: &Cone) -> solidcore::Result<()> {
    let rev = s.rev();
    o.extend([
        s.type_code() as f64,
        flag(s.is_kind_of(CYLINDER_TYPE)?),
        flag(s.is_kind_of(CONE_TYPE)?),
        flag(s.is_cylinder()),
        s.base_radius(),
        s.semi_angle_deg(),
        flag(s.inside_out()),
        flag(rev.swap_uv()),
        flag(rev.planar_generator()),
        flag(rev.make_nurb_gen_curve()),
        flag(rev.is_bounded()?),
    ]);
    let pos = rev.position();
    push_v(o, pos.origin());
    push_v(o, pos.x_axis());
    push_v(o, pos.y_axis());
    let d = rev.step_uv_domain();
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
    o.extend([s.bot_radius(), s.top_radius()]);
    let mx = s.max_analytic_domain(k)?;
    o.extend([mx.min().x, mx.min().y, mx.max().x, mx.max().y]);
    let apex = s.apex(k)?;
    o.push(if apex.is_cylinder { SM_ERR } else { SUCCESS });
    o.push(flag(apex.is_cylinder));
    push_v(o, apex.point);
    o.push(apex.param);
    o.push(s.height(k)?);
    let pc = rev.polar_converter();
    o.push(flag(rev.have_polar_conversion()));
    let angles = pc.angles();
    o.push(angles.len() as f64);
    if let (Some(a), Some(b)) = (angles.first(), angles.last()) {
        o.extend([*a, *b]);
    }
    let polar_curve = rev.polar_curve().map(|e| Curve::from(e.clone()));
    put_curve(o, polar_curve.as_ref());
    put_curve(o, rev.gen_curve());
    let sur = rev.base().gw_nurb();
    o.push(flag(sur.is_some()));
    let Some(sur) = sur else { return Ok(()) };
    let ix = sur.indices();
    o.extend([ix.p, ix.q, ix.n + 1, ix.m + 1].map(|x| x as f64));
    for row in sur.rows() {
        for p in row {
            o.extend([p.x, p.y, p.z, p.w]);
        }
    }
    o.push(sur.u_knots().len() as f64);
    o.extend_from_slice(sur.u_knots());
    o.push(sur.v_knots().len() as f64);
    o.extend_from_slice(sur.v_knots());
    Ok(())
}

/// The shim's `edit_nurb` on the NURB control points.
fn edit_nurb(sur: &mut NurbSurface, edit: &ffi::Edit, swap: bool) -> solidcore::Result<()> {
    let (n, m) = (sur.n(), sur.m());
    for i in 0..=n {
        for j in 0..=m {
            let last = if swap { i == n } else { j == m };
            let p = sur.cpt_mut(i, j)?;
            let w = if p.w == NOW { 1.0 } else { p.w };
            match *edit {
                ffi::Edit::MoveLast { r, t } | ffi::Edit::MoveAll { r, t }
                    if last || matches!(edit, ffi::Edit::MoveAll { .. }) =>
                {
                    let (x, y, z) = (p.x, p.y, p.z);
                    let [r0, r1, r2, r3, r4, r5, r6, r7, r8] = r;
                    let [tx, ty, tz] = t;
                    p.x = r0 * x + r1 * y + r2 * z + w * tx;
                    p.y = r3 * x + r4 * y + r5 * z + w * ty;
                    p.z = r6 * x + r7 * y + r8 * z + w * tz;
                }
                ffi::Edit::ScaleLast { f } if last => {
                    p.x *= f;
                    p.y *= f;
                    p.z *= f;
                    if p.w != NOW {
                        p.w *= f;
                    }
                }
                ffi::Edit::Flatten => p.z = 0.0,
                ffi::Edit::ScaleLastXy { f } if last => {
                    p.x *= f;
                    p.y *= f;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The shim's `offset_defined`: the apex truncation's `CreateCanonical` with the same
/// formulas succeeds and its height is finite (or the truncation is not reached).
fn offset_defined(k: &Kernel, s: &Cone, dist: f64, tol: f64) -> solidcore::Result<bool> {
    let tol = approx_tol3d(k, tol);
    let swap = s.rev().swap_uv();
    let inside = s.inside_out();
    let sign = 1.0 * (if swap { -1.0 } else { 1.0 }) * (if inside { -1.0 } else { 1.0 });
    let pos = s.rev().position();
    let (origin, x_axis, y_axis, z_axis) = (pos.origin(), pos.x_axis(), pos.y_axis(), pos.z_axis());
    let height = s.height(k)?;
    let semi = s.semi_angle_deg();
    let sin = (semi * PI / 180.0).sin();
    let cos = (semi * PI / 180.0).cos();
    let off_z = -sin * sign * dist;
    let off_r = cos * sign * dist;
    let old_bot = s.bot_radius();
    let old_top = s.top_radius();
    let mut new_bot = old_bot + off_r;
    let mut new_top = old_top + off_r;
    if new_bot < tol && new_top < tol {
        return Ok(true);
    }
    if !(new_bot < tol || new_top < tol) {
        return Ok(true);
    }
    let (new_height, bot_origin) = if new_bot < tol {
        let tan = height / old_top;
        let nh = new_top * tan;
        new_bot = 0.0;
        (nh, origin + (height + off_z - nh) * z_axis)
    } else {
        let tan = height / old_bot;
        let nh = new_bot * tan;
        new_top = 0.0;
        (nh, origin + off_z * z_axis)
    };
    if !new_height.is_finite()
        || !bot_origin.x.is_finite()
        || !bot_origin.y.is_finite()
        || !bot_origin.z.is_finite()
    {
        return Ok(false);
    }
    let placement = Axis2Placement::new(bot_origin, x_axis, y_axis);
    match Cone::create_canonical(k, &placement, new_bot, new_top, new_height, swap, inside) {
        Ok(_) => Ok(true),
        Err(e) if e.is_unported() => Err(e),
        Err(_) => Ok(false),
    }
}

/// The shim's trim domain from the fractions.
fn trim_domain(s: &Cone, f: [f64; 4]) -> Extent2d {
    let nat = s.rev().base().natural_uv_domain();
    let [f0, f1, f2, f3] = f;
    Extent2d::from_points(nat.evaluate(f0, f1), nat.evaluate(f2, f3))
}

/// The shim's `trim_defined`: the two seam line / ellipse plane intersections succeed.
fn trim_defined(k: &Kernel, s: &Cone, x: Vec3, y: Vec3, origin: Vec3, dom: &Extent2d) -> bool {
    let z = x.cross(y);
    // status ignored as upstream: a failed corner conversion leaves the default domain, a
    // reversed result is replaced by the NURB domain
    let mut anal = Extent2d::new();
    if let (Ok(a), Ok(b)) = (
        s.convert_uv_from_nurbs_to_step(k, dom.min()),
        s.convert_uv_from_nurbs_to_step(k, dom.max()),
    ) {
        if anal
            .set_min_max_coords(a.x, a.y.min(b.y), b.x, a.y.max(b.y))
            .is_err()
        {
            anal = *dom;
        }
    }
    let corner = |u: f64, v: f64| {
        s.evaluate_step_point(anal.evaluate(u, v))
            .unwrap_or(Vec3::UNDEF)
    };
    let c = [
        corner(0.0, 0.0),
        corner(0.0, 1.0),
        corner(1.0, 0.0),
        corner(1.0, 1.0),
    ];
    let [c0, c1, c2, c3] = c;
    line_plane_intersect(c0, c1 - c0, origin, z).is_ok()
        && line_plane_intersect(c2, c3 - c2, origin, z).is_ok()
}

/// Runs the op on the Rust side; `Err` only for a Rust error inside the record.
fn rust_op(k: &Kernel, s: &mut Cone, op: &OpIn, o: &mut Vec<f64>) -> solidcore::Result<()> {
    match op {
        OpIn::Offset { distance, tol } => {
            if !offset_defined(k, s, *distance, *tol)? {
                o.push(SC_ERR_ARG);
                return Ok(());
            }
            match s.create_offset_surface(k, *distance, *tol) {
                Ok(off) => {
                    o.push(SUCCESS);
                    o.push(flag(off.is_some()));
                    if let Some(off) = off {
                        put_state(k, o, &off)?;
                    }
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => o.extend([status(&e), 0.0]),
            }
        }
        OpIn::Recognize { edit, scale } => {
            let swap = s.rev().swap_uv();
            let mut test = s.rev().base().clone();
            edit_nurb(test.get_or_create_gw_nurb()?, &ffi_edit(edit), swap)?;
            match Cone::is_nurb_surface_cone(k, &test, *scale) {
                Ok(found) => {
                    o.push(flag(found.is_some()));
                    o.push(flag(found.is_some()));
                    if let Some(found) = found {
                        put_state(k, o, &found)?;
                    }
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => o.extend([status(&e), 0.0]),
            }
        }
        OpIn::Rebuild { edit } => {
            let swap = s.rev().swap_uv();
            edit_nurb(s.base_mut().get_or_create_gw_nurb()?, &ffi_edit(edit), swap)?;
            match s.rebuild_step_from_nurb_parameters(k) {
                Ok(()) => o.push(SUCCESS),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => o.push(status(&e)),
            }
            put_state(k, o, s)?;
        }
        OpIn::Trim {
            origin,
            x,
            y,
            rx,
            ry,
            start,
            end,
            dom,
            tol,
        } => {
            let dom = trim_domain(s, *dom);
            let (origin, x, y) = (v3(*origin), v3(*x), v3(*y));
            if !trim_defined(k, s, x, y, origin, &dom) {
                o.push(SC_ERR_ARG);
                return Ok(());
            }
            match ellipse_trim_intervals(k, origin, x, y, *rx, *ry, *start, *end, s, &dom, *tol) {
                Ok(ivls) => {
                    o.push(SUCCESS);
                    o.push(ivls.len() as f64);
                    for ivl in ivls {
                        o.extend([ivl.min(), ivl.max()]);
                    }
                }
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => o.push(status(&e)),
            }
        }
    }
    Ok(())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match rust_cone(&k, &i.cone) {
        Err(e) if e.is_unported() => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
        }
        Err(e) => {
            o.push(status(&e));
            return to_output(&o);
        }
        Ok(mut s) => {
            o.push(SUCCESS);
            if let Err(e) = rust_op(&k, &mut s, &i.op, &mut o) {
                return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
            }
        }
    }
    to_output(&o)
}

// ---- generators ---------------------------------------------------------------------

fn unit_vec(rng: &mut Rng) -> [f64; 3] {
    loop {
        let [x, y, z] = rng.point3(-1.0, 1.0);
        let l = (x * x + y * y + z * z).sqrt();
        if l > 0.1 && l <= 1.0 {
            return [x / l, y / l, z / l];
        }
    }
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = a;
    let l = (x * x + y * y + z * z).sqrt();
    [x / l, y / l, z / l]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    let [x, y, z] = a;
    [x * s, y * s, z * s]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax + bx, ay + by, az + bz]
}

/// An orthonormal placement (canonical sometimes); with `skew`, sometimes skewed or
/// non-unit axes.
fn frame(rng: &mut Rng, scale: f64, skew: bool) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let origin = scale3(rng.point3(-10.0, 10.0), scale);
    if rng.chance(0.2) {
        return (origin, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    }
    let x = unit_vec(rng);
    let mut y = normalize(cross3(cross3(x, unit_vec(rng)), x));
    if !y.iter().all(|v| v.is_finite()) {
        y = normalize(cross3(x, [0.3, 0.5, 0.8]));
    }
    if skew && rng.chance(0.1) {
        let x2 = scale3(x, rng.uniform(0.5, 2.0));
        let y2 = add3(
            scale3(y, rng.uniform(0.5, 2.0)),
            scale3(x, rng.uniform(-0.3, 0.3)),
        );
        return (origin, x2, y2);
    }
    (origin, x, y)
}

fn pick_scale(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.7 {
        1.0
    } else if r < 0.8 {
        1e-3
    } else if r < 0.9 {
        1e3
    } else if r < 0.95 {
        1e5
    } else {
        1e-6
    }
}

fn radius(rng: &mut Rng, scale: f64) -> f64 {
    let r = rng.unit();
    if r < 0.1 {
        0.0
    } else if r < 0.12 {
        1e-13 * scale
    } else if r < 0.14 {
        -rng.uniform(0.1, 2.0) * scale
    } else {
        rng.uniform(0.05, 5.0) * scale
    }
}

fn semi(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.15 {
        0.0
    } else if r < 0.18 {
        1e-13
    } else if r < 0.25 {
        -rng.uniform(1.0, 60.0)
    } else if r < 0.28 {
        rng.uniform(80.0, 89.0)
    } else {
        rng.uniform(1.0, 70.0)
    }
}

fn angles(rng: &mut Rng) -> (f64, f64) {
    let r = rng.unit();
    if r < 0.5 {
        (0.0, 360.0)
    } else if r < 0.6 {
        (-180.0, 180.0)
    } else {
        let a = rng.uniform(-300.0, 300.0);
        let len = rng.uniform(5.0, 360.0);
        let b = (a + len).min(360.0);
        if b <= a {
            (0.0, 90.0)
        } else {
            (a, b)
        }
    }
}

/// A cone; `bounded_only` keeps to bounded cones with an orthonormal placement (for the
/// trim, whose ellipses are made on the cone).
fn gen_cone(rng: &mut Rng, bounded_only: bool) -> (ConeIn, f64) {
    let scale = pick_scale(rng);
    let (origin, x, y) = frame(rng, scale, !bounded_only);
    let swap = rng.chance(0.35);
    let inside = rng.chance(0.35);
    let h = |rng: &mut Rng| {
        if rng.chance(0.04) {
            1e-9 * scale
        } else {
            rng.uniform(0.1, 10.0) * scale
        }
    };
    let pick = if bounded_only {
        match rng.below(3) {
            0 => 1,
            1 => 4,
            _ => 5,
        }
    } else {
        rng.below(7)
    };
    let mode = match pick {
        0 => {
            let hh = if rng.chance(0.6) { Some(h(rng)) } else { None };
            ModeIn::CanonicalSemi {
                bot: radius(rng, scale).abs(),
                semi: semi(rng).abs(),
                height: hh,
                swap,
                inside,
            }
        }
        1 => ModeIn::Canonical {
            bot: radius(rng, scale).abs(),
            top: radius(rng, scale).abs(),
            height: h(rng),
            swap,
            inside,
        },
        2 => ModeIn::CylCanonical {
            radius: rng.uniform(0.05, 5.0) * scale,
        },
        3 => ModeIn::ConeUnbounded {
            base: radius(rng, scale),
            semi: semi(rng),
            swap,
            inside,
        },
        4 => {
            let (start, end) = angles(rng);
            ModeIn::ConeBounded {
                bot: radius(rng, scale),
                top: radius(rng, scale),
                start,
                end,
                height: h(rng),
                swap,
                inside,
                make_nurb_gen: rng.chance(0.2),
            }
        }
        5 => {
            let (start, end) = angles(rng);
            ModeIn::CylBounded {
                radius: rng.uniform(0.05, 5.0) * scale,
                start,
                end,
                height: h(rng),
                swap,
                inside,
                make_nurb_gen: rng.chance(0.2),
            }
        }
        _ => ModeIn::CylUnbounded {
            radius: radius(rng, scale),
            swap,
            inside,
        },
    };
    (
        ConeIn {
            mode,
            origin,
            x,
            y,
            oob: rng.chance(0.1),
        },
        scale,
    )
}

fn gen_edit(rng: &mut Rng, scale: f64) -> EditIn {
    let r = rng.unit();
    if r < 0.35 {
        EditIn::None
    } else if r < 0.5 {
        // twist the last row about the world z axis (a small or a tolerance-sized angle)
        let angle = if rng.chance(0.3) {
            rng.uniform(-1e-7, 1e-7)
        } else {
            rng.uniform(-10.0, 10.0)
        };
        EditIn::MoveLast {
            axis: [0.0, 0.0, 1.0],
            angle,
            t: if rng.chance(0.3) {
                scale3(rng.point3(-0.5, 0.5), scale)
            } else {
                [0.0; 3]
            },
        }
    } else if r < 0.6 {
        EditIn::ScaleLast {
            f: if rng.chance(0.2) {
                1.0 + rng.uniform(-1e-12, 1e-12)
            } else {
                rng.uniform(0.2, 5.0)
            },
        }
    } else if r < 0.9 {
        EditIn::MoveAll {
            axis: unit_vec(rng),
            angle: rng.uniform(-180.0, 180.0),
            t: scale3(rng.point3(-10.0, 10.0), scale),
        }
    } else {
        EditIn::Flatten
    }
}

/// The radius of a cone at height `z` above its placement (bounded constructions).
fn cone_radius_slope(mode: &ModeIn) -> Option<(f64, f64, f64)> {
    match *mode {
        ModeIn::Canonical {
            bot, top, height, ..
        }
        | ModeIn::ConeBounded {
            bot, top, height, ..
        } => Some((bot, (top - bot) / height, height)),
        ModeIn::CylBounded { radius, height, .. } => Some((radius, 0.0, height)),
        _ => None,
    }
}

/// An ellipse on the bounded cone `c`: the section by a plane through the axis point at
/// height `h0` tilted by `phi` about the local y axis (a circle for `phi = 0`), in world
/// coordinates; sometimes perturbed off the cone.
fn gen_trim(rng: &mut Rng, c: &ConeIn, scale: f64) -> OpIn {
    let (r0, slope, height) = cone_radius_slope(&c.mode).unwrap_or((scale, 0.0, scale));
    let ex = normalize(c.x);
    let ez = normalize(cross3(c.x, c.y));
    let ey = cross3(ez, ex);
    let h0 = if rng.chance(0.15) {
        // through a boundary circle
        if rng.chance(0.5) {
            0.0
        } else {
            height
        }
    } else {
        rng.uniform(-0.3, 1.3) * height
    };
    let rr = r0 + slope * h0;
    // keep |slope * tan(phi)| < 1 (an ellipse, not a parabola or hyperbola)
    let max_tan = if slope.abs() > 1e-9 {
        0.9 / slope.abs()
    } else {
        3.0
    };
    let phi = if rng.chance(0.25) {
        0.0
    } else {
        let t = rng.uniform(-max_tan, max_tan).clamp(-5.0, 5.0);
        t.atan()
    };
    let t = slope * phi.tan();
    let one_t2 = 1.0 - t * t;
    let xc = -rr * t / one_t2;
    let ax = rr / one_t2;
    let b = rr / one_t2.sqrt();
    let (sp, cp) = phi.sin_cos();
    let a = ax / cp;
    // local center (xc, 0, h0 - xc tan(phi)), major axis (cos phi, 0, -sin phi)
    let lc = [xc, 0.0, h0 - xc * phi.tan()];
    let lu = [cp, 0.0, -sp];
    let to_world = |p: [f64; 3]| {
        let [px, py, pz] = p;
        add3(add3(scale3(ex, px), scale3(ey, py)), scale3(ez, pz))
    };
    let mut origin = add3(c.origin, to_world(lc));
    let mut xa = to_world(lu);
    let mut ya = ey;
    // in-plane rotation of the ellipse frame (swap of the axes) sometimes
    let (mut rx, mut ry) = (a.abs(), b.abs());
    if rng.chance(0.2) {
        core::mem::swap(&mut xa, &mut ya);
        core::mem::swap(&mut rx, &mut ry);
    }
    if rng.chance(0.15) {
        // off the cone: shifted along the axis
        origin = add3(origin, scale3(ez, rng.uniform(-0.2, 0.2) * height));
    }
    if rng.chance(0.1) {
        // non-unit axes
        xa = scale3(xa, rng.uniform(0.5, 2.0));
    }
    let (start, end) = if rng.chance(0.6) {
        (0.0, 360.0)
    } else {
        let s = rng.uniform(-360.0, 300.0);
        (s, s + rng.uniform(10.0, 360.0))
    };
    let dom = if rng.chance(0.5) {
        [0.0, 0.0, 1.0, 1.0]
    } else {
        [
            rng.uniform(0.0, 0.4),
            rng.uniform(0.0, 0.4),
            rng.uniform(0.6, 1.0),
            rng.uniform(0.6, 1.0),
        ]
    };
    let tol = match rng.below(4) {
        0 => 1e-6 * scale,
        1 => 1e-3 * scale,
        2 => 1e-9 * scale,
        _ => 1e-5,
    };
    OpIn::Trim {
        origin,
        x: xa,
        y: ya,
        rx,
        ry,
        start,
        end,
        dom,
        tol,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let r = rng.unit();
    if r < 0.25 {
        let (cone, scale) = gen_cone(rng, true);
        let op = gen_trim(rng, &cone, scale);
        return Inputs { cone, op };
    }
    let (cone, scale) = gen_cone(rng, false);
    let op = if r < 0.6 {
        let tol = match rng.below(5) {
            0 => 1e-6 * scale,
            1 => 1e-3 * scale,
            2 => 0.0,
            3 => solidcore::geom::tol::USE_DEFAULT,
            _ => -1e-4 * scale,
        };
        let distance = if rng.chance(0.15) {
            // near zero
            rng.uniform(-1e-12, 1e-12) * scale
        } else {
            rng.uniform(-4.0, 4.0) * scale
        };
        OpIn::Offset { distance, tol }
    } else if r < 0.8 {
        let scale_t = match rng.below(4) {
            0 => 1.0,
            1 => 1e3,
            2 => 1e-3,
            _ => rng.uniform(0.1, 10.0),
        };
        OpIn::Recognize {
            edit: gen_edit(rng, scale),
            scale: scale_t,
        }
    } else {
        OpIn::Rebuild {
            edit: gen_edit(rng, scale),
        }
    };
    Inputs { cone, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.cone_p2"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Offset { .. } => "offset",
            OpIn::Recognize { .. } => "recognize",
            OpIn::Rebuild { .. } => "rebuild",
            OpIn::Trim { .. } => "trim",
        }
    }

    /// The generators reach real values of every op on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..1200u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let b = out.values.get("build").and_then(|v| v.first()).copied();
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                let n = out.values.get("out").map_or(0, Vec::len);
                format!("build:{b:?} status:{st:?} out>0:{}", n > 0)
            };
            *tally.entry((op_name(&i.op).to_string(), kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for (op, st) in [
            ("offset", "1000.0"),
            ("recognize", "1.0"),
            ("recognize", "0.0"),
            ("rebuild", "1000.0"),
            ("rebuild", "1001.0"),
            ("trim", "1000.0"),
        ] {
            let want = format!("build:Some(1000.0) status:Some({st}) out>0:true");
            let ok = tally.iter().any(|((o, k), _)| o == op && *k == want)
                || (st == "0.0"
                    && tally.iter().any(|((o, k), _)| {
                        o == op && k == "build:Some(1000.0) status:Some(0.0) out>0:true"
                    }));
            assert!(ok, "{op} never gives {st} with values");
        }
    }

    /// The generated cases reach the offset branches (no surface, apex truncation,
    /// moved copy) and trims with zero, one and several intervals.
    #[test]
    fn generated_cases_cover_branches() {
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        for seed in 0..1200u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let out = rust(&i).expect("harness");
            let st = out.values.get("status").and_then(|v| v.first()).copied();
            let rest = out.values.get("out").cloned().unwrap_or_default();
            let key = match (&i.op, st) {
                (OpIn::Offset { .. }, Some(SUCCESS)) => {
                    if rest.first() == Some(&0.0) {
                        "offset:none".to_string()
                    } else if ConeIn::mode_is_apex_truncation(&rest) {
                        "offset:truncated".to_string()
                    } else {
                        "offset:moved".to_string()
                    }
                }
                (OpIn::Trim { .. }, Some(SUCCESS)) => {
                    let n = rest.first().copied().unwrap_or(-1.0);
                    format!("trim:{}", n.min(3.0))
                }
                _ => continue,
            };
            *tally.entry(key).or_default() += 1;
        }
        eprintln!("{tally:?}");
        for key in [
            "offset:none",
            "offset:truncated",
            "offset:moved",
            "trim:0",
            "trim:1",
            "trim:2",
        ] {
            assert!(tally.contains_key(key), "{key} never reached");
        }
    }

    /// Every fixture case builds its cone and compares real values.
    #[test]
    fn fixtures_compare_values() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/surface.cone_p2/cases.json"
        );
        let text = std::fs::read_to_string(path).expect("fixtures");
        let v: Value = serde_json::from_str(&text).expect("json");
        let cases = v["cases"].as_array().expect("cases");
        for c in cases {
            let i: Inputs = serde_json::from_value(c["inputs"].clone()).expect("inputs");
            let out = rust(&i).expect("harness");
            let b = out.values.get("build").and_then(|v| v.first()).copied();
            let st = out.values.get("status").and_then(|v| v.first()).copied();
            let n = out.values.get("out").map_or(0, Vec::len);
            eprintln!("{} build:{b:?} status:{st:?} out:{n}", c["name"]);
            assert!(out.deferred.is_none());
            assert_eq!(b, Some(SUCCESS));
            assert!(n > 0 && st != Some(SC_ERR_ARG), "{}", c["name"]);
        }
    }

    /// The #713 fixtures keep pinned behavior by default and exercise the
    /// existing opt-in independently of the unpatched C++ oracle.
    #[test]
    fn issue713_fixtures_exercise_both_behaviors() {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/surface.cone_p2/issue713.json"
        ))
        .unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 24);
        for enabled in [false, true] {
            let mut k = Kernel::default();
            k.fixes.cylinder_rebuild_mean_radius = enabled;
            for case in cases {
                let i: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
                let name = case["name"].as_str().unwrap();
                let mut cone = rust_cone(&k, &i.cone).unwrap();
                let before = cone.clone();
                let mut record = Vec::new();
                rust_op(&k, &mut cone, &i.op, &mut record).unwrap();
                assert!(cone.is_cylinder_class(), "{name}");
                if enabled && name.contains("taper_") {
                    assert_eq!(record.first(), Some(&SM_ERR), "{name}");
                    assert_eq!(cone.base_radius(), before.base_radius());
                    assert_eq!(cone.semi_angle_deg(), before.semi_angle_deg());
                } else {
                    assert_eq!(record.first(), Some(&SUCCESS), "{enabled} {name}");
                    if enabled {
                        assert_eq!(cone.semi_angle_deg(), 0.0, "{name}");
                    } else if !name.contains("unchanged_") {
                        assert_ne!(cone.semi_angle_deg(), 0.0, "{name}");
                    }
                }
            }
        }
    }

    impl ConeIn {
        /// An offset state (after `has`) whose bottom or top radius is exactly `0` while
        /// the input's were not: the apex truncation.
        fn mode_is_apex_truncation(rest: &[f64]) -> bool {
            // state: has type is_cyl is_cone is_cylinder base semi inside swap planar
            // make_nurb_gen bounded O[3] X[3] Y[3] D[4] bot_r top_r ..
            let bot = rest.get(1 + 11 + 9 + 4).copied();
            let top = rest.get(1 + 11 + 9 + 4 + 1).copied();
            matches!(bot, Some(0.0)) || matches!(top, Some(0.0))
        }
    }
}
