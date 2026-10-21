//! Runner for unit `intersect.ray_tracer`: `SmRayTracer` and `SmLSI3LocalSolver`.
//!
//! C++: `solidcore_ffi::units::intersect_ray_tracer` (upstream
//! `source/SMLib/src/SmRayTracer.cpp`). Rust: `solidcore::intersect::ray_tracer`.
//!
//! A case is a set of surfaces (planes and Bezier-knot B-spline patches), a grid with the
//! tracer's tolerances and a program of ops: `solver_tol` (`SetSolverTolerance`), `add_leaf`
//! (one surface subdivision leaf into the grid: `SmGrid::AddElement(SM_GE_SURFACE)`, with the
//! leaf's domain, boxes and polar vectors as data and the mid-point data computed from the
//! surface as `SmSurfaceGridElement::Init` does), `fire` (`SmRayTracer::FireRay`), `local`
//! (`SmLSI3LocalSolver::LocalSolve` on one leaf) and `add_surface`
//! (`SmRayTracer::AddSurfaceToGrid`, not run in C++: it needs the surface cache). Both sides
//! produce the flat record documented in the ffi module, reported as the values `rec`.
//!
//! Deferred cases: every ray or solve that gets past the leaf's pseudo box test reaches
//! `SmSurface::LocalLineIntersect` (`surface.surface_p8`); `add_surface` builds the surface cache and
//! then needs `SmSurface::CalculateContinuities` (`surface.surface_p6`). The generator makes mostly cases whose rays are rejected before
//! that (grid miss, element box rejection by parameter, skipped element, pseudo box miss) and
//! checks that with the Rust side, plus free cases that defer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::grid::{ElementId, Grid, SurfaceGridElement};
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::Solution;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::intersect::ray_tracer::{
    surface_leaf, LightSource, Lsi3LocalSolver, RayObject, RayRenderer, RayTracer,
};
use solidcore::kernel::Kernel;
use solidcore::math::BIG_DOUBLE;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::SolidError;
use solidcore_ffi::units::intersect_ray_tracer as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `intersect.ray_tracer` runner.
pub struct Runner;

