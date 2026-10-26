//! Differential runner for `surface.surface_p11`: `SmSurface::LocalSurfaceIntersect`
//! (`SmSurface.cpp:27439`). Each case intersects two surfaces, each a biquadratic
//! Bezier patch, an analytic plane or a canonical analytic torus, from a pair of guess
//! points, with or without a plane normal, a previous point and boundary testing.
//! Cases whose C++ path reaches `sm_IntersectSrfBndryCrv` (`surface.surface_p8`) or
//! `LocalSurfaceSolve` (`surface.surface_p3`) are deferred.
//!
//! Op 2 is `SmSurface::LiftCurve` (`SmSurface.cpp:24054`): a UV curve (an upstream
//! `.dat` file read as `SmBSplineCurve::ReadFromFile` does, or an inline 2D B-spline)
//! lifted onto a surface (a file read as `SmBSplineSurface::ReadFromFile` does, or a
//! patch / plane / torus as above) over its natural or a given domain and interval; the
//! status, `rdMaxDistToSurf` and the whole lifted curve are compared.
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::p2::BSPLINE_CURVE_TYPE;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p11::LocalSurfaceIntersectOptions;
use solidcore::surface::surface::p2::B_SPLINE_SURFACE_TYPE;
use solidcore::surface::surface::Surface;
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore_ffi::units::surface_surface_p11 as ffi;

/// Differential runner for `LocalSurfaceIntersect`.
pub struct Runner;

/// Generated `LiftCurve` cases per run, whatever `--seeds` says (it can only raise
/// this).
pub const LIFT_CASES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfaceIn {
    /// 0: biquadratic Bezier patch; 1: plane (origin, x axis, y axis in `net[0..3]`);
    /// 2: canonical analytic torus (centre `net[0]`, sweep start `net[1]`, 90 degree
    /// direction `net[2]`, `(major, minor)` radii `net[3][0..2]`; the remaining net
    /// values are unused payload and are written as zeros).
    kind: i32,
    net: [[f64; 3]; 9],
    domain: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// 0: `LocalSurfaceIntersect`; 1: `IntersectSurfaceBoundaries`.
    #[serde(default)]
    op: i32,
    a: SurfaceIn,
    b: SurfaceIn,
    tol: f64,
    guesses: [f64; 4],
    #[serde(default)]
    plane_normal: Option<[f64; 3]>,
    #[serde(default)]
    previous_uv: Option<[f64; 2]>,
    #[serde(default)]
    boundary_testing: bool,
}

/// Just the op of a case.
#[derive(Debug, Deserialize)]
struct OpOnly {
    #[serde(default)]
    op: i32,
}

/// The surface of a `LiftCurve` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LiftSurfaceIn {
    /// An upstream `.dat` B-spline surface file (`SmBSplineSurface::ReadFromFile`); the
    /// domain is its natural one unless given.
    File {
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        domain: Option<[f64; 4]>,
    },
    /// A patch or plane; its `domain` is the `LiftCurve` domain.
    Inline(SurfaceIn),
}

/// The UV curve of a `LiftCurve` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LiftCurveIn {
    /// An upstream `.dat` B-spline curve file (`SmBSplineCurve::ReadFromFile`).
    File { file: String },
    /// A non-rational 2D B-spline.
    Inline {
        degree: i64,
        cpts: Vec<[f64; 2]>,
        knots: Vec<f64>,
    },
}

/// Op 2: `surface.LiftCurve(ctx, domain, curve, interval, tol, max_dist, lifted,
/// skip_continuity_checks)`; the interval is the curve's natural one unless given.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LiftInputs {
    op: i32,
    surface: LiftSurfaceIn,
    curve: LiftCurveIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    interval: Option<[f64; 2]>,
    tol: f64,
    #[serde(default)]
    skip_continuity_checks: bool,
}

fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// The full analytic STEP domain of a fixture torus (`[0, 360]` degrees each way), as
/// `SmTorus::CreateCanonical`'s default. The torus's NURB domain (and the intersection
/// domain a case passes) is its own `domain`, `[0, 1]^2` for the fixtures.
fn torus_analytic_domain() -> Extent2d {
    Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0)
}

