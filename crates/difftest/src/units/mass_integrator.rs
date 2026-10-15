//! Differential coverage of the real upstream scalar and Cash–Karp integrators.
//! Compares values, adaptive step sizes, mutated scales and accepted/reduced counts.
//! Includes coupled-to-state equations to detect stage/array adaptation errors.
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::mass::integrator::{integrate, IntegrationAlgorithm, OdeIntegrator, OdeOptions};
use solidcore::SolidError;
use solidcore_ffi::units::mass_integrator as ffi;

/// Runner for `mass.integrator`.
pub struct Runner;
#[derive(Clone, Serialize, Deserialize)]
struct Input {
    op: i32,
    kind: i32,
    coefficients: Vec<[f64; 4]>,
    initial: Vec<f64>,
    #[serde(default)]
    scale: Vec<f64>,
    controls: [f64; 7],
    #[serde(default = "default_steps")]
    max_steps: u32,
}
fn default_steps() -> u32 {
    10_000
}
fn eval(i: &Input, t: f64, y: &[f64], dy: &mut [f64]) -> solidcore::Result<()> {
    for ((p, v), d) in i.coefficients.iter().zip(y).zip(dy) {
        if i.kind == 3 && t >= p[0] {
            return Err(SolidError::sm_err("test evaluator"));
        }
        *d = match i.kind {
            1 => p[0] * (p[1] * t + p[2]).sin() + p[3] * v,
            2 => p[0] * (p[1] * t).exp() + p[2] * v + p[3],
            _ => p[0] + p[1] * t + p[2] * t * t + p[3] * v,
        };
    }
    Ok(())
}
fn output(v: Vec<f64>) -> Output {
    let mut it = v.into_iter();
    let status = it.next().unwrap_or(-1.0) as i64;
    Output::new()
        .int("status", status)
        .values("out", it.collect::<Vec<_>>())
}
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1001.0,
    }
}
impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "mass.integrator"
    }
    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(1200),
                ..ctx.clone()
            },
        )
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if let Some(g) = case.inputs.get("geometry") {
            let g =
                serde_json::from_value(g.clone()).map_err(|e| HarnessError::new(e.to_string()))?;
            return geometry_cpp(&g);
        }
        let i: Input = case.parse()?;
        let flat: Vec<f64> = i.coefficients.iter().flatten().copied().collect();
        ffi::eval(
            i.op,
            i.kind,
            &flat,
            &i.initial,
            &i.scale,
            &i.controls,
            i.max_steps,
        )
        .map(output)
        .map_err(|e| HarnessError::new(e.to_string()))
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if let Some(g) = case.inputs.get("geometry") {
            let g =
                serde_json::from_value(g.clone()).map_err(|e| HarnessError::new(e.to_string()))?;
            return Ok(geometry_rust(&g));
        }
        let i: Input = case.parse()?;
        let [a, b, eps, h, hmin, tiny, algorithm] = i.controls;
        let integrator =
            OdeIntegrator::new(i.max_steps, tiny).map_err(|e| HarnessError::new(e.to_string()))?;
        let mut y = i.initial.clone();
        let mut scale = i.scale.clone();
        let result = match i.op {
            0 => integrate(
                a,
                b,
                if algorithm == 0.0 {
                    IntegrationAlgorithm::Romberg
                } else {
                    IntegrationAlgorithm::Simpsons
                },
                eps,
                |t| {
                    let mut dy = [0.0];
                    eval(&i, t, &[0.0], &mut dy)?;
                    Ok(dy[0])
                },
            )
            .map(|v| vec![0.0, v]),
            1 => integrator
                .integrate(
                    &mut y,
                    a,
                    b,
                    OdeOptions {
                        accuracy: eps,
                        initial_step: h,
                        minimum_step: hmin,
                        scale: if scale.is_empty() { None } else { Some(&scale) },
                    },
                    |t, y, dy| eval(&i, t, y, dy),
                )
                .map(|c| {
                    let mut out = vec![0.0];
                    out.extend(y);
                    out.extend([f64::from(c.good), f64::from(c.reduced)]);
                    out
                }),
            2 => {
                let mut dy = vec![0.0; y.len()];
                eval(&i, a, &y, &mut dy)
                    .and_then(|()| {
                        integrator.step(&y, &dy, a, h, eps, &mut scale, |t, y, dy| {
                            eval(&i, t, y, dy)
                        })
                    })
                    .map(|s| {
                        let mut out = vec![0.0];
                        out.extend(s.values);
                        out.extend([s.x, s.taken, s.next]);
                        out.extend(scale);
                        out
                    })
            }
            _ => return Err(HarnessError::new("unknown integrator op")),
        };
        match result {
            Ok(v) => Ok(output(v)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| output(vec![status(&e)]))),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        if rng.below(3) == 0 {
            let g = if rng.below(2) == 0 {
                let size = rng.choose(&[1e-5, 1.0, 1e5]).copied().unwrap_or(1.0);
                let p = core::array::from_fn(|_| rng.uniform(-2.0, 2.0) * size);
                Geometry::Edge {
                    kind: rng.below(2) as i32,
                    p,
                    weights: [1.0; 3],
                    controls: [
                        rng.uniform(0.0, 0.3),
                        rng.uniform(0.7, 1.0),
                        1e-6,
                        0.3 * size,
                        -0.1 * size,
                        0.2 * size,
                        0.05 * size,
                    ],
                    query: i32::from(rng.below(3) == 0),
                }
            } else {
                Geometry::Face {
                    kind: 0,
                    parameters: [
                        rng.uniform(-3.0, 3.0),
                        rng.uniform(-3.0, 3.0),
                        rng.uniform(-3.0, 3.0),
                        rng.uniform(0.2, 3.0),
                        rng.uniform(0.2, 3.0),
                        -0.5,
                        -0.3,
                        1.5,
                        2.0,
                        0.1,
                        0.2,
                        0.3,
                        1e-5,
                        0.0,
                        rng.below(2) as f64,
                        rng.choose(&[1.0, 3.0, 7.0, 8.0, 24.0, 63.0])
                            .copied()
                            .unwrap_or(63.0),
                    ],
                    query: 0,
                }
            };
            return Some(serde_json::json!({"geometry":g}));
        }
        let op = rng.below(3) as i32;
        let kind = rng.below(3) as i32;
        let n = if op == 0 {
            1
        } else {
            1 + rng.below(5) as usize
        };
        let factor = match rng.below(5) {
            0 => 1e-15,
            1 => 1e15,
            _ => 1.0,
        };
        let coefficients = (0..n)
            .map(|_| {
                [
                    rng.uniform(-2.0, 2.0) * factor,
                    rng.uniform(-2.0, 2.0),
                    rng.uniform(-2.0, 2.0),
                    if op == 0 { 0.0 } else { rng.uniform(-1.0, 1.0) },
                ]
            })
            .collect();
        let a = rng.uniform(-2.0, 2.0);
        let b = a + rng.uniform(-2.0, 2.0);
        let h = (b - a) / rng.choose(&[2.0, 8.0, 64.0]).copied().unwrap_or(2.0);
        let scale = if op == 2 || rng.below(2) == 0 {
            (0..n).map(|_| rng.uniform(0.1, 5.0) * factor).collect()
        } else {
            vec![]
        };
        let i = Input {
            op,
            kind,
            coefficients,
            initial: (0..n).map(|_| rng.uniform(-5.0, 5.0) * factor).collect(),
            scale,
            controls: [
                a,
                b,
                rng.choose(&[1e-3, 1e-6, 1e-9]).copied().unwrap_or(1e-6),
                h,
                0.0,
                1e-24,
                rng.below(2) as f64,
            ],
            max_steps: 10_000,
        };
        serde_json::to_value(i).ok()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Geometry {
    Edge {
        kind: i32,
        p: [f64; 9],
        weights: [f64; 3],
        controls: [f64; 7],
        query: i32,
    },
    Face {
        #[serde(default)]
        kind: i32,
        parameters: [f64; 16],
        query: i32,
    },
}
fn geometry_cpp(g: &Geometry) -> Result<Output, HarnessError> {
    let result = match g {
        Geometry::Edge {
            kind,
            p,
            weights,
            controls,
            query,
        } => ffi::edge(*query, *kind, p, weights, controls),
        Geometry::Face {
            kind,
            parameters,
            query,
        } => ffi::face(*query, *kind, parameters),
    };
    result
        .map(output)
        .map_err(|e| HarnessError::new(e.to_string()))
}
fn geometry_rust(g: &Geometry) -> Output {
    use solidcore::curve::curve::Curve;
    use solidcore::geom::extent1d::Extent1d;
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::kernel::Kernel;
    use solidcore::mass::integrator::properties as props;
    use solidcore::math::{Vec2, Vec3};
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::math::CPoint;
    use solidcore::topology::Brep;
    let kernel = Kernel::default();
    let result: solidcore::Result<Vec<f64>> = (|| {
        match g {
            Geometry::Edge {
                kind,
                p,
                weights,
                controls,
                query,
            } => {
                let c = if *kind == 1 {
                    Curve::from(solidcore::curve::line::Line::bounded(
                        Vec3::new(p[0], p[1], p[2]),
                        Vec3::new(p[3], p[4], p[5]),
                        &Extent1d::from_min_max(0.0, 1.0),
                        &solidcore::curve::line::BoundedLineOptions::default(),
                    ))
                } else {
                    let cp = [
                        CPoint::new(p[0], p[1], p[2], weights[0]),
                        CPoint::new(p[3], p[4], p[5], weights[1]),
                        CPoint::new(p[6], p[7], p[8], weights[2]),
                    ];
                    Curve::from(BSplineCurve::from_data(
                        3,
                        2,
                        5,
                        2,
                        &cp,
                        &[0., 0., 0., 1., 1., 1.],
                    )?)
                };
                let [a, b, accuracy, ox, oy, oz, radius] = *controls;
                let ivl = Extent1d::from_min_max(a, b);
                if *query == 1 {
                    return Ok(vec![0.0, c.length(&kernel, &ivl, accuracy)?]);
                }
                if *query == 2 {
                    let store = solidcore::geom::attribute::AttributeStore::default();
                    let attrs = props::PropertyAttributes {
                        store: &store,
                        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
                    };
                    props::brep_properties(
                        &kernel,
                        &Brep::new(),
                        accuracy,
                        Vec3::new(ox, oy, oz),
                        props::MassPropertyBehavior {
                            edge_radius: radius,
                            ..Default::default()
                        },
                        &attrs,
                    )?;
                    return Err(SolidError::sm_err("unexpected brep dependency"));
                }
                let p = props::curve_properties(&c, &ivl, accuracy, Vec3::new(ox, oy, oz), radius)?;
                let mut out = vec![0.0, p.length, p.properties.area, p.properties.volume];
                for m in p.properties.moments {
                    out.extend([m.x, m.y, m.z]);
                }
                Ok(out)
            }
            Geometry::Face {
                kind,
                parameters: p,
                query,
            } => {
                use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
                if *query != 0 && *query != 7 {
                    let store = solidcore::geom::attribute::AttributeStore::default();
                    let attrs = props::PropertyAttributes {
                        store: &store,
                        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
                    };
                    let brep = Brep::new();
                    let f = solidcore::topology::FaceId::from_index(0);
                    match *query {
                        2 => {
                            props::face_area(&kernel, &brep, f, p[12])?;
                        }
                        3 => {
                            props::face_centroid(&kernel, &brep, f, p[12])?;
                        }
                        4 => {
                            props::brep_area(&kernel, &brep, p[12])?;
                        }
                        5 => {
                            props::brep_volume(&kernel, &brep, p[12], &attrs)?;
                        }
                        6 => {
                            props::brep_mass_properties(
                                &kernel,
                                &brep,
                                p[12],
                                1.0,
                                Vec3::new(p[9], p[10], p[11]),
                                &attrs,
                            )?;
                        }
                        _ => return Err(SolidError::sm_err("unknown query")),
                    }
                    return Err(SolidError::sm_err("unexpected topology dependency"));
                }
                // Import only input geometry: C++ constructs and exports its actual
                // oriented trims. Both sides then integrate those same boundaries.
                let raw = ffi::face(1, *kind, p).map_err(|_| SolidError::sm_err("trim export"))?;
                let mut it = raw.into_iter();
                if it.next() != Some(0.0) {
                    return Err(SolidError::sm_err("trim construction"));
                }
                let estimated_area = it.next().ok_or_else(|| SolidError::sm_err("bbox export"))?;
                let u0 = it
                    .next()
                    .ok_or_else(|| SolidError::sm_err("domain export"))?;
                let v0 = it
                    .next()
                    .ok_or_else(|| SolidError::sm_err("domain export"))?;
                let u1 = it
                    .next()
                    .ok_or_else(|| SolidError::sm_err("domain export"))?;
                let v1 = it
                    .next()
                    .ok_or_else(|| SolidError::sm_err("domain export"))?;
                let n = it.next().unwrap_or(0.0) as usize;
                let mut curves = Vec::new();
                for _ in 0..n {
                    let n = it.next().ok_or_else(|| SolidError::sm_err("trim n"))? as usize;
                    let degree =
                        it.next().ok_or_else(|| SolidError::sm_err("trim degree"))? as usize;
                    let m = it.next().ok_or_else(|| SolidError::sm_err("trim m"))? as usize;
                    let cp: Vec<_> = (0..=n)
                        .map(|_| {
                            CPoint::new(
                                it.next().unwrap_or(0.0),
                                it.next().unwrap_or(0.0),
                                it.next().unwrap_or(0.0),
                                it.next().unwrap_or(1.0),
                            )
                        })
                        .collect();
                    let knots: Vec<_> = it.by_ref().take(m + 1).collect();
                    curves.push(Curve::from(BSplineCurve::from_data(
                        2, n, m, degree, &cp, &knots,
                    )?));
                }
                let domain = if *query == 7 {
                    // Surface::Area integrates the requested rectangle, independent
                    // of the face's trims or its resulting UV bounding domain.
                    Extent2d::from_points(Vec2::new(p[5], p[6]), Vec2::new(p[7], p[8]))
                } else {
                    Extent2d::from_points(Vec2::new(u0, v0), Vec2::new(u1, v1))
                };
                let surface = if *kind == 3 {
                    let cp: Vec<Vec<_>> = (0..3)
                        .map(|i| {
                            (0..3)
                                .map(|j| {
                                    CPoint::new(
                                        p[0] + f64::from(i) * 0.5 * p[3],
                                        p[1] + f64::from(j) * 0.5 * p[4],
                                        p[2] + if i == 1 && j == 1 { 2.0 } else { 0.0 },
                                        1.0,
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    solidcore::surface::surface::Surface::from(
                        solidcore::nurbs::b_spline_surface::BSplineSurface::from_data(
                            2,
                            2,
                            2,
                            2,
                            5,
                            5,
                            &cp,
                            &[0., 0., 0., 1., 1., 1.],
                            &[0., 0., 0., 1., 1., 1.],
                        )?,
                    )
                } else {
                    solidcore::surface::surface::Surface::from(
                        solidcore::surface::plane::Plane::new(
                            Vec3::new(p[0], p[1], p[2]),
                            if *kind == 4 {
                                Vec3::new(0.6, 0.8, 0.0)
                            } else {
                                Vec3::X
                            },
                            if *kind == 4 {
                                Vec3::new(-0.8, 0.6, 0.0)
                            } else {
                                Vec3::Y
                            },
                            Vec2::new(p[3], p[4]),
                            &domain,
                        ),
                    )
                };
                if *query == 7 {
                    let eval = solidcore::surface::surface::p1::AreaIntegrand {
                        surface: &surface,
                        uv_domain: domain,
                        iso_curve_accuracy: p[12] / 100.0,
                    };
                    let area = integrate(
                        domain.min().x,
                        domain.max().x,
                        IntegrationAlgorithm::Romberg,
                        p[12],
                        |t| eval.evaluate(t),
                    )?;
                    return Ok(if area == 0.0 {
                        vec![1001.0]
                    } else {
                        vec![0.0, area]
                    });
                }
                let properties = props::boundary_properties(
                    &surface,
                    &curves,
                    &domain,
                    if p[14] == 0.0 {
                        OrientType::Same
                    } else {
                        OrientType::Opposite
                    },
                    p[12],
                    Vec3::new(p[9], p[10], p[11]),
                    estimated_area,
                    p[13],
                    p[15] as u64,
                )?;
                let mut out = vec![0.0, properties.area, properties.volume];
                for m in properties.moments {
                    out.extend([m.x, m.y, m.z]);
                }
                Ok(out)
            }
        }
    })();
    match result {
        Ok(v) => output(v),
        Err(e) => Output::unported(&e).unwrap_or_else(|| output(vec![status(&e)])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonfinite_values_unused_or_supported_by_upstream() {
        for (accuracy, floor) in [(1e-6, f64::INFINITY), (f64::INFINITY, 1.0)] {
            let controls = [0.0, 1.0, accuracy, 0.1, 0.0, 1e-24, 0.0];
            let cpp = ffi::eval(
                1,
                0,
                &[1.0, 0.0, 0.0, 0.0],
                &[f64::NAN],
                &[floor],
                &controls,
                10000,
            )
            .unwrap();
            let mut y = [f64::NAN];
            let count = OdeIntegrator::default()
                .integrate(
                    &mut y,
                    0.0,
                    1.0,
                    OdeOptions {
                        accuracy,
                        initial_step: 0.1,
                        minimum_step: 0.0,
                        scale: Some(&[floor]),
                    },
                    |_, _, dy| {
                        dy[0] = 1.0;
                        Ok(())
                    },
                )
                .unwrap();
            assert_eq!(
                cpp,
                vec![0.0, y[0], f64::from(count.good), f64::from(count.reduced)]
            );
        }
        let controls = [2.0, 2.0, f64::NAN, f64::NAN, f64::NAN, 1e-24, 0.0];
        let cpp = ffi::eval(
            1,
            0,
            &[f64::NAN; 4],
            &[f64::INFINITY],
            &[f64::NAN],
            &controls,
            10000,
        )
        .unwrap();
        assert_eq!(cpp, vec![0.0, f64::INFINITY, 0.0, 0.0]);
    }
}