type P3 = [f64; 3];
type E6 = [f64; 6];

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Bezier {
        p: usize,
        q: usize,
        net: Vec<Vec<[f64; 4]>>,
    },
    Plane {
        o: P3,
        x: P3,
        y: P3,
        scale: [f64; 2],
        domain: [f64; 4],
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct GridIn {
    bbox: E6,
    size: [usize; 3],
    ray_tol: f64,
    ray_acc: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    SolverTol {
        tol: f64,
    },
    AddLeaf {
        surface: usize,
        uv: [f64; 4],
        bbox: E6,
        pseudo: E6,
        /// Orthonormal basis of a rotated pseudo box (`pseudo` then holds its intervals).
        #[serde(default)]
        pseudo_basis: Option<[P3; 3]>,
        polar: Vec<P3>,
        elem_box: E6,
    },
    Fire {
        p: P3,
        v: P3,
        max_t: f64,
        hit_any: bool,
        skip: Option<usize>,
    },
    Local {
        leaf: usize,
        p: P3,
        v: P3,
        d3d_tol: f64,
        dist_tol: f64,
        acc: f64,
        ray_fire: bool,
        have_t: bool,
        t_min: f64,
        t_max: f64,
        interval: Option<[f64; 2]>,
    },
    AddSurface {
        surface: usize,
    },
    Renderer {
        origin: P3,
        x: P3,
        y: P3,
        view: E6,
        nx: usize,
        ny: usize,
        rate: usize,
    },
    Light {
        v: P3,
        point: bool,
    },
    DoRender {
        shadows: bool,
        background: P3,
        object: P3,
    },
    RenderPixel {
        x: usize,
        y: usize,
    },
    RenderRay {
        p: P3,
        v: P3,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surfaces: Vec<SurfIn>,
    grid: GridIn,
    ops: Vec<OpIn>,
}

// ---- C++ side -----------------------------------------------------------------------

fn ffi_program(i: &Inputs) -> ffi::Program {
    ffi::Program {
        surfaces: i
            .surfaces
            .iter()
            .map(|s| match s {
                SurfIn::Bezier { p, q, net } => ffi::SurfaceIn::Bezier {
                    p: *p,
                    q: *q,
                    net: net.clone(),
                },
                SurfIn::Plane {
                    o,
                    x,
                    y,
                    scale,
                    domain,
                } => ffi::SurfaceIn::Plane {
                    o: *o,
                    x: *x,
                    y: *y,
                    scale: *scale,
                    domain: *domain,
                },
            })
            .collect(),
        grid: ffi::GridIn {
            bbox: i.grid.bbox,
            size: i.grid.size,
            ray_tolerance: i.grid.ray_tol,
            ray_accuracy: i.grid.ray_acc,
        },
        ops: i
            .ops
            .iter()
            .map(|op| match op {
                OpIn::SolverTol { tol } => ffi::Op::SolverTol(*tol),
                OpIn::AddLeaf {
                    surface,
                    uv,
                    bbox,
                    pseudo,
                    pseudo_basis,
                    polar,
                    elem_box,
                } => ffi::Op::AddLeaf(ffi::LeafIn {
                    surface: *surface,
                    uv_domain: *uv,
                    bbox: *bbox,
                    pseudo_box: *pseudo,
                    pseudo_basis: *pseudo_basis,
                    polar: polar.clone(),
                    elem_box: *elem_box,
                }),
                OpIn::Fire {
                    p,
                    v,
                    max_t,
                    hit_any,
                    skip,
                } => ffi::Op::Fire(ffi::FireIn {
                    point: *p,
                    vector: *v,
                    max_t: *max_t,
                    hit_any: *hit_any,
                    skip: *skip,
                }),
                OpIn::Local {
                    leaf,
                    p,
                    v,
                    d3d_tol,
                    dist_tol,
                    acc,
                    ray_fire,
                    have_t,
                    t_min,
                    t_max,
                    interval,
                } => ffi::Op::Local(ffi::LocalIn {
                    leaf: *leaf,
                    point: *p,
                    vector: *v,
                    d3d_tolerance: *d3d_tol,
                    distance_tolerance: *dist_tol,
                    ray_accuracy: *acc,
                    ray_fire: *ray_fire,
                    have_t_min_t_max: *have_t,
                    t_min: *t_min,
                    t_max: *t_max,
                    interval: *interval,
                }),
                OpIn::AddSurface { surface } => ffi::Op::AddSurface(*surface),
                OpIn::Renderer {
                    origin,
                    x,
                    y,
                    view,
                    nx,
                    ny,
                    rate,
                } => ffi::Op::Renderer(ffi::RendererIn {
                    origin: *origin,
                    x: *x,
                    y: *y,
                    view: *view,
                    nx: *nx,
                    ny: *ny,
                    rate: *rate,
                }),
                OpIn::Light { v, point } => ffi::Op::Light {
                    vector: *v,
                    is_point: *point,
                },
                OpIn::DoRender {
                    shadows,
                    background,
                    object,
                } => ffi::Op::DoRender {
                    shadows: *shadows,
                    background: *background,
                    object: *object,
                },
                OpIn::RenderPixel { x, y } => ffi::Op::RenderPixel(*x, *y),
                OpIn::RenderRay { p, v } => ffi::Op::RenderRay(*p, *v),
            })
            .collect(),
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn v3(p: P3) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn ext3(e: E6) -> Extent3d {
    Extent3d::from_bounds(e[0], e[1], e[2], e[3], e[4], e[5])
}

fn ext2(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn cpoint(c: &[f64; 4]) -> CPoint {
    CPoint::new(c[0], c[1], c[2], c[3])
}

fn rust_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext2(*domain),
        ))),
        SurfIn::Bezier { p, q, net } => {
            let net = net
                .iter()
                .map(|r| r.iter().map(cpoint).collect())
                .collect();
            let clamped = |d: usize| -> Vec<f64> {
                (0..2 * (d + 1))
                    .map(|i| if i <= d { 0.0 } else { 1.0 })
                    .collect()
            };
            let sur = NurbSurface::new(net, *p, *q, clamped(*p), clamped(*q))
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
    }
}

/// The status of a failed Rust operation, as upstream numbers it.
fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1001.0,
    }
}

fn put_solution(o: &mut Vec<f64>, s: &Solution<RayObject>) {
    o.push(f64::from(s.solution_type.code()));
    o.push(s.num_variables as f64);
    o.push(s.num_objects as f64);
    o.push(s.start.solution_value);
    o.extend(s.start.parameters);
    o.push(match s.objects.first().copied().flatten() {
        Some(RayObject::Surface(i)) => i as f64,
        Some(_) => -2.0,
        None => -1.0,
    });
    o.push(match s.objects.get(1).copied().flatten() {
        None => -1.0,
        Some(_) => -2.0,
    });
    o.push(if s.nodes.first().copied().flatten().is_some() {
        1.0
    } else {
        0.0
    });
}

