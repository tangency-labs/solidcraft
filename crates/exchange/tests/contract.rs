//! The snapshot contract: shapes, materials, parameters, determinism, the tagged 3MF, JSON and
//! hostile input.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use common::*;
use solidcraft_doc::{FeatureKind, Model, Operation, UserMaterial, point_expr};
use solidcraft_exchange::{
    BodyName, Elasticity, Error, FaceName, MaterialAxes, Mm, Options, Override, ParameterValue, Point, Property, Quantity, Snapshot, Surface,
    Tolerance, from_json, open, to_json,
};
use solidcraft_geom::Vec3;

fn face_names(s: &Snapshot) -> BTreeSet<FaceName> {
    s.bodies.iter().flat_map(|b| b.faces.iter().map(|f| f.name.clone())).collect()
}

#[test]
fn a_box_is_six_named_outward_planes_closed_with_its_volume() {
    let s = snap(&box_doc("20 mm"));
    assert_eq!((s.schema, s.design.as_str()), (1, "block"));
    let [b] = s.bodies.as_slice() else { panic!("one body: {:?}", s.bodies.len()) };
    assert_eq!(b.name, *"Body1");
    assert!(b.closed && watertight(b));
    assert_eq!(b.faces.len(), 6);
    let centre = [10.0, 5.0, 2.5];
    for (i, f) in b.faces.iter().enumerate() {
        assert_eq!(f.index.index(), i);
        assert!(!f.triangles.is_empty(), "face {} has triangles", f.name);
        let Surface::Plane { point: Point(point), normal } = f.surface else { panic!("face {} is {:?}", f.name, f.surface) };
        let normal = normal.get();
        // An axis direction, pointing away from the centre, and every vertex of the face on it.
        assert!(normal.iter().filter(|x| x.abs() > 1.0 - 1e-12).count() == 1, "{normal:?}");
        let on: Vec<[f64; 3]> = f.triangles.iter().flatten().map(|v| at(b, *v)).collect();
        assert!(dot(sub(on[0], centre), normal) > 0.0, "{} points outward", f.name);
        for p in on {
            assert!(dot(sub(p, point), normal).abs() < 1e-9);
        }
    }
    let names = face_names(&s);
    assert_eq!(names.len(), 6, "unique names: {names:?}");
    assert!((volume(b) - 1000.0).abs() < 1e-9);
    assert_eq!(bbox(b), ([0.0, 0.0, 0.0], [20.0, 10.0, 5.0]));
}

#[test]
fn a_hole_is_a_cylinder_face_with_its_axis_and_radius() {
    let doc = holed_doc();
    // A design container (`.sc`) opens as the JSON does.
    let container = solidcraft_io::write_design_container(&doc).unwrap();
    let s = open(&container).unwrap().snapshot(&Options::default()).unwrap();
    assert_eq!(to_json(&s).unwrap(), to_json(&snap(&doc)).unwrap());
    let [b] = s.bodies.as_slice() else { panic!("one body") };
    assert!(b.closed && watertight(b));
    let mut cylinders = 0;
    for f in &b.faces {
        let Surface::Cylinder { origin: Point(origin), axis, radius: Mm(radius) } = f.surface else { continue };
        cylinders += 1;
        assert!((radius - 3.0).abs() < 1e-9 && (axis.get()[2].abs() - 1.0).abs() < 1e-12);
        assert!((origin[0] - 10.0).abs() < 1e-9 && (origin[1] - 10.0).abs() < 1e-9, "{origin:?}");
        for v in f.triangles.iter().flatten() {
            let p = at(b, *v);
            assert!(((p[0] - 10.0).hypot(p[1] - 10.0) - 3.0).abs() < 1e-6);
        }
    }
    assert!(cylinders > 0);
    assert_eq!(b.faces.iter().filter(|f| matches!(f.surface, Surface::Plane { .. })).count(), 6);
    // The chord error removes at most (hole area × tolerance) from the hole.
    let exact = 20.0 * 20.0 * 10.0 - PI * 9.0 * 10.0;
    let tol = 2e-4 * (20f64 * 20.0 + 20.0 * 20.0 + 10.0 * 10.0).sqrt();
    let v = volume(b);
    assert!(v >= exact - 1e-9 && v - exact <= 2.0 * PI * 3.0 * 10.0 * tol, "{v} vs {exact}");
}

