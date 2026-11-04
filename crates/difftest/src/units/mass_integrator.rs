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
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
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
/// The shim's face surface: kind 3 is a quadratic B-spline patch, kind 5 a multi-span
/// rational quadratic patch (u knot 0.4, double v knot 0.5; #1435), every other kind an
/// `SmPlane` over `domain` (kind 4 with rotated axes).
fn face_surface(
    kind: i32,
    p: &[f64; 16],
    domain: &solidcore::geom::extent2d::Extent2d,
) -> solidcore::Result<solidcore::surface::surface::Surface> {
    use solidcore::math::{Vec2, Vec3};
    use solidcore::nurbs::math::CPoint;
    Ok(if kind == 3 {
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
    } else if kind == 5 {
        let cp: Vec<Vec<_>> = (0..4)
            .map(|i: i32| {
                (0..5)
                    .map(|j: i32| {
                        CPoint::new(
                            p[0] + f64::from(i) * p[3] / 3.0,
                            p[1] + f64::from(j) * p[4] / 4.0,
                            p[2] + f64::from(i * (3 - i) * j * (4 - j)) * 0.125,
                            if j == 2 { 0.8 } else { 1.0 },
                        )
                    })
                    .collect()
            })
            .collect();
        solidcore::surface::surface::Surface::from(
            solidcore::nurbs::b_spline_surface::BSplineSurface::from_data(
                3,
                4,
                2,
                2,
                6,
                7,
                &cp,
                &[0., 0., 0., 0.4, 1., 1., 1.],
                &[0., 0., 0., 0.5, 0.5, 1., 1., 1.],
            )?,
        )
    } else {
        solidcore::surface::surface::Surface::from(solidcore::surface::plane::Plane::new(
            Vec3::new(p[0], p[1], p[2]),
            if kind == 4 {
                Vec3::new(0.6, 0.8, 0.0)
            } else {
                Vec3::X
            },
            if kind == 4 {
                Vec3::new(-0.8, 0.6, 0.0)
            } else {
                Vec3::Y
            },
            Vec2::new(p[3], p[4]),
            domain,
        ))
    })
}
fn geometry_rust(g: &Geometry) -> Output {
    use solidcore::curve::curve::Curve;
    use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
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
                    // As the shim: a wire edge over the curve interval in a fresh B-rep
                    // (`CreateWireEdgeFromCurve`), then the B-rep's precise properties.
                    let mut wire_kernel = Kernel::default();
                    let mut brep = Brep::new_with_infinite_region()?;
                    solidcore::topology::brep::p5::create_wire_edge_from_curve(
                        &mut wire_kernel,
                        &mut brep,
                        c,
                        ivl,
                    )?;
                    let p = props::brep_properties(
                        &wire_kernel,
                        &mut brep,
                        accuracy,
                        Vec3::new(ox, oy, oz),
                        props::MassPropertyBehavior {
                            edge_radius: radius,
                            ..Default::default()
                        },
                        &attrs,
                    )?;
                    let mut out = vec![0.0, p.area, p.volume, p.mass];
                    for m in p.moments {
                        out.extend([m.x, m.y, m.z]);
                    }
                    return Ok(out);
                }
                let p = props::curve_properties(
                    &kernel,
                    &c,
                    &ivl,
                    accuracy,
                    Vec3::new(ox, oy, oz),
                    radius,
                )?;
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
                {
                    // The aggregate queries (face area / centroid, B-rep area / volume /
                    // mass properties): as the shim, `new SmBrep()`, then the public API
                    // query on that face or B-rep. The shim makes the face with
                    // SmBrep::CreateFaceFromSurface over the requested UV domain, except
                    // for the disk / annulus kinds, which it makes with
                    // SmBrep::CreateFacesWithCurves from their bounding circles.
                    let store = solidcore::geom::attribute::AttributeStore::default();
                    let attrs = props::PropertyAttributes {
                        store: &store,
                        key_of: &|_| solidcore::geom::attribute::ObjectKey(0),
                    };
                    let mut fk = Kernel::default();
                    let mut brep = solidcore::topology::brep::p3::new_brep(
                        &mut fk,
                        solidcore::geom::tol::USE_DEFAULT,
                        solidcore::geom::tol::USE_DEFAULT,
                    )?;
                    let dom = Extent2d::from_points(Vec2::new(p[5], p[6]), Vec2::new(p[7], p[8]));
                    let surface = face_surface(*kind, p, &dom)?;
                    let face = {
                        use crate::units::topology_brep_p8::key_of;
                        use solidcore::EntityRef;
                        let mut records = solidcore::topology::topology::NotifyRecords::new();
                        let mut attributes = solidcore::geom::attribute::AttributeStore::default();
                        let mut env = solidcore::topology::edge::NotifyEnv {
                            records: &mut records,
                            attributes: &mut attributes,
                            context: None,
                            key_of: &key_of,
                        };
                        let mut marks = solidcore::topology::topology_traverser::Marks::default();
                        let this_face = |f| key_of(EntityRef::Face(f));
                        let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
                            env: &mut env,
                            marks: &mut marks,
                            this_face: &this_face,
                        };
                        if *kind == 1 || *kind == 2 {
                            // As the shim: concentric circles on the plane (radius 0.5 and
                            // 0.2 over the full 0..360 degree domain), the disk's one loop
                            // SAME and the annulus's outer loop SAME with its hole OPPOSITE.
                            let n_loops = if *kind == 2 { 2 } else { 1 };
                            let mut curves = Vec::with_capacity(n_loops);
                            let mut orientations = Vec::with_capacity(n_loops);
                            for i in 0..n_loops {
                                let circle = Ellipse::new_circle(
                                    &fk,
                                    Vec3::new(p[0], p[1], p[2]),
                                    Vec3::new(1.0, 0.0, 0.0),
                                    Vec3::new(0.0, 1.0, 0.0),
                                    Extent1d::from_min_max(0.0, 360.0),
                                    if i == 0 { 0.5 } else { 0.2 },
                                    &EllipseOptions::default(),
                                )?;
                                curves.push(Some(Curve::from(circle)));
                                orientations.push(if i == 0 {
                                    OrientType::Same
                                } else {
                                    OrientType::Opposite
                                });
                            }
                            let made = solidcore::topology::brep::p8::create_faces_with_curves(
                                &mut fk,
                                &mut brep,
                                None,
                                &vec![1; n_loops],
                                Some(curves),
                                &orientations,
                                Some(surface),
                                &mut fenv,
                            )?;
                            *made.faces.first().ok_or_else(|| {
                                SolidError::sm_err("CreateFacesWithCurves made no face")
                            })?
                        } else {
                            solidcore::topology::brep::p8::create_face_from_surface(
                                &mut fk,
                                &mut brep,
                                Some(surface),
                                &dom,
                                &mut fenv,
                            )?
                        }
                    };
                    let accuracy = p[12];
                    match *query {
                        0 => {
                            let bbox = solidcore::query::face::p1::calculate_bounding_box(
                                &fk, &brep, face, false,
                            )?;
                            let size = bbox.size();
                            let d = size.z.max(size.x.max(size.y));
                            let properties = props::face_properties(
                                &fk,
                                &mut brep,
                                face,
                                if p[14] == 0.0 {
                                    OrientType::Same
                                } else {
                                    OrientType::Opposite
                                },
                                accuracy,
                                Vec3::new(p[9], p[10], p[11]),
                                d * d / 100.0,
                                p[13],
                                p[15] as u64,
                            )?;
                            let mut out = vec![0.0, properties.area, properties.volume];
                            for m in properties.moments {
                                out.extend([m.x, m.y, m.z]);
                            }
                            Ok(out)
                        }
                        7 => {
                            let surface = brep
                                .face(face)?
                                .surface
                                .as_ref()
                                .ok_or_else(|| SolidError::sm_err("missing surface"))?;
                            let eval = solidcore::surface::surface::p1::AreaIntegrand {
                                surface,
                                uv_domain: dom,
                                iso_curve_accuracy: accuracy / 100.0,
                            };
                            let area = integrate(
                                dom.min().x,
                                dom.max().x,
                                IntegrationAlgorithm::Romberg,
                                accuracy,
                                |t| eval.evaluate(&fk, t),
                            )?;
                            Ok(if area == 0.0 {
                                vec![1001.0]
                            } else {
                                vec![0.0, area]
                            })
                        }
                        2 => Ok(vec![0.0, props::face_area(&fk, &mut brep, face, accuracy)?]),
                        3 => {
                            let (area, c) = props::face_centroid(&fk, &mut brep, face, accuracy)?;
                            Ok(vec![0.0, area, c.x, c.y, c.z])
                        }
                        4 => Ok(vec![0.0, props::brep_area(&fk, &mut brep, accuracy)?]),
                        5 => Ok(vec![
                            0.0,
                            props::brep_volume(&fk, &mut brep, accuracy, &attrs)?,
                        ]),
                        6 => {
                            let m = props::brep_mass_properties(
                                &fk,
                                &mut brep,
                                accuracy,
                                1.0,
                                Vec3::new(p[9], p[10], p[11]),
                                &attrs,
                            )?;
                            Ok(vec![0.0, m.properties.area])
                        }
                        _ => Err(SolidError::sm_err("unknown query")),
                    }
                }
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
    fn face_queries_run_without_native_reference() {
        for query in [0, 7] {
            let case = Case {
                unit: "mass.integrator".into(),
                name: format!("independent_face_query_{query}"),
                source: crate::case::CaseSource::Inline,
                inputs: serde_json::json!({"geometry": {
                    "op": "face", "kind": 0,
                    "parameters": [0.0, 0.0, 0.0, 2.0, 3.0, 0.0, 0.0, 1.0, 1.0,
                                   0.0, 0.0, 0.0, 1e-6, 0.0, 0.0, 63.0],
                    "query": query
                }}),
            };
            let result = Runner.run_rust(&case).unwrap();
            assert!(result.deferred.is_none(), "query {query}: {result:?}");
            assert_eq!(result.ints["status"], [0], "query {query}: {result:?}");
            assert!((result.values["out"][0] - 6.0).abs() < 1e-6);
        }
    }

    fn independent_face(kind: i32, parameters: [f64; 16], query: i32) -> Output {
        let case = Case {
            unit: "mass.integrator".into(),
            name: format!("independent_face_kind_{kind}_query_{query}"),
            source: crate::case::CaseSource::Inline,
            inputs: serde_json::json!({"geometry": {
                "op": "face", "kind": kind, "parameters": parameters, "query": query
            }}),
        };
        Runner.run_rust(&case).unwrap()
    }

    fn independent_success(out: &Output) -> &[f64] {
        assert!(out.deferred.is_none(), "{out:?}");
        assert!(out.rejected.is_none(), "{out:?}");
        assert_eq!(out.ints["status"], [0], "{out:?}");
        &out.values["out"]
    }

    #[test]
    fn face_queries_run_without_native_reference_analytic() {
        use core::f64::consts::PI;

        // The rectangular subdomain is not the circular face's trim domain.
        for kind in [0, 1, 2, 4] {
            let circular = kind == 1 || kind == 2;
            let mut p = if circular {
                [
                    2.5, -1.25, 3.75, 1.0, 1.0, -1.5, -1.5, 1.5, 1.5, 0.0, 0.0, 0.0, 1e-6, 0.0,
                    0.0, 1.0,
                ]
            } else {
                [
                    1.0, 2.0, 3.0, 2.0, 3.0, 0.1, 0.2, 0.8, 0.9, 2.37, 3.21, 3.41, 1e-6, 0.0, 0.0,
                    1.0,
                ]
            };
            let area = match kind {
                1 => PI * 0.25,
                2 => PI * (0.25 - 0.04),
                _ => 2.0 * 3.0 * 0.7 * 0.7,
            };
            let rectangle_area = if circular { 9.0 } else { area };
            let center = match kind {
                1 | 2 => [2.5, -1.25, 3.75],
                4 => [
                    1.0 + 0.6 * 0.9 - 0.8 * 1.65,
                    2.0 + 0.8 * 0.9 + 0.6 * 1.65,
                    3.0,
                ],
                _ => [1.9, 3.65, 3.0],
            };
            for orientation in [0.0, 1.0] {
                for mask in [1.0, 63.0] {
                    p[14] = orientation;
                    p[15] = mask;
                    let result = independent_face(kind, p, 0);
                    let out = independent_success(&result);
                    assert!((out[0] - area).abs() < 1e-6, "kind {kind}: {out:?}");
                    if mask == 63.0 {
                        for (i, coordinate) in center.iter().enumerate() {
                            let expected = area * (coordinate - p[9 + i]);
                            assert!((out[2 + i] - expected).abs() < 1e-6, "kind {kind}: {out:?}");
                        }
                    }
                }
                let result = independent_face(kind, p, 7);
                let out = independent_success(&result);
                assert!(
                    (out[0] - rectangle_area).abs() < 1e-6,
                    "kind {kind}: {out:?}"
                );
            }
            // Analytic centroid control through the same runner, independent of C++.
            let result = independent_face(kind, p, 3);
            let out = independent_success(&result);
            assert!((out[0] - area).abs() < 1e-6, "kind {kind}: {out:?}");
            for (actual, expected) in out[1..].iter().zip(center) {
                assert!((actual - expected).abs() < 1e-6, "kind {kind}: {out:?}");
            }
        }
    }

    #[test]
    fn face_queries_run_without_native_reference_curved_and_invalid() {
        // Kinds 3 and 5 have fixed positive-height control points. Their graphs
        // project onto a 2 x 3 rectangle, so their area exceeds 6. This is a
        // finite, non-planar output control, not an asserted analytic surface area.
        for kind in [3, 5] {
            let mut p = [
                1.0, 2.0, 3.0, 2.0, 3.0, 0.0, 0.0, 1.0, 1.0, 2.37, 3.21, 3.41, 1e-6, 0.0, 0.0, 1.0,
            ];
            for orientation in [0.0, 1.0] {
                for mask in [1.0, 63.0] {
                    p[14] = orientation;
                    p[15] = mask;
                    for query in [0, 7] {
                        let result = independent_face(kind, p, query);
                        let out = independent_success(&result);
                        assert!(
                            out.iter().all(|v| v.is_finite()),
                            "kind {kind} query {query}: {out:?}"
                        );
                        assert!(out[0] > 6.0, "kind {kind} query {query}: {out:?}");
                    }
                }
            }
        }

        // Existing face_invalid_mask fixture: invalid property selection, not a
        // malformed surface or an FFI argument rejection.
        let p = [
            1.0, 2.0, 3.0, 1.0, 1.0, 0.0, 0.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1e-4, 0.0, 0.0, 0.0,
        ];
        let result = independent_face(0, p, 0);
        assert!(result.deferred.is_none(), "{result:?}");
        assert!(result.rejected.is_none(), "{result:?}");
        assert_eq!(result.ints["status"], [1007]);
        assert!(result.values["out"].is_empty());
    }

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

    /// The disk / annulus aggregate fixtures build a real face on both sides:
    /// both engines agree on the values the report compares, the areas and
    /// centroids match their closed forms, a sheet's B-rep volume is zero, and the
    /// B-rep mass-properties query refuses a sheet with the same status.
    #[cfg(feature = "cpp")]
    #[test]
    fn circular_aggregate_queries_have_substantive_values() {
        use core::f64::consts::PI;

        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/mass.integrator/circular_aggregate_queries.json"
        );
        let text = std::fs::read_to_string(path).unwrap();
        let fixtures: Value = serde_json::from_str(&text).unwrap();
        let cases = fixtures["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 14);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let g: Geometry = serde_json::from_value(case["inputs"]["geometry"].clone()).unwrap();
            let Geometry::Face {
                kind,
                parameters,
                query,
            } = &g
            else {
                panic!("{name}: not a face case");
            };
            let cpp = geometry_cpp(&g).unwrap();
            let rust = geometry_rust(&g);
            assert!(rust.deferred.is_none(), "{name}: {:?}", rust.deferred);
            assert!(rust.rejected.is_none(), "{name}: {:?}", rust.rejected);
            let d = crate::compare::compare(&cpp, &rust, &[]);
            assert!(d.pass(), "{name}: {:?}", d.mismatches);
            assert!(
                d.values_compared + d.exact_compared > 0,
                "{name}: nothing compared"
            );
            let area = if *kind == 2 {
                PI * (0.25 - 0.04)
            } else {
                PI * 0.25
            };
            match *query {
                2 => {
                    assert_eq!(rust.ints["status"], [0], "{name}");
                    assert!(
                        (rust.values["out"][0] - area).abs() <= 1e-7 * area,
                        "{name}: area {} vs {area}",
                        rust.values["out"][0]
                    );
                }
                3 => {
                    assert_eq!(rust.ints["status"], [0], "{name}");
                    let out = &rust.values["out"];
                    assert!((out[0] - area).abs() <= 1e-7 * area, "{name}");
                    let center = [parameters[0], parameters[1], parameters[2]];
                    for (v, e) in out[1..].iter().zip(center) {
                        assert!((v - e).abs() <= 1e-7, "{name}: centroid {v} vs {e}");
                    }
                }
                4 => {
                    assert_eq!(rust.ints["status"], [0], "{name}");
                    assert!(
                        (rust.values["out"][0] - area).abs() <= 1e-7 * area,
                        "{name}: B-rep area {} vs {area}",
                        rust.values["out"][0]
                    );
                }
                5 => {
                    // one sheet face bounds no volume: both engines return zero
                    assert_eq!(rust.ints["status"], [0], "{name}");
                    assert_eq!(rust.values["out"], [0.0], "{name}");
                }
                6 => {
                    // SmApiBrepComputeMassProperties refuses a sheet: the status is
                    // the observable result, as in face_api_dependency_6.
                    assert_eq!(rust.ints["status"], [1007], "{name}");
                    assert!(rust.values["out"].is_empty(), "{name}");
                }
                _ => panic!("{name}: unexpected aggregate query"),
            }
        }
    }
}