fn put_color(
    rec: &mut Vec<f64>,
    res: solidcore::Result<Vec3>,
    shaded: u64,
) -> solidcore::Result<()> {
    match res {
        Ok(c) => {
            rec.push(0.0);
            rec.extend([c.x, c.y, c.z]);
        }
        Err(e @ SolidError::Unported { .. }) => return Err(e),
        Err(e) => {
            rec.push(status_of(&e));
            rec.extend([0.0; 3]);
        }
    }
    rec.push(shaded as f64);
    Ok(())
}

/// Coverage counters of a Rust run (used by the generator test).
#[derive(Debug, Default)]
struct Stats {
    fires: usize,
    grid_misses: usize,
    walks: usize,
    elements_seen: usize,
    rejected_leaves: usize,
    errors: usize,
}

fn run_rust(k: &Kernel, i: &Inputs, st: &mut Stats) -> solidcore::Result<Vec<f64>> {
    let surfaces: Vec<Surface> = i
        .surfaces
        .iter()
        .map(|s| {
            rust_surface(s).map_err(|_| SolidError::InvalidInput {
                operation: "difftest::intersect_ray_tracer",
                reason: "surface construction failed",
            })
        })
        .collect::<Result<_, _>>()?;
    let grid = Grid::new(&ext3(i.grid.bbox), i.grid.size)?;
    let mut tracer_slot = Some(RayTracer::new(grid, i.grid.ray_tol, i.grid.ray_acc));
    let mut renderer: Option<RayRenderer> = None;
    if let Some(t) = tracer_slot.as_mut() {
        for s in &surfaces {
            t.register_surface(s);
        }
    }
    // the tracer lives in the renderer once there is one
    macro_rules! tracer {
        () => {
            match (tracer_slot.as_mut(), renderer.as_mut()) {
                (Some(t), _) => t,
                (None, Some(r)) => r.ray_tracer_mut(),
                (None, None) => {
                    return Err(SolidError::InvalidInput {
                        operation: "difftest::intersect_ray_tracer",
                        reason: "tracer",
                    })
                }
            }
        };
    }
    let mut leaves: Vec<SurfaceGridElement<usize>> = Vec::new();
    let mut rec: Vec<f64> = Vec::new();
    let bad_index = |what: &'static str| SolidError::InvalidInput {
        operation: "difftest::intersect_ray_tracer",
        reason: what,
    };
    for op in &i.ops {
        match op {
            OpIn::SolverTol { tol } => tracer!().set_solver_tolerance(*tol),
            OpIn::AddLeaf {
                surface,
                uv,
                bbox,
                pseudo,
                pseudo_basis,
                polar,
                elem_box,
            } => {
                let surf = surfaces.get(*surface).ok_or_else(|| bad_index("surface"))?;
                let mut polar_box = PolarBox::from_vector(v3(
                    polar.first().copied().ok_or_else(|| bad_index("polar"))?,
                ));
                for v in polar.iter().skip(1) {
                    polar_box.add_vector(v3(*v), None);
                }
                let leaf = surface_leaf(
                    k, *surface,
                    surf,
                    &ext2(*uv),
                    &ext3(*bbox),
                    &match pseudo_basis {
                        Some([x, y, z]) => PseudoBox::from_basis_intervals(
                            [v3(*x), v3(*y), v3(*z)],
                            [
                                Extent1d::from_min_max(pseudo[0], pseudo[3]),
                                Extent1d::from_min_max(pseudo[1], pseudo[4]),
                                Extent1d::from_min_max(pseudo[2], pseudo[5]),
                            ],
                        ),
                        None => PseudoBox::from_extent3d(&ext3(*pseudo)),
                    },
                    &polar_box,
                )?;
                tracer!()
                    .grid_mut()
                    .add_surface_element(leaf, &ext3(*elem_box))?;
                leaves.push(leaf);
            }
            OpIn::Fire {
                p,
                v,
                max_t,
                hit_any,
                skip,
            } => {
                let tracer = tracer!();
                tracer.hit_any_thing = *hit_any;
                tracer.skip_element = skip.map(ElementId);
                st.fires += 1;
                match tracer.fire_ray(k, v3(*p), v3(*v), *max_t) {
                    Ok(hit) => {
                        rec.push(0.0);
                        rec.push(f64::from(u8::from(hit.hits_something)));
                        put_solution(&mut rec, &hit.solution);
                        rec.push(hit.grid_elements.len() as f64);
                        rec.extend(hit.grid_elements.iter().map(|e| e.0 as f64));
                        rec.extend(&hit.min_distances);
                        rec.extend(&hit.max_distances);
                        st.walks += 1;
                        st.elements_seen += hit.grid_elements.len();
                        if hit.grid_elements.is_empty() {
                            st.grid_misses += 1;
                        }
                        st.rejected_leaves += usize::from(!hit.hits_something);
                    }
                    Err(e @ SolidError::Unported { .. }) => return Err(e),
                    Err(e) => {
                        st.errors += 1;
                        rec.push(status_of(&e));
                        rec.push(0.0);
                        put_solution(&mut rec, &Solution::new());
                        rec.push(0.0);
                    }
                }
            }
            OpIn::Local {
                leaf,
                p,
                v,
                d3d_tol,
                dist_tol,
                acc,
                ray_fire,
                have_t,
                t_min,
                t_max,
                interval,
            } => {
                let leaf = leaves.get(*leaf).ok_or_else(|| bad_index("leaf"))?;
                let surf = surfaces
                    .get(leaf.surface)
                    .ok_or_else(|| bad_index("surface"))?;
                let ivl = interval.map(|[a, b]| Extent1d::from_min_max(a, b));
                let mut solver = Lsi3LocalSolver::new(
                    &leaf.uv_domain,
                    v3(*p),
                    v3(*v),
                    ivl.as_ref(),
                    *ray_fire,
                    *dist_tol,
                    *acc,
                );
                solver.d3d_tolerance = *d3d_tol;
                solver.have_t_min_t_max = *have_t;
                solver.t_min_of_bbox = *t_min;
                solver.t_max_of_bbox = *t_max;
                let r = solver.local_solve(k, surf, leaf);
                match &r.status {
                    Ok(()) => rec.push(0.0),
                    Err(e @ SolidError::Unported { .. }) => return Err(e.clone()),
                    Err(e) => rec.push(status_of(e)),
                }
                rec.push(f64::from(u8::from(r.found_intersection)));
                rec.push(f64::from(u8::from(r.needs_more_subdivision)));
                put_solution(&mut rec, &r.solution);
            }
            OpIn::AddSurface { surface } => {
                let surf = surfaces.get(*surface).ok_or_else(|| bad_index("surface"))?;
                tracer!().add_surface_to_grid(&Kernel::default(), surf)?;
            }
            OpIn::Renderer {
                origin,
                x,
                y,
                view,
                nx,
                ny,
                rate,
            } => {
                let t = tracer_slot.take().ok_or_else(|| bad_index("renderer"))?;
                let place = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
                let mut r = RayRenderer::new(t, &place, &ext3(*view), *nx as u64, *ny as u64);
                r.set_sample_rate(*rate as u64);
                renderer = Some(r);
            }
            OpIn::Light { v, point } => {
                let r = renderer.as_mut().ok_or_else(|| bad_index("renderer"))?;
                r.add_light_source(LightSource::new(v3(*v), *point));
            }
            OpIn::DoRender {
                shadows,
                background,
                object,
            } => {
                let r = renderer.as_mut().ok_or_else(|| bad_index("renderer"))?;
                let res = r.do_render(k, *shadows, v3(*background), v3(*object));
                match res {
                    Ok(_) => rec.push(0.0),
                    Err(e @ SolidError::Unported { .. }) => return Err(e),
                    Err(e) => rec.push(status_of(&e)),
                }
                rec.push(r.pixels_shaded() as f64);
            }
            OpIn::RenderPixel { x, y } => {
                let r = renderer.as_mut().ok_or_else(|| bad_index("renderer"))?;
                let res = r.render_pixel(k, *x as u64, *y as u64);
                put_color(&mut rec, res, r.pixels_shaded())?;
            }
            OpIn::RenderRay { p, v } => {
                let r = renderer.as_mut().ok_or_else(|| bad_index("renderer"))?;
                let res = r.render_ray(k, v3(*p), v3(*v));
                put_color(&mut rec, res, r.pixels_shaded())?;
            }
        }
    }
    Ok(rec)
}