#[test]
fn a_sphere_is_a_sphere_face_with_its_centre_and_radius() {
    let s = snap(&sphere_doc());
    let [b] = s.bodies.as_slice() else { panic!("one body") };
    assert!(b.closed && watertight(b));
    for f in &b.faces {
        let Surface::Sphere { center: Point(center), radius: Mm(radius) } = f.surface else { panic!("{:?}", f.surface) };
        assert!((radius - 8.0).abs() < 1e-9 && dist(center, [1.0, 2.0, 3.0]) < 1e-9);
    }
    for Point(p) in &b.positions {
        assert!((dist(*p, [1.0, 2.0, 3.0]) - 8.0).abs() < 1e-6);
    }
    let exact = 4.0 / 3.0 * PI * 512.0;
    let tol = 2e-4 * 16.0 * 3f64.sqrt();
    let v = volume(b);
    assert!(v <= exact && exact - v <= 4.0 * PI * 64.0 * tol, "{v} vs {exact}");
    // A finer tolerance comes closer.
    let fine = open(&bytes(&sphere_doc())).unwrap().snapshot(&Options::default().with_tolerance(Tolerance::Absolute(0.001))).unwrap();
    assert!(exact - volume(&fine.bodies[0]) < exact - v);
}

#[test]
fn materials_carry_density_and_card_properties() {
    let mut doc = box_doc("20 mm");
    for x in [50.0, 80.0] {
        doc.add_feature(FeatureKind::Sphere { center: point_expr(Vec3::new(x, 0.0, 0.0)), radius: "4".into(), operation: Operation::NewBody }, None)
            .unwrap();
    }
    let card = serde_json::json!({
        "schema_version": 1,
        "designation": "Test alloy",
        "family": "metal",
        "sources": [{"id": "s1", "title": "Made up for a test", "kind": "illustrative", "license": null, "reference": null}],
        "properties": [
            {"data": {"kind": "elasticity", "model": {"symmetry": "isotropic", "young_modulus_pa": 2.0e11, "poisson_ratio": 0.3}}, "source_id": "s1",
             "conditions": {"temperature_k": 293.15, "moisture_dry_basis_kg_kg": null, "strain_rate_per_s": null, "state": "annealed"}},
            {"data": {"kind": "thermal_conductivity", "w_m_k": [40.0, 41.0, 42.0], "axes": "material123"}, "source_id": "s1"}
        ]
    });
    doc.user_materials.push(UserMaterial {
        name: "Alloy X".into(),
        density: 7.5,
        appearance: None,
        physical_card: Some(solidcraft_doc::physical_material::ValidatedCard::from_value(&card).unwrap()),
    });
    doc.materials.insert("Body1".into(), "steel".into());
    doc.materials.insert("Body2".into(), "Alloy X".into());
    let s = snap(&doc);
    let by_name: BTreeMap<&str, Option<&str>> = s.bodies.iter().map(|b| (b.name.as_str(), b.material.as_ref().map(|m| m.as_str()))).collect();
    assert_eq!(by_name, BTreeMap::from([("Body1", Some("Steel")), ("Body2", Some("Alloy X")), ("Body3", None)]));
    let names: Vec<&str> = s.materials.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Alloy X", "Steel"]);
    assert!((s.materials[1].density_kg_m3.unwrap() - 7850.0).abs() < 1e-9 && s.materials[1].properties.is_empty());
    let alloy = &s.materials[0];
    assert!((alloy.density_kg_m3.unwrap() - 7500.0).abs() < 1e-9);
    let elastic = alloy.properties.iter().find(|p| matches!(p.property, Property::Elasticity { .. })).unwrap();
    assert_eq!(elastic.property, Property::Elasticity { elasticity: Elasticity::Isotropic { young_modulus_pa: 2.0e11, poisson_ratio: 0.3 } });
    assert_eq!(
        (elastic.conditions.temperature_k, elastic.conditions.state.as_deref(), elastic.source.as_str()),
        (Some(293.15), Some("annealed"), "s1")
    );
    let k = alloy.properties.iter().find(|p| matches!(p.property, Property::ThermalConductivity { .. })).unwrap();
    assert_eq!(k.property, Property::ThermalConductivity { w_m_k: [40.0, 41.0, 42.0], axes: MaterialAxes::Material123 });
    // A body filter keeps only the bodies, and the materials, asked for.
    let one = open(&bytes(&doc)).unwrap().snapshot(&Options::default().with_bodies(vec![BodyName::from("Body1")])).unwrap();
    assert_eq!(one.bodies.len(), 1);
    assert_eq!(one.materials.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Steel"]);
}

#[test]
fn an_override_changes_the_dimension_and_keeps_the_face_names() {
    let design = open(&bytes(&box_doc("20 mm"))).unwrap();
    let l = design.parameters().into_iter().find(|p| p.name == *"L").unwrap();
    assert_eq!((&l.value, l.display_unit.as_str()), (&ParameterValue::Quantity(Quantity::Length(Mm(20.0))), "mm"));
    let before = design.snapshot(&Options::default()).unwrap();
    let after = design.snapshot(&Options::default().with_override("L", Quantity::Length(Mm(30.0)))).unwrap();
    let as_expression = design.snapshot(&Options::default().with_override("L", "3 cm")).unwrap();
    assert_eq!(after.bodies, as_expression.bodies);
    let reported = as_expression.parameters.iter().find(|p| p.name == *"L").unwrap();
    assert_eq!((reported.expression.as_str(), &reported.value), ("3 cm", &ParameterValue::Quantity(Quantity::Length(Mm(30.0)))));
    assert!((volume(&before.bodies[0]) - 1000.0).abs() < 1e-9);
    assert!((volume(&after.bodies[0]) - 1500.0).abs() < 1e-9);
    assert_eq!(bbox(&after.bodies[0]).1, [30.0, 10.0, 5.0]);
    assert_eq!(face_names(&before), face_names(&after));
    // The same face keeps its name and its plane's direction.
    for (f0, f1) in before.bodies[0].faces.iter().zip(&after.bodies[0].faces) {
        assert_eq!(f0.name, f1.name);
        let (Surface::Plane { normal: a, .. }, Surface::Plane { normal: b, .. }) = (&f0.surface, &f1.surface) else { panic!() };
        assert_eq!(a, b);
    }
    // The design itself is unchanged, and going back gives the first snapshot again.
    assert_eq!(design.parameters().into_iter().find(|p| p.name == *"L").unwrap().value, l.value);
    assert_eq!(to_json(&design.snapshot(&Options::default()).unwrap()).unwrap(), to_json(&before).unwrap());
}

#[test]
fn snapshots_are_byte_identical() {
    let doc = holed_doc();
    let a = to_json(&snap(&doc)).unwrap();
    assert_eq!(a, to_json(&snap(&doc)).unwrap());
    let design = open(&bytes(&doc)).unwrap();
    let c = to_json(&design.snapshot(&Options::default()).unwrap()).unwrap();
    let d = to_json(&design.snapshot(&Options::default()).unwrap()).unwrap();
    assert_eq!((&a, &a), (&c, &d));
}

/// The `ft:face` of every triangle, per object, read from the package's model part.
fn face_tags(package: &[u8]) -> Vec<Vec<u32>> {
    use std::io::Read as _;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(package)).unwrap();
    let mut xml = String::new();
    zip.by_name("3D/3dmodel.model").unwrap().read_to_string(&mut xml).unwrap();
    xml.split("<object ")
        .skip(1)
        .map(|object| {
            object
                .split("<triangle ")
                .skip(1)
                .map(|t| {
                    let rest = &t[t.find("ft:face=\"").unwrap() + 9..];
                    rest[..rest.find('"').unwrap()].parse().unwrap()
                })
                .collect()
        })
        .collect()
}