fn surface(k: &Kernel, s: &SurfaceIn) -> Result<Surface, HarnessError> {
    if s.kind == 1 {
        return Ok(Surface::from(Plane::new(
            v3(s.net[0]),
            v3(s.net[1]),
            v3(s.net[2]),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        )));
    }
    if s.kind == 2 {
        // A real analytic torus, not a B-spline stand-in: `SmTorus::CreateCanonical`
        // (centre, sweep start, 90 degree direction, the two radii, the full analytic
        // STEP domain), which also makes the NURB. Its iso boundaries are the typed
        // circles the boundary-curve override builds.
        let frame = Axis2Placement::new(v3(s.net[0]), v3(s.net[1]), v3(s.net[2]));
        let analytic = torus_analytic_domain();
        return Torus::create_canonical(
            k,
            &frame,
            s.net[3][0],
            s.net[3][1],
            TorusOptions::default(),
            Some(&analytic),
        )
        .map(Surface::from)
        .map_err(harness);
    }
    let net = s
        .net
        .chunks(3)
        .map(|row| {
            row.iter()
                .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
                .collect()
        })
        .collect();
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let nurb = NlSurface::new(net, 2, 2, knots.clone(), knots).map_err(harness)?;
    Ok(Surface::from(BSplineSurface::from_nurb(nurb, false)))
}