// ---- generation ---------------------------------------------------------------------

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn len3(a: P3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unit3(a: P3) -> P3 {
    let l = len3(a);
    if l == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        scale3(a, 1.0 / l)
    }
}

fn dot3(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A random right-handed orthonormal basis (Gram-Schmidt on random directions).
fn rotation_basis(rng: &mut Rng) -> [P3; 3] {
    let x = unit_dir(rng);
    loop {
        let t = unit_dir(rng);
        let k = dot3(t, x);
        let y = [t[0] - k * x[0], t[1] - k * x[1], t[2] - k * x[2]];
        if len3(y) > 0.1 {
            let y = unit3(y);
            let z = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            return [x, y, z];
        }
    }
}

fn unit_dir(rng: &mut Rng) -> P3 {
    loop {
        let d = [
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        ];
        if len3(d) > 0.1 {
            return unit3(d);
        }
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    if rng.chance(0.4) {
        let x = unit_dir(rng);
        let mut y = unit_dir(rng);
        let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
        if dot.abs() > 0.9 {
            y = [x[1], -x[0], 0.0];
            if y[0] * y[0] + y[1] * y[1] < 1e-6 {
                y = [0.0, x[2], -x[1]];
            }
        }
        let o = rng.point3(-scale, scale);
        let s = [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)];
        let (u0, v0) = (
            rng.uniform(-1.0, 1.0) * scale,
            rng.uniform(-1.0, 1.0) * scale,
        );
        SurfIn::Plane {
            o,
            x,
            y,
            scale: s,
            domain: [
                u0,
                v0,
                u0 + rng.uniform(1.0, 4.0) * scale,
                v0 + rng.uniform(1.0, 4.0) * scale,
            ],
        }
    } else {
        let p = rng.range_i64(1, 3) as usize;
        let q = rng.range_i64(1, 3) as usize;
        let origin = rng.point3(-scale, scale);
        let (du, dv) = (unit_dir(rng), unit_dir(rng));
        let rational = rng.chance(0.3);
        let net = (0..=p)
            .map(|i| {
                (0..=q)
                    .map(|j| {
                        let a = i as f64 / p as f64 * 2.0 * scale;
                        let b = j as f64 / q as f64 * 2.0 * scale;
                        let n = scale * 0.3;
                        let c = add(
                            add(origin, add(scale3(du, a), scale3(dv, b))),
                            [
                                rng.uniform(-n, n),
                                rng.uniform(-n, n),
                                rng.uniform(-n, n),
                            ],
                        );
                        let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
                        [c[0], c[1], c[2], w]
                    })
                    .collect()
            })
            .collect();
        SurfIn::Bezier { p, q, net }
    }
}

fn natural(s: &SurfIn) -> [f64; 4] {
    match s {
        SurfIn::Plane { domain, .. } => *domain,
        SurfIn::Bezier { .. } => [0.0, 0.0, 1.0, 1.0],
    }
}

fn grow(b: E6, d: f64) -> E6 {
    [
        b[0] - d,
        b[1] - d,
        b[2] - d,
        b[3] + d,
        b[4] + d,
        b[5] + d,
    ]
}

fn box_union(b: E6, p: P3) -> E6 {
    [
        b[0].min(p[0]),
        b[1].min(p[1]),
        b[2].min(p[2]),
        b[3].max(p[0]),
        b[4].max(p[1]),
        b[5].max(p[2]),
    ]
}

/// Leaf ops for a surface split into `nu` x `nv` cells: sampled boxes, polar vectors from
/// the normals at the corners and middle, element boxes a little larger than the pseudo
/// boxes (so rays can cross an element box and miss its pseudo box).
fn gen_leaves(k: &Kernel, 
    rng: &mut Rng,
    index: usize,
    s: &SurfIn,
    surf: &Surface,
    scale: f64,
    ray_tol: f64,
) -> Vec<OpIn> {
    let nat = natural(s);
    let (nu, nv) = (rng.range_i64(1, 3) as usize, rng.range_i64(1, 3) as usize);
    let mut ops = Vec::new();
    for iu in 0..nu {
        for iv in 0..nv {
            // the last cell ends exactly on the natural domain
            let cut = |lo: f64, hi: f64, i: usize, n: usize| -> f64 {
                if i == n {
                    hi
                } else {
                    lo + (hi - lo) * i as f64 / n as f64
                }
            };
            let u0 = cut(nat[0], nat[2], iu, nu);
            let u1 = cut(nat[0], nat[2], iu + 1, nu);
            let v0 = cut(nat[1], nat[3], iv, nv);
            let v1 = cut(nat[1], nat[3], iv + 1, nv);
            let mut bbox: Option<E6> = None;
            for a in 0..5 {
                for b in 0..5 {
                    let uv = Vec2::new(
                        u0 + (u1 - u0) * f64::from(a) / 4.0,
                        v0 + (v1 - v0) * f64::from(b) / 4.0,
                    );
                    if let Ok(p) = surf.evaluate_point(k, uv) {
                        let p = [p.x, p.y, p.z];
                        bbox = Some(match bbox {
                            None => [p[0], p[1], p[2], p[0], p[1], p[2]],
                            Some(bb) => box_union(bb, p),
                        });
                    }
                }
            }
            let Some(bbox) = bbox else { continue };
            let mut polar = Vec::new();
            for (a, b) in [(0.5, 0.5), (0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let uv = Vec2::new(u0 + (u1 - u0) * a, v0 + (v1 - v0) * b);
                if let Ok(n) = surf.evaluate_normal(k, uv, Side::Left, Side::Left) {
                    let n = [n.x, n.y, n.z];
                    if len3(n) > 1e-6 {
                        polar.push(n);
                    }
                }
            }
            if polar.is_empty() {
                continue;
            }
            let size = len3(sub([bbox[3], bbox[4], bbox[5]], [bbox[0], bbox[1], bbox[2]]));
            let pseudo = if rng.chance(0.3) {
                bbox
            } else {
                grow(bbox, rng.uniform(0.0, 0.05) * size)
            };
            // like AddSurfaceToGrid: the element box is the box grown by the tolerance, here
            // by more so that rays can pass between the boxes
            let elem_box = grow(bbox, ray_tol + rng.uniform(0.0, 0.3) * size.max(scale * 0.1));
            // a rotated pseudo box (its intervals along an orthonormal basis), or an
            // inverted one: both are defined `LocalSolve` / `FireRay` paths
            let (pseudo, pseudo_basis) = if rng.chance(0.15) {
                let b = rotation_basis(rng);
                let r = 0.5 * size.max(scale * 0.1);
                let c = [(bbox[0] + bbox[3]) * 0.5, (bbox[1] + bbox[4]) * 0.5, (bbox[2] + bbox[5]) * 0.5];
                let cc = [dot3(c, b[0]), dot3(c, b[1]), dot3(c, b[2])];
                let h = r * rng.uniform(0.3, 1.2);
                (
                    [cc[0] - h, cc[1] - h, cc[2] - h, cc[0] + h, cc[1] + h, cc[2] + h],
                    Some(b),
                )
            } else if rng.chance(0.05) {
                (
                    [pseudo[3], pseudo[4], pseudo[5], pseudo[0], pseudo[1], pseudo[2]],
                    None,
                )
            } else {
                (pseudo, None)
            };
            ops.push(OpIn::AddLeaf {
                surface: index,
                uv: [u0, v0, u1, v1],
                bbox,
                pseudo,
                pseudo_basis,
                polar,
                elem_box,
            });
        }
    }
    ops
}

fn gen_case(rng: &mut Rng, safe: bool) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 0.01, 100.0]).unwrap_or(&1.0);
    let ns = rng.range_i64(1, 3) as usize;
    let surfaces: Vec<SurfIn> = (0..ns).map(|_| gen_surface(rng, scale)).collect();
    let rust: Vec<Surface> = surfaces.iter().filter_map(|s| rust_surface(s).ok()).collect();
    let ray_tol = *rng.choose(&[1e-5, 1e-6, 1e-3, 0.0]).unwrap_or(&1e-5) * scale;
    let ray_acc = *rng.choose(&[1e-8, 1e-10, 0.0, 1e-6]).unwrap_or(&1e-8);

    let mut leaf_ops = Vec::new();
    for (k, (s, r)) in surfaces.iter().zip(&rust).enumerate() {
        leaf_ops.extend(gen_leaves(&Kernel::default(), rng, k, s, r, scale, ray_tol));
    }
    leaf_ops.truncate(40);
    // the grid box: around every leaf box
    let mut all: Option<E6> = None;
    for op in &leaf_ops {
        if let OpIn::AddLeaf { elem_box, .. } = op {
            all = Some(match all {
                None => *elem_box,
                Some(a) => box_union(box_union(a, [elem_box[0], elem_box[1], elem_box[2]]), [
                    elem_box[3],
                    elem_box[4],
                    elem_box[5],
                ]),
            });
        }
    }
    let base = all.unwrap_or([-scale, -scale, -scale, scale, scale, scale]);
    let margin = rng.uniform(0.0, 0.3) * scale.max(len3(sub(
        [base[3], base[4], base[5]],
        [base[0], base[1], base[2]],
    )));
    let gbox = grow(base, margin + 1e-3 * scale);
    let size = [
        rng.range_i64(1, 9) as usize,
        rng.range_i64(1, 9) as usize,
        rng.range_i64(1, 9) as usize,
    ];
    let grid = GridIn {
        bbox: gbox,
        size,
        ray_tol,
        ray_acc,
    };
    let center = [
        0.5 * (gbox[0] + gbox[3]),
        0.5 * (gbox[1] + gbox[4]),
        0.5 * (gbox[2] + gbox[5]),
    ];
    let diag = len3(sub([gbox[3], gbox[4], gbox[5]], [gbox[0], gbox[1], gbox[2]]));

    let mut ops = Vec::new();
    if rng.chance(0.4) {
        ops.push(OpIn::SolverTol {
            tol: rng.uniform(0.0, 1e-4) * scale,
        });
    }
    let n_leaves = leaf_ops.len();
    ops.extend(leaf_ops);

    // a point of a random leaf box
    let leaf_target = |rng: &mut Rng, ops: &[OpIn]| -> P3 {
        let boxes: Vec<E6> = ops
            .iter()
            .filter_map(|o| match o {
                OpIn::AddLeaf { elem_box, .. } => Some(*elem_box),
                _ => None,
            })
            .collect();
        match rng.choose(&boxes) {
            Some(b) => [
                b[0] + rng.unit() * (b[3] - b[0]),
                b[1] + rng.unit() * (b[4] - b[1]),
                b[2] + rng.unit() * (b[5] - b[2]),
            ],
            None => center,
        }
    };

    for _ in 0..rng.range_i64(1, 4) {
        let target = leaf_target(rng, &ops);
        let start = add(center, scale3(unit_dir(rng), diag * rng.uniform(0.2, 1.5)));
        let mut dir = match rng.below(8) {
            0 => unit_dir(rng),
            1 => {
                // along an axis
                let mut d = [0.0; 3];
                if let Some(x) = d.get_mut(rng.below(3) as usize) {
                    *x = if rng.chance(0.5) { 1.0 } else { -1.0 };
                }
                d
            }
            _ => sub(target, start),
        };
        if len3(dir) < 1e-9 {
            dir = [1.0, 0.0, 0.0];
        }
        if rng.chance(0.15) {
            // inside the grid: start at the target, go on
            let d = unit3(dir);
            let st = target;
            ops.push(OpIn::Fire {
                p: st,
                v: d,
                max_t: if rng.chance(0.7) { BIG_DOUBLE } else { rng.uniform(0.0, 1.0) * diag },
                hit_any: rng.chance(0.4),
                skip: if n_leaves > 0 && rng.chance(0.4) {
                    Some(rng.below(n_leaves as u64) as usize)
                } else {
                    None
                },
            });
            continue;
        }
        let d = if rng.chance(0.1) { scale3(dir, 1e-3) } else { unit3(dir) };
        ops.push(OpIn::Fire {
            p: start,
            v: d,
            max_t: match rng.below(4) {
                0 => rng.uniform(0.0, 1.0) * diag,
                1 => -rng.unit() * diag,
                _ => BIG_DOUBLE,
            },
            hit_any: rng.chance(0.4),
            skip: if n_leaves > 0 && rng.chance(0.3) {
                Some(rng.below(n_leaves as u64) as usize)
            } else {
                None
            },
        });
    }
    if rng.chance(0.07) {
        ops.push(OpIn::Fire {
            p: center,
            v: [0.0, 0.0, 0.0],
            max_t: BIG_DOUBLE,
            hit_any: false,
            skip: None,
        });
    }
    if n_leaves > 0 {
        for _ in 0..rng.range_i64(0, 4) {
            let leaf = rng.below(n_leaves as u64) as usize;
            let target = leaf_target(rng, &ops);
            let start = add(target, scale3(unit_dir(rng), diag * rng.uniform(0.05, 1.0)));
            let dir = if rng.chance(0.7) {
                sub(target, start)
            } else {
                unit_dir(rng)
            };
            // a zero ray vector is a defined path (`LocalSolve` returns early)
            let d = if rng.chance(0.05) { [0.0; 3] } else { unit3(dir) };
            ops.push(OpIn::Local {
                leaf,
                p: start,
                v: d,
                d3d_tol: rng.uniform(0.0, 1e-5) * scale,
                dist_tol: ray_tol,
                acc: ray_acc,
                ray_fire: rng.chance(0.8),
                have_t: rng.chance(0.8),
                t_min: rng.uniform(0.0, 0.5) * diag,
                t_max: rng.uniform(0.5, 1.5) * diag,
                interval: match rng.below(4) {
                    0 | 1 => Some([0.0, 1e10]),
                    2 => Some([0.0, rng.uniform(0.2, 2.0) * diag]),
                    _ => None,
                },
            });
        }
    }
    if rng.chance(0.15) {
        // look down +z from below the grid, over a random part of its xy extent
        let (x0, x1) = (rng.uniform(gbox[0], gbox[3]), rng.uniform(gbox[0], gbox[3]));
        let (y0, y1) = (rng.uniform(gbox[1], gbox[4]), rng.uniform(gbox[1], gbox[4]));
        let z = gbox[2] - 0.1 * diag;
        ops.push(OpIn::Renderer {
            origin: [0.0, 0.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            view: [x0.min(x1), y0.min(y1), z, x0.max(x1), y0.max(y1), z],
            nx: rng.range_i64(1, 3) as usize,
            ny: rng.range_i64(1, 3) as usize,
            rate: rng.range_i64(1, 2) as usize,
        });
        for _ in 0..rng.range_i64(0, 2) {
            ops.push(OpIn::Light {
                v: unit_dir(rng),
                point: rng.chance(0.3),
            });
        }
        let shadows = rng.chance(0.5);
        ops.push(OpIn::DoRender {
            shadows,
            background: [0.1, 0.2, 0.3],
            object: [rng.unit(), rng.unit(), rng.unit()],
        });
        ops.push(OpIn::RenderPixel { x: 0, y: 0 });
        ops.push(OpIn::RenderRay {
            p: center,
            v: unit_dir(rng),
        });
    }
    if !safe && rng.chance(0.1) && !surfaces.is_empty() {
        ops.push(OpIn::AddSurface { surface: 0 });
    }
    Inputs {
        surfaces,
        grid,
        ops,
    }
}

