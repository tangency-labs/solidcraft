//! Differential tests of analytic cone intersections, including full analytic and
//! NURBS curve records, UV curve presence, classification and deviations.
//! `rejected_*` fixtures assert native FFI guard rejection only; they do not
//! compare geometry, but still execute the Rust intersection (result ignored) so
//! a Rust panic or hang on those inputs fails the case.
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::surface_cone_p3 as ffi;

/// Cone intersection differential runner.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Inputs {
    op: i32,
    cone: Vec<f64>,
    other: Vec<f64>,
    domains: [f64; 8],
    tolerance: Option<f64>,
    #[serde(default)]
    same: bool,
}
fn at(a: &[f64], i: usize) -> f64 {
    a.get(i).copied().unwrap_or(f64::NAN)
}
fn set(a: &mut [f64], i: usize, v: f64) {
    if let Some(p) = a.get_mut(i) {
        *p = v;
    }
}
fn v(a: &[f64], i: usize) -> Vec3 {
    Vec3::new(at(a, i), at(a, i + 1), at(a, i + 2))
}
fn ext(a: &[f64]) -> Extent2d {
    Extent2d::from_bounds(at(a, 0), at(a, 1), at(a, 2), at(a, 3))
}
fn build(k: &Kernel, a: &[f64]) -> solidcore::Result<Cone> {
    let mut c = if at(a, 16) == 0.0 {
        Cone::new_bounded(
            v(a, 0),
            v(a, 3),
            v(a, 6),
            at(a, 9),
            at(a, 10),
            at(a, 12),
            at(a, 13),
            at(a, 11),
            at(a, 14) != 0.0,
            at(a, 15) != 0.0,
            false,
        )
    } else {
        Cone::new_cylinder_bounded(
            v(a, 0),
            v(a, 3),
            v(a, 6),
            at(a, 9),
            at(a, 12),
            at(a, 13),
            at(a, 11),
            at(a, 14) != 0.0,
            at(a, 15) != 0.0,
            false,
        )
    };
    c.make_nurb(k)?;
    Ok(c)
}
fn domain(n: Extent2d, f: &[f64]) -> Extent2d {
    let u = n.u_min();
    let v = n.v_min();
    let du = n.u_max() - u;
    let dv = n.v_max() - v;
    Extent2d::from_bounds(
        u + du * at(f, 0),
        v + dv * at(f, 1),
        u + du * at(f, 2),
        v + dv * at(f, 3),
    )
}
fn fl(x: bool) -> f64 {
    f64::from(u8::from(x))
}
fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}
fn put_ivl(o: &mut Vec<f64>, v: &solidcore::geom::extent1d::Extent1d) {
    o.extend([v.min(), v.max()]);
}
fn put_curve_nurb(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.extend([f64::from(c.knot_type() as i32), f64::from(c.form() as i32)]);
    if let Some(cur) = c.gw_nurb() {
        o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend_from_slice(cur.knots());
    } else {
        o.push(0.0);
    }
}
fn put_curve(o: &mut Vec<f64>, c: Option<&Curve>) -> solidcore::Result<()> {
    let Some(c) = c else {
        o.push(0.0);
        return Ok(());
    };
    o.extend([1.0, c.type_code() as f64, c.dim() as f64]);
    match c.kind() {
        CurveKind::Line(l) => {
            put_vec(o, l.line_point());
            put_vec(o, l.line_vector());
            put_ivl(o, &l.step_interval());
            o.extend([l.line_scale(), fl(l.inside_out())]);
            put_curve_nurb(o, l.base());
        }
        CurveKind::Ellipse(e) => {
            put_vec(o, e.position().origin());
            put_vec(o, e.position().x_axis());
            put_vec(o, e.position().y_axis());
            o.extend([e.x_radius(), e.y_radius()]);
            put_ivl(o, &e.step_interval());
            o.push(fl(e.inside_out()));
            put_curve_nurb(o, e.base());
        }
        CurveKind::BSpline(c) => put_curve_nurb(o, c),
        _ => return Err(SolidError::sm_err("difftest::surface_cone_p3::curve")),
    }
    put_ivl(o, &c.natural_interval());
    Ok(())
}
fn put_intersection(more: bool, x: &SurfaceIntersection) -> solidcore::Result<Vec<f64>> {
    let mut o = vec![1000.0, fl(more), x.curves_3d.len() as f64];
    for c in &x.curves_3d {
        put_curve(&mut o, Some(c))?;
    }
    for uv in [&x.surface1_uv_curves, &x.surface2_uv_curves] {
        o.push(uv.len() as f64);
        for c in uv {
            put_curve(&mut o, c.as_ref())?;
        }
    }
    o.push(x.curve_types.len() as f64);
    o.extend(x.curve_types.iter().map(|t| f64::from(*t as i32)));
    o.push(x.deviations.len() as f64);
    o.extend_from_slice(&x.deviations);
    Ok(o)
}
fn rust(i: &Inputs) -> solidcore::Result<Vec<f64>> {
    let k = Kernel::default();
    let a = build(&k, &i.cone)?;
    let da = domain(
        a.rev().base().natural_uv_domain(),
        i.domains.get(..4).unwrap_or(&[]),
    );
    let (more, out) = if i.op == 0 || i.op == 3 {
        let b = build(&k, &i.other)?;
        let b = if i.same { &a } else { &b };
        let db = domain(
            b.rev().base().natural_uv_domain(),
            i.domains.get(4..).unwrap_or(&[]),
        );
        if i.op == 3 {
            // SmCone::GlobalSurfaceIntersect through the common Surface dispatch: its
            // special cases, else the base SmSurface::GlobalSurfaceIntersect.
            let sa = Surface::from(a.clone());
            let sb = Surface::from(b.clone());
            let sb = if i.same { &sa } else { &sb };
            let out =
                sa.global_surface_intersect(&k, &da, sb, &db, [true, true], i.tolerance, None)?;
            (false, out)
        } else {
            a.intersect_with_cone(&k, &da, b, &db, i.tolerance)?
        }
    } else {
        let b = Plane::new(
            v(&i.other, 0),
            v(&i.other, 3),
            v(&i.other, 6),
            Vec2::new(1.0, 1.0),
            &ext(i.other.get(9..).unwrap_or(&[])),
        );
        let db = domain(b.natural_uv_domain(), i.domains.get(4..).unwrap_or(&[]));
        if i.op == 2 {
            let sa = Surface::from(a.clone());
            let sb = Surface::from(b);
            let out =
                sa.global_surface_intersect(&k, &da, &sb, &db, [true, true], i.tolerance, None)?;
            (false, out)
        } else {
            a.intersect_with_plane(&k, &da, &b, &db, i.tolerance)?
        }
    };
    put_intersection(more, &out)
}
fn output(v: &[f64]) -> Result<Output, HarnessError> {
    if v.len() < 3 {
        return Err(HarnessError::new("short cone intersection result"));
    }
    if at(v, 0) != 1000.0 {
        return Ok(Output::err(format!("status_{}", at(v, 0) as i64)));
    }
    Ok(Output::new()
        .ints("status_and_more", vec![at(v, 0) as i64, at(v, 1) as i64])
        .values("geometry", v.get(2..).unwrap_or(&[]).to_vec()))
}
fn cone(bot: f64, top: f64, height: f64) -> Vec<f64> {
    vec![
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, bot, top, height, 0.0, 360.0, 0.0, 0.0, 0.0,
    ]
}
fn plane(origin: [f64; 3], x: [f64; 3], y: [f64; 3], size: f64) -> Vec<f64> {
    [
        origin.to_vec(),
        x.to_vec(),
        y.to_vec(),
        vec![-size, -size, size, size],
    ]
    .concat()
}
fn input(op: i32, cone: Vec<f64>, other: Vec<f64>) -> Inputs {
    Inputs {
        op,
        cone,
        other,
        domains: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
        tolerance: Some(1e-7),
        same: false,
    }
}
/// Cylinder record: origin, +x, +y, radius, (unused), height, 0..360 deg, flags, cylinder.
fn cylinder(origin: [f64; 3], radius: f64, height: f64) -> Vec<f64> {
    let [x, y, z] = origin;
    vec![
        x, y, z, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, radius, 0.0, height, 0.0, 360.0, 0.0, 0.0, 1.0,
    ]
}