#[test]
fn the_tagged_3mf_export_and_the_snapshot_agree_on_faces() {
    for doc in [box_doc("20 mm"), holed_doc(), sphere_doc()] {
        let s = snap(&doc);
        let mut model = Model::new();
        model.evaluate(&doc);
        let world = solidcraft_doc::world_state(&doc, &model.state());
        let bytes = solidcraft_io::export(&world, &[], solidcraft_io::Format::ThreeMfTagged, "x").unwrap();
        let (objects, _) = solidcraft_io::read_3mf(&bytes).unwrap();
        let tagged = face_tags(&bytes);
        assert_eq!(objects.len(), s.bodies.len());
        for ((o, b), tags) in objects.iter().zip(&s.bodies).zip(&tagged) {
            assert_eq!(b.name, *o.name.as_str());
            let positions: Vec<[f64; 3]> = o.positions.iter().map(|p| [p.x, p.y, p.z]).collect();
            assert_eq!(positions, b.positions.iter().map(|p| p.0).collect::<Vec<_>>());
            assert_eq!(tags.len(), o.triangles.len());
            let mut theirs: Vec<(u32, [u32; 3])> = o.triangles.iter().zip(tags).map(|(t, f)| (*f, *t)).collect();
            let mut ours: Vec<(u32, [u32; 3])> = b.faces.iter().flat_map(|f| f.triangles.iter().map(|t| (f.index.0, t.map(|v| v.0)))).collect();
            theirs.sort_unstable();
            ours.sort_unstable();
            assert_eq!(theirs, ours);
        }
    }
}