/// Mostly cases the Rust side can finish (rays rejected before `LocalLineIntersect`),
/// and some that defer.
fn gen_checked(k: &Kernel, rng: &mut Rng) -> Inputs {
    let want_safe = rng.chance(0.75);
    let mut last = gen_case(rng, want_safe);
    for _ in 0..12 {
        if !want_safe {
            return last;
        }
        match run_rust(k, &last, &mut Stats::default()) {
            Err(SolidError::Unported { .. }) => last = gen_case(rng, want_safe),
            _ => return last,
        }
    }
    last
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.ray_tracer"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let rec = ffi::run(&ffi_program(&i)).map_err(|e| harness(e.to_string()))?;
        Ok(Output::new().values("rec", rec))
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        let i: Inputs = case.parse()?;
        match run_rust(k, &i, &mut Stats::default()) {
            Ok(rec) => Ok(Output::new().values("rec", rec)),
            Err(e) => Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("rust_error: {e}")))),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        serde_json::to_value(gen_checked(k, rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generator_covers_walks() {
        let mut st = Stats::default();
        let mut deferred = 0;
        let mut ok = 0;
        for k in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(k);
            let i = gen_checked(&Kernel::default(), &mut rng);
            match run_rust(&Kernel::default(), &i, &mut st) {
                Ok(_) => ok += 1,
                Err(e) => {
                    assert!(e.is_unported(), "{e}");
                    deferred += 1;
                }
            }
        }
        assert!(ok > 200, "{ok} {deferred} {st:?}");
        // (`LocalSolve` no longer defers since `LocalLineIntersect` is ported)
        assert!(ok + deferred == MIN_GENERATED, "{ok} {deferred} {st:?}");
        assert!(st.walks > 200, "{st:?}");
        assert!(st.elements_seen > 100, "{st:?}");
    }
}