/// Parallel-axis cylinders (SolidCraft boolean: R=20 at z 2..13 vs r=3.04 through
/// (0,20,2)) with full and v-trimmed face domains. Domains are fractions of the natural
/// uv domain, which for a cylinder is v in [0, height] (height 11 here).
fn parallel_cylinder_cases() -> Vec<Case> {
    let h = 11.0;
    let (lo, hi) = (0.8 / h, 10.5 / h);
    let full = [0.0, 0.0, 1.0, 1.0];
    let trimmed = [0.0, lo, 1.0, hi];
    let min_only = [0.0, lo, 1.0, 1.0];
    let max_only = [0.0, 0.0, 1.0, hi];
    [
        ("full", full, full),
        ("a_vtrim", trimmed, full),
        ("b_vtrim", full, trimmed),
        ("both_vtrim", trimmed, trimmed),
        ("a_vmin_only", min_only, full),
        ("a_vmax_only", max_only, full),
    ]
    .into_iter()
    .map(|(name, a, b)| {
        let mut i = input(
            0,
            cylinder([0.0, 0.0, 2.0], 20.0, h),
            cylinder([0.0, 20.0, 2.0], 3.04, h),
        );
        i.domains = [a, b].concat().try_into().unwrap_or(i.domains);
        Case {
            unit: "surface.cone_p3".into(),
            name: format!("parallel_cyl_{name}"),
            source: crate::case::CaseSource::Inline,
            inputs: serde_json::to_value(i).unwrap_or(Value::Null),
        }
    })
    .collect()
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.cone_p3"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(1200),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &ctx)?;
        v.extend(parallel_cylinder_cases());
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let result = ffi::intersect(i.op, &i.cone, &i.other, &i.domains, i.tolerance, i.same);
        if case.name.starts_with("rejected_") {
            return match result {
                Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => {
                    Ok(Output::err("ffi_rejected"))
                }
                Err(e) => Err(HarnessError::new(format!("unexpected rejection: {e}"))),
                Ok(_) => Err(HarnessError::new("expected native guard rejection")),
            };
        }
        let out = result.map_err(|e| HarnessError::new(e.to_string()))?;
        output(&out)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        if case.name.starts_with("rejected_") {
            // Upstream crashes on these inputs; only require that Rust returns.
            let _ = rust(&i);
            return Ok(Output::err("ffi_rejected"));
        }
        match rust(&i) {
            Ok(v) => output(&v),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| {
                Output::err(format!(
                    "status_{}",
                    match e {
                        SolidError::Upstream { code, .. } => code,
                        SolidError::InvalidInput { .. } => 1007,
                        _ => -1,
                    }
                ))
            })),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = *[1e-7, 1.0, 1e6].get(rng.below(3) as usize)?;
        let radius = rng.uniform(0.5, 2.0) * scale;
        let height = rng.uniform(1.0, 5.0) * scale;
        // Weight cases which produce geometry, while retaining disjoint,
        // coincident, general-request and unfinished-dependency paths.
        let mode = match rng.below(20) {
            10..=13 => 0,
            14..=16 => 2,
            17..=19 => 7,
            mode => mode,
        };
        let top = if mode.is_multiple_of(3) && mode != 9 {
            radius * 0.5
        } else {
            radius
        };
        let mut a = cone(radius, top, height);
        set(&mut a, 14, rng.below(2) as f64);
        set(&mut a, 15, rng.below(2) as f64);
        if rng.below(4) == 0 {
            set(&mut a, 12, -90.0);
            set(&mut a, 13, rng.uniform(10.0, 200.0));
        }
        let mut i = if mode < 5 {
            let b = match mode {
                0 => plane(
                    [0.0, 0.0, height * 0.5],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    4.0 * scale,
                ),
                1 => plane(
                    [radius, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    4.0 * scale,
                ),
                2 => plane(
                    [rng.uniform(-1.2, 1.2) * radius, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    4.0 * scale,
                ),
                3 => plane(
                    [0.0, 0.0, height * 0.5],
                    [1.0, 0.0, 0.0],
                    [0.0, 0.6, 0.8],
                    4.0 * scale,
                ),
                _ => plane(
                    [0.0, 0.0, height * 3.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    4.0 * scale,
                ),
            };
            input(1, a, b)
        } else {
            let mut b = cone(radius, top, height);
            set(
                &mut b,
                0,
                if mode == 5 {
                    5.0 * scale
                } else if mode == 6 {
                    radius * 2.0
                } else if mode == 7 {
                    radius
                } else {
                    0.0
                },
            );
            if mode == 9 {
                set(&mut b, 1, height * 0.5);
                set(&mut b, 2, height * 0.5);
                set(&mut b, 6, 0.0);
                set(&mut b, 7, 0.0);
                set(&mut b, 8, 1.0);
            }
            let mut i = input(0, a, b);
            if mode == 8 {
                i.same = rng.below(2) == 0;
            }
            i
        };
        if rng.below(4) == 0 {
            set(&mut i.domains, 0, 0.125);
            set(&mut i.domains, 2, 0.875);
        }
        if rng.below(4) == 0 {
            set(&mut i.domains, 5, 0.125);
            set(&mut i.domains, 7, 0.875);
        }
        i.tolerance = Some(1e-7 * scale);
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints curve count and line records (point, vector, interval) of both sides for
    /// the parallel-cylinder cases; run with `--nocapture`.
    fn lines(v: &[f64]) -> String {
        use std::fmt::Write as _;
        let n = at(v, 2) as usize;
        let mut s = format!("status={} more={} curves={n}", at(v, 0), at(v, 1));
        // the first curve only: later offsets depend on NURB sizes
        if n > 0 {
            // [1, type, dim, point3, vector3, ivl2, ...] for line curves
            let p = 3;
            let f = |a: usize| v.get(p + a..p + a + 3).map(<[f64]>::to_vec);
            let _ = write!(
                s,
                "\n    type={} pt={:?} vec={:?} ivl={:?}",
                at(v, p + 1),
                f(3),
                f(6),
                v.get(p + 9..p + 11)
            );
        }
        s
    }

    #[test]
    fn parallel_cylinders_report() {
        for c in parallel_cylinder_cases() {
            let i: Inputs = c.parse().unwrap();
            let cpp = ffi::intersect(i.op, &i.cone, &i.other, &i.domains, i.tolerance, i.same);
            let rs = rust(&i);
            println!("{}\n  C++ : {}", c.name, cpp.map_or_else(|e| format!("{e}"), |v| lines(&v)));
            println!("  Rust: {}", rs.map_or_else(|e| format!("{e}"), |v| lines(&v)));
        }
    }
}