#[test]
fn json_reads_back_and_tolerates_what_it_does_not_know() {
    let s = snap(&holed_doc());
    let text = to_json(&s).unwrap();
    assert_eq!(from_json(&text).unwrap(), s);
    // Added fields and unknown kinds read; a missing field is its default.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["future_field"] = serde_json::json!({"x": 1});
    v["bodies"][0]["faces"][0]["surface"] = serde_json::json!({"type": "subdivision", "level": 3});
    v["bodies"][0]["faces"][0]["also_new"] = serde_json::json!(true);
    v["bodies"][0]["edges"][0]["curve"] = serde_json::json!({"type": "clothoid"});
    v["features"][0]["kind"] = serde_json::json!({"type": "lattice", "cell": 4});
    v["parameters"] = serde_json::json!([{"name": "p", "source": "plugin"}]);
    v["bodies"][0].as_object_mut().unwrap().remove("attributes");
    let read = from_json(&v.to_string()).unwrap();
    assert_eq!(read.bodies[0].faces[0].surface, Surface::Other);
    assert_eq!(read.bodies[0].edges[0].curve, solidcraft_exchange::Curve::Other);
    assert_eq!(read.features[0].kind, solidcraft_exchange::FeatureKind::Other);
    assert_eq!(read.parameters[0].source, solidcraft_exchange::ParameterSource::Other);
    assert!(read.bodies[0].attributes.is_empty());
    // A direction must be one.
    v["bodies"][0]["faces"][1]["surface"] = serde_json::json!({"type": "plane", "point": [0, 0, 0], "normal": [0, 0, 0]});
    assert!(from_json(&v.to_string()).is_err());
    // A later, breaking schema is refused.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["schema"] = serde_json::json!(2);
    assert!(matches!(from_json(&v.to_string()), Err(Error::NotSupported(_))));
}

