//! Differential runner for `surface.surface_p11`: `SmSurface::LocalSurfaceIntersect`
//! (`SmSurface.cpp:27439`). Each case intersects two surfaces, each a biquadratic
//! Bezier patch or an analytic plane, from a pair of guess points, with or without a
//! plane normal, a previous point and boundary testing. Cases whose C++ path reaches
//! `sm_IntersectSrfBndryCrv` (`surface.surface_p8`) or `LocalSurfaceSolve`
//! (`surface.surface_p3`) are deferred.
use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p11::LocalSurfaceIntersectOptions;
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::surface_surface_p11 as ffi;

/// Differential runner for `LocalSurfaceIntersect`.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SurfaceIn {
    /// 0: biquadratic Bezier patch; 1: plane (origin, x axis, y axis in `net[0..3]`).
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

fn harness(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn surface(s: &SurfaceIn) -> Result<Surface, HarnessError> {
    if s.kind == 1 {
        return Ok(Surface::from(Plane::new(
            v3(s.net[0]),
            v3(s.net[1]),
            v3(s.net[2]),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        )));
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
    let a = surface(&i.a)?;
    let b = surface(&i.b)?;
    let k = Kernel::default();
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
                let p = c.evaluate_point(ivl.evaluate(f64::from(j) / 4.0))?;
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

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    if i.op == 1 {
        return run_rust_isb(i);
    }
    let a = surface(&i.a)?;
    let b = surface(&i.b)?;
    let opts = LocalSurfaceIntersectOptions {
        plane_normal: i.plane_normal.map(v3),
        previous_uv: i.previous_uv.map(|p| Point2::new(p[0], p[1])),
        do_boundary_testing: i.boundary_testing,
    };
    let r = a.local_surface_intersect(
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
        let ctx = CaseContext {
            seeds: ctx.seeds.max(600),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
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
/// `SmLine`s upstream, and `Surface::create_iso_boundaries` (unit
/// `surface.surface_p9`) returns them as B-spline curves (plan/findings.md).
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