fn domain(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn ffi_surface(s: &SurfaceIn) -> ffi::SurfaceInput {
    ffi::SurfaceInput {
        kind: s.kind,
        net: s.net,
        domain: s.domain,
    }
}

/// `[status, found, u, v, u2, v2, deviation]` as an output.
fn output(flat: &[f64]) -> Output {
    match flat {
        [status, ..] if *status != 0.0 => Output::err("LocalSurfaceIntersect failed"),
        [_, found, rest @ ..] => {
            let out = Output::new().int("found", i64::from(*found != 0.0));
            if *found == 0.0 {
                out
            } else {
                out.values("uv", rest.get(..4).unwrap_or(&[]).to_vec())
                    .scalar("deviation", rest.get(4).copied().unwrap_or(f64::NAN))
            }
        }
        _ => Output::err("short output"),
    }
}

/// `[status, n, per curve: deviation, type, interval min, max, 5 points]` as an
/// output.
fn isb_output(flat: &[f64]) -> Output {
    match flat {
        [status, ..] if *status != 0.0 => Output::err("IntersectSurfaceBoundaries failed"),
        [_, n, rest @ ..] => Output::new()
            .int("curves", *n as i64)
            .values("data", rest.to_vec()),
        _ => Output::err("short output"),
    }
}

fn run_rust_isb(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let a = surface(&k, &i.a)?;
    let b = surface(&k, &i.b)?;
    let r = a.intersect_surface_boundaries(
        &k,
        &domain(i.a.domain),
        &b,
        &domain(i.b.domain),
        i.tol,
    );
    let r = r.and_then(|found| {
        let mut flat = vec![0.0, found.curves_3d.len() as f64];
        for (c, dev) in found.curves_3d.iter().zip(&found.deviations) {
            let ivl = c.natural_interval();
            // (SM_TC_TANGENT)
            flat.extend([*dev, 3.0, ivl.min(), ivl.max()]);
            for j in 0..=4 {
                let p = c.evaluate_point(&k, ivl.evaluate(f64::from(j) / 4.0))?;
                flat.extend([p.x, p.y, p.z]);
            }
        }
        Ok(flat)
    });
    Ok(match r {
        Ok(flat) => isb_output(&flat),
        Err(e) => Output::unported(&e).unwrap_or_else(|| isb_output(&[1.0])),
    })
}

/// The ASCII database stream `SmCurve::ReadFromFile` / `SmSurface::ReadFromFile` read
/// `path` from, for a file without an `Object` header (database version 30): a leading
/// line feed is skipped, and a first line starting with `header` (`C` / `S`) too.
fn read_file_db(path: &str, header: u8) -> Result<DatabaseReader, HarnessError> {
    let bytes = std::fs::read(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let mut rest: &[u8] = &bytes;
    if let Some((b'\n', tail)) = rest.split_first() {
        rest = tail;
    }
    match rest.first() {
        Some(b'O') => return Err(harness(format!("{path}: versioned file not supported"))),
        Some(&c) if c == header => {
            let at = rest.iter().position(|&b| b == b'\n').map_or(rest.len(), |p| p + 1);
            rest = rest.get(at..).unwrap_or(&[]);
        }
        _ => {}
    }
    let mut db = DatabaseReader::new();
    db.open_for_read(rest.to_vec(), FileType::Ascii);
    Ok(db)
}

/// The `SmBSplineCurve` part of a lifted curve, with its NURB.
fn bspline_part(c: &Curve) -> Result<std::borrow::Cow<'_, BSplineCurve>, HarnessError> {
    use std::borrow::Cow;
    match c.kind() {
        CurveKind::BSpline(b) => Ok(Cow::Borrowed(b)),
        CurveKind::Line(l) => l.base_with_nurb().map_err(harness),
        CurveKind::Ellipse(e) => Ok(Cow::Borrowed(e.base())),
        CurveKind::Hyperbola(h) => Ok(Cow::Borrowed(h.base())),
        CurveKind::Parabola(p) => Ok(Cow::Borrowed(p.base())),
        _ => Err(harness("lifted curve is not an SmBSplineCurve")),
    }
}

/// `[status, max_dist, type, dim, degree, ncp, cpts (x y z w), nknots, knots, mults,
/// interval min, max]` as an output.
fn lift_output(flat: &[f64]) -> Output {
    match flat {
        [status, ..] if *status != 0.0 => Output::err("LiftCurve failed"),
        [_, max_dist, ty, dim, rest @ ..] => Output::new()
            .scalar("max_dist", *max_dist)
            .int("type", *ty as i64)
            .int("dim", *dim as i64)
            .values("curve", rest.to_vec()),
        _ => Output::err("short output"),
    }
}

fn lift_flat(k: &Kernel, c: &Curve, max_dist: f64) -> solidcore::Result<Vec<f64>> {
    let b = bspline_part(c).map_err(|_| solidcore::SolidError::sm_err("difftest::lift"))?;
    let mut o = vec![0.0, max_dist, c.type_code() as f64, c.dim() as f64];
    o.push(b.degree()? as f64);
    let cp = b.control_points()?;
    o.push(cp.len() as f64);
    for p in cp {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    let (kn, m) = c.knots(k, None)?;
    o.push(kn.len() as f64);
    o.extend_from_slice(&kn);
    o.extend(m.iter().map(|&x| x as f64));
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    Ok(o)
}

fn run_rust_lift(i: &LiftInputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let (s, dom) = match &i.surface {
        LiftSurfaceIn::File { file, domain } => {
            let mut db = read_file_db(file, b'S')?;
            let s = Surface::read_from_db(&k, B_SPLINE_SURFACE_TYPE, &mut db, 30)
                .map_err(|e| harness(format!("{file}: {e}")))?;
            let d = domain.map_or_else(|| s.natural_uv_domain(), domain_of);
            (s, d)
        }
        LiftSurfaceIn::Inline(si) => (surface(&k, si)?, domain(si.domain)),
    };
    let c = match &i.curve {
        LiftCurveIn::File { file } => {
            let mut db = read_file_db(file, b'C')?;
            Curve::read_from_db(&k, BSPLINE_CURVE_TYPE, &mut db, 3, 30)
                .map_err(|e| harness(format!("{file}: {e}")))?
        }
        LiftCurveIn::Inline {
            degree,
            cpts,
            knots,
        } => {
            let p = usize::try_from(*degree).map_err(harness)?;
            let pw = cpts.iter().map(|c| CPoint::new(c[0], c[1], NOZ, NOW)).collect();
            let cur = NlCurve::new(pw, p, knots.clone()).map_err(harness)?;
            Curve::from(BSplineCurve::from_nurb(cur, 2, false))
        }
    };
    let ivl = i
        .interval
        .map_or_else(|| c.natural_interval(), |v| Extent1d::from_min_max(v[0], v[1]));
    let r = s
        .lift_curve(&k, &dom, &c, &ivl, i.tol, i.skip_continuity_checks)
        .and_then(|l| lift_flat(&k, &l.curve, l.max_approx_dev));
    Ok(match r {
        Ok(flat) => lift_output(&flat),
        Err(e) => Output::unported(&e).unwrap_or_else(|| lift_output(&[1.0])),
    })
}

fn domain_of(d: [f64; 4]) -> Extent2d {
    domain(d)
}

fn run_cpp_lift(i: &LiftInputs) -> Result<Output, HarnessError> {
    let (surface, dom) = match &i.surface {
        LiftSurfaceIn::File { file, domain } => (ffi::LiftSurface::File(file.clone()), *domain),
        LiftSurfaceIn::Inline(s) => (
            ffi::LiftSurface::Inline(Box::new(ffi_surface(s))),
            Some(s.domain),
        ),
    };
    let curve = match &i.curve {
        LiftCurveIn::File { file } => ffi::LiftUvCurve::File(file.clone()),
        LiftCurveIn::Inline {
            degree,
            cpts,
            knots,
        } => ffi::LiftUvCurve::Inline {
            degree: *degree,
            cpts: cpts.clone(),
            knots: knots.clone(),
        },
    };
    let flat = ffi::lift_curve(
        &surface,
        dom,
        &curve,
        i.interval,
        i.tol,
        i.skip_continuity_checks,
    )
    .map_err(harness)?;
    Ok(lift_output(&flat))
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    if i.op == 1 {
        return run_rust_isb(i);
    }
    let a = surface(&k, &i.a)?;
    let b = surface(&k, &i.b)?;
    let opts = LocalSurfaceIntersectOptions {
        plane_normal: i.plane_normal.map(v3),
        previous_uv: i.previous_uv.map(|p| Point2::new(p[0], p[1])),
        do_boundary_testing: i.boundary_testing,
    };
    let r = a.local_surface_intersect(
        &k,
        &domain(i.a.domain),
        &b,
        &domain(i.b.domain),
        i.tol,
        Point2::new(i.guesses[0], i.guesses[1]),
        Point2::new(i.guesses[2], i.guesses[3]),
        &opts,
    );
    Ok(match r {
        Ok(Some(s)) => output(&[
            0.0,
            1.0,
            s.uv.x,
            s.uv.y,
            s.other_uv.x,
            s.other_uv.y,
            s.deviation,
        ]),
        Ok(None) => output(&[0.0, 0.0]),
        Err(e) => Output::unported(&e).unwrap_or_else(|| output(&[1.0])),
    })
}

/// A random biquadratic patch over roughly `[0, scale]^2` in `x, y` with heights.
fn patch(rng: &mut Rng, scale: f64, height: f64) -> [[f64; 3]; 9] {
    let mut net = [[0.0; 3]; 9];
    for (index, p) in net.iter_mut().enumerate() {
        *p = [
            (index / 3) as f64 * scale / 2.0 + rng.uniform(-0.1, 0.1) * scale,
            (index % 3) as f64 * scale / 2.0 + rng.uniform(-0.1, 0.1) * scale,
            rng.uniform(-height, height),
        ];
    }
    net
}

/// Swaps the `y` and `z` coordinates (a patch standing across the `xy` one), then
/// shifts it in `y` by `shift`.
fn stand_up(net: &mut [[f64; 3]; 9], shift: f64) {
    for p in net.iter_mut() {
        *p = [p[0], p[2] + shift, p[1] - 0.5 * shift];
    }
}

fn sub_domain(rng: &mut Rng) -> [f64; 4] {
    if rng.chance(0.4) {
        [0.0, 0.0, 1.0, 1.0]
    } else {
        [
            rng.uniform(0.0, 0.3),
            rng.uniform(0.0, 0.3),
            rng.uniform(0.7, 1.0),
            rng.uniform(0.7, 1.0),
        ]
    }
}

fn guess_in(rng: &mut Rng, d: [f64; 4], on_boundary: bool) -> [f64; 2] {
    let mut g = [rng.uniform(d[0], d[2]), rng.uniform(d[1], d[3])];
    if on_boundary {
        match rng.below(4) {
            0 => g[0] = d[0],
            1 => g[0] = d[2],
            2 => g[1] = d[1],
            _ => g[1] = d[3],
        }
    }
    g
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p11"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let main = CaseContext {
            seeds: ctx.seeds.max(600),
            ..ctx.clone()
        };
        let mut v = crate::runner::default_cases(self, &main)?;
        // LiftCurve cases: their own seed stream (`<unit>:lift`), so the other ops'
        // generated cases are unchanged; named `liftNNNN`.
        let lift = CaseContext {
            seeds: ctx.seeds.max(LIFT_CASES),
            ..ctx.clone()
        };
        for mut c in lift.generated_cases("surface.surface_p11:lift", generate_lift) {
            c.name = c.name.replacen("gen", "lift", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.parse::<OpOnly>()?.op == 2 {
            return run_cpp_lift(&case.parse()?);
        }
        let i: Inputs = case.parse()?;
        if i.op == 1 {
            let flat =
                ffi::intersect_surface_boundaries(&ffi_surface(&i.a), &ffi_surface(&i.b), i.tol)
                    .map_err(harness)?;
            return Ok(isb_output(&flat));
        }
        let flat = ffi::local_surface_intersect(
            &ffi_surface(&i.a),
            &ffi_surface(&i.b),
            i.tol,
            i.guesses,
            i.plane_normal,
            i.previous_uv,
            i.boundary_testing,
        )
        .map_err(harness)?;
        Ok(output(&flat))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if case.parse::<OpOnly>()?.op == 2 {
            return run_rust_lift(&case.parse()?);
        }
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.chance(0.2) {
            return generate_isb(rng);
        }

        let tiny = rng.chance(0.1);
        let huge = !tiny && rng.chance(0.1);
        let (lo, hi) = if tiny {
            (1e-4, 1e-2)
        } else if huge {
            (100.0, 1000.0)
        } else {
            (0.5, 3.0)
        };
        let scale = rng.uniform(lo, hi);
        let height = scale * rng.uniform(0.0, 0.6);
        let net_a = patch(rng, scale, height);

        // the other surface: a standing patch, a plane across the first one, or a
        // nearly parallel patch (tangent-touching refinements)
        let mode = rng.below(5);
        let (kind_b, net_b) = match mode {
            0 | 1 => {
                let mut n = patch(rng, scale, height);
                stand_up(&mut n, rng.uniform(0.2, 0.8) * scale);
                (0, n)
            }
            2 | 3 => {
                // a plane across the patch (unit parameter speed: its domain is in
                // model units)
                let mut n = [[0.0; 3]; 9];
                n[0] = [rng.uniform(0.2, 0.8) * scale, 0.0, 0.0];
                n[1] = [rng.uniform(-0.5, 0.5), 1.0, 0.0];
                n[2] = [rng.uniform(-0.3, 0.3), 0.0, 1.0];
                (1, n)
            }
            _ => {
                let mut n = net_a;
                let lift = rng.uniform(-1e-3, 1e-3) * scale;
                for p in &mut n {
                    p[2] += lift + rng.uniform(-0.05, 0.05) * height;
                }
                (0, n)
            }
        };
        let (kind_a, net_a) = if rng.chance(0.15) {
            let mut n = [[0.0; 3]; 9];
            n[1] = [1.0, 0.0, 0.0];
            n[2] = [0.0, 1.0, rng.uniform(-0.3, 0.3)];
            (1, n)
        } else {
            (0, net_a)
        };

        let plane_domain = |rng: &mut Rng| {
            [
                -scale * rng.uniform(0.0, 1.0),
                -scale * rng.uniform(0.0, 1.0),
                scale * rng.uniform(0.5, 1.5),
                scale * rng.uniform(0.5, 1.5),
            ]
        };
        let dom_a = if kind_a == 1 {
            plane_domain(rng)
        } else {
            sub_domain(rng)
        };
        let dom_b = if kind_b == 1 {
            plane_domain(rng)
        } else {
            sub_domain(rng)
        };
        let boundary_testing = rng.chance(0.3);
        let on_boundary = boundary_testing && rng.chance(0.5);
        let a_on = on_boundary && rng.chance(0.5);
        let b_on = on_boundary && (!a_on || rng.chance(0.5));
        let ga = guess_in(rng, dom_a, a_on);
        let gb = guess_in(rng, dom_b, b_on);
        let tol = scale
            * match rng.below(4) {
                0 => 1e-6,
                1 => 1e-4,
                2 => 1e-2,
                _ => rng.uniform(1e-3, 0.3),
            };
        let plane_normal = rng.chance(0.25).then(|| {
            let n = rng.point3(-1.0, 1.0);
            if rng.chance(0.1) {
                [0.0, 0.0, 0.0]
            } else {
                n
            }
        });
        let previous_uv = rng
            .chance(0.25)
            .then(|| guess_in(rng, dom_a, false));
        serde_json::to_value(Inputs {
            op: 0,
            a: SurfaceIn {
                kind: kind_a,
                net: net_a,
                domain: dom_a,
            },
            b: SurfaceIn {
                kind: kind_b,
                net: net_b,
                domain: dom_b,
            },
            tol,
            guesses: [ga[0], ga[1], gb[0], gb[1]],
            plane_normal,
            previous_uv,
            boundary_testing,
        })
        .ok()
    }
}

/// An `IntersectSurfaceBoundaries` case: coincident surfaces (a flat patch in the
/// plane z = 0 and that plane, a patch and itself or a shifted copy) or crossing
/// ones. The other surface is always a patch: an `SmPlane`'s boundary curves are
/// `SmLine`s upstream, which `Surface::create_iso_boundaries` (unit
/// `surface.surface_p9`) returned as B-spline curves until PR #598 (plan/findings.md);
/// plane cases are not generated yet.
fn generate_isb(rng: &mut Rng) -> Option<Value> {
    let scale = if rng.chance(0.1) {
        rng.uniform(1e-3, 1e-2)
    } else {
        rng.uniform(0.5, 3.0)
    };
    let plane_a = rng.chance(0.5);
    let flat = plane_a || rng.chance(0.5);
    let height = if flat { 0.0 } else { scale * rng.uniform(0.0, 0.4) };
    let net_a = patch(rng, scale, height);
    let (kind_b, net_b) = match rng.below(4) {
        // another flat patch in the plane z = 0 (or a curved one)
        0 => (0, patch(rng, scale, height)),
        // the same patch
        1 => (0, net_a),
        // the patch shifted a little in its plane (flat) or in z
        2 => {
            let mut n = net_a;
            let d = [rng.uniform(-0.3, 0.3) * scale, rng.uniform(-0.3, 0.3) * scale];
            for p in &mut n {
                p[0] += d[0];
                p[1] += d[1];
            }
            (0, n)
        }
        // a crossing patch
        _ => {
            let mut n = patch(rng, scale, height);
            stand_up(&mut n, rng.uniform(0.2, 0.8) * scale);
            (0, n)
        }
    };
    let dom_b = sub_domain(rng);
    let tol = scale
        * match rng.below(3) {
            0 => 1e-6,
            1 => 1e-4,
            _ => 1e-3,
        };
    // this surface: the patch, or (dropping through the plane's DropPointFast) the
    // plane z = 0 over a domain around the patch
    let (kind_a, net_a, dom_a) = if plane_a {
        let mut n = [[0.0; 3]; 9];
        n[1] = [1.0, 0.0, 0.0];
        n[2] = [0.0, 1.0, 0.0];
        let dom = [
            -scale * rng.uniform(0.0, 0.5),
            -scale * rng.uniform(0.0, 0.5),
            scale * rng.uniform(0.5, 1.5),
            scale * rng.uniform(0.5, 1.5),
        ];
        (1, n, dom)
    } else {
        (0, net_a, sub_domain(rng))
    };
    serde_json::to_value(Inputs {
        op: 1,
        a: SurfaceIn {
            kind: kind_a,
            net: net_a,
            domain: dom_a,
        },
        b: SurfaceIn {
            kind: kind_b,
            net: net_b,
            domain: dom_b,
        },
        tol,
        guesses: [0.0; 4],
        plane_normal: None,
        previous_uv: None,
        boundary_testing: false,
    })
    .ok()
}

/// A `LiftCurve` case: a 2D B-spline UV curve (an iso line, possibly on the domain
/// boundary, a line segment or a random curve inside the domain) lifted onto a patch
/// over its natural domain or a sub-domain, over the curve's natural interval or a
/// sub-interval.
fn generate_lift(rng: &mut Rng) -> Option<Value> {
    let scale = match rng.below(10) {
        0 => rng.uniform(1e-4, 1e-2),
        1 => rng.uniform(100.0, 1000.0),
        _ => rng.uniform(0.5, 3.0),
    };
    let height = scale * rng.uniform(0.0, 0.6);
    // patches only: a plane defers on curve.crv_on_surf (SmCrvOnSurf over a non-B-spline
    // surface, unported) and would keep this green unit partial; the runner and FFI
    // still take planes (`kind` 1)
    let (kind, net) = (0, patch(rng, scale, height));
    let dom = sub_domain(rng);
    let degree = 1 + rng.below(3) as i64;
    let ncp = degree as usize + 1 + rng.below(4) as usize;
    let at = |rng: &mut Rng, lo: f64, hi: f64| lo + (hi - lo) * rng.unit();
    let cpts: Vec<[f64; 2]> = match rng.below(4) {
        // an iso line (u or v constant), on the boundary half of the time
        0 => {
            let along_v = rng.chance(0.5);
            let (lo, hi) = if along_v { (dom[0], dom[2]) } else { (dom[1], dom[3]) };
            let c = if rng.chance(0.5) {
                if rng.chance(0.5) {
                    lo
                } else {
                    hi
                }
            } else {
                at(rng, lo, hi)
            };
            let (a, b) = if along_v { (dom[1], dom[3]) } else { (dom[0], dom[2]) };
            let (s0, s1) = (at(rng, a, a + 0.3 * (b - a)), at(rng, b - 0.3 * (b - a), b));
            (0..ncp)
                .map(|j| {
                    let t = s0 + (s1 - s0) * j as f64 / (ncp - 1) as f64;
                    if along_v {
                        [c, t]
                    } else {
                        [t, c]
                    }
                })
                .collect()
        }
        // a straight segment
        1 => {
            let p0 = [at(rng, dom[0], dom[2]), at(rng, dom[1], dom[3])];
            let p1 = [at(rng, dom[0], dom[2]), at(rng, dom[1], dom[3])];
            (0..ncp)
                .map(|j| {
                    let t = j as f64 / (ncp - 1) as f64;
                    [p0[0] + (p1[0] - p0[0]) * t, p0[1] + (p1[1] - p0[1]) * t]
                })
                .collect()
        }
        // a random curve inside the domain
        _ => (0..ncp)
            .map(|_| [at(rng, dom[0], dom[2]), at(rng, dom[1], dom[3])])
            .collect(),
    };
    let (k0, k1) = if rng.chance(0.3) {
        let a = rng.uniform(-5.0, 5.0);
        (a, a + rng.uniform(0.1, 10.0))
    } else {
        (0.0, 1.0)
    };
    let p = degree as usize;
    let mut inner: Vec<f64> = (0..ncp - p - 1).map(|_| at(rng, k0, k1)).collect();
    inner.sort_by(f64::total_cmp);
    let mut knots = vec![k0; p + 1];
    knots.extend(inner);
    knots.extend(vec![k1; p + 1]);
    let interval = rng.chance(0.35).then(|| {
        let a = at(rng, k0, k0 + 0.4 * (k1 - k0));
        [a, at(rng, a + 0.1 * (k1 - k0), k1)]
    });
    let tol = scale
        * match rng.below(4) {
            0 => 1e-2,
            1 => 1e-4,
            2 => 1e-6,
            _ => rng.uniform(1e-5, 1e-1),
        };
    serde_json::to_value(LiftInputs {
        op: 2,
        surface: LiftSurfaceIn::Inline(SurfaceIn {
            kind,
            net,
            domain: if kind == 1 { [0.0, 0.0, 1.0, 1.0] } else { dom },
        }),
        curve: LiftCurveIn::Inline {
            degree,
            cpts,
            knots,
        },
        interval,
        tol,
        skip_continuity_checks: rng.chance(0.2),
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    use solidcore::surface::surface::p2::SurfaceKind;

    /// The torus payload of
    /// `fixtures/surface.surface_p11/analytic-torus-iso-boundary.json`.
    fn torus_payload() -> SurfaceIn {
        let mut net = [[0.0; 3]; 9];
        net[0] = [0.2, 0.5, 0.5];
        net[1] = [1.0, 0.0, 0.0];
        net[2] = [0.0, 1.0, 0.0];
        net[3] = [0.3, 0.2, 0.0];
        SurfaceIn {
            kind: 2,
            net,
            domain: [0.0, 0.0, 1.0, 1.0],
        }
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Kind 2 builds the canonical analytic torus (not a B-spline stand-in) over the
    /// full analytic STEP domain, whose NURB — the domain `IntersectSurfaceBoundaries`
    /// is given for it — stays `[0, 1]^2`. A Rust-only diagnostic of the shared
    /// builder; it proves no native parity.
    #[test]
    fn kind2_builds_a_canonical_analytic_torus() {
        let k = Kernel::default();
        let s = surface(&k, &torus_payload()).expect("canonical torus");
        let SurfaceKind::Torus(t) = s.kind() else {
            panic!("kind 2 must build the analytic torus, not a B-spline stand-in");
        };
        assert!(close(t.major_radius(), 0.3, 1e-12));
        assert!(close(t.minor_radius(), 0.2, 1e-12));
        assert!(!t.inside_out());
        let nurb = s.natural_uv_domain();
        assert!(close(nurb.min().x, 0.0, 1e-12) && close(nurb.max().x, 1.0, 1e-12));
        assert!(close(nurb.min().y, 0.0, 1e-12) && close(nurb.max().y, 1.0, 1e-12));
        let step = s.step_uv_domain();
        assert!(close(step.min().x, 0.0, 1e-12) && close(step.max().x, 360.0, 1e-12));
        assert!(close(step.min().y, 0.0, 1e-12) && close(step.max().y, 360.0, 1e-12));
    }

    /// The torus's u-min iso boundary (what `CreateIsoBoundaries` hands
    /// `IntersectSurfaceBoundaries` as the other surface's first boundary curve) is the
    /// typed minor circle of the source prediction: centre `(0.5, 0.5, 0.5)`, radius
    /// `0.2`, in the plane `y = 0.5`. This checks the constructed geometry only; the
    /// number of boundary curves the native operation keeps is deliberately not
    /// asserted here.
    #[test]
    fn kind2_u_min_iso_boundary_is_the_predicted_minor_circle() {
        let k = Kernel::default();
        let s = surface(&k, &torus_payload()).expect("canonical torus");
        let (min, _) = s
            .create_iso_boundaries(&k, SurfParamType::U, 1e-6)
            .expect("iso boundaries")
            .expect("both U boundaries");
        let CurveKind::Ellipse(_) = min.kind() else {
            panic!("the torus iso boundary must stay the typed circle");
        };
        let centre = Vec3::new(0.5, 0.5, 0.5);
        let ivl = min.natural_interval();
        for j in 0..=4 {
            let t = ivl.evaluate(f64::from(j) / 4.0);
            let p = min.evaluate_point(&k, t).expect("iso boundary point");
            let radius = (p - centre).length();
            assert!(close(radius, 0.2, 1e-9), "t={t}: radius {radius}");
            assert!(close(p.y, 0.5, 1e-12), "t={t}: point {p:?} off y=0.5");
        }
    }
}