#[test]
fn hostile_input_is_refused_without_panicking() {
    assert!(matches!(open(b"not a design"), Err(Error::Read(_))));
    assert!(matches!(open(&[0xff, 0xfe, 0x00]), Err(Error::Read(_))));
    let mut doc = box_doc("20 mm");
    doc.text_params.push(solidcraft_doc::text_params::TextParam { name: "label".into(), text: "A".into(), comment: String::new() });
    let design = open(&bytes(&doc)).unwrap();
    let with = |name: &str, value: Override| design.snapshot(&Options::default().with_override(name, value));
    let length = |v: f64| Override::Value(Quantity::Length(Mm(v)));
    for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1e300] {
        assert!(matches!(with("L", length(bad)), Err(Error::ParameterValue { .. })), "{bad}");
    }
    assert!(matches!(with("nope", length(1.0)), Err(Error::UnknownParameter(_))));
    assert!(matches!(with("label", length(1.0)), Err(Error::ParameterValue { .. })));
    assert!(matches!(with("L", Override::Value(Quantity::Number(2.0))), Err(Error::ParameterValue { .. })), "a number for a length");
    assert!(matches!(with("L", "L * 2".into()), Err(Error::Overrides(_))), "circular");
    assert!(matches!(with("L", "1 +".into()), Err(Error::Overrides(_))));
    assert!(matches!(with("L", "\u{0}".into()), Err(Error::ParameterValue { .. })));
    assert!(matches!(with("L", "1".repeat(5000).as_str().into()), Err(Error::ParameterValue { .. })));
    let twice = Options::default().with_override("L", length(1.0)).with_override("L", length(2.0));
    assert!(matches!(design.snapshot(&twice), Err(Error::Overrides(_))), "duplicate");
    let mut many = Options::default();
    for _ in 0..2000 {
        many = many.with_override("L", length(1.0));
    }
    assert!(matches!(design.snapshot(&many), Err(Error::Limit(_))));
    for t in [Tolerance::Relative(0.0), Tolerance::Relative(f64::NAN), Tolerance::Relative(5.0), Tolerance::Absolute(-1.0), Tolerance::Absolute(1e-9)]
    {
        assert!(matches!(design.snapshot(&Options::default().with_tolerance(t)), Err(Error::Invalid(_))), "{t:?}");
    }
    assert!(matches!(design.snapshot(&Options::default().with_bodies(vec!["Body9".into()])), Err(Error::NoBody(_))));
    // A value the feature cannot take fails the evaluation, naming the feature; the design still
    // snapshots at its own values afterwards.
    match with("L", length(0.0)) {
        Err(Error::Evaluation(failed)) => assert!(!failed.is_empty() && failed[0].id.0 > 0),
        Err(Error::Overrides(_)) => {}
        other => panic!("a zero-length box: {other:?}"),
    }
    assert_eq!(design.snapshot(&Options::default()).unwrap().bodies.len(), 1);
    // A text parameter takes a text.
    assert!(with("label", "\"B\"".into()).is_ok());
}

/// What this crate writes is consistent; what refers to nothing, or is malformed, is refused
/// when read, so a `Snapshot` in hand can be walked safely.
#[test]
fn snapshots_are_checked_when_read() {
    let doc = holed_doc();
    let s = snap(&doc);
    s.validate().unwrap();
    let blob = open(include_bytes!("fixtures/spline-extrude.json")).unwrap().snapshot(&Options::default()).unwrap();
    blob.validate().unwrap();
    let text = to_json(&s).unwrap();
    let broken = |path: &[&str], value: serde_json::Value| {
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut at = &mut v;
        for k in path {
            at = match k.parse::<usize>() {
                Ok(i) => &mut at[i],
                Err(_) => &mut at[*k],
            };
        }
        *at = value;
        from_json(&v.to_string())
    };
    let cases: [(&[&str], serde_json::Value); 8] = [
        (&["bodies", "0", "faces", "0", "triangles", "0"], serde_json::json!([0, 1, 999_999])),
        (&["bodies", "0", "faces", "1", "index"], serde_json::json!(7)),
        (&["bodies", "0", "edges", "0", "bounds"], serde_json::json!({"between": [0, 99]})),
        (&["bodies", "0", "edges", "0", "bounds"], serde_json::json!({"between": [2, 2]})),
        (&["bodies", "0", "edges", "0", "path"], serde_json::json!([0, 999_999])),
        (&["bodies", "0", "vertices", "0", "edges"], serde_json::json!([99])),
        (&["bodies", "0", "material"], serde_json::json!("Unobtainium")),
        (
            &["bodies", "0", "faces", "0", "surface"],
            serde_json::json!({"type": "nurbs", "u": {"degree": 1, "knots": [0, 1]}, "v": {"degree": 1, "knots": [0, 0, 1, 1]}, "net": [[{"point": [0, 0, 0], "weight": 1}, {"point": [1, 0, 0], "weight": 1}]]}),
        ),
    ];
    for (path, value) in cases {
        assert!(matches!(broken(path, value.clone()), Err(Error::Invalid(_)) | Err(Error::Read(_))), "{path:?} = {value}");
    }
    // Two bodies of one name.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let first = v["bodies"][0].clone();
    v["bodies"].as_array_mut().unwrap().push(first);
    assert!(matches!(from_json(&v.to_string()), Err(Error::Invalid(_))));
}

