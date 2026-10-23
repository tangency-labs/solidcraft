//! Differential runner for primitive maps, pivot selection and path helpers.
//! All native helper calls execute the included, renamed upstream source.
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::error::EntityRef;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::primitive::primitive_creation::p1::{self as api, EntityMaps, PrimitiveCreation};
use solidcore::topology::{Brep, BrepId, RegionId};
use solidcore_ffi::units::primitive_primitive_creation_p1 as ffi;

/// Primitive creation differential runner.
pub struct Runner;
#[derive(Serialize, Deserialize)]
struct Inputs {
    op: i32,
    data: Vec<f64>,
}
fn value(data: &[f64], index: usize) -> f64 {
    data.get(index).copied().unwrap_or(f64::NAN)
}
fn curve(v: &[f64]) -> solidcore::Result<BSplineCurve> {
    let [kind, x, y, z, a, b, c] = v else {
        return Err(solidcore::SolidError::invalid_input(
            "difftest::primitive_p1",
            "curve layout",
        ));
    };
    let origin = Vec3::new(*x, *y, *z);
    if *kind == 0.0 {
        BSplineCurve::create_line_segment(3, origin, Vec3::new(*a, *b, *c))
    } else {
        BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::new(origin, Vec3::X, Vec3::Y),
            *a,
            *b,
            *c,
            NurbCircleParam::Quadratic,
        )
    }
}
fn sweep_curve(k: &Kernel, v: &[f64], analytic: bool) -> solidcore::Result<Curve> {
    if !analytic {
        return curve(v).map(Curve::from);
    }
    let origin = Vec3::new(value(v, 1), value(v, 2), value(v, 3));
    if value(v, 0) == 0.0 {
        Ok(Curve::from(solidcore::curve::line::Line::from_points(
            origin,
            Vec3::new(value(v, 4), value(v, 5), value(v, 6)),
            3,
        )))
    } else {
        solidcore::curve::ellipse::Ellipse::create_canonical_circle(
            k,
            &Axis2Placement::new(origin, Vec3::X, Vec3::Y),
            value(v, 4),
            Some(&solidcore::geom::extent1d::Extent1d::from_min_max(
                value(v, 5),
                value(v, 6),
            )),
            None,
        )
        .map(Curve::from)
    }
}
fn samples(k: &Kernel, curve: &Curve, record: &mut Vec<f64>) -> solidcore::Result<()> {
    let interval = curve.natural_interval();
    record.extend([interval.min(), interval.max()]);
    for i in 0..=4 {
        let values = curve.evaluate(k, interval.evaluate(f64::from(i) / 4.0), 1, true, true)?;
        for value in values {
            record.extend(value.to_array());
        }
    }
    Ok(())
}
fn maps_record(maps: &EntityMaps, out: &mut Vec<f64>) {
    for map in [
        &maps.to_same,
        &maps.to_higher,
        &maps.from_same,
        &maps.from_lower,
    ] {
        out.push(map.len() as f64);
        for i in 0..4 {
            let key = EntityRef::Brep(BrepId::from_index(i));
            out.push(map.get(&key).map_or(-1.0, |v| match v {
                EntityRef::Brep(v) => f64::from(v.index()),
                _ => -2.0,
            }));
        }
    }
}
fn boolean_face(brep: &mut Brep, k: &Kernel, data: &[f64]) -> solidcore::Result<()> {
    use solidcore::geom::extent2d::Extent2d;
    use solidcore::surface::{plane::p1::Plane, sphere::Sphere, surface::Surface};
    use solidcore::topology::{Face, ShellId};
    let angle = value(data, 1).to_radians();
    let frame = Axis2Placement::new(
        Vec3::new(0.0, 0.0, value(data, 2)),
        Vec3::X,
        Vec3::new(0.0, angle.cos(), angle.sin()),
    );
    let domain = Extent2d::from_bounds(
        value(data, 3),
        value(data, 4),
        value(data, 5),
        value(data, 6),
    );
    let surface = if value(data, 0) == 0.0 {
        let mut plane = Plane::create_canonical(&frame)?;
        plane.adjust_step_uv_domain(&domain)?;
        Surface::from(plane)
    } else {
        // IsPlanar uses the revolution base; Surface has no sphere variant yet.
        Surface::from(Sphere::create_canonical(k, &frame, 1.0)?.base().clone())
    };
    let mut face = Face::new(ShellId::from_index(0));
    face.surface = Some(surface);
    face.uv_domain = domain;
    brep.add_face_record(face)?;
    Ok(())
}