/// The typed accessors walk a body without indices.
#[test]
fn bodies_are_walked_by_reference() {
    let s = snap(&holed_doc());
    let b = &s.bodies[0];
    assert_eq!(b.triangles().count(), b.faces.iter().map(|f| f.triangles.len()).sum::<usize>());
    let area: f64 = b.triangles().map(|(_, [a, c, d])| norm(cross(sub(c.0, a.0), sub(d.0, a.0))) / 2.0).sum();
    // 2 × 400 − 2 × 9π of the faces, 4 × 200 sides and the faceted hole wall (2π·3·10, a little less).
    let exact = 2.0 * 400.0 - 2.0 * 9.0 * PI + 800.0 + 2.0 * PI * 3.0 * 10.0;
    assert!((area - exact).abs() < 0.5, "{area} {exact}");
    let top = b.faces.iter().find(|f| matches!(f.surface, Surface::Plane { normal, .. } if normal.get()[2] > 0.5)).unwrap();
    assert_eq!(b.face_named(top.name.as_str()).map(|f| f.index), Some(top.index));
    // The top is bounded by the box's 4 top edges and the hole's 2 rim arcs.
    assert_eq!(b.edges_of(top.index).count(), 6);
    // A map keyed by face name looks up by `&str`.
    let by_name: BTreeMap<FaceName, usize> = b.faces.iter().map(|f| (f.name.clone(), f.triangles.len())).collect();
    assert_eq!(by_name.get(top.name.as_str()), Some(&top.triangles.len()));
}

fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}

#[test]
fn mass_size_shells_and_failures_come_when_asked() {
    let design = open(&bytes(&holed_doc())).unwrap();
    let plain = design.snapshot(&Options::default()).unwrap();
    let b = &plain.bodies[0];
    assert_eq!(b.mass, None);
    assert_eq!(b.shells, 1);
    // The kernel's size: the diagonal of the 20 × 20 × 10 box's corners.
    assert!((b.size.0 - (20f64 * 20.0 + 20.0 * 20.0 + 10.0 * 10.0).sqrt()).abs() < 1e-9, "{}", b.size);
    assert!(b.faces.iter().all(|f| !f.inferred));
    let measured = design.snapshot(&Options::default().with_mass(true)).unwrap();
    let m = measured.bodies[0].mass.unwrap();
    let volume = 20.0 * 20.0 * 10.0 - std::f64::consts::PI * 9.0 * 10.0;
    assert!((m.volume - volume).abs() < 1e-3 * volume, "{} vs {volume}", m.volume);
    assert!((m.centroid.0[0] - 10.0).abs() < 1e-6 && (m.max.0[2] - 10.0).abs() < 1e-9, "{m:?}");
    // A failing feature: refused by default, listed with failures allowed, the rest built.
    let mut doc = holed_doc();
    doc.set_param("L", "0", Some("mm"), None).unwrap();
    doc.add_feature(
        FeatureKind::Box {
            corner: point_expr(Vec3::new(30.0, 0.0, 0.0)),
            length: "L".into(),
            width: "1".into(),
            height: "1".into(),
            operation: Operation::NewBody,
        },
        None,
    )
    .unwrap();
    let design = open(&bytes(&doc)).unwrap();
    assert!(matches!(design.snapshot(&Options::default()), Err(Error::Evaluation(_))));
    let partial = design.snapshot(&Options::default().with_failures_allowed(true)).unwrap();
    assert_eq!(partial.failures.len(), 1, "{:?}", partial.failures);
    assert!(partial.failures[0].feature.0 > 0 && !partial.failures[0].message.is_empty());
    assert_eq!(partial.bodies.len(), 1);
    assert!(plain.failures.is_empty());
}