fn topology_key(e: EntityRef) -> solidcore::geom::object::ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    solidcore::geom::object::ObjectRef {
        type_code,
        key: solidcore::geom::attribute::ObjectKey((kind + 1) << 40 | u64::from(i)),
    }
}

fn rust(i: &Inputs) -> solidcore::Result<Vec<f64>> {
    let mut k = Kernel::default();
    let mut records = std::collections::BTreeMap::new();
    let mut attributes = solidcore::geom::attribute::AttributeStore::default();
    let mut notifications = solidcore::topology::edge::NotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        context: None,
        key_of: &topology_key,
    };
    let v = &i.data;
    let mut record = Vec::new();
    match i.op {
        0 => {
            let mut p = PrimitiveCreation::new(
                Some(RegionId::from_index(7)),
                value(v, 0) as i64,
                value(v, 1) != 0.0,
            );
            for (m, values) in [
                &mut p.maps.to_same,
                &mut p.maps.to_higher,
                &mut p.maps.from_same,
                &mut p.maps.from_lower,
            ]
            .into_iter()
            .zip(v.get(4..).unwrap_or_default().as_chunks::<4>().0.iter())
            {
                for (key, target) in values.iter().enumerate() {
                    if *target >= 0.0 {
                        m.insert(
                            EntityRef::Brep(BrepId::from_index(key as u32)),
                            EntityRef::Brep(BrepId::from_index(*target as u32)),
                        );
                    }
                }
            }
            let mut sweep = EntityMaps::default();
            sweep.to_same.insert(
                EntityRef::Brep(BrepId::from_index(0)),
                EntityRef::Brep(BrepId::from_index(3)),
            );
            sweep.to_higher.insert(
                EntityRef::Brep(BrepId::from_index(1)),
                EntityRef::Brep(BrepId::from_index(2)),
            );
            if value(v, 2) != 0.0 {
                p.init_entity_maps();
            }
            if value(v, 3) != 0.0 {
                p.copy_entity_maps(&mut sweep);
            }
            record.extend([
                p.tag_id() as f64,
                f64::from(p.region() == Some(RegionId::from_index(7))),
            ]);
            maps_record(&p.maps, &mut record);
            maps_record(&sweep, &mut record);
        }
        1 => {
            record.extend(
                api::find_pivot_point(
                    &curve(v.get(..7).unwrap_or_default())?,
                    &curve(v.get(7..14).unwrap_or_default())?,
                    value(v, 14),
                )?
                .to_array(),
            );
        }
        2 => {
            let paths = v
                .get(4..)
                .unwrap_or_default()
                .as_chunks::<7>()
                .0
                .iter()
                .map(|chunk| curve(chunk))
                .collect::<solidcore::Result<Vec<_>>>()?;
            let extended = api::extend_path_curves(
                &paths,
                value(v, 0) != 0.0,
                value(v, 1) != 0.0,
                value(v, 2),
            )?;
            record.push(extended.len() as f64);
            for item in extended {
                samples(&k, &Curve::from(item), &mut record)?;
            }
        }
        3 => {
            let path = Curve::from(curve(v.get(3..10).unwrap_or_default())?);
            let split = 10 + 7 * value(v, 0) as usize;
            let profiles = v
                .get(10..split)
                .unwrap_or_default()
                .as_chunks::<7>()
                .0
                .iter()
                .map(|v| curve(v).map(Curve::from))
                .collect::<solidcore::Result<Vec<_>>>()?;
            let mut output = v
                .get(split..)
                .unwrap_or_default()
                .as_chunks::<7>()
                .0
                .iter()
                .map(|v| curve(v).map(Curve::from))
                .collect::<solidcore::Result<Vec<_>>>()?;
            api::position_profile_curves(&k, &profiles, value(v, 2), &path, &mut output)?;
            record.push(output.len() as f64);
            for item in output {
                samples(&k, &item, &mut record)?;
            }
        }
        4 => {
            use solidcore::nurbs::srf_nurb::EllipticCone;
            let shape = value(v, 0) as i32;
            let frame = Axis2Placement::new(
                Vec3::new(value(v, 8), value(v, 9), value(v, 10)),
                Vec3::new(value(v, 11), value(v, 12), value(v, 13)),
                Vec3::new(value(v, 14), value(v, 15), value(v, 16)),
            );
            if shape <= 1 {
                // The static constructors return their own B-rep (NULL on failure).
                let made = if shape == 0 {
                    api::create_rectangle(&mut k, value(v, 7), value(v, 1), value(v, 2), &frame)
                } else {
                    api::create_circle(&mut k, value(v, 7), value(v, 1), &frame)
                };
                match made {
                    Err(e) if e.is_unported() => return Err(e),
                    Ok(b) => {
                        record.extend([
                            0.0,
                            f64::from(b.editing_enabled()),
                            0.0,
                            solidcore::topology::brep::p1::faces(&b, None)?.len() as f64,
                            solidcore::topology::brep::p1::edges(&b, None)?.len() as f64,
                            solidcore::topology::brep::p1::vertices(&b, None)?.len() as f64,
                        ]);
                        crate::util::primitive::append(&k, &b, &mut record)?;
                    }
                    Err(_) => record.extend([
                        f64::from(solidcore::error::SM_ERR),
                        -1.0,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                    ]),
                }
                return Ok(record);
            }
            // `SmBrep b(SM_USE_DEFAULT, SM_USE_DEFAULT, context)`
            let mut brep = solidcore::topology::brep::p3::new_brep(
                &mut k,
                solidcore::geom::tol::USE_DEFAULT,
                solidcore::geom::tol::USE_DEFAULT,
            )?;
            brep.set_editing_enabled(value(v, 17) != 0.0);
            let mut primitive = PrimitiveCreation::new(brep.infinite_region(), 17, true);
            if value(v, 18) != 0.0 {
                primitive.maps.to_same.insert(
                    EntityRef::Brep(BrepId::from_index(0)),
                    EntityRef::Brep(BrepId::from_index(0)),
                );
            }
            let result = match shape {
                2 => primitive.create_box(
                    &mut k,
                    &mut brep,
                    value(v, 1),
                    value(v, 2),
                    value(v, 3),
                    &frame,
                ),
                3 => primitive.create_cylindrical_box(
                    &mut k,
                    &mut brep,
                    [
                        value(v, 1),
                        value(v, 2),
                        value(v, 3),
                        value(v, 4),
                        value(v, 5),
                    ],
                    &frame,
                    &mut notifications,
                ),
                4 => primitive.create_cone(
                    &mut k,
                    &mut brep,
                    value(v, 1),
                    value(v, 2),
                    value(v, 3),
                    &frame,
                ),
                5 => primitive.create_sphere(&mut k, &mut brep, value(v, 1), &frame),
                6 => primitive.create_torus(&mut k, &mut brep, value(v, 1), value(v, 2), &frame),
                7 => PrimitiveCreation::create_sphere_no_pole(
                    &mut k,
                    &mut brep,
                    value(v, 1),
                    frame.origin(),
                    value(v, 7),
                    &mut notifications,
                ),
                8 => PrimitiveCreation::create_cone_elliptic_ends(
                    &mut k,
                    &mut brep,
                    &EllipticCone {
                        radius_front: value(v, 1),
                        radius_rear: value(v, 2),
                        height: value(v, 3),
                        tilt_y_front: value(v, 4),
                        tilt_y_rear: value(v, 5),
                        tilt_x_front: value(v, 6),
                        tilt_x_rear: v.get(19).copied().unwrap_or(0.0),
                    },
                    value(v, 7) != 0.0,
                ),
                9 => {
                    let curves = v
                        .get(20..)
                        .unwrap_or_default()
                        .as_chunks::<7>()
                        .0
                        .iter()
                        .map(|chunk| sweep_curve(&k, chunk, value(v, 6) != 0.0))
                        .collect::<solidcore::Result<Vec<_>>>()?;
                    primitive.create_linear_sweep(
                        &mut k,
                        &mut brep,
                        curves,
                        frame.z_axis(),
                        value(v, 1),
                        value(v, 7) != 0.0,
                    )
                }
                10 => {
                    let mut output = (value(v, 6) != 0.0).then(Brep::new);
                    let mut marks_a = solidcore::topology::topology_traverser::Marks::default();
                    let mut marks_b = solidcore::topology::topology_traverser::Marks::default();
                    let operation = match value(v, 1) as i32 {
                        0 => api::Boolean2d::Union,
                        1 => api::Boolean2d::Intersection,
                        2 => api::Boolean2d::Difference,
                        3 => api::Boolean2d::ExclusiveOr,
                        _ => api::Boolean2d::Merge,
                    };
                    let result = primitive.boolean_2d(
                        &mut k,
                        &mut brep,
                        &mut Brep::new(),
                        operation,
                        &mut output,
                        (&mut marks_a, &mut marks_b),
                        &mut notifications,
                    );
                    record.push(f64::from(output.is_some()));
                    result
                }
                _ => unreachable!(),
            };
            let status = match result {
                Ok(()) => 0,
                Err(e) if e.is_unported() => return Err(e),
                Err(solidcore::SolidError::InvalidInput { .. }) => 1007,
                Err(solidcore::SolidError::Upstream { code, .. }) => code,
                Err(_) => solidcore::error::SM_ERR,
            };
            // `GetFaces` / `GetEdges` / `GetVertices` as the shim reads them (a traversal
            // of the B-rep, not every record in the arenas)
            record.extend([
                f64::from(status),
                f64::from(brep.editing_enabled()),
                primitive.maps.to_same.len() as f64,
                solidcore::topology::brep::p1::faces(&brep, None)?.len() as f64,
                solidcore::topology::brep::p1::edges(&brep, None)?.len() as f64,
                solidcore::topology::brep::p1::vertices(&brep, None)?.len() as f64,
            ]);
            if status == 0 {
                crate::util::primitive::append(&k, &brep, &mut record)?;
            }
        }
        5 => {
            let mut brep = solidcore::topology::brep::p3::new_brep(
                &mut k,
                solidcore::geom::tol::USE_DEFAULT,
                solidcore::geom::tol::USE_DEFAULT,
            )?;
            let result = api::analytic_rotational_sweep(
                &mut k,
                &mut brep,
                &Curve::from(curve(v.get(..7).unwrap_or_default())?),
                Vec3::new(value(v, 7), value(v, 8), value(v, 9)),
                Vec3::new(value(v, 10), value(v, 11), value(v, 12)),
            );
            let status = match result {
                Ok(()) => 0,
                Err(e) if e.is_unported() => return Err(e),
                Err(solidcore::SolidError::InvalidInput { .. }) => 1007,
                Err(solidcore::SolidError::Upstream { code, .. }) => code,
                Err(_) => solidcore::error::SM_ERR,
            };
            record.extend([
                f64::from(status),
                brep.faces().len() as f64,
                f64::from(brep.editing_enabled()),
            ]);
        }

        6 => {
            let mut a = Brep::new();
            let mut b = Brep::new();
            let split = 7 + 7 * value(v, 4) as usize;
            for face in v.get(7..split).unwrap_or_default().as_chunks::<7>().0 {
                boolean_face(&mut a, &k, face)?;
            }
            for face in v.get(split..).unwrap_or_default().as_chunks::<7>().0 {
                boolean_face(&mut b, &k, face)?;
            }
            a.set_tolerance(value(v, 6));
            b.set_tolerance(value(v, 6));
            a.set_editing_enabled(value(v, 2) != 0.0);
            b.set_editing_enabled(value(v, 3) != 0.0);
            let mut output = (value(v, 1) != 0.0).then(Brep::new);
            let mut marks_a = solidcore::topology::topology_traverser::Marks::default();
            let mut marks_b = solidcore::topology::topology_traverser::Marks::default();
            let operation = match value(v, 0) as i32 {
                0 => api::Boolean2d::Union,
                1 => api::Boolean2d::Intersection,
                2 => api::Boolean2d::Difference,
                3 => api::Boolean2d::ExclusiveOr,
                _ => api::Boolean2d::Merge,
            };
            let result = PrimitiveCreation::new(None, 17, true).boolean_2d(
                &mut k,
                &mut a,
                &mut b,
                operation,
                &mut output,
                (&mut marks_a, &mut marks_b),
                &mut notifications,
            );
            let status = match result {
                Ok(()) => 0,
                Err(e) if e.is_unported() => return Err(e),
                Err(solidcore::SolidError::InvalidInput { .. }) => 1007,
                Err(solidcore::SolidError::Upstream { code, .. }) => code,
                Err(_) => solidcore::error::SM_ERR,
            };
            record.extend([
                f64::from(status),
                f64::from(a.editing_enabled()),
                f64::from(b.editing_enabled()),
                f64::from(output.is_some()),
            ]);
        }

        _ => unreachable!(),
    }
    Ok(record)
}
fn output(input: &Inputs, data: Vec<f64>) -> Result<Output, HarnessError> {
    let op = input.op;
    if op == 4 {
        let prefix = if value(&input.data, 0) == 10.0 { 7 } else { 6 };
        let head = data
            .get(..prefix)
            .ok_or_else(|| HarnessError::new("missing primitive topology record"))?;
        Ok(Output::new()
            .ints("maps", head.iter().map(|v| *v as i64).collect::<Vec<_>>())
            .values("geometry", data.get(prefix..).unwrap_or_default().to_vec()))
    } else if op == 0 || op == 5 || op == 6 {
        Ok(Output::new().ints(
            "maps",
            data.into_iter().map(|v| v as i64).collect::<Vec<_>>(),
        ))
    } else if op == 2 || op == 3 {
        let Some((count, values)) = data.split_first() else {
            return Err(HarnessError::new("missing curve-count record"));
        };
        Ok(Output::new()
            .int("count", *count as i64)
            .values("curves", values.to_vec()))
    } else {
        Ok(Output::new().values("pivot", data))
    }
}
impl UnitDiff for Runner {
    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<Case>, HarnessError> {
        crate::runner::default_cases(
            self,
            &crate::case::CaseContext {
                seeds: ctx.seeds.max(600),
                ..ctx.clone()
            },
        )
    }
    fn id(&self) -> &'static str {
        "primitive.primitive_creation_p1"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::eval(i.op, &i.data) {
            Ok(data) => output(&i, data),
            Err(solidcore_ffi::FfiError::Status(-1010)) => Ok(Output::rejected(
                "native fixture preflight rejected undefined construction",
            )),
            Err(e) => Err(HarnessError::new(e.to_string())),
        }
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        ffi::validate(i.op, &i.data).map_err(|e| HarnessError::new(e.to_string()))?;
        match rust(&i) {
            Ok(data) => output(&i, data),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = rng.below(4) as i32;
        let data = match op {
            0 => {
                let mut d = vec![
                    rng.below(1000) as f64,
                    f64::from(rng.below(2) != 0),
                    f64::from(rng.below(2) != 0),
                    f64::from(rng.below(2) != 0),
                ];
                d.extend((0..16).map(|_| rng.below(5) as f64 - 1.0));
                d
            }
            1 => {
                let p = rng.point3(-10.0, 10.0);
                let q = rng.point3(-10.0, 10.0);
                let r = rng.point3(-10.0, 10.0);
                let mut d = vec![0.0];
                d.extend(p);
                d.extend(q);
                d.push(0.0);
                let mode = rng.below(3);
                d.extend(if mode == 0 {
                    p
                } else if mode == 1 {
                    q
                } else {
                    r
                });
                d.extend(rng.point3(-10.0, 10.0));
                d.push(rng.uniform(0.0, 0.1));
                d
            }
            2 => {
                let count = 1 + rng.below(5);
                let mut d = vec![
                    f64::from(rng.below(2) != 0),
                    f64::from(rng.below(2) != 0),
                    rng.uniform(0.0, 0.5),
                    count as f64,
                ];
                for _ in 0..count {
                    d.push(0.0);
                    d.extend(rng.point3(-10.0, 10.0));
                    d.extend(rng.point3(-10.0, 10.0));
                }
                d
            }
            _ => {
                // One arc profile exercises ComputeCrvsNormal and Copy; the path
                // has independently selected position and a skew or vertical tangent.
                let mut d = vec![1.0, 0.0, rng.uniform(-1.0, 1.0), 0.0];
                d.extend(rng.point3(-5.0, 5.0));
                d.extend(rng.point3(6.0, 10.0));
                d.push(1.0);
                d.extend(rng.point3(-10.0, 10.0));
                d.extend([rng.uniform(0.1, 4.0), 0.0, 360.0]);
                d
            }
        };
        serde_json::to_value(Inputs { op, data }).ok()
    }
}
