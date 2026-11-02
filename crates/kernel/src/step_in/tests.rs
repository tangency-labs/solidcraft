use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use super::*;
use crate::{BoolOp, boolean, box_solid, cylinder, extrude, fillet, measure, revolve, sphere, step_export, torus};

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-9)
}

/// A single spherical face bounded by one meridian edge used in both directions.
/// `end_angle` is measured from the north pole toward the seam; PI is the south pole.
fn seam_sphere_step(center: Vec3, radius: f64, axis: Vec3, reference: Vec3, azimuth: f64, reverse_edge: bool, end_angle: f64) -> String {
    let seam = reference * azimuth.cos() + axis.cross(reference) * azimuth.sin();
    let end = center + (axis * end_angle.cos() + seam * end_angle.sin()) * radius;
    let north = center + axis * radius;
    let circle_axis = axis.cross(seam);
    let point = |p: Vec3| format!("({:.16},{:.16},{:.16})", p.x, p.y, p.z);
    let (start, finish, sense, first, second) = if reverse_edge { (9, 7, "F", 15, 14) } else { (7, 9, "T", 14, 15) };
    format!(
        "{HEAD}#1=CARTESIAN_POINT('',{});\n#2=DIRECTION('',{});\n#3=DIRECTION('',{});\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#5=SPHERICAL_SURFACE('',#4,{radius:.16});\n#6=CARTESIAN_POINT('',{});\n#7=VERTEX_POINT('',#6);\n#8=CARTESIAN_POINT('',{});\n#9=VERTEX_POINT('',#8);\n#10=DIRECTION('',{});\n#11=AXIS2_PLACEMENT_3D('',#1,#10,#12);\n#12=DIRECTION('',{});\n#13=CIRCLE('',#11,{radius:.16});\n#16=EDGE_CURVE('',#{start},#{finish},#13,.{sense}.);\n#14=ORIENTED_EDGE('',*,*,#16,.T.);\n#15=ORIENTED_EDGE('',*,*,#16,.F.);\n#17=EDGE_LOOP('',(#{first},#{second}));\n#18=FACE_OUTER_BOUND('',#17,.T.);\n#19=ADVANCED_FACE('',(#18),#5,.T.);\n#20=CLOSED_SHELL('',(#19));\n#21=MANIFOLD_SOLID_BREP('Ball',#20);\n#22=SHAPE_REPRESENTATION('',(#21),$);\n{TAIL}",
        point(center),
        point(axis),
        point(reference),
        point(north),
        point(end),
        point(circle_axis),
        point(seam),
    )
}

fn assert_seam_sphere(text: &str, radius: f64) {
    let imp = step_import(text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let body = &imp.bodies[0];
    assert!(body.closed);
    let m = measure(&body.body).unwrap();
    let want_volume = 4.0 / 3.0 * std::f64::consts::PI * radius.powi(3);
    let want_area = 4.0 * std::f64::consts::PI * radius.powi(2);
    assert!(rel(m.volume, want_volume) < 1e-3, "volume {} vs {want_volume}", m.volume);
    assert!(rel(m.area, want_area) < 1e-3, "area {} vs {want_area}", m.area);
    let mesh = body.body.mesh(&crate::MeshTolerance::chord(radius * 1e-3)).unwrap();
    assert!(!mesh.positions.is_empty() && !mesh.triangles.is_empty());
}

#[test]
fn seam_only_sphere_imports_as_a_whole_ball() {
    let text = seam_sphere_step(Vec3::ZERO, 2.0, Vec3::Z, Vec3::X, 0.0, false, std::f64::consts::PI);
    assert_seam_sphere(&text, 2.0);
}

#[test]
fn tilted_seam_only_sphere_imports_as_a_whole_ball() {
    let axis = Vec3::new(0.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
    let text = seam_sphere_step(Vec3::new(30.0, -12.0, 7.0), 1.9845, axis, Vec3::X, 0.7, false, std::f64::consts::PI);
    assert_seam_sphere(&text, 1.9845);
}

#[test]
fn reversed_seam_only_sphere_imports_as_a_whole_ball() {
    let text = seam_sphere_step(Vec3::ZERO, 2.0, Vec3::Z, Vec3::X, 0.0, true, std::f64::consts::PI);
    assert_seam_sphere(&text, 2.0);
}

#[test]
fn seam_only_sphere_whose_placement_axis_misses_its_poles_imports_as_a_whole_ball() {
    // The seam runs between poles on the X axis, while the surface's placement points along Z.
    let text = seam_sphere_step(Vec3::new(5.0, 6.0, 7.0), 3.0, Vec3::X, Vec3::Y, 0.4, false, std::f64::consts::PI);
    let placement = |axis: &str, reference: &str| format!("#2=DIRECTION('',{axis});\n#3=DIRECTION('',{reference});\n");
    let (one, zero) = ("1.0000000000000000", "0.0000000000000000");
    let (x, y, z) = (format!("({one},{zero},{zero})"), format!("({zero},{one},{zero})"), format!("({zero},{zero},{one})"));
    let text = text.replacen(&placement(&x, &y), &placement(&z, &x), 1);
    assert!(text.contains(&placement(&z, &x)), "the placement was not replaced");
    assert_seam_sphere(&text, 3.0);
}

#[test]
fn seam_only_sphere_void_is_turned_inside_out() {
    // A 5 mm ball with a 2 mm ball-shaped void, written as voids are: the void's closed shell
    // (faces facing out of it) used through an ORIENTED_CLOSED_SHELL with orientation .F.
    let renumber = |text: &str, by: u64| {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            if c == '#' {
                let mut n = String::new();
                while let Some(d) = chars.next_if(char::is_ascii_digit) {
                    n.push(d);
                }
                out += &(n.parse::<u64>().unwrap() + by).to_string();
            }
        }
        out
    };
    let inner = seam_sphere_step(Vec3::ZERO, 2.0, Vec3::Z, Vec3::X, 0.0, false, std::f64::consts::PI);
    let inner = &inner[inner.find("#1=").unwrap()..inner.find("#21=").unwrap()];
    let outer = seam_sphere_step(Vec3::ZERO, 5.0, Vec3::Z, Vec3::X, 1.0, false, std::f64::consts::PI);
    let brep = "#21=MANIFOLD_SOLID_BREP('Ball',#20);\n";
    assert!(outer.contains(brep));
    let void = format!("{}#200=ORIENTED_CLOSED_SHELL('',*,#120,.F.);\n#21=BREP_WITH_VOIDS('Hollow',#20,(#200));\n", renumber(inner, 100));
    let text = outer.replacen(brep, &void, 1);
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    let want = 4.0 / 3.0 * std::f64::consts::PI * (125.0 - 8.0);
    assert!(rel(m.volume, want) < 1e-3, "volume {} vs {want}", m.volume);
    let area = 4.0 * std::f64::consts::PI * (25.0 + 4.0);
    assert!(rel(m.area, area) < 1e-3, "area {} vs {area}", m.area);
}

/// A block with a closed cavity, written as BREP_WITH_VOIDS. SolidCraft writes the void's
/// shell facing into the cavity with ORIENTED_CLOSED_SHELL `.T.`; some exporters write the
/// same faces with `.F.` (#3730), which turned the void inside out and added its volume. Either
/// way the cavity is taken out.
#[test]
fn a_void_reads_as_a_cavity_whichever_way_its_shell_flag_points() {
    let block = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 12.5)).unwrap();
    let cavity = box_solid(Vec3::new(10.0, 12.0, 4.0), Vec3::new(30.0, 18.0, 9.0)).unwrap();
    let Ok(Some(hollow)) = boolean(&block, &cavity, BoolOp::Cut) else { panic!("cut") };
    let want = 40.0 * 30.0 * 12.5 - 20.0 * 6.0 * 5.0;
    assert!(rel(measure(&hollow).unwrap().volume, want) < 1e-9);
    let text = step_export(&[&hollow], "void").unwrap();
    let flag = |t: &str| t.lines().filter(|l| l.contains("ORIENTED_CLOSED_SHELL")).map(str::to_owned).collect::<Vec<_>>();
    let written = flag(&text);
    assert!(written.len() == 1 && written[0].ends_with(".T.);"), "{written:?}");
    let flipped = text.replace(&written[0], &written[0].replace(".T.);", ".F.);"));
    for t in [text, flipped] {
        let imp = step_import(&t).unwrap();
        assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
        let m = measure(&imp.bodies[0].body).unwrap();
        assert!(rel(m.volume, want) < 1e-6, "{:?}: volume {} vs {want}", flag(&t), m.volume);
    }
}

/// A ball whose one face is bounded by a meridian edge from the north pole that stops short of
/// the south pole (at 2.5 rad), run out and back: the face is the whole sphere less a slit of no
/// area, so it is the same 2 mm ball as when the seam runs pole to pole.
#[test]
fn non_antipodal_seam_sphere_imports_as_a_whole_ball() {
    let text = seam_sphere_step(Vec3::ZERO, 2.0, Vec3::Z, Vec3::X, 0.0, false, 2.5);
    assert_seam_sphere(&text, 2.0);
}

/// A whole ring torus (major radius 10, minor 3, about Z) written as one face bounded only by
/// seams: its tube circle at u = 0 and its ring circle at v = 0, each run out and back, in
/// either order round the loop; or only a slit along the tube circle, from the outer equator 2.5
/// rad towards the top and back, the torus less a slit of no area. Each is the whole torus,
/// volume 2π²Rr² and area 4π²Rr.
#[test]
fn seam_only_torus_imports_as_a_whole_ring() {
    use std::f64::consts::PI;
    let (major, minor) = (10.0_f64, 3.0_f64);
    let outer = major + minor;
    let (end_x, end_z) = (major + minor * 2.5_f64.cos(), minor * 2.5_f64.sin());
    let tube = format!(
        "#14=CARTESIAN_POINT('',({major:.16},0.,0.));\n#15=DIRECTION('',(0.,-1.,0.));\n#16=AXIS2_PLACEMENT_3D('',#14,#15,#3);\n#17=CIRCLE('',#16,{minor:.16});\n"
    );
    let ring = format!("#18=CIRCLE('',#4,{outer:.16});\n#19=EDGE_CURVE('',#11,#11,#18,.T.);\n");
    let cases = [
        ("tube then ring", format!("{tube}#20=EDGE_CURVE('',#11,#11,#17,.T.);\n{ring}#30=EDGE_LOOP('',(#21,#22,#23,#24));\n")),
        ("ring then tube", format!("{tube}#20=EDGE_CURVE('',#11,#11,#17,.T.);\n{ring}#30=EDGE_LOOP('',(#22,#21,#24,#23));\n")),
        (
            "a slit along the tube",
            format!(
                "#12=CARTESIAN_POINT('',({end_x:.16},0.,{end_z:.16}));\n#13=VERTEX_POINT('',#12);\n{tube}#20=EDGE_CURVE('',#11,#13,#17,.T.);\n#30=EDGE_LOOP('',(#21,#23));\n"
            ),
        ),
    ];
    for (what, edges) in cases {
        let mut t = String::from(HEAD);
        t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
        t += &format!(
            "#5=TOROIDAL_SURFACE('',#4,{major:.16},{minor:.16});\n#10=CARTESIAN_POINT('',({outer:.16},0.,0.));\n#11=VERTEX_POINT('',#10);\n"
        );
        t += &edges;
        t += "#21=ORIENTED_EDGE('',*,*,#20,.T.);\n#22=ORIENTED_EDGE('',*,*,#19,.T.);\n#23=ORIENTED_EDGE('',*,*,#20,.F.);\n#24=ORIENTED_EDGE('',*,*,#19,.F.);\n";
        t += "#31=FACE_OUTER_BOUND('',#30,.T.);\n#32=ADVANCED_FACE('',(#31),#5,.T.);\n#33=CLOSED_SHELL('',(#32));\n#34=MANIFOLD_SOLID_BREP('Ring',#33);\n";
        t += "#35=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#34),#70);\n";
        t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        t += TAIL;
        let imp = step_import(&t).unwrap_or_else(|e| panic!("{what}: {e}"));
        assert_eq!(imp.bodies.len(), 1, "{what}: {:?}", imp.warnings);
        assert!(imp.warnings.is_empty(), "{what}: {:?}", imp.warnings);
        assert!(imp.bodies[0].closed, "{what}");
        let m = measure(&imp.bodies[0].body).unwrap();
        let (volume, area) = (2.0 * PI * PI * major * minor * minor, 4.0 * PI * PI * major * minor);
        assert!(rel(m.volume, volume) < 1e-3, "{what}: volume {} vs {volume}", m.volume);
        assert!(rel(m.area, area) < 1e-3, "{what}: area {} vs {area}", m.area);
    }
}

#[test]
#[ignore = "manual STEP import timing; run without concurrent builds"]
fn benchmark_independent_step_solids() {
    let count = solidcraft_geom::env::var("STEP_BENCH_COUNT").map(|s| s.parse::<usize>().unwrap().clamp(1, 512)).unwrap_or(48);
    let kind = solidcraft_geom::env::var("STEP_BENCH_KIND").unwrap_or_else(|| "cylinder".into());
    let bodies: Vec<_> = (0..count)
        .map(|i| {
            let at = Vec3::new((i % 8) as f64 * 60.0, (i / 8) as f64 * 60.0, 0.0);
            match kind.as_str() {
                "torus" => torus(at, 20.0, 4.0).unwrap(),
                "box" => box_solid(at, at + Vec3::new(10.0, 15.0, 12.0)).unwrap(),
                _ => cylinder(at, Vec3::Z, 5.0, 12.0).unwrap(),
            }
        })
        .collect();
    let text = step_export(&bodies.iter().collect::<Vec<_>>(), "independent bodies").unwrap();
    if let Ok(path) = solidcraft_geom::env::var("STEP_BENCH_FIXTURE").ok_or(()) {
        std::fs::write(path, &text).unwrap();
    }
    let start = std::time::Instant::now();
    let ex = p21::parse(&text).unwrap();
    let parse = start.elapsed();
    let mut items: Vec<_> = ex.entities.iter().filter(|(_, e)| e.name() == "MANIFOLD_SOLID_BREP").map(|(id, _)| *id).collect();
    items.sort();
    let start = std::time::Instant::now();
    let solids: Vec<_> = items.iter().map(|id| solidcraft_step::topology::read_solid(&ex, *id, Units::default()).unwrap()).collect();
    let read = start.elapsed();
    let start = std::time::Instant::now();
    for solid in solids {
        std::hint::black_box(Body::from_record(solid.brep).unwrap().unmeshed_faces());
    }
    println!(
        "STEP_PHASE parse_ms={:.3} brep_ms={:.3} body_ms={:.3}",
        parse.as_secs_f64() * 1000.0,
        read.as_secs_f64() * 1000.0,
        start.elapsed().as_secs_f64() * 1000.0
    );
    for run in 0..5 {
        for workers in if run % 2 == 0 { [1, 8] } else { [8, 1] } {
            let start = std::time::Instant::now();
            let imported = step_import_with_workers(&text, workers).unwrap();
            let elapsed = start.elapsed();
            assert_eq!(imported.bodies.len(), count);
            assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
            println!(
                "STEP_BENCH kind={kind} run={run} workers={workers} bodies={count} bytes={} ms={:.3}",
                text.len(),
                elapsed.as_secs_f64() * 1000.0
            );
            std::hint::black_box(imported);
        }
    }
}

fn assert_same_import(serial: &StepImport, parallel: &StepImport) {
    assert_eq!(serial.tree, parallel.tree);
    assert_eq!(serial.warnings, parallel.warnings);
    assert_eq!((&serial.schema, &serial.system), (&parallel.schema, &parallel.system));
    assert_eq!(serial.bodies.len(), parallel.bodies.len());
    assert_eq!(
        step_export(&serial.bodies.iter().map(|b| &b.body).collect::<Vec<_>>(), "equivalence").unwrap(),
        step_export(&parallel.bodies.iter().map(|b| &b.body).collect::<Vec<_>>(), "equivalence").unwrap()
    );
    for (a, b) in serial.bodies.iter().zip(&parallel.bodies) {
        assert_eq!(
            (&a.name, &a.path, a.source_item, &a.face_origin, a.closed, a.file_faces, a.color),
            (&b.name, &b.path, b.source_item, &b.face_origin, b.closed, b.file_faces, b.color)
        );
        assert_eq!(a.body.paint(), b.body.paint());
        assert_eq!(a.body.face_sources, b.body.face_sources);
        assert_same_measure(&measure(&a.body).unwrap(), &measure(&b.body).unwrap());
        let (a, b) = (a.body.mesh(&crate::MeshTolerance::chord(0.05)).unwrap(), b.body.mesh(&crate::MeshTolerance::chord(0.05)).unwrap());
        assert_eq!(a.positions, b.positions);
        assert_eq!(a.normals, b.normals);
        assert_eq!(a.triangles, b.triangles);
        assert_eq!(a.tri_face, b.tri_face);
        assert_eq!(a.edges, b.edges);
        assert_eq!(a.seams, b.seams);
        assert_eq!(a.smooth_edges, b.smooth_edges);
        assert_eq!(a.sharp_edges, b.sharp_edges);
        assert_eq!(a.edge_faces, b.edge_faces);
    }
}

fn assert_same_measure(a: &crate::BodyMeasure, b: &crate::BodyMeasure) {
    assert_eq!((a.volume, a.area, a.centroid, a.bbox.min, a.bbox.max), (b.volume, b.area, b.centroid, b.bbox.min, b.bbox.max));
    assert_eq!((a.faces, a.edges, a.vertices, a.shells, &a.merged), (b.faces, b.edges, b.vertices, b.shells, &b.merged));
}

#[test]
fn parallel_import_retains_units_order_sources_and_partial_failure_across_batches() {
    let mut text = String::from(HEAD);
    text += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    text += &cube_entities();
    text += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))");
    text += &product(50, "Batch");
    let mut refs = Vec::new();
    for i in 0..70 {
        let id = 2000 + i;
        text += &format!("#{id}=MANIFOLD_SOLID_BREP('Cube{i}',#100);\n");
        refs.push(format!("#{id}"));
    }
    refs.insert(3, "#3000".into());
    refs.insert(8, "#2000".into());
    text += "#3000=MANIFOLD_SOLID_BREP('Broken',#999999);\n";
    text += &format!("#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',({}),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n", refs.join(","));
    text += TAIL;
    let serial = step_import_with_workers(&text, 1).unwrap();
    assert_eq!(serial.bodies.len(), 70);
    assert!(serial.warnings.iter().any(|warning| warning.contains("Broken")));
    assert_eq!(serial.bodies.first().unwrap().name, "Cube0");
    assert_eq!(serial.bodies.last().unwrap().name, "Cube69");
    assert!((measure(&serial.bodies[0].body).unwrap().volume - 1000.0).abs() < 1e-9);
    for workers in [2, 3, 8, usize::MAX] {
        assert_same_import(&serial, &step_import_with_workers(&text, workers).unwrap());
    }
}

#[test]
fn parallel_body_preparation_reports_errors_in_place_and_preserves_placement() {
    let body = cylinder(Vec3::new(2.0, 2.0, 0.0), Vec3::Z, 2.0, 4.0).unwrap();
    let ex = p21::parse(&step_export(&[&body], "test").unwrap()).unwrap();
    let solid = *ex.entities.iter().find(|(_, e)| e.name() == "MANIFOLD_SOLID_BREP").map(|(id, _)| id).unwrap();
    let units = Units::default();
    let placement = translation([10.0, 20.0, 30.0]);
    // The second item is not in the file.
    let jobs: Vec<_> = [solid, 999_999_999, solid, solid]
        .into_iter()
        .map(|item| parallel::Job { source: parallel::Source::Item { item, units }, placement, check_mesh: true })
        .collect();
    let serial = parallel::bodies(&ex, &jobs, 1);
    let threaded = parallel::bodies(&ex, &jobs, 4);
    assert_eq!(serial.len(), 4);
    assert_eq!(threaded.len(), 4);
    for (index, (a, b)) in serial.iter().zip(&threaded).enumerate() {
        if index == 1 {
            let (Err(a), Err(b)) = (a, b) else { panic!("job 1 has no item") };
            assert!(a.read && b.read);
            assert_eq!(a.msg, b.msg);
        } else {
            let (Ok(a), Ok(b)) = (a, b) else { panic!("job {index} failed") };
            assert_eq!(a.missing, b.missing);
            assert!(a.read.is_some() && b.read.is_some(), "each job read its item");
            let a = measure(&a.body).unwrap();
            let b = measure(&b.body).unwrap();
            assert_same_measure(&a, &b);
            // Curved bounds come from the measurement tessellation, not analytic extrema.
            assert!(a.bbox.min.dist(Vec3::new(10.0, 20.0, 30.0)) < 1e-3);
        }
    }
}

#[test]
fn parallel_curved_import_keeps_face_colours_and_exact_display_meshes() {
    let bodies: Vec<_> = (0..8)
        .map(|i| {
            let body = cylinder(Vec3::new(i as f64 * 20.0, 0.0, 0.0), Vec3::Z, 5.0, 12.0).unwrap();
            body.with_color(Some([0.2, 0.4, 0.6]))
                .with_paint(Some(crate::Paint { opacity: 0.7, faces: vec![crate::FacePaint { face: 0, color: [0.8, 0.1, 0.3], opacity: 0.5 }] }))
        })
        .collect();
    let text = step_export(&bodies.iter().collect::<Vec<_>>(), "coloured cylinders").unwrap();
    let serial = step_import_with_workers(&text, 1).unwrap();
    assert_eq!(serial.bodies.len(), 8);
    assert!(serial.bodies.iter().all(|b| b.color == Some([0.2, 0.4, 0.6])));
    assert!(serial.bodies.iter().all(|b| b.body.paint().is_some_and(|p| !p.faces.is_empty())));
    for workers in [2, 3, 8] {
        assert_same_import(&serial, &step_import_with_workers(&text, workers).unwrap());
    }
}

#[test]
fn parallel_import_does_not_prepare_past_body_budget_with_duplicates_or_mapped_items() {
    let text = format!(
        "{HEAD}{}#2000=MANIFOLD_SOLID_BREP('Duplicate',#100);\n#2001=MANIFOLD_SOLID_BREP('First',#100);\n#2002=MANIFOLD_SOLID_BREP('Second',#100);\n#2003=MANIFOLD_SOLID_BREP('Beyond budget',#999999);\n#3000=MAPPED_ITEM('',#999998,#999997);\n#60=SHAPE_REPRESENTATION('',(#2000,#2000,#2001,#3000,#2002,#2003),$);\n{TAIL}",
        cube_entities()
    );
    let ex = std::sync::Arc::new(p21::parse(&text).unwrap());
    let seed =
        step_import_with_workers(&format!("{HEAD}{}#60=SHAPE_REPRESENTATION('',(#101),$);\n{TAIL}", cube_entities()), 1).unwrap().bodies.remove(0);
    for workers in [1, 8] {
        let mut reader = Reader::new(ex.clone(), workers);
        reader.index();
        reader.bodies = vec![seed.clone(); MAX_BODIES - 2];
        reader.node_items.insert(2000);
        let mut out = Vec::new();
        reader.rep_bodies(60, &IDENTITY, &[], "Budget", None, &mut out, 0);
        assert!(reader.oriented.is_empty() && reader.unreadable.is_empty(), "the walk only plans the solids");
        while reader.convert_next(workers == 1) {}
        let out = reader.remap(&out);
        assert_eq!(reader.bodies.len(), MAX_BODIES);
        assert_eq!(out, vec![MAX_BODIES - 2, MAX_BODIES - 1]);
        assert_eq!(reader.bodies.last().unwrap().name, "Second");
        assert_eq!(reader.oriented.len(), 2);
        assert!(!reader.oriented.contains_key(&2003) && !reader.unreadable.contains_key(&2003));
        assert!(!reader.used_items.contains(&3000));
        assert_eq!(reader.warnings.len(), 1);
        assert!(reader.warnings.keys().next().unwrap().contains("more than"));
    }
}

/// Export → import → same volume, area and face count.
fn round_trip(b: &Body) -> StepImport {
    let text = step_export(&[b], "test").unwrap();
    step_validate(&text).unwrap();
    assert_eq!(crate::step_orientation_errors(&text).unwrap(), Vec::<String>::new());
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let (m0, m1) = (measure(b).unwrap(), measure(&imp.bodies[0].body).unwrap());
    assert!(rel(m1.volume, m0.volume) < 1e-3, "volume {} vs {}", m1.volume, m0.volume);
    assert!(rel(m1.area, m0.area) < 1e-3, "area {} vs {}", m1.area, m0.area);
    // The file has no more faces than the body (volume and area above check that nothing was
    // lost).
    assert!(imp.bodies[0].file_faces <= b.face_count());
    assert!(imp.bodies[0].closed);
    imp
}

#[test]
fn round_trip_primitives() {
    round_trip(&box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap());
    round_trip(&cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 5.0, 12.0).unwrap());
    round_trip(&sphere(Vec3::new(1.0, 2.0, 3.0), 10.0).unwrap());
    round_trip(&torus(Vec3::ZERO, 20.0, 4.0).unwrap());
}

#[test]
fn round_trip_features() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&plate, &pin, BoolOp::Cut).unwrap().unwrap();
    round_trip(&holed);
    let edge = Vec3::new(20.0, 0.0, 10.0);
    round_trip(&fillet(&plate, &[edge], 3.0).unwrap());
    let ring = Region2 { outer: Loop2::circle(Vec2::new(10.0, 0.0), 3.0), holes: vec![] };
    let donut = revolve(&Plane::XZ, &[ring], Vec2::ZERO, Vec2::Y, 1.5).unwrap();
    round_trip(&donut[0]);
}

/// Robustness seed 17: a thin three-quarter tube shelled with its bottom open reads back whole
/// (an export once wrote a 270° wall on a quarter of its surface, and the part read back about
/// 7% short with no warning).
#[test]
fn shelled_three_quarter_tube_round_trips() {
    let rect =
        Region2 { outer: Loop2::polygon(&[Vec2::new(10.0, 0.0), Vec2::new(12.5, 0.0), Vec2::new(12.5, 27.5), Vec2::new(10.0, 27.5)]), holes: vec![] };
    let tube = revolve(&Plane::XZ, &[rect], Vec2::ZERO, Vec2::Y, 1.5 * std::f64::consts::PI).unwrap().pop().unwrap();
    let b = crate::shell(&tube, &[Vec3::new(-7.182422893157272, 7.182422893245109, 0.0)], 1.0).unwrap();
    round_trip(&b);
}

/// A half tube (half a torus's tube, half way round, closed by a flat annulus and
/// two flat ends) whose torus the file places 5 nm off the rest, well within the file's
/// accuracy: the torus face's boundary then projects a hair past both ends of its half of the
/// tube, and its NURBS form once took the quarter past each end too, going round the whole tube,
/// which SMLib does not take for a torus: the part came in as a mesh of its faces.
#[test]
fn half_tube_a_hair_off_its_torus_imports_as_a_solid() {
    use solidcraft_geom::Seg2;
    use std::f64::consts::PI;
    let (major, minor) = (10.75, 1.25);
    let d = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::new(major, 0.0), radius: minor, start: 0.0, sweep: PI },
            Seg2::Line { a: Vec2::new(major - minor, 0.0), b: Vec2::new(major + minor, 0.0) },
        ],
    };
    let tube = revolve(&Plane::XZ, &[Region2 { outer: d, holes: vec![] }], Vec2::ZERO, Vec2::Y, PI).unwrap().pop().unwrap();
    let text = step_export(&[&tube], "test").unwrap();
    // The torus's placement moved to a point of its own, 5 nm along the axis, off the face's side.
    let torus = text.lines().find_map(|l| l.split_once("=TOROIDAL_SURFACE('',#")).unwrap().1.split(',').next().unwrap().to_string();
    let place = text.lines().find(|l| l.starts_with(&format!("#{torus}=AXIS2_PLACEMENT_3D('',#"))).unwrap();
    let (_, axes) = place.split_once("('',#").unwrap().1.split_once(',').unwrap();
    let mut low = text.replace(place, &format!("#{torus}=AXIS2_PLACEMENT_3D('',#900000,{axes}"));
    let data_end = low.rfind("ENDSEC;").unwrap();
    low.insert_str(data_end, "#900000=CARTESIAN_POINT('',(0.0,0.0,-5.E-6));\n");
    let imp = step_import(&low).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let b = &imp.bodies[0];
    assert!(b.refused.is_none() && b.closed && !b.body.is_mesh() && !b.body.is_surface(), "{:?} {:?}", b.refused, imp.warnings);
    assert!(rel(measure(&b.body).unwrap().volume, measure(&tube).unwrap().volume) < 1e-3);
}

#[test]
fn several_bodies_and_names() {
    let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(30.0, 10.0, 10.0)).unwrap();
    let text = step_export(&[&a, &b], "test").unwrap();
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 2);
    assert!(imp.bodies.iter().all(|b| !b.name.is_empty()));
    let vol: f64 = imp.bodies.iter().map(|b| measure(&b.body).unwrap().volume).sum();
    assert!(rel(vol, 2000.0) < 1e-6);
}

#[test]
fn imported_bodies_take_features() {
    let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let imp = step_import(&step_export(&[&plate], "t").unwrap()).unwrap();
    let body = &imp.bodies[0].body;
    // Fillet an edge, cut a hole through it.
    let rounded = fillet(body, &[Vec3::new(20.0, 0.0, 10.0)], 2.0).unwrap();
    let v_fillet = 40.0 * (4.0 - std::f64::consts::PI);
    assert!(rel(measure(&rounded).unwrap().volume, 12000.0 - v_fillet) < 1e-3);
    let pin = cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let holed = boolean(&rounded, &pin, BoolOp::Cut).unwrap().unwrap();
    let want = 12000.0 - v_fillet - std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(measure(&holed).unwrap().volume, want) < 1e-3);
}

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','hand','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n";
const TAIL: &str = "ENDSEC;\nEND-ISO-10303-21;\n";

#[test]
fn offset_surface_cube_import_is_closed_and_editable() {
    // Move each basis plane half a file unit inward; nested quarter-unit offsets
    // bring its supporting surface back to the original unit cube boundary.
    let mut entities = cube_entities();
    let origins = [(0.0, 0.0, 0.5), (0.0, 0.0, 0.5), (0.0, 0.5, 0.0), (0.5, 0.0, 0.0), (1.0, 0.5, 0.0), (0.5, 1.0, 0.0)];
    for (f, (x, y, z)) in origins.into_iter().enumerate() {
        let base = 400 + f * 20;
        let extra = 1000 + f * 10;
        entities = entities
            .replace(&format!("#{}=PLANE('',#{});", base + 10, base + 9), &format!("#{}=OFFSET_SURFACE('',#{},0.25,.F.);", base + 10, extra + 3));
        entities += &format!(
            "#{extra}=CARTESIAN_POINT('',({x},{y},{z}));\n#{}=AXIS2_PLACEMENT_3D('',#{extra},#{},$);\n#{}=PLANE('',#{});\n#{}=OFFSET_SURFACE('',#{},0.25,.F.);\n",
            extra + 1,
            base + 8,
            extra + 2,
            extra + 1,
            extra + 3,
            extra + 2
        );
    }
    entities += &context(10, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))");
    entities += "#9=ADVANCED_BREP_SHAPE_REPRESENTATION('Offsets',(#101),#10);\n";
    let import = step_import(&format!("{HEAD}{entities}{TAIL}")).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert!(import.bodies[0].closed);
    assert_eq!(import.bodies[0].file_faces, 6);
    let body = &import.bodies[0].body;
    assert!(rel(measure(body).unwrap().volume, 1000.0) < 1e-6);
    let cut = cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 12.0).unwrap();
    let result = boolean(body, &cut, BoolOp::Cut).unwrap().unwrap();
    assert!(rel(measure(&result).unwrap().volume, 1000.0 - 40.0 * std::f64::consts::PI) < 1e-3);
}

/// A solid whose B-rep SMLib refuses, its faces too (here every SMLib call: the build has no
/// SMLib), is refused with the kernel's reason, naming the solid. It once came in as a mesh of
/// its faces, which the planner could still triangulate.
#[cfg(not(feature = "smlib"))]
#[test]
fn a_solid_smlib_refuses_with_its_faces_is_refused_not_meshed() {
    let text = format!("{HEAD}{}#60=SHAPE_REPRESENTATION('',(#101),$);\n{TAIL}", cube_entities());
    let why = step_import(&text).map(|imp| imp.warnings).unwrap_err().to_string();
    assert!(why.contains("solid 'Cube': not imported: ") && why.contains("SMLib"), "{why}");
}

/// A hand-written unit cube brep (`#100`…) in a representation with the given unit and items.
fn cube_entities() -> String {
    let mut s = String::new();
    let pts = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)];
    for (i, (x, y, z)) in pts.iter().enumerate() {
        s += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,{z}.));\n#{}=VERTEX_POINT('',#{});\n", 200 + i, 210 + i, 200 + i);
    }
    // 12 edges, each on the line through its two vertices (trimmed by them). Their directions
    // and vectors are #340 to #363, clear of the ids the assembly fixtures add (#600 on).
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    for (k, (a, b)) in edges.iter().enumerate() {
        let (p, q) = (pts[*a], pts[*b]);
        let d = (q.0 - p.0, q.1 - p.1, q.2 - p.2);
        s += &format!("#{}=DIRECTION('',({}.,{}.,{}.));\n#{}=VECTOR('',#{},1.);\n", 340 + 2 * k, d.0, d.1, d.2, 341 + 2 * k, 340 + 2 * k);
        s += &format!(
            "#{}=LINE('',#{},#{});\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n",
            300 + k,
            200 + a,
            341 + 2 * k,
            320 + k,
            210 + a,
            210 + b,
            300 + k
        );
    }
    // Faces: (edge, forward?) loops counter-clockwise seen from outside, and plane frames.
    let faces: [(&[(usize, bool)], (i32, i32, i32), (i32, i32, i32)); 6] = [
        (&[(3, false), (2, false), (1, false), (0, false)], (0, 0, 0), (0, 0, -1)),
        (&[(4, true), (5, true), (6, true), (7, true)], (0, 0, 1), (0, 0, 1)),
        (&[(0, true), (9, true), (4, false), (8, false)], (0, 0, 0), (0, -1, 0)),
        (&[(1, true), (10, true), (5, false), (9, false)], (1, 0, 0), (1, 0, 0)),
        (&[(2, true), (11, true), (6, false), (10, false)], (1, 1, 0), (0, 1, 0)),
        (&[(3, true), (8, true), (7, false), (11, false)], (0, 1, 0), (-1, 0, 0)),
    ];
    let mut face_ids = Vec::new();
    for (f, (lp, o, n)) in faces.iter().enumerate() {
        let base = 400 + f * 20;
        let oes: Vec<String> = lp
            .iter()
            .enumerate()
            .map(|(j, (e, fwd))| {
                s += &format!("#{}=ORIENTED_EDGE('',*,*,#{},{});\n", base + j, 320 + e, if *fwd { ".T." } else { ".F." });
                format!("#{}", base + j)
            })
            .collect();
        s += &format!("#{}=EDGE_LOOP('',({}));\n#{}=FACE_OUTER_BOUND('',#{},.T.);\n", base + 5, oes.join(","), base + 6, base + 5);
        s += &format!("#{}=CARTESIAN_POINT('',({}.,{}.,{}.));\n#{}=DIRECTION('',({}.,{}.,{}.));\n", base + 7, o.0, o.1, o.2, base + 8, n.0, n.1, n.2);
        s += &format!("#{}=AXIS2_PLACEMENT_3D('',#{},#{},$);\n#{}=PLANE('',#{});\n", base + 9, base + 7, base + 8, base + 10, base + 9);
        s += &format!("#{}=ADVANCED_FACE('',(#{}),#{},.T.);\n", base + 11, base + 6, base + 10);
        face_ids.push(format!("#{}", base + 11));
    }
    s += &format!("#100=CLOSED_SHELL('',({}));\n#101=MANIFOLD_SOLID_BREP('Cube',#100);\n", face_ids.join(","));
    s
}

fn context(id: u64, unit: &str) -> String {
    format!(
        "#{id}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{},#{}))REPRESENTATION_CONTEXT('',''));\n#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#{},'','');\n#{}={unit};\n#{}=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n",
        id + 1,
        id + 2,
        id + 3,
        id + 1,
        id + 2,
        id + 2,
        id + 3
    )
}

fn product(pd: u64, name: &str) -> String {
    format!(
        "#{}=PRODUCT('{name}','{name}','',(#{}));\n#{}=PRODUCT_CONTEXT('',#9,'mechanical');\n#{}=PRODUCT_DEFINITION_FORMATION('','',#{});\n#{pd}=PRODUCT_DEFINITION('design','',#{},#8);\n#{}=PRODUCT_DEFINITION_SHAPE('','',#{pd});\n",
        pd + 1,
        pd + 2,
        pd + 2,
        pd + 3,
        pd + 1,
        pd + 3,
        pd + 4
    )
}

/// A surface model of a closed shell and an open one beside it (Poppy Humanoid's thighs: the
/// part, and a sheet of two faces on it) comes in as two bodies: the closed shell a solid of its
/// own, the open one a surface body, each in the model's colour. It once came in as one body,
/// open for its sheet's edges.
#[test]
fn a_surface_models_closed_shell_beside_a_sheet_is_a_solid_of_its_own() {
    let mut t = String::from(HEAD);
    t += &cube_entities();
    // A unit square sheet at z = 5.
    let corners = [(0, 0), (1, 0), (1, 1), (0, 1)];
    for (i, (x, y)) in corners.iter().enumerate() {
        t += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,5.));\n#{}=VERTEX_POINT('',#{});\n", 800 + i, 810 + i, 800 + i);
    }
    let mut oes = Vec::new();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        t += &format!("#{}=DIRECTION('',({}.,{}.,0.));\n#{}=VECTOR('',#{},1.);\n", 820 + 2 * i, b.0 - a.0, b.1 - a.1, 821 + 2 * i, 820 + 2 * i);
        t += &format!("#{}=LINE('',#{},#{});\n", 830 + i, 800 + i, 821 + 2 * i);
        t += &format!("#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n", 840 + i, 810 + i, 810 + (i + 1) % 4, 830 + i);
        t += &format!("#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n", 850 + i, 840 + i);
        oes.push(format!("#{}", 850 + i));
    }
    t += &format!("#860=EDGE_LOOP('',({}));\n#861=FACE_OUTER_BOUND('',#860,.T.);\n", oes.join(","));
    t += "#862=CARTESIAN_POINT('',(0.,0.,5.));\n#863=DIRECTION('',(0.,0.,1.));\n#864=AXIS2_PLACEMENT_3D('',#862,#863,$);\n#865=PLANE('',#864);\n";
    t += "#866=ADVANCED_FACE('',(#861),#865,.T.);\n#130=OPEN_SHELL('',(#866));\n#110=SHELL_BASED_SURFACE_MODEL('Thigh',(#100,#130));\n";
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += "#60=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#110),#20);\n";
    t += "#70=STYLED_ITEM('',(#71),#110);\n#71=PRESENTATION_STYLE_ASSIGNMENT((#72));\n#72=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#73=SURFACE_SIDE_STYLE('',(#74));\n#74=SURFACE_STYLE_FILL_AREA(#75);\n#75=FILL_AREA_STYLE('',(#76));\n#76=FILL_AREA_STYLE_COLOUR('',#77);\n#77=COLOUR_RGB('',1.,0.5,0.);\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert_eq!(imp.bodies.len(), 2, "{:?}", imp.warnings);
    let (solid, sheet) = (&imp.bodies[0], &imp.bodies[1]);
    assert!(solid.closed && solid.refused.is_none() && !solid.body.is_surface(), "{:?}", imp.warnings);
    assert_eq!((solid.source_item, solid.file_faces), (100, 6));
    assert!(rel(measure(&solid.body).unwrap().volume, 1.0) < 1e-9);
    assert!(!sheet.closed && sheet.body.is_surface() && sheet.body.face_count() == 1);
    assert_eq!((sheet.source_item, sheet.name.as_str()), (110, "Thigh"));
    assert!(imp.bodies.iter().all(|b| b.color == Some([1.0, 0.5, 0.0])));
}

#[test]
fn hand_written_part_in_inches_with_colour() {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(
        20,
        "(CONVERSION_BASED_UNIT('INCH',#30)LENGTH_UNIT()NAMED_UNIT(*));\n#30=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#31);\n#31=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
    );
    t += &product(50, "Widget");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#62);\n#62=SHAPE_REPRESENTATION('',(),#20);\n#63=SHAPE_REPRESENTATION_RELATIONSHIP('','',#62,#60);\n";
    t += "#70=STYLED_ITEM('',(#71),#101);\n#71=PRESENTATION_STYLE_ASSIGNMENT((#72));\n#72=SURFACE_STYLE_USAGE(.BOTH.,#73);\n#73=SURFACE_SIDE_STYLE('',(#74));\n#74=SURFACE_STYLE_FILL_AREA(#75);\n#75=FILL_AREA_STYLE('',(#76));\n#76=FILL_AREA_STYLE_COLOUR('',#77);\n#77=COLOUR_RGB('',1.,0.5,0.);\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1);
    let b = &imp.bodies[0];
    assert_eq!(b.name, "Cube");
    assert_eq!(b.path, vec!["Widget".to_string()]);
    assert_eq!(b.instance, None, "a root part is no occurrence");
    assert_eq!(b.color, Some([1.0, 0.5, 0.0]));
    let m = measure(&b.body).unwrap();
    assert!(rel(m.volume, 25.4f64.powi(3)) < 1e-9, "{}", m.volume);
    assert_eq!(m.faces, 6);
    assert_eq!(imp.tree.len(), 1);
    assert_eq!(imp.tree[0].name, "Widget");
}

/// A cube part (in cm) placed twice in a millimetre assembly: the file asks one brep item for
/// two bodies.
fn two_instance_assembly() -> String {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))");
    t += &context(30, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    // Part (cube, in cm) and assembly (mm) with two placed instances of it.
    t += &product(50, "Block");
    t += &product(70, "Assembly");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101,#65),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    t += "#65=AXIS2_PLACEMENT_3D('',#66,$,$);\n#66=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#80=SHAPE_REPRESENTATION('',(#81,#84),#30);\n#81=AXIS2_PLACEMENT_3D('',#82,$,$);\n#82=CARTESIAN_POINT('',(0.,0.,0.));\n";
    t += "#84=AXIS2_PLACEMENT_3D('',#85,#86,#87);\n#85=CARTESIAN_POINT('',(100.,0.,0.));\n#86=DIRECTION('',(0.,0.,1.));\n#87=DIRECTION('',(0.,1.,0.));\n";
    t += "#88=SHAPE_DEFINITION_REPRESENTATION(#74,#80);\n";
    for (k, item) in [(0u64, 81u64), (1, 84)] {
        let n = 600 + k * 10;
        t += &format!("#{n}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{k}','Block:{k}','',#70,#50,$);\n");
        t += &format!("#{}=PRODUCT_DEFINITION_SHAPE('','',#{n});\n", n + 1);
        t += &format!("#{}=ITEM_DEFINED_TRANSFORMATION('','',#65,#{item});\n", n + 2);
        t += &format!(
            "#{}=(REPRESENTATION_RELATIONSHIP('','',#60,#80)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{})SHAPE_REPRESENTATION_RELATIONSHIP());\n",
            n + 3,
            n + 2
        );
        t += &format!("#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n", n + 4, n + 3, n + 1);
    }
    t += TAIL;
    t
}

#[test]
fn hand_written_assembly() {
    let t = two_instance_assembly();
    let imp = step_import(&t).unwrap();
    assert_same_import(&step_import_with_workers(&t, 1).unwrap(), &step_import_with_workers(&t, 8).unwrap());
    let expanded = t.replace("(#101,#65)", "(#101,#2000,#2001,#2002,#2003,#65)");
    let extra: String = (0..4).map(|i| format!("#{}=MANIFOLD_SOLID_BREP('Extra{i}',#100);\n", 2000 + i)).collect();
    let expanded = expanded.replace(TAIL, &(extra + TAIL));
    let serial = step_import_with_workers(&expanded, 1).unwrap();
    assert_eq!(serial.bodies.len(), 10);
    assert_same_import(&serial, &step_import_with_workers(&expanded, 8).unwrap());
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 2);
    let boxes: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
    for m in &boxes {
        assert!(rel(m.volume, 1000.0) < 1e-9, "{}", m.volume);
    }
    // First at the origin, second moved 100 mm along X and turned 90° about Z.
    assert!(boxes[0].bbox.min.dist(Vec3::ZERO) < 1e-6 && boxes[0].bbox.max.dist(Vec3::new(10.0, 10.0, 10.0)) < 1e-6);
    assert!(boxes[1].bbox.min.dist(Vec3::new(90.0, 0.0, 0.0)) < 1e-6, "{:?}", boxes[1].bbox);
    assert!(boxes[1].bbox.max.dist(Vec3::new(100.0, 10.0, 10.0)) < 1e-6, "{:?}", boxes[1].bbox);
    assert_eq!(imp.tree.len(), 1);
    let root = &imp.tree[0];
    assert_eq!(root.name, "Assembly");
    assert_eq!(root.children.len(), 2);
    assert!(root.children.iter().all(|c| c.name == "Block" && c.bodies.len() == 1));
    assert_eq!(imp.bodies[1].path, vec!["Assembly".to_string(), "Block".to_string()]);
    assert_eq!([imp.bodies[0].instance.as_deref(), imp.bodies[1].instance.as_deref()], [Some("Block:0"), Some("Block:1")]);
    assert!((root.children[1].transform[3][0] - 100.0).abs() < 1e-9);
}

/// The cube part (in cm) placed `n` times in a millimetre assembly, each instance moved along X
/// and turned about Z.
fn instanced_assembly(n: u64) -> String {
    instanced_assembly_of(1, n)
}

/// [`instanced_assembly`] with `parts` distinct parts (the cube solid under different items and
/// products), instance `k` placing part `k % parts`.
fn instanced_assembly_of(parts: u64, n: u64) -> String {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.))");
    t += &context(30, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += &product(70, "Assembly");
    for p in 0..parts {
        let pd = 1000 + p * 100;
        t += &product(pd, if p == 0 { "Block".to_string() } else { format!("Block{p}") }.as_str());
        let item = if p == 0 { 101 } else { pd + 20 };
        if p > 0 {
            t += &format!("#{item}=MANIFOLD_SOLID_BREP('Cube{p}',#100);\n");
        }
        t += &format!("#{}=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{item},#{}),#20);\n", pd + 10, pd + 15);
        t += &format!("#{}=SHAPE_DEFINITION_REPRESENTATION(#{},#{});\n", pd + 11, pd + 4, pd + 10);
        t += &format!("#{}=AXIS2_PLACEMENT_3D('',#{},$,$);\n#{}=CARTESIAN_POINT('',(0.,0.,0.));\n", pd + 15, pd + 16, pd + 16);
    }
    let items: Vec<String> = (0..n).map(|k| format!("#{}", 3000 + k * 10)).collect();
    t += &format!("#80=SHAPE_REPRESENTATION('',({}),#30);\n#88=SHAPE_DEFINITION_REPRESENTATION(#74,#80);\n", items.join(","));
    for k in 0..n {
        let (a, turn, pd) = (3000 + k * 10, k as f64 * 0.7, 1000 + (k % parts) * 100);
        t += &format!("#{a}=AXIS2_PLACEMENT_3D('',#{},#{},#{});\n", a + 1, a + 2, a + 3);
        t += &format!("#{}=CARTESIAN_POINT('',({}.,0.,0.));\n", a + 1, 50 * k);
        t += &format!("#{}=DIRECTION('',(0.,0.,1.));\n#{}=DIRECTION('',({},{},0.));\n", a + 2, a + 3, turn.cos(), turn.sin());
        let u = 6000 + k * 10;
        t += &format!("#{u}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('{k}','Block:{k}','',#70,#{pd},$);\n");
        t += &format!("#{}=PRODUCT_DEFINITION_SHAPE('','',#{u});\n", u + 1);
        t += &format!("#{}=ITEM_DEFINED_TRANSFORMATION('','',#{},#{a});\n", u + 2, pd + 15);
        t += &format!(
            "#{}=(REPRESENTATION_RELATIONSHIP('','',#{},#80)REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{})SHAPE_REPRESENTATION_RELATIONSHIP());\n",
            u + 3,
            pd + 10,
            u + 2
        );
        t += &format!("#{}=CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{},#{});\n", u + 4, u + 3, u + 1);
    }
    t += TAIL;
    t
}

/// A part placed many times is oriented (meshed whole) once, not once per instance (#2814:
/// an assembly with hundreds of instances spent most of an hour re-meshing the same parts),
/// and every instance still comes out outward-facing at its own place.
#[test]
fn instanced_parts_are_oriented_once() {
    let n = 24;
    let t = instanced_assembly(n);
    let imp = step_import_with_workers(&t, 1).unwrap();
    assert_eq!(imp.bodies.len(), n as usize);
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    for (k, b) in imp.bodies.iter().enumerate() {
        let m = measure(&b.body).unwrap();
        assert!(rel(m.volume, 1000.0) < 1e-9, "instance {k}: {}", m.volume);
        let centre = (m.bbox.min + m.bbox.max) * 0.5;
        let turn = k as f64 * 0.7;
        let expected = Vec3::new(50.0 * k as f64 + 5.0 * (turn.cos() - turn.sin()), 5.0 * (turn.sin() + turn.cos()), 5.0);
        assert!(centre.dist(expected) < 1e-6, "instance {k}: {centre:?} vs {expected:?}");
    }
    assert_same_import(&imp, &step_import_with_workers(&t, 8).unwrap());
}

/// Distinct parts are oriented together, not one occurrence at a time: an assembly placing
/// two parts in turn converts in one step (both parts' orientations found side by side on the
/// workers, every other instance placed after its part's first), where converting instance by
/// instance took one step each, and stopping the step at a part's second instance two.
#[test]
fn distinct_parts_convert_together_across_instances() {
    let n = 24;
    let t = instanced_assembly_of(2, n);
    let mut reading = StepReading::with_workers(&t, 8).unwrap();
    let mut steps = 0;
    while reading.step() {
        steps += 1;
    }
    assert_eq!(steps, 1, "both parts and every other instance in one step");
    let threaded = reading.finish().unwrap();
    assert_eq!(threaded.bodies.len(), n as usize);
    assert!(threaded.warnings.is_empty(), "{:?}", threaded.warnings);
    let serial = step_import_with_workers(&t, 1).unwrap();
    assert_same_import(&serial, &threaded);
    assert_eq!(threaded.bodies[1].name, "Cube1");
    assert_eq!(threaded.bodies[1].path, vec!["Assembly".to_string(), "Block1".to_string()]);
}

/// A part an assembly places twice is meshed once: the second instance shows the first one's
/// display mesh, placed (the same triangles and faces, at its own place), and keeps its own
/// placed geometry for everything else.
#[test]
fn later_instances_show_the_first_instances_mesh_placed() {
    let imp = step_import(&two_instance_assembly()).unwrap();
    let (first, second) = (&imp.bodies[0].body, &imp.bodies[1].body);
    // The first instance shows the shared mesh as well (meshing its own would mesh the part twice).
    let (Some(a), Some(b)) = (first.display_of.as_deref(), second.display_of.as_deref()) else { panic!("both instances share a display") };
    assert!(std::sync::Arc::ptr_eq(&a.0, &b.0));
    let a = first.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap();
    let b = second.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap();
    assert_eq!(a.triangles, b.triangles);
    assert_eq!(a.tri_face, b.tri_face);
    assert_eq!(a.edges.len(), b.edges.len());
    // Turned 90 degrees about Z and moved 100 mm along X.
    for (p, q) in a.positions.iter().zip(&b.positions) {
        assert!(q.dist(Vec3::new(100.0 - p.y, p.x, p.z)) < 1e-9, "{p:?} -> {q:?}");
    }
    let own = measure(second).unwrap().bbox;
    let shown = b.bounds();
    assert!(shown.min.dist(own.min) < 1e-6 && shown.max.dist(own.max) < 1e-6, "{shown:?} vs {own:?}");
    // Anything that changes what the body shows drops the shared mesh.
    let split = second.clone().with_split_lines(Some(std::sync::Arc::new(vec![vec![Vec3::new(91.0, 0.0, 5.0), Vec3::new(91.0, 10.0, 5.0)]])));
    assert!(split.display_of.is_none());
}

/// A body knows the product that holds it and the assembly occurrence it is, next to the name of
/// its solid: a caller can name bodies after the part or the instance when the solids carry the
/// name of the last feature that made them (`Cut-Extrude3`), which says nothing about the part.
#[test]
fn bodies_know_their_product_and_instance() {
    use crate::step_out::{ExportBody, ExportProduct, StepHeader, step_export_products};
    let plate = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 2.0)).unwrap();
    let post = box_solid(Vec3::ZERO, Vec3::new(2.0, 2.0, 8.0)).unwrap();
    let at = |x: f64| [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, 0.0, 0.0, 1.0]];
    let part = |name: &str, label: &str, body| ExportProduct {
        name: name.into(),
        bodies: vec![ExportBody { name: label.into(), body, color: None }],
        children: Vec::new(),
    };
    let products = vec![
        ExportProduct {
            name: "Frame".into(),
            bodies: Vec::new(),
            children: vec![(1, at(0.0), "Plate-1".into()), (2, at(20.0), "Post-1".into()), (2, at(30.0), "Post-2".into())],
        },
        part("Plate", "Cut-Extrude3", &plate),
        part("Post", "Boss-Extrude1", &post),
    ];
    let text = step_export_products(&products, 0, &StepHeader::default()).unwrap();
    let imp = step_import(&text).unwrap();
    let seen: Vec<(&str, Vec<&str>, Option<&str>)> =
        imp.bodies.iter().map(|b| (b.name.as_str(), b.path.iter().map(String::as_str).collect(), b.instance.as_deref())).collect();
    assert_eq!(
        seen,
        [
            ("Cut-Extrude3", vec!["Frame", "Plate"], Some("Plate-1")),
            ("Boss-Extrude1", vec!["Frame", "Post"], Some("Post-1")),
            ("Boss-Extrude1", vec!["Frame", "Post"], Some("Post-2")),
        ]
    );
    // The same product's two placements are two occurrences, told apart by their names.
    assert_eq!(imp.tree[0].children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Plate", "Post", "Post"]);
}

/// The import keeps a conversion only where the file can ask for the item again. A flat file of
/// separate solids converts each item exactly once (no second copy of the whole import is held);
/// an assembly that instances one item keeps it, and each instance still gets geometry of its
/// own. Both files import to the same bodies either way.
#[test]
fn repeated_items_are_shared_and_one_use_items_are_not() {
    let a = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let b = box_solid(Vec3::new(20.0, 0.0, 0.0), Vec3::new(30.0, 10.0, 10.0)).unwrap();
    let flat = step_export(&[&a, &b], "test").unwrap();
    let ex = std::sync::Arc::new(p21::parse(&flat).unwrap());
    let mut rd = Reader::new(ex.clone(), 1);
    rd.index();
    assert_eq!(rd.once.len(), 2, "each solid of a flat file is converted once");
    let imp = step_import(&flat).unwrap();
    assert_eq!(imp.bodies.len(), 2);
    assert!(imp.bodies.iter().all(|b| b.closed));
    let vol: f64 = imp.bodies.iter().map(|b| measure(&b.body).unwrap().volume).sum();
    assert!(rel(vol, 2000.0) < 1e-6, "{vol}");

    let assembly = two_instance_assembly();
    let ex = std::sync::Arc::new(p21::parse(&assembly).unwrap());
    let mut rd = Reader::new(ex.clone(), 1);
    rd.index();
    assert!(rd.once.is_empty(), "an assembly may convert an item again for the second instance");
    let imp = step_import(&assembly).unwrap();
    assert_eq!(imp.bodies.len(), 2);
    assert!(imp.bodies.iter().all(|b| b.name == "Cube" && b.closed));
    let boxes: Vec<_> = imp.bodies.iter().map(|b| measure(&b.body).unwrap()).collect();
    for m in &boxes {
        assert!(rel(m.volume, 1000.0) < 1e-9, "{}", m.volume);
    }
    // Each instance owns its placed geometry: the two bodies are not one shared solid.
    assert!(!imp.bodies[0].body.same(&imp.bodies[1].body));
    // The two instances sit at their own placements, so the second conversion was not skipped.
    assert!(boxes[1].bbox.min.dist(Vec3::new(90.0, 0.0, 0.0)) < 1e-6, "{:?}", boxes[1].bbox);
}

/// A face on a surface the reader does not know is left out and named; the rest of the solid
/// comes in as a surface body of the faces that read, with no "open body" warning on top (as
/// `a_refused_solid_comes_in_as_a_surface_body_of_the_faces_that_read`).
#[test]
fn unsupported_face_is_left_out_and_reported() {
    let mut t = String::from(HEAD);
    let cube = cube_entities().replace("#410=PLANE('',#409);", "#410=CURVE_BOUNDED_SURFACE('',#409,(),.F.);");
    t += &cube;
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n";
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert_eq!(imp.bodies.len(), 1);
    assert!(!imp.bodies[0].closed);
    assert_eq!(imp.bodies[0].body.face_count(), 5);
    assert!(imp.bodies[0].body.is_surface() && !imp.bodies[0].body.is_mesh());
    let unsupported = "face #411: not supported yet: surface CURVE_BOUNDED_SURFACE";
    assert!(imp.warnings.iter().any(|w| w.contains("'Cube': imported as surfaces: ") && w.contains(unsupported)), "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("'Cube': left out: ") && w.contains(unsupported)), "{:?}", imp.warnings);
    assert!(!imp.warnings.iter().any(|w| w.contains("open body")), "{:?}", imp.warnings);
}

#[test]
fn hostile_files_never_panic() {
    let good = step_export(&[&box_solid(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0)).unwrap()], "t").unwrap();
    let mut cases: Vec<String> = vec![
        String::new(),
        "ISO-10303-21;".into(),
        format!("{HEAD}{TAIL}"),
        format!("{HEAD}#1=MANIFOLD_SOLID_BREP('',#1);\n#2=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#1),#3);\n{TAIL}"),
        format!(
            "{HEAD}#1=MANIFOLD_SOLID_BREP('',#2);\n#2=CLOSED_SHELL('',(#3));\n#3=ADVANCED_FACE('',(#4),#5,.T.);\n#4=FACE_BOUND('',#6,.T.);\n#6=EDGE_LOOP('',(#7));\n#7=ORIENTED_EDGE('',*,*,#7,.T.);\n#5=PLANE('',#5);\n#9=SHAPE_REPRESENTATION('',(#1),#10);\n{TAIL}"
        ),
        format!(
            "{HEAD}#1=CARTESIAN_POINT('',(1.E308,1.E308,1.E308,1.,2.));\n#2=MANIFOLD_SOLID_BREP('',#1);\n#3=SHAPE_REPRESENTATION('',(#2),$);\n{TAIL}"
        ),
        format!(
            "{HEAD}#1=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#2,#2,$);\n#2=PRODUCT_DEFINITION('','',#3,#3);\n#3=PRODUCT_DEFINITION_FORMATION('','',#3);\n{TAIL}"
        ),
    ];
    // Truncations and byte flips of a real file.
    for cut in (0..good.len()).step_by(97) {
        cases.push(good.get(..cut).unwrap_or("").to_string());
    }
    for (i, from, to) in [(0, "1.0", "-1.0"), (1, "#1", "#9999"), (2, ".T.", ".F."), (3, "(0.0", "(nan"), (4, "B_SPLINE", "X_SPLINE")] {
        let _ = i;
        cases.push(good.replacen(from, to, 3));
    }
    cases.push(good.replace("CLOSED_SHELL", "OPEN_SHELL"));
    for c in &cases {
        let r = std::panic::catch_unwind(|| step_import(c));
        assert!(r.is_ok(), "panicked on {:?}", c.get(..c.len().min(200)));
    }
}

#[test]
fn assembly_cycles_are_bounded() {
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += &product(50, "A");
    t += &product(70, "B");
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#54,#60);\n";
    // A contains B contains A.
    t += "#90=NEXT_ASSEMBLY_USAGE_OCCURRENCE('1','','',#50,#70,$);\n#91=NEXT_ASSEMBLY_USAGE_OCCURRENCE('2','','',#70,#50,$);\n";
    t += TAIL;
    let r = step_import(&t);
    // Every product is somebody's component: the solid is still imported (outside the tree).
    let imp = r.unwrap();
    assert_eq!(imp.bodies.len(), 1);
}

#[test]
fn exponential_assemblies_are_bounded() {
    // P0 uses P1 twice, P1 uses P2 twice, … 30 levels: 2^30 occurrences if followed blindly.
    let mut t = String::from(HEAD);
    t += "#8=PRODUCT_DEFINITION_CONTEXT('part definition',#9,'design');\n#9=APPLICATION_CONTEXT('');\n";
    t += &cube_entities();
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let levels = 30u64;
    for l in 0..=levels {
        t += &product(1000 + l * 10, &format!("P{l}"));
        if l < levels {
            for k in 0..2 {
                t += &format!("#{}=NEXT_ASSEMBLY_USAGE_OCCURRENCE('','','',#{},#{},$);\n", 2000 + l * 10 + k, 1000 + l * 10, 1000 + (l + 1) * 10);
            }
        }
    }
    t += &format!("#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n#61=SHAPE_DEFINITION_REPRESENTATION(#{},#60);\n", 1000 + levels * 10 + 4);
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(!imp.bodies.is_empty() && imp.bodies.len() <= MAX_BODIES);
    assert!(imp.warnings.iter().any(|w| w.contains("left out")), "{:?}", imp.warnings);
}

#[test]
fn validator_catches_broken_files() {
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n{TAIL}")).is_ok());
    assert!(
        step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n#3=C();\n{TAIL}")).unwrap_err().contains("not referenced")
    );
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#2);\n#2=B();\n#2=C();\n{TAIL}")).unwrap_err().contains("duplicate"));
    assert!(step_validate(&format!("{HEAD}#1=A((#7));\n{TAIL}")).unwrap_err().contains("undefined"));
    assert!(step_validate(&format!("{HEAD}#1=SHAPE_DEFINITION_REPRESENTATION(#5);\n#5=B();\n{TAIL}")).unwrap_err().contains("dense"));
}

/// Revolved faces of a mirrored tool keep their orientation through STEP (the writer used to
/// drop an inverted or mirrored revolution's sense).
#[test]
fn mirrored_revolved_faces_round_trip() {
    for mirror in [false, true] {
        let pl = Plane { origin: Vec3::new(20.0, 15.0, 0.0), ..Plane::XZ };
        let prof =
            Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 4.0), Vec2::new(3.0, 5.0), Vec2::new(3.0, 12.0), Vec2::new(0.0, 12.0)]), holes: vec![] };
        let mut tool = revolve(&pl, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
        if mirror {
            tool =
                crate::transform_matrix(&tool, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0], [0.0, 0.0, 16.0, 1.0]]).unwrap();
        }
        let plate = box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 10.0)).unwrap();
        round_trip(&boolean(&plate, &tool, BoolOp::Cut).unwrap().unwrap());
    }
}

/// Faces whose boundaries are nowhere near their surfaces, or wild B-spline surfaces, mesh
/// within bounded work (the mesher once looped for ever on such input) and are reported if
/// they cannot be meshed.
#[test]
fn hostile_boundaries_mesh_in_bounded_time() {
    let t0 = std::time::Instant::now();
    // A wild 12 × 12 B-spline patch (alternating ±50 mm) bounded by four straight edges that
    // lie far from it, plus a plane face bounded by a circle 500 mm off the plane. The bound
    // catches a hang, not a slow machine (it took ~50 s at 24 × 24 with the machine loaded).
    let n = 12;
    let mut t = String::from(HEAD);
    let mut rows = Vec::new();
    for i in 0..n {
        let mut row = Vec::new();
        for j in 0..n {
            let id = 1000 + i * n + j;
            let z = if (i * 7 + j * 13) % 2 == 0 { 50.0 } else { -50.0 };
            t += &format!("#{id}=CARTESIAN_POINT('',({}.,{}.,{z}));\n", i * 10, j * 10);
            row.push(format!("#{id}"));
        }
        rows.push(format!("({})", row.join(",")));
    }
    let knots = |m: usize| {
        let mut mult = vec!["4".to_string()];
        mult.extend(std::iter::repeat_n("1".to_string(), m - 4));
        mult.push("4".into());
        let ks: Vec<String> = (0..=(m - 3)).map(|k| format!("{k}.")).collect();
        (mult.join(","), ks.join(","))
    };
    let (mu, ku) = knots(n);
    t += &format!(
        "#10=B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,({mu}),({mu}),({ku}),({ku}),.UNSPECIFIED.);\n",
        rows.join(",")
    );
    let corners = [(0, 0), (110, 0), (110, 110), (0, 110)];
    for (k, (x, y)) in corners.iter().enumerate() {
        t += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,300.));\n#{}=VERTEX_POINT('',#{});\n", 20 + k, 30 + k, 20 + k);
    }
    t += "#40=DIRECTION('',(1.,0.,0.));\n#41=VECTOR('',#40,1.);\n";
    for k in 0..4 {
        t += &format!(
            "#{}=LINE('',#{},#41);\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            50 + k,
            20 + k,
            60 + k,
            30 + k,
            30 + (k + 1) % 4,
            50 + k,
            70 + k,
            60 + k
        );
    }
    t += "#80=EDGE_LOOP('',(#70,#71,#72,#73));\n#81=FACE_OUTER_BOUND('',#80,.T.);\n#82=ADVANCED_FACE('',(#81),#10,.T.);\n";
    t += "#90=CARTESIAN_POINT('',(0.,0.,0.));\n#91=DIRECTION('',(0.,0.,1.));\n#92=AXIS2_PLACEMENT_3D('',#90,#91,#40);\n#93=PLANE('',#92);\n";
    t += "#94=CARTESIAN_POINT('',(0.,0.,500.));\n#95=AXIS2_PLACEMENT_3D('',#94,#91,#40);\n#96=CIRCLE('',#95,30.);\n#97=CARTESIAN_POINT('',(30.,0.,500.));\n#98=VERTEX_POINT('',#97);\n";
    t += "#99=EDGE_CURVE('',#98,#98,#96,.T.);\n#100=ORIENTED_EDGE('',*,*,#99,.T.);\n#101=EDGE_LOOP('',(#100));\n#102=FACE_OUTER_BOUND('',#101,.T.);\n#103=ADVANCED_FACE('',(#102),#93,.T.);\n";
    t += "#110=OPEN_SHELL('',(#82,#103));\n#111=SHELL_BASED_SURFACE_MODEL('Wild',(#110));\n#112=SHAPE_REPRESENTATION('',(#111),$);\n";
    t += TAIL;
    let r = std::panic::catch_unwind(|| step_import(&t));
    assert!(r.is_ok(), "panicked");
    if let Ok(Ok(imp)) = r {
        for b in &imp.bodies {
            let _ = measure(&b.body);
        }
    }
    assert!(t0.elapsed().as_secs_f64() < 180.0, "took {:?}", t0.elapsed());
}

/// A cylinder written as a surface of revolution, oriented the ISO 10303-42 way (normal
/// ∂S/∂angle × ∂S/∂profile, outward here, so `same_sense` .T.). Strict readers read such faces this
/// way; reading them with the profile's parameter first turned faces inside out.
#[test]
fn surface_of_revolution_sense_follows_iso() {
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS1_PLACEMENT('',#1,#2);\n";
    t += "#5=CARTESIAN_POINT('',(5.,0.,0.));\n#6=VECTOR('',#2,1.);\n#7=LINE('',#5,#6);\n#8=SURFACE_OF_REVOLUTION('',#7,#4);\n";
    t += "#10=CARTESIAN_POINT('',(5.,0.,10.));\n#11=VERTEX_POINT('',#5);\n#12=VERTEX_POINT('',#10);\n";
    t += "#13=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#14=CIRCLE('',#13,5.);\n#15=CARTESIAN_POINT('',(0.,0.,10.));\n#16=AXIS2_PLACEMENT_3D('',#15,#2,#3);\n#17=CIRCLE('',#16,5.);\n";
    t += "#20=EDGE_CURVE('',#11,#11,#14,.T.);\n#21=EDGE_CURVE('',#12,#12,#17,.T.);\n#22=EDGE_CURVE('',#11,#12,#7,.T.);\n";
    t += "#30=ORIENTED_EDGE('',*,*,#20,.T.);\n#31=ORIENTED_EDGE('',*,*,#22,.T.);\n#32=ORIENTED_EDGE('',*,*,#21,.F.);\n#33=ORIENTED_EDGE('',*,*,#22,.F.);\n";
    t += "#34=EDGE_LOOP('',(#30,#31,#32,#33));\n#35=FACE_OUTER_BOUND('',#34,.T.);\n#36=ADVANCED_FACE('',(#35),#8,.T.);\n";
    t += "#40=PLANE('',#13);\n#41=ORIENTED_EDGE('',*,*,#20,.F.);\n#42=EDGE_LOOP('',(#41));\n#43=FACE_OUTER_BOUND('',#42,.T.);\n#44=ADVANCED_FACE('',(#43),#40,.F.);\n";
    t += "#50=PLANE('',#16);\n#51=ORIENTED_EDGE('',*,*,#21,.T.);\n#52=EDGE_LOOP('',(#51));\n#53=FACE_OUTER_BOUND('',#52,.T.);\n#54=ADVANCED_FACE('',(#53),#50,.T.);\n";
    t += "#60=CLOSED_SHELL('',(#36,#44,#54));\n#61=MANIFOLD_SOLID_BREP('Cyl',#60);\n#62=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#61),#70);\n";
    t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    assert_eq!(step_orientation_errors(&t).unwrap(), Vec::<String>::new());
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    let want = std::f64::consts::PI * 25.0 * 10.0;
    assert!(rel(m.volume, want) < 1e-3, "{} vs {want}", m.volume);
    // Read into schema 4 (a placed revolution), the same cylinder, outward.
    let v = schema_4_volume(&t);
    assert!(rel(v, want) < 1e-3, "schema 4: {v} vs {want}");
    // The same face with the opposite sense is inside out.
    let flipped = t.replace("#36=ADVANCED_FACE('',(#35),#8,.T.);", "#36=ADVANCED_FACE('',(#35),#8,.F.);");
    let bad = step_import(&flipped).unwrap();
    let v = measure(&bad.bodies[0].body).map(|m| m.volume).unwrap_or(0.0);
    assert!(!bad.warnings.is_empty() || rel(v, want) > 1e-2, "{v}");
}

/// A tube revolved about a line it touches (a horn torus, as a tight sweep makes) is written as
/// one rational B-spline face whose loop passes the pole twice, with no zero-length edge there:
/// no TOROIDAL_SURFACE (it would be degenerate) and no surface of revolution. (Its volume, built
/// and read back, is the rig's import/cov-step-horn-torus-quarter-round-trip.)
#[test]
fn horn_torus_written_as_one_spline_face() {
    let prof = Region2 { outer: Loop2::circle(Vec2::new(5.0, 0.0), 5.0), holes: vec![] };
    let b = revolve(&Plane::XZ, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::FRAC_PI_2).unwrap().pop().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    assert!(!text.contains("TOROIDAL_SURFACE") && !text.contains("SURFACE_OF_REVOLUTION"));
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 3, "{text}");
}

/// How a torus-capped cylinder ([`torus_capped_cylinder`]) ends.
enum Tip {
    /// The tube's quarter from its outside over to its top, at `z`, and a disc there of radius `r`.
    Disc { z: f64, r: f64 },
    /// The tube on to where it meets the axis: the pole, a vertex at height `apex` on the axis
    /// reached by the tube's meridian as the torus face's seam, or (`seam` false) not, its one
    /// bound the circle at z = 0.
    Pole { apex: f64, seam: bool },
}

/// A cylinder about Z of radius `major + minor` from z = -2 to 0, its top a torus about Z with
/// those radii (`torus`: the surface entity at #5 on placement #4) ending as `tip` says.
fn torus_capped_cylinder(torus: &str, major: f64, minor: f64, tip: Tip) -> String {
    let outer = major + minor;
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
    t += &format!("{torus}\n#6=CYLINDRICAL_SURFACE('',#4,{outer:.16});\n");
    t += "#7=CARTESIAN_POINT('',(0.,0.,-2.));\n#8=AXIS2_PLACEMENT_3D('',#7,#2,#3);\n#9=PLANE('',#8);\n";
    t += &format!(
        "#13=CARTESIAN_POINT('',({outer:.16},0.,-2.));\n#14=VERTEX_POINT('',#13);\n#15=CARTESIAN_POINT('',({outer:.16},0.,0.));\n#16=VERTEX_POINT('',#15);\n"
    );
    t += &format!(
        "#19=CIRCLE('',#8,{outer:.16});\n#20=EDGE_CURVE('',#14,#14,#19,.T.);\n#21=CIRCLE('',#4,{outer:.16});\n#22=EDGE_CURVE('',#16,#16,#21,.T.);\n"
    );
    t += "#25=VECTOR('',#2,1.);\n#26=LINE('',#13,#25);\n#27=EDGE_CURVE('',#14,#16,#26,.T.);\n";
    t += "#40=ORIENTED_EDGE('',*,*,#20,.F.);\n#41=EDGE_LOOP('',(#40));\n#42=FACE_OUTER_BOUND('',#41,.T.);\n#43=ADVANCED_FACE('',(#42),#9,.F.);\n";
    t += "#44=ORIENTED_EDGE('',*,*,#20,.T.);\n#45=ORIENTED_EDGE('',*,*,#27,.T.);\n#46=ORIENTED_EDGE('',*,*,#22,.F.);\n#47=ORIENTED_EDGE('',*,*,#27,.F.);\n";
    t += "#48=EDGE_LOOP('',(#44,#45,#46,#47));\n#49=FACE_OUTER_BOUND('',#48,.T.);\n#50=ADVANCED_FACE('',(#49),#6,.T.);\n#51=ORIENTED_EDGE('',*,*,#22,.T.);\n";
    // The tube's meridian at u = 0 from (outer, 0, 0) to `end`, the torus face's seam: centre
    // (major, 0, 0), angle from X towards Z (its axis -Y).
    let meridian = |t: &mut String, end: [f64; 2]| {
        *t += &format!("#17=CARTESIAN_POINT('',({:.16},0.,{:.16}));\n#18=VERTEX_POINT('',#17);\n", end[0], end[1]);
        *t += &format!(
            "#28=CARTESIAN_POINT('',({major:.16},0.,0.));\n#29=DIRECTION('',(0.,-1.,0.));\n#30=AXIS2_PLACEMENT_3D('',#28,#29,#3);\n#31=CIRCLE('',#30,{minor:.16});\n#32=EDGE_CURVE('',#16,#18,#31,.T.);\n"
        );
        *t += "#52=ORIENTED_EDGE('',*,*,#32,.T.);\n#54=ORIENTED_EDGE('',*,*,#32,.F.);\n";
    };
    let faces = match tip {
        Tip::Disc { z, r } => {
            meridian(&mut t, [r, z]);
            t += &format!("#10=CARTESIAN_POINT('',(0.,0.,{z:.16}));\n#11=AXIS2_PLACEMENT_3D('',#10,#2,#3);\n#12=PLANE('',#11);\n");
            t += &format!("#23=CIRCLE('',#11,{r:.16});\n#24=EDGE_CURVE('',#18,#18,#23,.T.);\n#53=ORIENTED_EDGE('',*,*,#24,.F.);\n");
            t += "#55=EDGE_LOOP('',(#51,#52,#53,#54));\n#56=FACE_OUTER_BOUND('',#55,.T.);\n#57=ADVANCED_FACE('',(#56),#5,.T.);\n";
            t += "#58=ORIENTED_EDGE('',*,*,#24,.T.);\n#59=EDGE_LOOP('',(#58));\n#60=FACE_OUTER_BOUND('',#59,.T.);\n#61=ADVANCED_FACE('',(#60),#12,.T.);\n";
            "#43,#50,#57,#61"
        }
        Tip::Pole { apex, seam } => {
            if seam {
                meridian(&mut t, [0.0, apex]);
                t += "#55=EDGE_LOOP('',(#51,#52,#54));\n";
            } else {
                t += "#55=EDGE_LOOP('',(#51));\n";
            }
            t += "#56=FACE_OUTER_BOUND('',#55,.T.);\n#57=ADVANCED_FACE('',(#56),#5,.T.);\n";
            "#43,#50,#57"
        }
    };
    t += &format!("#62=CLOSED_SHELL('',({faces}));\n#63=MANIFOLD_SOLID_BREP('Pin',#62);\n#64=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#63),#70);\n");
    t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    t
}

/// The one closed body of `text`, its volume and area against the closed forms; `warnings`
/// the import's warnings.
fn assert_closed_measures(text: &str, volume: f64, area: f64, warnings: &[&str]) {
    let imp = step_import(text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert_eq!(imp.warnings, warnings, "{:?}", imp.warnings);
    assert!(imp.bodies[0].closed);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.volume, volume) < 1e-3, "volume {} vs {volume}", m.volume);
    assert!(rel(m.area, area) < 1e-3, "area {} vs {area}", m.area);
}

/// A horn torus (major radius = minor, its tube touching its axis) as a round of radius 1 on a
/// cylinder of radius 2, from the cylinder's top edge over to a disc of radius 1 at z = 1 (a
/// round pin tip). Under the round, the solid's radius at height z is 1 + √(1 − z²):
/// V = 8π + π∫₀¹(1 + √(1 − z²))² dz = 8π + π(5/3 + π/2); the round's area is
/// 2π∫₀^{π/2}(1 + cos v) dv = π² + 2π, with the cylinder's 8π and the discs' 4π and π.
#[test]
fn horn_torus_round_on_a_pin_imports_whole() {
    use std::f64::consts::PI;
    let t = torus_capped_cylinder("#5=TOROIDAL_SURFACE('',#4,1.,1.);", 1.0, 1.0, Tip::Disc { z: 1.0, r: 1.0 });
    assert_closed_measures(&t, 8.0 * PI + PI * (5.0 / 3.0 + PI / 2.0), 15.0 * PI + PI * PI, &[]);
}

/// The volume and area of a spindle torus's pin end ([`Tip::Pole`]): in the meridian plane the
/// disc of radius `r` about (`major`, 0) above z = 0 and right of the axis, which the tube meets
/// at z = c = √(r² − R²) (cos v = −R/r). With s = √(r² − z²) and S(a) = ∫₀^a s dz =
/// (a√(r² − a²) + r² asin(a/r))/2: V = π∫₀^c (R + s)² dz + π∫_c^r 4Rs dz =
/// π(R²c + r²c − c³/3 + 2R·S(c)) + 4πR(S(r) − S(c)); area 2πr(R·acos(−R/r) + c).
fn pin_end(major: f64, r: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    let c = (r * r - major * major).sqrt();
    let s = |a: f64| 0.5 * (a * (r * r - a * a).max(0.0).sqrt() + r * r * (a / r).asin());
    let volume = PI * (major * major * c + r * r * c - c.powi(3) / 3.0 + 2.0 * major * s(c)) + 4.0 * PI * major * (s(r) - s(c));
    (volume, 2.0 * PI * r * (major * (-major / r).acos() + c))
}

/// A spindle torus (major radius below the minor, its tube across its axis) as a pin's end: the
/// apple's part from the cylinder at z = 0 up over the tube's top and down to where the tube
/// meets the axis, a pole in a dimple ([`pin_end`], with the cylinder's side and its disc).
/// Major 1, minor 2 (the tube meets the axis at z = √3: V = 3√3π + 8π²/3, area
/// 4π(2π/3 + √3)), written as a TOROIDAL_SURFACE (as SolidWorks writes it) and as the schema's
/// DEGENERATE_TOROIDAL_SURFACE, its apple selected; with a seam to the pole, and with the
/// circle at z = 0 its one bound (the reader adds the seam).
#[test]
fn spindle_torus_pin_tip_imports_whole() {
    use std::f64::consts::PI;
    let s3 = 3f64.sqrt();
    let (volume, area) = pin_with_cylinder(1.0, 2.0);
    assert!(rel(volume, 18.0 * PI + 3.0 * s3 * PI + 8.0 * PI * PI / 3.0) < 1e-12 && rel(area, 21.0 * PI + 4.0 * PI * (2.0 * PI / 3.0 + s3)) < 1e-12);
    for torus in ["#5=TOROIDAL_SURFACE('',#4,1.,2.);", "#5=DEGENERATE_TOROIDAL_SURFACE('',#4,1.,2.,.T.);"] {
        for (seam, warnings) in [(true, &[][..]), (false, &["solid 'Pin': 1 seam edges added to faces that wind around their surface"][..])] {
            let t = torus_capped_cylinder(torus, 1.0, 2.0, Tip::Pole { apex: s3, seam });
            assert_closed_measures(&t, volume, area, warnings);
        }
    }
}

/// [`pin_end`] on its cylinder from z = -2 ([`torus_capped_cylinder`]): its volume and area.
fn pin_with_cylinder(major: f64, minor: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    let (v, a) = pin_end(major, minor);
    let outer = major + minor;
    (v + 2.0 * PI * outer * outer, a + 4.0 * PI * outer + PI * outer * outer)
}

/// A nearly round pin end as SolidWorks writes it: a spindle torus of major radius 0.005 and
/// minor 0.345 on a cylinder of radius 0.35, its apple's part in two halves (u in [0, π] and
/// [π, 2π]) between the meridians at u = 0 and π, which meet at an apex vertex written at the
/// tube's top, z = 0.345: 3.6e-5 mm above the pole (z = √(0.345² − 0.005²)), in a file of
/// uncertainty 1e-5 mm. The meridians end 1.7e-6 rad short of the pole, where the tube's
/// circle about the axis is 1.2e-6 mm across: a pole to within the uncertainty, so each half's
/// loop jumps from one meridian to the other there. Volume and area as [`pin_end`].
#[test]
fn nearly_round_spindle_torus_pin_end_in_halves_imports_whole() {
    let (major, minor) = (0.005, 0.345);
    let outer = major + minor;
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
    // The torus's angle from 45° below X: the halves are u in [π/4, 5π/4] and [5π/4, 9π/4] on
    // it, three quarters of its NURBS form each, the meridians two apart.
    t += "#88=DIRECTION('',(0.7071067811865476,-0.7071067811865476,0.));\n#89=AXIS2_PLACEMENT_3D('',#1,#2,#88);\n";
    t += &format!("#5=TOROIDAL_SURFACE('',#89,{major:.16},{minor:.16});\n#6=CYLINDRICAL_SURFACE('',#4,{outer:.16});\n");
    t += "#7=CARTESIAN_POINT('',(0.,0.,-2.));\n#8=AXIS2_PLACEMENT_3D('',#7,#2,#3);\n#9=PLANE('',#8);\n";
    t += &format!(
        "#13=CARTESIAN_POINT('',({outer:.16},0.,-2.));\n#14=VERTEX_POINT('',#13);\n#15=CARTESIAN_POINT('',({outer:.16},0.,0.));\n#16=VERTEX_POINT('',#15);\n"
    );
    t += &format!(
        "#65=CARTESIAN_POINT('',({:.16},0.,0.));\n#66=VERTEX_POINT('',#65);\n#17=CARTESIAN_POINT('',(0.,0.,{minor:.16}));\n#18=VERTEX_POINT('',#17);\n",
        -outer
    );
    t += &format!("#19=CIRCLE('',#8,{outer:.16});\n#20=EDGE_CURVE('',#14,#14,#19,.T.);\n#21=CIRCLE('',#4,{outer:.16});\n");
    t += "#22=EDGE_CURVE('',#16,#66,#21,.T.);\n#67=EDGE_CURVE('',#66,#16,#21,.T.);\n#25=VECTOR('',#2,1.);\n#26=LINE('',#13,#25);\n#27=EDGE_CURVE('',#14,#16,#26,.T.);\n";
    // The meridians, each angle from -Z: at u = 0 about (major, 0, 0) towards X (axis -Y); at
    // u = π about (-major, 0, 0) towards -X (axis +Y).
    t += "#82=DIRECTION('',(0.,0.,-1.));\n";
    t += &format!(
        "#28=CARTESIAN_POINT('',({major:.16},0.,0.));\n#29=DIRECTION('',(0.,-1.,0.));\n#30=AXIS2_PLACEMENT_3D('',#28,#29,#82);\n#31=CIRCLE('',#30,{minor:.16});\n#32=EDGE_CURVE('',#16,#18,#31,.T.);\n"
    );
    t += &format!(
        "#80=CARTESIAN_POINT('',({:.16},0.,0.));\n#81=DIRECTION('',(0.,1.,0.));\n#83=AXIS2_PLACEMENT_3D('',#80,#81,#82);\n#84=CIRCLE('',#83,{minor:.16});\n#85=EDGE_CURVE('',#66,#18,#84,.T.);\n",
        -major
    );
    t += "#40=ORIENTED_EDGE('',*,*,#20,.F.);\n#41=EDGE_LOOP('',(#40));\n#42=FACE_OUTER_BOUND('',#41,.T.);\n#43=ADVANCED_FACE('',(#42),#9,.F.);\n";
    t += "#44=ORIENTED_EDGE('',*,*,#20,.T.);\n#45=ORIENTED_EDGE('',*,*,#27,.T.);\n#46=ORIENTED_EDGE('',*,*,#67,.F.);\n#47=ORIENTED_EDGE('',*,*,#22,.F.);\n#48=ORIENTED_EDGE('',*,*,#27,.F.);\n";
    t += "#49=EDGE_LOOP('',(#44,#45,#46,#47,#48));\n#50=FACE_OUTER_BOUND('',#49,.T.);\n#51=ADVANCED_FACE('',(#50),#6,.T.);\n";
    t += "#52=ORIENTED_EDGE('',*,*,#22,.T.);\n#53=ORIENTED_EDGE('',*,*,#85,.T.);\n#54=ORIENTED_EDGE('',*,*,#32,.F.);\n#55=EDGE_LOOP('',(#52,#53,#54));\n#56=FACE_OUTER_BOUND('',#55,.T.);\n#57=ADVANCED_FACE('',(#56),#5,.T.);\n";
    t += "#58=ORIENTED_EDGE('',*,*,#67,.T.);\n#59=ORIENTED_EDGE('',*,*,#32,.T.);\n#60=ORIENTED_EDGE('',*,*,#85,.F.);\n#61=EDGE_LOOP('',(#58,#59,#60));\n#62=FACE_OUTER_BOUND('',#61,.T.);\n#63=ADVANCED_FACE('',(#62),#5,.T.);\n";
    t += "#64=CLOSED_SHELL('',(#43,#51,#57,#63));\n#86=MANIFOLD_SOLID_BREP('Pin',#64);\n#87=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#86),#70);\n";
    t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))").replace("LENGTH_MEASURE(1.E-6)", "LENGTH_MEASURE(1.E-5)");
    t += TAIL;
    let (volume, area) = pin_with_cylinder(major, minor);
    assert_closed_measures(&t, volume, area, &[]);
}

/// The lemon of a spindle torus (the tube's part across the axis: a closed surface with a pole
/// at each end), one face bounded by its meridian used both ways, written with a negative major
/// radius (-1, minor 2: the tube's centre on the other side of the axis, as some writers have
/// it) and as the schema's DEGENERATE_TOROIDAL_SURFACE with its lemon selected (major radius 1,
/// where the lemon is the tube's far side, R + r cos v < 0, and the surface's normal
/// ∂S/∂u × ∂S/∂v points into it: `same_sense` .F.). Its radius at height z is √(4 − z²) − 1
/// for |z| ≤ √3: V = π∫(5 − z² − 2√(4 − z²)) dz = π(6√3 − 8π/3); area
/// 2π∫_{−π/3}^{π/3}(2 cos v − 1)·2 dv = 4π(2√3 − 2π/3).
#[test]
fn spindle_torus_lemon_with_a_negative_major_radius_imports_whole() {
    use std::f64::consts::PI;
    let s3 = 3f64.sqrt();
    for (torus, sense) in [("#5=TOROIDAL_SURFACE('',#4,-1.,2.);", "T"), ("#5=DEGENERATE_TOROIDAL_SURFACE('',#4,1.,2.,.F.);", "F")] {
        let mut t = String::from(HEAD);
        t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
        t += torus;
        // The meridian at u = 0: the circle about (-1, 0, 0) from v = -π/3 to π/3, angle from X
        // towards Z (its axis -Y).
        t += &format!(
            "\n#6=CARTESIAN_POINT('',(0.,0.,{:.16}));\n#7=VERTEX_POINT('',#6);\n#8=CARTESIAN_POINT('',(0.,0.,{s3:.16}));\n#9=VERTEX_POINT('',#8);\n",
            -s3
        );
        t += "#10=CARTESIAN_POINT('',(-1.,0.,0.));\n#11=DIRECTION('',(0.,-1.,0.));\n#12=AXIS2_PLACEMENT_3D('',#10,#11,#3);\n#13=CIRCLE('',#12,2.);\n#14=EDGE_CURVE('',#7,#9,#13,.T.);\n";
        t += "#15=ORIENTED_EDGE('',*,*,#14,.T.);\n#16=ORIENTED_EDGE('',*,*,#14,.F.);\n#17=EDGE_LOOP('',(#15,#16));\n#18=FACE_OUTER_BOUND('',#17,.T.);\n";
        t += &format!("#19=ADVANCED_FACE('',(#18),#5,.{sense}.);\n");
        t += "#20=CLOSED_SHELL('',(#19));\n#21=MANIFOLD_SOLID_BREP('Lemon',#20);\n#22=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#21),#70);\n";
        t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
        t += TAIL;
        assert_closed_measures(&t, PI * (6.0 * s3 - 8.0 * PI / 3.0), 4.0 * PI * (2.0 * s3 - 2.0 * PI / 3.0), &[]);
    }
}

/// Booleans and blends build the same body every time, so an export is byte-identical from
/// run to run (an export once listed a boolean's faces in hash order of their addresses). The
/// tee of oracle 35 with its rolling-ball fillet round the junction.
#[test]
fn tee_fillet_exports_byte_identical() {
    let main = cylinder(Vec3::new(-40.0, 0.0, 0.0), Vec3::X, 15.0, 80.0).unwrap();
    let branch = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 35.0).unwrap();
    let export = || {
        let tee = boolean(&main, &branch, BoolOp::Union).unwrap().unwrap();
        let f = fillet(&tee, &[Vec3::new(0.0, 10.0, 125f64.sqrt())], 3.0).unwrap();
        step_export(&[&f], "tee").unwrap()
    };
    let first = export();
    for _ in 0..2 {
        assert!(export() == first, "export differs between runs");
    }
}

/// A plate with a hole glued to its mirror image: the faces split at the mirror plane and the
/// hole's B-spline halves are written as one plane or one cylinder each, as other systems
/// count them.
#[test]
fn glued_halves_export_as_whole_faces() {
    let half = Region2 {
        outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 40.0), Vec2::new(0.0, 40.0)]),
        holes: vec![Loop2::circle(Vec2::new(15.0, 20.0), 4.0).reversed()],
    };
    let right = extrude(&Plane::XY, &[half], 0.0, 10.0).unwrap().pop().unwrap();
    let left = crate::transform_matrix(&right, [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]).unwrap();
    let b = boolean(&right, &left, BoolOp::Union).unwrap().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    assert!(!text.contains("B_SPLINE_SURFACE"), "{text}");
    assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 2);
    let imp = round_trip(&b);
    // Box sides and caps (6) and two holes.
    assert_eq!(imp.bodies[0].file_faces, 8);
}

/// A slot plate with a round on its top outline (issue #612): every surface the file writes is
/// bounded by its face, whatever kind it is written as. The rounds' sheets once reached far past
/// their edges, and a reader bounding the part by its surfaces saw a 60 mm part as 130 mm long.
/// Read back, the part has its exact volume.
#[test]
fn rounded_slot_exports_cylinders() {
    use solidcraft_geom::Seg2;
    use std::f64::consts::{FRAC_PI_2, PI};
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(10.0, -10.0), b: Vec2::new(50.0, -10.0) },
            Seg2::Arc { center: Vec2::new(50.0, 0.0), radius: 10.0, start: -FRAC_PI_2, sweep: PI },
            Seg2::Line { a: Vec2::new(50.0, 10.0), b: Vec2::new(10.0, 10.0) },
            Seg2::Arc { center: Vec2::new(10.0, 0.0), radius: 10.0, start: FRAC_PI_2, sweep: PI },
        ],
    };
    let plate = extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 4.0).unwrap().pop().unwrap();
    let edges = [Vec3::new(30.0, -10.0, 4.0), Vec3::new(30.0, 10.0, 4.0), Vec3::new(0.0, 0.0, 4.0), Vec3::new(60.0, 0.0, 4.0)];
    let b = fillet(&plate, &edges, 1.0).unwrap();
    let text = step_export(&[&b], "t").unwrap();
    // No point of the file (control points and surface placements included) lies outside the part.
    let imp = round_trip(&b);
    for line in text.lines().filter(|l| l.contains("CARTESIAN_POINT")) {
        let Some(xyz) = line.split("((").nth(1).and_then(|r| r.split("))").next()) else { continue };
        let x: f64 = xyz.split(',').next().unwrap().trim().parse().unwrap();
        assert!((-1e-6..=60.0 + 1e-6).contains(&x), "{line}");
    }
    // The volume analytically: the plate less the round's corner section swept along the edges
    // (Pappus: the section's centroid runs 0.2234 r inside the edge round the arcs).
    let section = 1.0 - PI / 4.0;
    let centroid = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let want = (800.0 + PI * 100.0) * 4.0 - section * (80.0 + std::f64::consts::TAU * (10.0 - centroid));
    assert!(rel(measure(&imp.bodies[0].body).unwrap().volume, want) < 1e-3);
}

/// A slot plate rounded along its top and bottom outlines, with a ramp cut across one end
/// (issue #1198): every surface the file writes is cut down to its face, so nothing reaches past
/// the part's ends (the rounds' sheets once reached 35 mm past both, and readers bounding the part
/// by its surfaces saw a 60 mm part as 130 mm long), and the part reads back whole.
#[test]
fn ramp_cut_slot_exports_sheets_bounded_by_their_faces() {
    use solidcraft_geom::Seg2;
    use std::f64::consts::{FRAC_PI_2, PI};
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(10.0, -10.0), b: Vec2::new(50.0, -10.0) },
            Seg2::Arc { center: Vec2::new(50.0, 0.0), radius: 10.0, start: -FRAC_PI_2, sweep: PI },
            Seg2::Line { a: Vec2::new(50.0, 10.0), b: Vec2::new(10.0, 10.0) },
            Seg2::Arc { center: Vec2::new(10.0, 0.0), radius: 10.0, start: FRAC_PI_2, sweep: PI },
        ],
    };
    let plate = extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 4.0).unwrap().pop().unwrap();
    let outline = |z: f64| [Vec3::new(30.0, -10.0, z), Vec3::new(30.0, 10.0, z), Vec3::new(0.0, 0.0, z), Vec3::new(60.0, 0.0, z)];
    let rounded = fillet(&fillet(&plate, &outline(4.0), 1.0).unwrap(), &outline(0.0), 1.0).unwrap();
    // The ramp: from the top at x = 40 down to z = 2 past the end, across the whole width.
    let side = Plane::new(Vec3::new(0.0, -6.0, 0.0), Vec3::X, Vec3::Z).unwrap();
    let ramp =
        Region2 { outer: Loop2::polygon(&[Vec2::new(40.0, 4.0), Vec2::new(62.0, 2.0), Vec2::new(62.0, 6.0), Vec2::new(40.0, 6.0)]), holes: vec![] };
    let tool = extrude(&side, &[ramp], -12.0, 0.0).unwrap().pop().unwrap();
    let b = boolean(&rounded, &tool, BoolOp::Cut).unwrap().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    // No point of the file (control points and placements included) lies past the part's ends.
    for line in text.lines().filter(|l| l.contains("CARTESIAN_POINT")) {
        let Some(xyz) = line.split("((").nth(1).and_then(|r| r.split("))").next()) else { continue };
        let x: f64 = xyz.split(',').next().unwrap().trim().parse().unwrap();
        assert!((-1e-6..=60.0 + 1e-6).contains(&x), "{line}");
    }
    let imp = round_trip(&b);
    assert_eq!(imp.bodies.len(), 1);
    assert!(rel(measure(&imp.bodies[0].body).unwrap().volume, measure(&b).unwrap().volume) < 1e-4);
}

/// A countersink cut into a plate (issue #612): the cone is a tapered extrusion of a disc, a
/// B-spline sheet per half, written as one CONICAL_SURFACE face (the sheets reached below the
/// plate).
#[test]
fn countersink_exports_one_cone() {
    use std::f64::consts::FRAC_PI_4;
    let plate = box_solid(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 4.0)).unwrap();
    // A 90° countersink, radius 4 at the top face, down from 1 mm above it to radius 1.8, and the
    // 2 mm drill through.
    let disc = Region2 { outer: Loop2::circle(Vec2::ZERO, 5.0), holes: vec![] };
    let cone = crate::extrude_tapered(&Plane::XY.offset(5.0), &disc, 3.2, -1.0, -FRAC_PI_4, crate::DraftCorner::default()).unwrap();
    let drill = cylinder(Vec3::new(0.0, 0.0, -1.0), Vec3::Z, 2.0, 6.0).unwrap();
    let b = boolean(&boolean(&plate, &cone, BoolOp::Cut).unwrap().unwrap(), &drill, BoolOp::Cut).unwrap().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    assert!(!text.contains("B_SPLINE_SURFACE"), "{text}");
    assert_eq!(text.matches("CONICAL_SURFACE(").count(), 1, "{text}");
    assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 1, "{text}");
    let imp = round_trip(&b);
    // Six box faces, the countersink and the drill.
    assert_eq!(imp.bodies[0].file_faces, 8);
}

/// Knot multiplicities are checked before anything is expanded: 1,000,000 knots of multiplicity
/// 64 against two control points asked for a 500 MB vector (20,000,000 for 10 GB) only to reject
/// it afterwards, and an allocation failure aborts the process where an error does not. A knot
/// above degree + 1 is refused on its own; knots each within it are refused by their sum.
#[test]
fn knot_multiplicities_are_checked_before_they_are_expanded() {
    let curve = |n: usize, mult: usize| {
        let knots: Vec<String> = (0..n).map(|i| format!("{i}.")).collect();
        let mults: Vec<String> = std::iter::repeat_n(mult.to_string(), n).collect();
        format!(
            "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=CARTESIAN_POINT('',(1.,0.,0.));\n\
             #3=B_SPLINE_CURVE_WITH_KNOTS('',1,(#1,#2),.UNSPECIFIED.,.F.,.F.,({}),({}),.UNSPECIFIED.);\n{TAIL}",
            mults.join(","),
            knots.join(",")
        )
    };
    let read = |text: &str| {
        let ex = p21::parse(text).unwrap();
        solidcraft_step::read::Reader::new(&ex, Default::default()).curve(3).map(|_| ())
    };
    // These diagnostics come from the checks before expansion; wall-clock time depends on CI load.
    let error = read(&curve(1_000_000, 64)).unwrap_err();
    assert!(error.contains("bad knot multiplicity"), "{error}");
    let error = read(&curve(1_000_000, 2)).unwrap_err();
    assert!(error.contains("knot multiplicities do not match"), "{error}");
    // A degree-1 curve with two control points needs exactly four knot values.
    assert!(read(&curve(2, 2)).is_ok());
    assert!(read(&curve(4, 1)).is_ok());
    assert!(read(&curve(2, 3)).is_err());
}

/// One `ADVANCED_FACE` may list any number of bounds, and each one clones its edge handles into
/// a new wire that the face extent then samples. A face naming the same loop a million times
/// held 1e11 handles.
#[test]
fn faces_with_too_many_bounds_are_refused() {
    let good = step_export(&[&box_solid(Vec3::ZERO, Vec3::new(5.0, 5.0, 5.0)).unwrap()], "t").unwrap();
    // One extra face on the cube's shell, listing its outer bound 100,000 times.
    let bounds = std::iter::repeat_n("#900001", 100_000).collect::<Vec<_>>().join(",");
    let mut extra = format!("#900000=ADVANCED_FACE('',({bounds}),#900010,.T.);\n");
    extra += "#900001=FACE_OUTER_BOUND('',#900002,.T.);\n#900002=EDGE_LOOP('',(#900003));\n";
    extra += "#900003=ORIENTED_EDGE('',*,*,#900004,.T.);\n#900004=EDGE_CURVE('',#900005,#900005,#900007,.T.);\n";
    extra += "#900005=VERTEX_POINT('',#900006);\n#900006=CARTESIAN_POINT('',(0.,0.,0.));\n";
    extra += "#900007=LINE('',#900006,#900008);\n#900008=VECTOR('',#900009,1.);\n#900009=DIRECTION('',(1.,0.,0.));\n";
    extra += "#900010=PLANE('',#900011);\n#900011=AXIS2_PLACEMENT_3D('',#900006,#900012,#900009);\n#900012=DIRECTION('',(0.,0.,1.));\n";
    let text =
        good.replacen("CLOSED_SHELL('',(", "CLOSED_SHELL('',(#900000,", 1).replacen("ENDSEC;\nEND-ISO", &format!("{extra}ENDSEC;\nEND-ISO"), 1);

    let start = std::time::Instant::now();
    let imp = step_import(&text).unwrap();
    assert!(imp.warnings.iter().any(|w| w.contains("face #900000 with 100000 bounds")), "{:?}", imp.warnings);
    assert!(start.elapsed() < std::time::Duration::from_secs(10), "took {:?}", start.elapsed());
}

/// Independent boxes as one STEP file (about 3 KB a box).
fn boxes_step(count: usize, name: &str) -> String {
    let bodies: Vec<_> = (0..count)
        .map(|i| {
            let at = Vec3::new((i % 32) as f64 * 12.0, (i / 32) as f64 * 12.0, 0.0);
            box_solid(at, at + Vec3::new(10.0, 10.0, 10.0)).unwrap()
        })
        .collect();
    step_export(&bodies.iter().collect::<Vec<_>>(), name).unwrap()
}

struct TestWatch {
    cancel_after: usize,
    reports: std::sync::atomic::AtomicUsize,
    last: std::sync::Mutex<(&'static str, usize, usize)>,
}

impl crate::watch::Watch for TestWatch {
    fn cancelled(&self) -> bool {
        self.reports.load(std::sync::atomic::Ordering::Relaxed) >= self.cancel_after
    }
    fn progress(&self, stage: &'static str, done: usize, total: usize) {
        self.reports.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *self.last.lock().unwrap() = (stage, done, total);
    }
}

#[test]
fn watched_import_reports_progress_and_stops_on_cancel_without_caching() {
    let text = boxes_step(40, "watched import");
    let watch = Arc::new(TestWatch { cancel_after: usize::MAX, reports: Default::default(), last: std::sync::Mutex::new(("", 0, 0)) });
    let imp = crate::watch::watching(watch.clone(), || step_import(&text)).unwrap();
    assert_eq!(imp.bodies.len(), 40);
    let (stage, done, total) = *watch.last.lock().unwrap();
    assert_eq!((stage, total), ("converting", 40));
    assert!(done <= total && watch.reports.load(std::sync::atomic::Ordering::Relaxed) > 1);

    // Cancelled part way: an error, no partial result, and nothing cached for the text.
    let text = boxes_step(41, "cancelled import");
    let watch = Arc::new(TestWatch { cancel_after: 3, reports: Default::default(), last: std::sync::Mutex::new(("", 0, 0)) });
    let err = crate::watch::watching(watch.clone(), || step_import_shared(&text).map(|_| ())).unwrap_err();
    assert!(err.to_string().contains("cancelled"), "{err}");
    let (_, done, _) = *watch.last.lock().unwrap();
    assert!(done < 41, "stopped before converting everything ({done})");
    // Unwatched, the same text imports in full (no cached failure or partial import).
    assert_eq!(step_import_shared(&text).unwrap().bodies.len(), 41);
    // Without a watch nothing is reported or stopped.
    assert_eq!(step_import(&boxes_step(2, "unwatched")).unwrap().bodies.len(), 2);
}

#[test]
fn large_imports_are_freed_with_their_last_user() {
    let text = boxes_step(400, "large shared import");
    assert!(text.len() > KEEP_SMALL_BYTES, "{} bytes", text.len());
    let first = step_import_shared(&text).unwrap();
    let again = step_import_shared(&text).unwrap();
    assert!(Arc::ptr_eq(&first, &again), "read once while it is in use");
    let weak = Arc::downgrade(&first);
    drop((first, again));
    assert!(weak.upgrade().is_none(), "the cache must not keep a large import alive after its design is gone");
}

/// A three-quarter turn of a rectangle about Z: planar end caps, revolved cylinders and annuli.
fn three_quarter_ring() -> Body {
    let rect =
        Region2 { outer: Loop2::polygon(&[Vec2::new(5.0, 0.0), Vec2::new(25.0, 0.0), Vec2::new(25.0, 5.0), Vec2::new(5.0, 5.0)]), holes: vec![] };
    revolve(&Plane::XZ, &[rect], Vec2::ZERO, Vec2::Y, 1.5 * std::f64::consts::PI).unwrap().pop().unwrap()
}

fn scaled(b: &Body, x: f64, y: f64, z: f64) -> Body {
    crate::transform_matrix(b, [[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0], [0.0, 0.0, 0.0, 1.0]]).unwrap()
}

/// A scale across the revolution axis that differs from the scale along it (how a shell or an
/// offset moves a revolved face) keeps the faces surfaces of revolution. The writer refused it
/// ("non-uniform scale"), and the refusal surfaced as a panic in export (robustness seeds 19, 24).
#[test]
fn revolved_faces_scaled_across_the_axis_round_trip() {
    let ring = three_quarter_ring();
    let v0 = measure(&ring).unwrap().volume;
    let b = scaled(&ring, 1.5, 1.5, 0.5);
    assert!(rel(measure(&b).unwrap().volume, v0 * 1.5 * 1.5 * 0.5) < 1e-3);
    round_trip(&b);
}

/// A transform that does not keep the faces surfaces of revolution: export may refuse the body,
/// but with an error, never a panic (the browser build aborts on any panic), and anything it does
/// write must read back as the same solid.
#[test]
fn revolved_faces_skewed_never_panic_on_export() {
    let b = scaled(&three_quarter_ring(), 1.5, 1.0, 1.0);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step_export(&[&b], "t")));
    let Ok(r) = r else { panic!("STEP export panicked") };
    match r {
        Err(e) => assert!(e.to_string().contains("not supported yet"), "{e}"),
        Ok(_) => {
            round_trip(&b);
        }
    }
}

/// Weights of a rational quadratic full circle through nine control points.
const CIRCLE_WEIGHTS: &str = "1.,0.707106781187,1.,0.707106781187,1.,0.707106781187,1.,0.707106781187,1.";
/// Angle knots as OpenCascade writes a closed (periodic) quadratic circle: the end knots
/// appear once, so the B-spline is defined from knot 2 to knot 9 only, -π to π.
const PERIODIC_MULTS: &str = "1,2,2,2,2,2,1";
const PERIODIC_KNOTS: &str = "-4.712388980385,-3.14159265359,-1.570796326795,0.,1.570796326795,3.14159265359,4.712388980385";

/// Nine control points of a quadratic circle of radius `r` about (`x`, 0, `z`), from angle 0.
fn circle_net(first: u64, x: f64, r: f64, z: f64) -> (String, Vec<String>) {
    let (mut s, mut ids) = (String::new(), Vec::new());
    for i in 0..9u64 {
        let a = i as f64 * std::f64::consts::FRAC_PI_4;
        let q = if i % 2 == 1 { r * std::f64::consts::SQRT_2 } else { r };
        s += &format!("#{}=CARTESIAN_POINT('',({:?},{:?},{z:?}));\n", first + i, x + q * a.cos(), q * a.sin());
        ids.push(format!("#{}", first + i));
    }
    (s, ids)
}

/// A tube face of radius `r` and length `h` about the Z axis through (`x`, 0, 0), written the
/// way OpenCascade writes one: a rational degree (1, 2) B-spline surface with the periodic
/// angle knots above, bounded by its two rims and a seam. Entities from `first` on; returns
/// them and the face's id.
fn periodic_knot_tube(first: u64, x: f64, r: f64, h: f64) -> (String, u64) {
    let id = |k: u64| first + k;
    let (net0, row0) = circle_net(id(40), x, r, 0.0);
    let (net1, row1) = circle_net(id(50), x, r, h);
    let (p0, p1, v0, v1) = (id(0), id(1), id(2), id(3));
    let (c0, c1, up, down, ref_dir) = (id(4), id(5), id(6), id(7), id(8));
    let (a0, a1, rim0, rim1) = (id(9), id(10), id(11), id(12));
    let (e0, e1, along, seam_line, seam) = (id(13), id(14), id(15), id(16), id(17));
    let (o0, o1, o2, o3, lp, bound, surface, face) = (id(18), id(19), id(20), id(21), id(22), id(23), id(24), id(25));
    let mut s = net0 + &net1;
    let xr = x + r;
    s += &format!("#{p0}=CARTESIAN_POINT('',({xr:?},0.,0.));\n#{p1}=CARTESIAN_POINT('',({xr:?},0.,{h:?}));\n");
    s += &format!("#{v0}=VERTEX_POINT('',#{p0});\n#{v1}=VERTEX_POINT('',#{p1});\n");
    s += &format!("#{c0}=CARTESIAN_POINT('',({x:?},0.,0.));\n#{c1}=CARTESIAN_POINT('',({x:?},0.,{h:?}));\n");
    s += &format!("#{up}=DIRECTION('',(0.,0.,1.));\n#{down}=DIRECTION('',(0.,0.,-1.));\n#{ref_dir}=DIRECTION('',(1.,0.,0.));\n");
    s += &format!("#{a0}=AXIS2_PLACEMENT_3D('',#{c0},#{up},#{ref_dir});\n#{a1}=AXIS2_PLACEMENT_3D('',#{c1},#{down},#{ref_dir});\n");
    s += &format!("#{rim0}=CIRCLE('',#{a0},{r:?});\n#{rim1}=CIRCLE('',#{a1},{r:?});\n");
    s += &format!("#{e0}=EDGE_CURVE('',#{v0},#{v0},#{rim0},.T.);\n#{e1}=EDGE_CURVE('',#{v1},#{v1},#{rim1},.T.);\n");
    s += &format!("#{along}=VECTOR('',#{up},1.);\n#{seam_line}=LINE('',#{p0},#{along});\n#{seam}=EDGE_CURVE('',#{v0},#{v1},#{seam_line},.T.);\n");
    s += &format!(
        "#{o0}=ORIENTED_EDGE('',*,*,#{e0},.F.);\n#{o1}=ORIENTED_EDGE('',*,*,#{seam},.T.);\n#{o2}=ORIENTED_EDGE('',*,*,#{e1},.F.);\n#{o3}=ORIENTED_EDGE('',*,*,#{seam},.F.);\n"
    );
    s += &format!("#{lp}=EDGE_LOOP('',(#{o0},#{o1},#{o2},#{o3}));\n#{bound}=FACE_BOUND('',#{lp},.T.);\n");
    s += &format!(
        "#{surface}=(BOUNDED_SURFACE()B_SPLINE_SURFACE(1,2,(({}),({})),.UNSPECIFIED.,.F.,.T.,.F.)B_SPLINE_SURFACE_WITH_KNOTS((2,2),({PERIODIC_MULTS}),(0.,{:?}),({PERIODIC_KNOTS}),.UNSPECIFIED.)GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_SURFACE((({CIRCLE_WEIGHTS}),({CIRCLE_WEIGHTS})))REPRESENTATION_ITEM('')SURFACE());\n",
        row0.join(","),
        row1.join(","),
        h / r
    );
    s += &format!("#{face}=ADVANCED_FACE('',(#{bound}),#{surface},.T.);\n");
    (s, face)
}

/// Two such tubes far apart in one body: the body's size sets a chord tolerance coarse beside
/// the tubes, and meshing a face at it divided the surface's whole parameter box, whose
/// corners lay outside the domain. The division never converged there (#1374's recursion),
/// and the import grew by gigabytes without end. The sheet comes in as SMLib's B-rep of its two
/// faces, each the file's face (SMLib lists the seam edges in another canonical order than the
/// file, and keeps the file's own order), each a cylinder's side.
#[test]
fn periodic_knot_tube_faces_import_and_mesh() {
    let t0 = std::time::Instant::now();
    let (r, h) = (1.7, 1.5);
    let (a, fa) = periodic_knot_tube(100, 0.0, r, h);
    let (b, fb) = periodic_knot_tube(200, 700.0, r, h);
    let text = format!(
        "{HEAD}{a}{b}#1=OPEN_SHELL('',(#{fa},#{fb}));\n#2=SHELL_BASED_SURFACE_MODEL('Tubes',(#1));\n#3=SHAPE_REPRESENTATION('',(#2),$);\n{TAIL}"
    );
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let tubes = &imp.bodies[0];
    assert!(tubes.refused.is_none() && !tubes.body.is_mesh() && tubes.body.is_surface(), "{:?} {:?}", tubes.refused, imp.warnings);
    assert_eq!(tubes.face_origin, vec![fa, fb], "{:?}", imp.warnings);
    // Each face is the file's quadratic circle of radius r (corner weights √2/2 to 12 digits,
    // a radius off by under 1e-12 of r) moved straight along its axis by h: a cylinder's side,
    // 2π r h. SMLib integrates to 1e-8.
    let m = measure(&tubes.body).unwrap();
    let area = 2.0 * std::f64::consts::TAU * r * h;
    assert!(rel(m.area, area) < 1e-6, "area {} (expected {area})", m.area);
    assert_eq!(tubes.body.unmeshed_faces(), 0);
    assert!(t0.elapsed().as_secs_f64() < 60.0, "took {:?}", t0.elapsed());
}

/// An original analytic STEP patch: a 7 mm high, 0.002 radian strip on a 3 m
/// cylinder. Its explicit outer bound encloses the strip even when the surface's
/// supplied sense disagrees with the loop (#1974).
fn shallow_cylinder_patch(sense: bool, outer: bool) -> String {
    shallow_cylinder_patch_at_radius(sense, outer, 3000.0)
}

fn shallow_cylinder_patch_at_radius(sense: bool, outer: bool, r: f64) -> String {
    let (angle, h) = (6.0 / r, 7.0);
    let (x, y) = (10.0 + r * (angle.cos() - 1.0), r * angle.sin());
    let origin = 10.0 - r;
    let bound = if outer { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
    let sense = if sense { "T" } else { "F" };
    format!(
        "{HEAD}
#1=CARTESIAN_POINT('',({origin},0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=CYLINDRICAL_SURFACE('',#4,{r});
#6=CARTESIAN_POINT('',(10.,0.,0.));
#7=CARTESIAN_POINT('',({x},{y},0.));
#8=CARTESIAN_POINT('',({x},{y},{h}));
#9=CARTESIAN_POINT('',(10.,0.,{h}));
#10=VERTEX_POINT('',#6);
#11=VERTEX_POINT('',#7);
#12=VERTEX_POINT('',#8);
#13=VERTEX_POINT('',#9);
#14=CIRCLE('',#4,{r});
#15=CARTESIAN_POINT('',({origin},0.,{h}));
#16=AXIS2_PLACEMENT_3D('',#15,#2,#3);
#17=CIRCLE('',#16,{r});
#18=VECTOR('',#2,1.);
#19=LINE('',#7,#18);
#20=LINE('',#6,#18);
#21=EDGE_CURVE('',#10,#11,#14,.T.);
#22=EDGE_CURVE('',#11,#12,#19,.T.);
#23=EDGE_CURVE('',#12,#13,#17,.F.);
#24=EDGE_CURVE('',#13,#10,#20,.F.);
#25=ORIENTED_EDGE('',*,*,#21,.T.);
#26=ORIENTED_EDGE('',*,*,#22,.T.);
#27=ORIENTED_EDGE('',*,*,#23,.T.);
#28=ORIENTED_EDGE('',*,*,#24,.T.);
#29=EDGE_LOOP('',(#25,#26,#27,#28));
#30={bound}('',#29,.T.);
#31=ADVANCED_FACE('',(#30),#5,.{sense}.);
#32=OPEN_SHELL('',(#31));
#33=SHELL_BASED_SURFACE_MODEL('Strip',(#32));
#34=SHAPE_REPRESENTATION('',(#33),$);
{TAIL}"
    )
}

#[test]
fn explicit_outer_bound_keeps_shallow_cylinder_patch_trimmed() {
    for (sense, radius) in [(true, 3000.), (false, 3000.), (true, 188_980.), (false, 188_980.)] {
        let imp = step_import(&shallow_cylinder_patch_at_radius(sense, true, radius)).unwrap();
        assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
        let b = &imp.bodies[0];
        assert_eq!(b.file_faces, 1);
        assert!(!b.closed);
        for mesh in [
            b.body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap(),
            b.body.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap(),
            b.body.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap(),
        ] {
            assert!(!mesh.triangles.is_empty());
            assert!(mesh.bounds().diagonal() < 10.0, "sense={sense}: {:?}", mesh.bounds());
            assert!((mesh.measure().area - 42.0).abs() < 0.01, "sense={sense}: area {}", mesh.measure().area);
            assert!(mesh.normals.iter().all(|n| n.x > 0.99), "the outer loop determines the face normal");
        }
    }
}

/// The strip's one bound, not marked outer, on a face whose same_sense runs against it: the
/// loop bounds the face and the face is turned over. Its complement is no face: the cylinder is
/// unbounded along its axis, so the loop as a hole would leave a face with no outer boundary.
#[test]
fn implicit_bound_against_its_face_sense_turns_the_face_over() {
    let imp = step_import(&shallow_cylinder_patch(false, false)).unwrap();
    assert!(imp.warnings.iter().any(|w| w.contains("1 faces turned over: their loops disagree with their same_sense")), "{:?}", imp.warnings);
    let mesh = imp.bodies[0].body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    assert!(mesh.bounds().diagonal() < 10.0, "{:?}", mesh.bounds());
    assert!((mesh.measure().area - 42.0).abs() < 0.01, "area {}", mesh.measure().area);
    assert!(mesh.normals.iter().all(|n| n.x > 0.99), "the loop determines the face normal");
}

#[test]
fn explicit_outer_bound_crossing_a_closed_nurbs_seam_stays_trimmed() {
    // The same analytic strip, with its cylinder expressed as a rational B-spline.
    // Put the parameter seam halfway through the strip, independently of its edges.
    let mut points = String::new();
    let mut rows = Vec::new();
    let mut weights = Vec::new();
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let circle = [(1., 0., 1.), (1., 1., w), (0., 1., 1.), (-1., 1., w), (-1., 0., 1.), (-1., -1., w), (0., -1., 1.), (1., -1., w), (1., 0., 1.)];
    let (sin, cos) = 0.001_f64.sin_cos();
    for (i, (x, y, weight)) in circle.into_iter().enumerate() {
        let id = 1000 + i * 2;
        let (x, y) = (10.0 - 3000.0 + 3000.0 * (x * cos - y * sin), 3000.0 * (x * sin + y * cos));
        points += &format!("#{id}=CARTESIAN_POINT('',({x:.16},{y:.16},0.));\n#{}=CARTESIAN_POINT('',({x:.16},{y:.16},7.));\n", id + 1);
        rows.push(format!("(#{id},#{})", id + 1));
        weights.push(format!("({weight:.16},{weight:.16})"));
    }
    let surface = format!(
        "#5=(BOUNDED_SURFACE() B_SPLINE_SURFACE(2,1,({}),.UNSPECIFIED.,.T.,.F.,.F.) B_SPLINE_SURFACE_WITH_KNOTS((3,2,2,2,3),(2,2),(0.,1.,2.,3.,4.),(0.,1.),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE());",
        rows.join(","),
        weights.join(",")
    );
    for sense in [true, false] {
        let text = shallow_cylinder_patch(sense, true);
        let old = "#5=CYLINDRICAL_SURFACE('',#4,3000);";
        assert!(text.contains(old));
        let text = text.replacen(old, &surface, 1).replace(TAIL, &format!("{points}{TAIL}"));
        let imp = step_import(&text).unwrap();
        let body = &imp.bodies[0].body;
        let mesh = body.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap();
        assert!(!mesh.triangles.is_empty(), "{:?}", imp.warnings);
        assert!(mesh.bounds().diagonal() < 10.0, "sense={sense}: {:?}", mesh.bounds());
        assert!((mesh.measure().area - 42.0).abs() < 0.01, "sense={sense}: area {}", mesh.measure().area);
    }
}

#[test]
fn extruded_ellipse_patch_crossing_profile_seam_stays_trimmed() {
    for (sense, outer) in [(true, true), (false, true), (true, false)] {
        for radius in [9.0, 3000.0] {
            let text = shallow_cylinder_patch_at_radius(sense, outer, radius);
            let old = format!("#5=CYLINDRICAL_SURFACE('',#4,{radius});");
            let angle = 3.0 / radius;
            let surface = format!(
                "#5=SURFACE_OF_LINEAR_EXTRUSION('',#35,#18);
#35=ELLIPSE('',#36,{radius},{radius});
#36=AXIS2_PLACEMENT_3D('',#1,#2,#37);
#37=DIRECTION('',({},{},0.));",
                angle.cos(),
                angle.sin()
            );
            assert!(text.contains(&old));
            let text = text.replacen(&old, &surface, 1);
            let imported = step_import(&text).unwrap();
            // Against its loop (same_sense false) the face is turned over, and the import says so.
            let open = "solid 'Strip': imported as an open body (the shell is not closed)";
            let turned = "solid 'Strip': 1 faces turned over: their loops disagree with their same_sense";
            let want: &[&str] = if sense { &[open] } else { &[turned, open] };
            assert_eq!(imported.warnings, want, "sense={sense}, outer={outer}, radius={radius}");
            assert_eq!(imported.bodies.len(), 1);
            let body = &imported.bodies[0].body;
            assert_eq!(imported.bodies[0].file_faces, 1);
            let plan = body.mesh_plan(&crate::MeshTolerance::chord(0.01)).unwrap();
            for mesh in [
                plan.evaluate(),
                body.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap(),
                body.mesh(&crate::MeshTolerance { chord: 0.01, angle: Some(crate::DISPLAY_ANGLE) }).unwrap(),
                body.mesh(&crate::MeshTolerance::chord(0.01)).unwrap(),
            ] {
                assert!(!mesh.triangles.is_empty());
                assert!(mesh.bounds().diagonal() < 10.0, "radius={radius}, sense={sense}, outer={outer}: {:?}", mesh.bounds());
                assert!((mesh.measure().area - 42.0).abs() < 0.1, "radius={radius}, sense={sense}, outer={outer}: area {}", mesh.measure().area);
            }
        }
    }
}

/// Parts of known shape from the step-conformance corpus (`tests/step-conformance`, generated
/// from formulas by https://github.com/dan335/step-conformance at 96f30bb, CC0 1.0):
/// each reads back with its exact volume (from the generator's own profile), its box and its
/// face count. Left out: `torus_ring` (meshes below its equator, a meshing issue for the
/// tessellation work) and the 1.4 MB `plate_900_holes`.
#[test]
fn step_conformance_parts_read_back_their_exact_shape() {
    // (file, exact volume (mm³), box (min, max), kernel faces, bodies, seam edges the reader adds
    // to the items it reads: `cylinder_twice` places one item twice)
    #[rustfmt::skip]
    let parts: [(&str, &str, f64, [f64; 6], usize, usize, usize); 17] = [
        ("cone_point", include_str!("../../../../tests/step-conformance/cone_point.step"), 1000.0 * std::f64::consts::PI, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 2, 1, 1),
        ("cone_point_vertex", include_str!("../../../../tests/step-conformance/cone_point_vertex.step"), 1000.0 * std::f64::consts::PI, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 2, 1, 1),
        ("cylinder", include_str!("../../../../tests/step-conformance/cylinder.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1, 1),
        ("cylinder_inch", include_str!("../../../../tests/step-conformance/cylinder_inch.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1, 1),
        ("cylinder_nurbs", include_str!("../../../../tests/step-conformance/cylinder_nurbs.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1, 1),
        ("cylinder_placed", include_str!("../../../../tests/step-conformance/cylinder_placed.step"), 9424.778, [31.835, -33.165, 1.835, 65.485, 0.485, 35.485], 3, 1, 1),
        ("cylinder_seam", include_str!("../../../../tests/step-conformance/cylinder_seam.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1, 0),
        ("cylinder_twice", include_str!("../../../../tests/step-conformance/cylinder_twice.step"), 18849.556, [-10.0, -10.0, 0.0, 90.0, 10.0, 30.0], 6, 2, 1),
        ("dome", include_str!("../../../../tests/step-conformance/dome.step"), 16755.161, [-20.0, -20.0, 0.0, 20.0, 20.0, 20.0], 2, 1, 1),
        ("dome_nurbs", include_str!("../../../../tests/step-conformance/dome_nurbs.step"), 16755.161, [-20.0, -20.0, 0.0, 20.0, 20.0, 20.0], 2, 1, 1),
        ("far_cylinder", include_str!("../../../../tests/step-conformance/far_cylinder.step"), 9424.778, [99990.0, -10.0, 0.0, 100010.0, 10.0, 30.0], 3, 1, 1),
        ("grooved_shaft", include_str!("../../../../tests/step-conformance/grooved_shaft.step"), 153532.251, [-10.0, -10.0, 0.0, 10.0, 10.0, 501.0], 753, 1, 0),
        ("plate_100_holes", include_str!("../../../../tests/step-conformance/plate_100_holes.step"), 71588.321, [0.0, 0.0, 0.0, 100.0, 100.0, 10.0], 110, 1, 0),
        ("sheet_disc", include_str!("../../../../tests/step-conformance/sheet_disc.step"), 193396443.755, [-1800.0, -1800.0, 0.0, 1800.0, 1800.0, 19.0], 3, 1, 1),
        ("tiny_pin", include_str!("../../../../tests/step-conformance/tiny_pin.step"), std::f64::consts::FRAC_PI_8, [-0.25, -0.25, 0.0, 0.25, 0.25, 2.0], 3, 1, 1),
        ("torus_ring_nurbs", include_str!("../../../../tests/step-conformance/torus_ring_nurbs.step"), 4934.802, [-25.0, -25.0, 0.0, 25.0, 25.0, 5.0], 2, 1, 1),
        ("washer", include_str!("../../../../tests/step-conformance/washer.step"), 3534.292, [-20.0, -20.0, 0.0, 20.0, 20.0, 3.0], 4, 1, 2),
    ];
    for (name, text, volume, bbox, faces, bodies, seams) in parts {
        let imp = step_import(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        // A face that winds around its surface without a seam edge (a cone or a dome bounded by
        // its base circle alone, a cylinder by its two rims) gets one, and the import says so.
        let added: Option<usize> = imp
            .warnings
            .iter()
            .map(|w| w.strip_suffix(" seam edges added to faces that wind around their surface")?.rsplit(": ").next()?.parse::<usize>().ok())
            .sum();
        assert_eq!(added, Some(seams), "{name}: {:?}", imp.warnings);
        assert_eq!(imp.bodies.len(), bodies, "{name}");
        let (mut v, mut n) = (0.0, 0);
        let mut b = solidcraft_geom::Aabb3::EMPTY;
        for body in &imp.bodies {
            let m = measure(&body.body).unwrap_or_else(|e| panic!("{name}: {e}"));
            v += m.volume;
            n += m.faces;
            b = b.union(&m.bbox);
        }
        assert!(rel(v, volume) < 1e-3, "{name}: volume {v} vs {volume}");
        assert_eq!(n, faces, "{name}: faces");
        let [x0, y0, z0, x1, y1, z1] = bbox;
        let size = Vec3::new(x1 - x0, y1 - y0, z1 - z0).len();
        for (got, want) in [(b.min, Vec3::new(x0, y0, z0)), (b.max, Vec3::new(x1, y1, z1))] {
            assert!(got.dist(want) <= size * 1e-3, "{name}: box corner {got:?} vs {want:?}");
        }
    }
}

/// The volume of the one solid `solidcraft_step::topology` reads from `text` into schema 4,
/// measured as the body of that record.
pub(super) fn schema_4_volume(text: &str) -> f64 {
    let ex = solidcraft_step::p21::parse(text).unwrap();
    let r = solidcraft_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibTrims);
    assert!(r.failures.is_empty() && r.solids.len() == 1, "{:?}", r.failures);
    measure(&Body::from_record(r.solids[0].brep.clone()).unwrap()).unwrap().volume
}

/// The step-conformance parts read into SMLib (`SOLIDCRAFT_KERNEL=smlib`): the STEP reader's
/// schema 4 record, SMLib's body of it, back as a kernel body. Each has its exact volume and
/// face count, and every face is traced to a face of the file (the NURBS cylinder, dome and
/// torus ring among them: their faces wind around a spline closed on itself, and get a seam).
#[cfg(feature = "smlib")]
#[test]
fn step_conformance_parts_read_into_smlib() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    // (file, exact volume (mm³) and faces of its solid items: cylinder_twice places its one
    // cylinder twice)
    #[rustfmt::skip]
    let parts: [(&str, f64, usize); 17] = [
        ("cone_point", 1000.0 * std::f64::consts::PI, 2), ("cone_point_vertex", 1000.0 * std::f64::consts::PI, 2),
        ("cylinder", 9424.778, 3), ("cylinder_inch", 9424.778, 3), ("cylinder_nurbs", 9424.778, 3),
        ("cylinder_placed", 9424.778, 3), ("cylinder_seam", 9424.778, 3), ("cylinder_twice", 9424.778, 3),
        ("dome", 16755.161, 2), ("dome_nurbs", 16755.161, 2), ("far_cylinder", 9424.778, 3),
        ("grooved_shaft", 153532.251, 753), ("plate_100_holes", 71588.321, 110), ("sheet_disc", 193396443.755, 3),
        ("tiny_pin", std::f64::consts::FRAC_PI_8, 3), ("torus_ring_nurbs", 4934.802, 2), ("washer", 3534.292, 4),
    ];
    for (name, volume, faces) in parts {
        let text = std::fs::read_to_string(dir.join(format!("{name}.step"))).unwrap();
        let ex = solidcraft_step::p21::parse(&text).unwrap();
        // The file's one representation context, as the import reads it.
        let ctx = ex.entities.iter().find(|(_, e)| e.record("GLOBAL_UNIT_ASSIGNED_CONTEXT").is_some()).map(|(id, _)| *id);
        let units = context_units(&ex, ctx, &mut HashMap::new());
        let mut items: Vec<u64> = ex.entities.iter().filter(|(_, e)| e.name() == "MANIFOLD_SOLID_BREP").map(|(id, _)| *id).collect();
        items.sort_unstable();
        let (mut v, mut n) = (0.0, 0);
        for item in items {
            let (part, _) = parallel::smlib_part(&ex, item, units).unwrap_or_else(|f| panic!("{name}: {}", f.msg));
            assert_eq!(part.face_origin.len(), part.body.face_count(), "{name}");
            assert!(part.face_origin.iter().all(|f| ex.get(*f).is_some_and(|e| e.name() == "ADVANCED_FACE")), "{name}: {:?}", part.face_origin);
            let m = measure(&part.body).unwrap_or_else(|e| panic!("{name}: {e}"));
            v += m.volume;
            n += m.faces;
        }
        assert!(rel(v, volume) < 1e-3, "{name}: volume {v} vs {volume}");
        assert_eq!(n, faces, "{name}: faces");
    }
}

/// A domed cubic patch 30 mm across whose four edges, cubic B-splines on its boundary rows,
/// are written `dz` mm above it, in a file that states 1e-6 mm: an intersection curve written
/// looser than its context. The patch has no closed-form pcurves for them, so SMLib makes them,
/// on the surface, and they lie `dz` from their edges.
fn dome_with_edges_raised(dz: f64) -> String {
    patch_with_edges_raised(dz, false)
}

/// [`dome_with_edges_raised`], or with `flat` the plane z = 0 in its place: its edges' pcurves
/// are then their control points' projections onto it, in closed form.
fn patch_with_edges_raised(dz: f64, flat: bool) -> String {
    let z = |i: usize, j: usize| if !flat && (1..=2).contains(&i) && (1..=2).contains(&j) { 6.0 } else { 0.0 };
    let mut t = String::from(HEAD);
    let mut rows = Vec::new();
    for i in 0..4 {
        let row: Vec<String> = (0..4).map(|j| format!("#{}", 100 + i * 4 + j)).collect();
        for j in 0..4 {
            t += &format!("#{}=CARTESIAN_POINT('',({}.,{}.,{}));\n", 100 + i * 4 + j, i * 10, j * 10, z(i, j));
        }
        rows.push(format!("({})", row.join(",")));
    }
    if flat {
        t += "#10=PLANE('',#13);\n#11=CARTESIAN_POINT('',(0.,0.,0.));\n#12=DIRECTION('',(0.,0.,1.));\n#14=DIRECTION('',(1.,0.,0.));\n#13=AXIS2_PLACEMENT_3D('',#11,#12,#14);\n";
    } else {
        t += &format!(
            "#10=B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,(4,4),(4,4),(0.,1.),(0.,1.),.UNSPECIFIED.);\n",
            rows.join(",")
        );
    }
    // The boundary, counter-clockwise from above: (i, j) runs i = 0 → 3 at j = 0, j = 0 → 3 at
    // i = 3, then back; each side's control points are the patch's, raised.
    let sides: [[(usize, usize); 4]; 4] =
        [[(0, 0), (1, 0), (2, 0), (3, 0)], [(3, 0), (3, 1), (3, 2), (3, 3)], [(3, 3), (2, 3), (1, 3), (0, 3)], [(0, 3), (0, 2), (0, 1), (0, 0)]];
    let p = |(i, j): (usize, usize)| format!("({}.,{}.,{})", i * 10, j * 10, z(i, j) + dz);
    for (k, side) in sides.iter().enumerate() {
        t += &format!("#{}=CARTESIAN_POINT('',{});\n#{}=VERTEX_POINT('',#{});\n", 200 + k, p(side[0]), 210 + k, 200 + k);
        let pts: Vec<String> = (0..4).map(|m| format!("#{}", 300 + k * 4 + m)).collect();
        for (m, &q) in side.iter().enumerate() {
            t += &format!("#{}=CARTESIAN_POINT('',{});\n", 300 + k * 4 + m, p(q));
        }
        t += &format!("#{}=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);\n", 400 + k, pts.join(","));
    }
    for k in 0..4 {
        t += &format!(
            "#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            500 + k,
            210 + k,
            210 + (k + 1) % 4,
            400 + k,
            510 + k,
            500 + k
        );
    }
    t += "#520=EDGE_LOOP('',(#510,#511,#512,#513));\n#521=FACE_OUTER_BOUND('',#520,.T.);\n#522=ADVANCED_FACE('',(#521),#10,.T.);\n";
    t += "#530=OPEN_SHELL('',(#522));\n#531=SHELL_BASED_SURFACE_MODEL('Dome',(#530));\n#532=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#531),#540);\n";
    t += &context(540, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    t
}

#[test]
fn edges_written_off_their_spline_face_carry_their_gap_as_their_tolerance() {
    // 0.2 µm off, 200 times the file's stated uncertainty: each edge's tolerance covers its
    // gap and the face imports, its notes saying how many edges and how far.
    let imp = step_import(&dome_with_edges_raised(2e-4)).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("4 edges lie off their faces' surfaces")), "{:?}", imp.warnings);
    let r = imp.bodies[0].body.record();
    assert!(r.edges.iter().all(|e| e.tolerance >= 2e-4 && e.tolerance < 1e-3), "{:?}", r.edges.iter().map(|e| e.tolerance).collect::<Vec<_>>());
    // Far off (1.5 mm) is refused, naming the gap.
    let far = step_import(&dome_with_edges_raised(1.5));
    let refused = match &far {
        Ok(i) => i.bodies.is_empty() && i.warnings.iter().any(|w| w.contains("off the face's surface")),
        Err(e) => e.to_string().contains("off the face's surface"),
    };
    assert!(refused, "{far:?}");
}

#[test]
fn spline_edges_off_their_plane_by_less_than_the_file_states_get_their_closed_form() {
    // 0.8e-6 mm off, inside the file's 1e-6 mm but past the half of it a closed-form pcurve is
    // checked within: each edge's distance from the plane, measured first, is its tolerance
    // (with margin), and the face reads with its pcurves the edges' projections. A plane has no
    // parameter box for SMLib to make pcurves in, so without them the face is refused.
    let imp = step_import(&patch_with_edges_raised(0.8e-6, true)).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("4 edges lie off their faces' surfaces")), "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.area, 900.0) < 1e-6, "{}", m.area);
}

/// A spherical triangle (radius 0.5) whose edge runs from the equator over the sphere's pole to
/// a vertex 1.8° past it, as a corner ball meets its rounds (Poppy Humanoid writes one): on the
/// sphere's parameters the pole is a line where that edge's pcurve would jump half a turn. The
/// edge is split at the pole, and the face reads with its area by the spherical excess.
#[test]
fn an_edge_over_a_spheres_pole_is_split_there() {
    let r = 0.5f64;
    let over = (91.8f64).to_radians();
    let a = [r, 0.0, 0.0];
    let b = [0.0, r, 0.0];
    let p = [r * over.cos(), 0.0, r * over.sin()];
    let f = |x: f64| format!("{x:.17E}");
    let pt = |id: u64, q: [f64; 3]| format!("#{id}=CARTESIAN_POINT('',({},{},{}));\n", f(q[0]), f(q[1]), f(q[2]));
    let dir = |id: u64, q: [f64; 3]| format!("#{id}=DIRECTION('',({},{},{}));\n", f(q[0]), f(q[1]), f(q[2]));
    let mut t = String::from(HEAD);
    t += &pt(1, [0.0; 3]);
    t += &dir(2, [0.0, 0.0, 1.0]);
    t += &dir(3, [1.0, 0.0, 0.0]);
    t += "#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#5=SPHERICAL_SURFACE('',#4,0.5);\n";
    t += &pt(10, a);
    t += &pt(11, b);
    t += &pt(12, p);
    t += "#13=VERTEX_POINT('',#10);\n#14=VERTEX_POINT('',#11);\n#15=VERTEX_POINT('',#12);\n";
    // A to P in the x–z plane over the pole (normal −y, from x towards z); P to B (normal P × B,
    // from P); B to A on the equator (normal −z, from y towards x).
    let n = |u: [f64; 3], v: [f64; 3]| {
        let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        [c[0] / l, c[1] / l, c[2] / l]
    };
    let unit = |u: [f64; 3]| [u[0] / r, u[1] / r, u[2] / r];
    t += &dir(20, [0.0, -1.0, 0.0]);
    t += "#21=AXIS2_PLACEMENT_3D('',#1,#20,#3);\n#22=CIRCLE('',#21,0.5);\n#23=EDGE_CURVE('',#13,#15,#22,.T.);\n";
    t += &dir(30, n(p, b));
    t += &dir(31, unit(p));
    t += "#32=AXIS2_PLACEMENT_3D('',#1,#30,#31);\n#33=CIRCLE('',#32,0.5);\n#34=EDGE_CURVE('',#15,#14,#33,.T.);\n";
    t += &dir(40, [0.0, 0.0, -1.0]);
    t += &dir(41, [0.0, 1.0, 0.0]);
    t += "#42=AXIS2_PLACEMENT_3D('',#1,#40,#41);\n#43=CIRCLE('',#42,0.5);\n#44=EDGE_CURVE('',#14,#13,#43,.T.);\n";
    // Counter-clockwise seen from outside: A, B, P, so every edge the other way round.
    t += "#50=ORIENTED_EDGE('',*,*,#44,.F.);\n#51=ORIENTED_EDGE('',*,*,#34,.F.);\n#52=ORIENTED_EDGE('',*,*,#23,.F.);\n";
    t += "#53=EDGE_LOOP('',(#50,#51,#52));\n#54=FACE_OUTER_BOUND('',#53,.T.);\n#55=ADVANCED_FACE('',(#54),#5,.T.);\n";
    t += "#60=OPEN_SHELL('',(#55));\n#61=SHELL_BASED_SURFACE_MODEL('Ball corner',(#60));\n#62=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#61),#70);\n";
    t += &context(70, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("1 edges split where they pass through a face's pole")), "{:?}", imp.warnings);
    // The spherical excess of the unit vectors' triangle, tan(E/2) = |a·(b×c)| / (1 + a·b + b·c + c·a).
    let (ua, ub, up) = (unit(a), unit(b), unit(p));
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let triple = dot(ua, n(ub, up)) * (1.0 - dot(ub, up).powi(2)).sqrt();
    let excess = 2.0 * triple.abs().atan2(1.0 + dot(ua, ub) + dot(ub, up) + dot(up, ua));
    let m = measure(&imp.bodies[0].body).unwrap();
    assert!(rel(m.area, excess * r * r) < 1e-3, "{} vs {}", m.area, excess * r * r);
}

/// A planar bicubic patch whose v = 0 side is shrunk to `side` mm (a corner of a round written as
/// a four-sided patch, Poppy Humanoid's connectors write them), bounded by three edges, its
/// boundary iso curves counter-clockwise in (u, v): up the u = 1 side, back across v = 1, down
/// the u = 0 side. The two side edges end `side` apart at one vertex, the end of the u = 0 side,
/// whose tolerance covers that gap; the face tolerance (the file's 1e-6 mm) does not.
fn patch_with_a_side_shrunk(side: f64) -> String {
    let f = |x: f64| format!("{x:.17E}");
    // (i, j), i along u: the v = 0 row runs over `side`, the v = 1 row over 30 mm.
    let p = |i: usize, j: usize| {
        let v = j as f64 / 3.0;
        [(1.0 - v) * side * i as f64 / 3.0 + v * 10.0 * i as f64, 30.0 * v, 0.0]
    };
    let pt = |id: usize, q: [f64; 3]| format!("#{id}=CARTESIAN_POINT('',({},{},{}));\n", f(q[0]), f(q[1]), f(q[2]));
    let mut t = String::from(HEAD);
    let mut rows = Vec::new();
    for i in 0..4 {
        rows.push(format!("({})", (0..4).map(|j| format!("#{}", 100 + i * 4 + j)).collect::<Vec<_>>().join(",")));
        for j in 0..4 {
            t += &pt(100 + i * 4 + j, p(i, j));
        }
    }
    t += &format!(
        "#10=B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,(4,4),(4,4),(0.,1.),(0.,1.),.UNSPECIFIED.);\n",
        rows.join(",")
    );
    // A at (0, 0), C at (1, 1), B at (0, 1).
    for (k, q) in [p(0, 0), p(3, 3), p(0, 3)].into_iter().enumerate() {
        t += &pt(200 + k, q);
        t += &format!("#{}=VERTEX_POINT('',#{});\n", 210 + k, 200 + k);
    }
    let sides: [[(usize, usize); 4]; 3] = [[(3, 0), (3, 1), (3, 2), (3, 3)], [(3, 3), (2, 3), (1, 3), (0, 3)], [(0, 3), (0, 2), (0, 1), (0, 0)]];
    for (k, side) in sides.iter().enumerate() {
        for (m, &(i, j)) in side.iter().enumerate() {
            t += &pt(300 + k * 4 + m, p(i, j));
        }
        let pts: Vec<String> = (0..4).map(|m| format!("#{}", 300 + k * 4 + m)).collect();
        t += &format!("#{}=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);\n", 400 + k, pts.join(","));
        t += &format!(
            "#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            500 + k,
            210 + k,
            210 + (k + 1) % 3,
            400 + k,
            510 + k,
            500 + k
        );
    }
    t += "#520=EDGE_LOOP('',(#510,#511,#512));\n#521=FACE_OUTER_BOUND('',#520,.T.);\n#522=ADVANCED_FACE('',(#521),#10,.T.);\n";
    t += "#530=OPEN_SHELL('',(#522));\n#531=SHELL_BASED_SURFACE_MODEL('Corner',(#530));\n#532=MANIFOLD_SURFACE_SHAPE_REPRESENTATION('',(#531),#540);\n";
    t += &context(540, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    t
}

#[test]
fn a_patch_side_shrunk_within_its_vertexs_tolerance_is_a_pole_of_the_loop() {
    // 4 µm: four times the face's tolerance, within the vertex's (1.5 × its 4 µm gap). The loop
    // runs along the shrunk side between the side edges' ends, a pole for it, and the face reads
    // with the patch's area: a trapezoid 30 mm high, 30 mm and `side` wide.
    let side = 4e-3 * 1e-3;
    let imp = step_import(&patch_with_a_side_shrunk(side)).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    let area = 0.5 * (30.0 + side) * 30.0;
    assert!(rel(m.area, area) < 1e-6, "{} vs {area}", m.area);
}

/// A can of radius 5 and height 10: its bottom (on a plane facing up, the face against it), its
/// wall, and its lid on the plane `lid_z` (10 where the lid's rim lies on it).
fn can_with_its_lid_at(lid_z: f64) -> String {
    format!(
        "{HEAD}
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=DIRECTION('',(0.,0.,-1.));
#5=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#6=AXIS2_PLACEMENT_3D('',#1,#4,#3);
#7=CARTESIAN_POINT('',(5.,0.,0.));
#8=VERTEX_POINT('',#7);
#9=CIRCLE('',#5,5.);
#10=EDGE_CURVE('',#8,#8,#9,.T.);
#11=ORIENTED_EDGE('',*,*,#10,.F.);
#12=EDGE_LOOP('',(#11));
#13=FACE_OUTER_BOUND('',#12,.T.);
#14=PLANE('',#5);
#15=ADVANCED_FACE('Bottom',(#13),#14,.F.);
#16=ORIENTED_EDGE('',*,*,#10,.T.);
#17=EDGE_LOOP('',(#16));
#18=FACE_BOUND('',#17,.T.);
#19=CYLINDRICAL_SURFACE('',#5,5.);
#20=ADVANCED_FACE('Wall',(#18,#37),#19,.T.);
#30=CARTESIAN_POINT('',(0.,0.,10.));
#31=AXIS2_PLACEMENT_3D('',#30,#2,#3);
#32=CARTESIAN_POINT('',(5.,0.,10.));
#33=VERTEX_POINT('',#32);
#34=CIRCLE('',#31,5.);
#35=EDGE_CURVE('',#33,#33,#34,.T.);
#36=ORIENTED_EDGE('',*,*,#35,.F.);
#37=FACE_BOUND('',#38,.T.);
#38=EDGE_LOOP('',(#36));
#39=ORIENTED_EDGE('',*,*,#35,.T.);
#40=EDGE_LOOP('',(#39));
#41=FACE_OUTER_BOUND('',#40,.T.);
#42=CARTESIAN_POINT('',(0.,0.,{lid_z:?}));
#43=AXIS2_PLACEMENT_3D('',#42,#2,#3);
#44=PLANE('',#43);
#45=ADVANCED_FACE('Lid',(#41),#44,.T.);
#21=CLOSED_SHELL('',(#15,#20,#45));
#22=MANIFOLD_SOLID_BREP('Can',#21);
#23=SHAPE_REPRESENTATION('',(#22),$);
{TAIL}"
    )
}

/// A solid no B-rep can be made of comes in as an open surface body of its exact faces, the
/// faces that make it refused left out and named, as Poppy Humanoid's stock head (a face around
/// an open surface of revolution with one bound) and Congé19 (an edge further off its face than
/// any tolerance SMLib takes) do. A can whose lid lies 1.5 mm off its rim: the lid is left out,
/// the bottom and the wall come in sharing their rim, and the existing surface tools close the
/// can again (the rim patched, the patch stitched on): a solid with the can's volume.
#[test]
fn a_refused_solid_comes_in_as_a_surface_body_of_the_faces_that_read() {
    let pi = std::f64::consts::PI;
    let whole = step_import(&can_with_its_lid_at(10.0)).unwrap();
    assert_eq!(whole.bodies.len(), 1, "{:?}", whole.warnings);
    assert!(whole.bodies[0].closed && whole.bodies[0].refused.is_none(), "{:?}", whole.warnings);
    assert!(rel(measure(&whole.bodies[0].body).unwrap().volume, 250.0 * pi) < 1e-6);

    let imp = step_import(&can_with_its_lid_at(11.5)).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let can = &imp.bodies[0];
    assert!(!can.body.is_mesh() && can.body.is_surface() && !can.closed, "{:?}", imp.warnings);
    let why = can.refused.as_deref().unwrap_or_default();
    assert!(why.contains("face #45") && why.contains("off the face's surface"), "{why}");
    assert_eq!((can.file_faces, can.body.face_count()), (3, 2));
    assert_eq!(can.face_origin, [15, 20]);
    // The bottom faces down as the file has it, against its plane's natural normal (SMLib gives
    // a sheet's faces back on their natural side; the import turns it back).
    let r = can.body.record();
    let bottom = can.face_origin.iter().position(|f| *f == 15).unwrap();
    let uses: Vec<bool> = r
        .shells
        .iter()
        .flat_map(|s| match s {
            solidcraft_geom::brep::Shell::Faces { faces, .. } => faces.iter().filter(|f| f.face as usize == bottom).map(|f| f.forward).collect(),
            _ => Vec::new(),
        })
        .collect();
    assert_eq!(uses, [false], "the bottom's use");
    // The report keeps the refusal and names the face left out; no "open body" warning on top.
    assert!(imp.warnings.iter().any(|w| w.contains("'Can': imported as surfaces: ") && w.contains(why)), "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("'Can': left out: ") && w.contains("face #45")), "{:?}", imp.warnings);
    assert!(!imp.warnings.iter().any(|w| w.contains("open body")), "{:?}", imp.warnings);
    // Exact: the bottom's disk and the wall's 10 mm of cylinder.
    assert!(rel(measure(&can.body).unwrap().area, 125.0 * pi) < 1e-6, "{}", measure(&can.body).unwrap().area);
    // Closed again with the existing tools.
    let lid = crate::patch_edges(&can.body, &[Vec3::new(0.0, 5.0, 10.0)]).unwrap();
    let back = crate::stitch(&[&can.body, &lid], 0.0).unwrap();
    assert!(!back.is_surface(), "closed");
    assert!(rel(measure(&back).unwrap().volume, 250.0 * pi) < 1e-6, "{}", measure(&back).unwrap().volume);
}

/// A sector of a cylinder (radius 5, height 10) about z, `sweep` radians from `start` on the
/// wall's frame: a pie slice of the cylinder, its two sides through the axis.
fn cylinder_sector(start: f64, sweep: f64) -> String {
    let (r, h) = (5.0, 10.0);
    let p = |x: f64, y: f64, z: f64| format!("({x:.17},{y:.17},{z:.17})");
    let (a0, a1) = (start, start + sweep);
    let (c0, s0, c1, s1) = (a0.cos(), a0.sin(), a1.cos(), a1.sin());
    let line = |id: u32, from: (f64, f64, f64), to: (f64, f64, f64)| {
        let d = Vec3::new(to.0 - from.0, to.1 - from.1, to.2 - from.2);
        let u = d.normalized().unwrap();
        format!(
            "#{id}=LINE('',#{},#{});\n#{}=CARTESIAN_POINT('',{});\n#{}=VECTOR('',#{},{:.17});\n#{}=DIRECTION('',{});\n",
            id + 1,
            id + 2,
            id + 1,
            p(from.0, from.1, from.2),
            id + 2,
            id + 3,
            d.len(),
            id + 3,
            p(u.x, u.y, u.z)
        )
    };
    let (a_0, a_1, p0, p1, q0, q1) =
        ((0.0, 0.0, 0.0), (0.0, 0.0, h), (r * c0, r * s0, 0.0), (r * c0, r * s0, h), (r * c1, r * s1, 0.0), (r * c1, r * s1, h));
    let mut lines = String::new();
    for (id, from, to) in [(100, a_0, a_1), (110, a_0, p0), (120, a_0, q0), (130, a_1, p1), (140, a_1, q1), (150, p0, p1), (160, q0, q1)] {
        lines.push_str(&line(id, from, to));
    }
    format!(
        "{HEAD}
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=DIRECTION('',(0.,0.,-1.));
#5=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#6=AXIS2_PLACEMENT_3D('',#1,#4,#3);
#7=CARTESIAN_POINT('',(0.,0.,{h:?}));
#8=AXIS2_PLACEMENT_3D('',#7,#2,#3);
#10=VERTEX_POINT('',#11);
#11=CARTESIAN_POINT('',{});
#12=VERTEX_POINT('',#13);
#13=CARTESIAN_POINT('',{});
#14=VERTEX_POINT('',#15);
#15=CARTESIAN_POINT('',{});
#16=VERTEX_POINT('',#17);
#17=CARTESIAN_POINT('',{});
#18=VERTEX_POINT('',#19);
#19=CARTESIAN_POINT('',{});
#20=VERTEX_POINT('',#21);
#21=CARTESIAN_POINT('',{});
{lines}#30=CIRCLE('',#5,{r:?});
#31=CIRCLE('',#8,{r:?});
#40=EDGE_CURVE('',#10,#12,#100,.T.);
#41=EDGE_CURVE('',#10,#14,#110,.T.);
#42=EDGE_CURVE('',#10,#18,#120,.T.);
#43=EDGE_CURVE('',#12,#16,#130,.T.);
#44=EDGE_CURVE('',#12,#20,#140,.T.);
#45=EDGE_CURVE('',#14,#16,#150,.T.);
#46=EDGE_CURVE('',#18,#20,#160,.T.);
#47=EDGE_CURVE('',#14,#18,#30,.T.);
#48=EDGE_CURVE('',#16,#20,#31,.T.);
#50=CYLINDRICAL_SURFACE('',#5,{r:?});
#51=EDGE_LOOP('',(#200,#201,#202,#203));
#200=ORIENTED_EDGE('',*,*,#47,.T.);
#201=ORIENTED_EDGE('',*,*,#46,.T.);
#202=ORIENTED_EDGE('',*,*,#48,.F.);
#203=ORIENTED_EDGE('',*,*,#45,.F.);
#52=FACE_OUTER_BOUND('',#51,.T.);
#53=ADVANCED_FACE('Wall',(#52),#50,.T.);
#54=PLANE('',#6);
#55=EDGE_LOOP('',(#204,#205,#206));
#204=ORIENTED_EDGE('',*,*,#42,.T.);
#205=ORIENTED_EDGE('',*,*,#47,.F.);
#206=ORIENTED_EDGE('',*,*,#41,.F.);
#56=FACE_OUTER_BOUND('',#55,.T.);
#57=ADVANCED_FACE('Bottom',(#56),#54,.T.);
#58=PLANE('',#8);
#59=EDGE_LOOP('',(#207,#208,#209));
#207=ORIENTED_EDGE('',*,*,#43,.T.);
#208=ORIENTED_EDGE('',*,*,#48,.T.);
#209=ORIENTED_EDGE('',*,*,#44,.F.);
#60=FACE_OUTER_BOUND('',#59,.T.);
#61=ADVANCED_FACE('Top',(#60),#58,.T.);
#62=DIRECTION('',{});
#63=AXIS2_PLACEMENT_3D('',#1,#62,#2);
#64=PLANE('',#63);
#65=EDGE_LOOP('',(#210,#211,#212,#213));
#210=ORIENTED_EDGE('',*,*,#41,.T.);
#211=ORIENTED_EDGE('',*,*,#45,.T.);
#212=ORIENTED_EDGE('',*,*,#43,.F.);
#213=ORIENTED_EDGE('',*,*,#40,.F.);
#66=FACE_OUTER_BOUND('',#65,.T.);
#67=ADVANCED_FACE('Start',(#66),#64,.T.);
#68=DIRECTION('',{});
#69=AXIS2_PLACEMENT_3D('',#1,#68,#2);
#70=PLANE('',#69);
#71=EDGE_LOOP('',(#214,#215,#216,#217));
#214=ORIENTED_EDGE('',*,*,#40,.T.);
#215=ORIENTED_EDGE('',*,*,#44,.T.);
#216=ORIENTED_EDGE('',*,*,#46,.F.);
#217=ORIENTED_EDGE('',*,*,#42,.F.);
#72=FACE_OUTER_BOUND('',#71,.T.);
#73=ADVANCED_FACE('End',(#72),#70,.T.);
#80=CLOSED_SHELL('',(#53,#57,#61,#67,#73));
#81=MANIFOLD_SOLID_BREP('Sector',#80);
#82=SHAPE_REPRESENTATION('',(#81),$);
{TAIL}",
        p(a_0.0, a_0.1, a_0.2),
        p(a_1.0, a_1.1, a_1.2),
        p(p0.0, p0.1, p0.2),
        p(p1.0, p1.1, p1.2),
        p(q0.0, q0.1, q0.2),
        p(q1.0, q1.1, q1.2),
        p(s0, -c0, 0.0),
        p(-s1, c1, 0.0),
    )
}

/// A round face of less than one turn that does not start on a quarter line of its frame (a
/// 300° sector of a cylinder from 75°, or 350° from 200°) needed a fifth quarter on the frame's quarter grid, so its
/// form ran past one turn and SMLib refused the solid (it came in as surfaces, an edge "12 mm off
/// the face's surface"). Its frame is turned about its axis to start on a quarter line: the same
/// cylinder, a form of four quarters, and the solid with its exact volume and area. One that fits
/// in four quarters as it is (300° from 37°) keeps its frame.
#[test]
fn a_round_face_off_its_frames_quarter_lines_is_formed_within_one_turn() {
    for (start, sweep) in
        [(75f64.to_radians(), 300f64.to_radians()), (37f64.to_radians(), 300f64.to_radians()), (200f64.to_radians(), 350f64.to_radians())]
    {
        let imp = step_import(&cylinder_sector(start, sweep)).unwrap();
        assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
        let b = &imp.bodies[0];
        assert!(b.closed && b.refused.is_none(), "{start} {sweep}: {:?} {:?}", b.refused, imp.warnings);
        let wall = b.face_origin.iter().position(|f| *f == 53).unwrap();
        let Some(solidcraft_geom::brep::Surface::RationalSpline { u_knots, .. }) = &b.body.record().faces[wall].nurbs else {
            panic!("the wall's form")
        };
        assert!(u_knots[u_knots.len() - 1] - u_knots[0] <= 4.0, "{start}: the wall's form spans {u_knots:?}");
        let m = measure(&b.body).unwrap();
        let (volume, area) = (0.5 * sweep * 25.0 * 10.0, sweep * 25.0 + sweep * 5.0 * 10.0 + 2.0 * 5.0 * 10.0);
        assert!(rel(m.volume, volume) < 1e-6, "{start}: volume {} vs {volume}", m.volume);
        assert!(rel(m.area, area) < 1e-6, "{start}: area {} vs {area}", m.area);
    }
}

/// A ball (radius 10, axis z) between the planes y = -6 and y = 6, its two halves z > 0 and
/// z < 0 meeting on the equator, neither with a seam: each gets one from the first vertex of its
/// loop to the pole it covers. The upper loop starts at (8, -6, 0), the lower at (8, 6, 0)
/// (`lower_first` 0) or (-8, -6, 0) (2): the seams are 73.7 degrees apart, so no one turn of
/// the frames the equator joins puts both on a quarter line. The lower half was left past one
/// turn of its form, its trim across the form's end crossed the face, and its mesh had 15.6
/// times its area, folded over the ball (a torn ball in a rotor's STEP file). Each half is
/// turned on its own: forms of four quarters, and each half's mesh has its area.
#[test]
fn a_balls_halves_with_seams_off_each_others_quarter_lines_each_form_within_one_turn() {
    let (r, c) = (10.0f64, 6.0f64);
    let rr = (r * r - c * c).sqrt();
    let p = |x: f64, y: f64, z: f64| format!("({x:.17},{y:.17},{z:.17})");
    for lower_first in [0usize, 2] {
        let lower = [
            "#60=ORIENTED_EDGE('',*,*,#40,.F.)",
            "#61=ORIENTED_EDGE('',*,*,#45,.F.)",
            "#62=ORIENTED_EDGE('',*,*,#41,.F.)",
            "#63=ORIENTED_EDGE('',*,*,#43,.T.)",
        ];
        let order: Vec<&str> = (0..4).map(|k| lower[(lower_first + k) % 4].split('=').next().unwrap_or("")).collect();
        let text = format!(
            "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#5=SPHERICAL_SURFACE('',#4,{r:?});
#10=CARTESIAN_POINT('',{});
#11=CARTESIAN_POINT('',{});
#12=CARTESIAN_POINT('',{});
#13=CARTESIAN_POINT('',{});
#14=VERTEX_POINT('',#10);
#15=VERTEX_POINT('',#11);
#16=VERTEX_POINT('',#12);
#17=VERTEX_POINT('',#13);
#20=DIRECTION('',(0.,-1.,0.));
#21=CARTESIAN_POINT('',(0.,{c:?},0.));
#22=AXIS2_PLACEMENT_3D('',#21,#20,#3);
#23=CARTESIAN_POINT('',(0.,{:?},0.));
#24=AXIS2_PLACEMENT_3D('',#23,#20,#3);
#30=CIRCLE('',#4,{r:?});
#31=CIRCLE('',#22,{rr:.17});
#32=CIRCLE('',#24,{rr:.17});
#40=EDGE_CURVE('',#14,#15,#30,.T.);
#41=EDGE_CURVE('',#16,#17,#30,.T.);
#42=EDGE_CURVE('',#15,#16,#31,.T.);
#43=EDGE_CURVE('',#16,#15,#31,.T.);
#44=EDGE_CURVE('',#14,#17,#32,.T.);
#45=EDGE_CURVE('',#17,#14,#32,.T.);
#50=ORIENTED_EDGE('',*,*,#40,.T.);
#51=ORIENTED_EDGE('',*,*,#42,.T.);
#52=ORIENTED_EDGE('',*,*,#41,.T.);
#53=ORIENTED_EDGE('',*,*,#44,.F.);
#54=EDGE_LOOP('',(#50,#51,#52,#53));
#55=FACE_OUTER_BOUND('',#54,.T.);
#56=ADVANCED_FACE('Upper',(#55),#5,.T.);
{};
{};
{};
{};
#64=EDGE_LOOP('',({}));
#65=FACE_OUTER_BOUND('',#64,.T.);
#66=ADVANCED_FACE('Lower',(#65),#5,.T.);
#70=DIRECTION('',(0.,1.,0.));
#71=AXIS2_PLACEMENT_3D('',#21,#70,#3);
#72=PLANE('',#71);
#73=ORIENTED_EDGE('',*,*,#43,.F.);
#74=ORIENTED_EDGE('',*,*,#42,.F.);
#75=EDGE_LOOP('',(#73,#74));
#76=FACE_OUTER_BOUND('',#75,.T.);
#77=ADVANCED_FACE('Top',(#76),#72,.T.);
#79=PLANE('',#24);
#80=ORIENTED_EDGE('',*,*,#44,.T.);
#81=ORIENTED_EDGE('',*,*,#45,.T.);
#82=EDGE_LOOP('',(#80,#81));
#83=FACE_OUTER_BOUND('',#82,.T.);
#84=ADVANCED_FACE('Bottom',(#83),#79,.T.);
#90=CLOSED_SHELL('',(#56,#66,#77,#84));
#91=MANIFOLD_SOLID_BREP('Ball',#90);
#92=SHAPE_REPRESENTATION('',(#91),$);
{TAIL}",
            p(rr, -c, 0.0),
            p(rr, c, 0.0),
            p(-rr, c, 0.0),
            p(-rr, -c, 0.0),
            -c,
            lower[0],
            lower[1],
            lower[2],
            lower[3],
            order.join(","),
        );
        let imp = step_import(&text).unwrap();
        assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
        let b = &imp.bodies[0];
        assert!(b.closed && b.refused.is_none(), "{lower_first}: {:?} {:?}", b.refused, imp.warnings);
        let pi = std::f64::consts::PI;
        let m = measure(&b.body).unwrap();
        let (volume, area) = (pi * (r * r * 2.0 * c - 2.0 * c.powi(3) / 3.0), 4.0 * pi * r * c + 2.0 * pi * rr * rr);
        // Measured on the sphere to 1e-5.
        assert!(rel(m.volume, volume) < 1e-4, "{lower_first}: volume {} vs {volume}", m.volume);
        assert!(rel(m.area, area) < 1e-4, "{lower_first}: area {} vs {area}", m.area);
        let meshed = b.body.faces(0.005).unwrap();
        for half in [56, 66] {
            let fi = b.face_origin.iter().position(|f| *f == half).unwrap();
            let Some(solidcraft_geom::brep::Surface::RationalSpline { u_knots, .. }) = &b.body.record().faces[fi].nurbs else {
                panic!("the half's form")
            };
            assert!(u_knots[u_knots.len() - 1] - u_knots[0] <= 4.0, "{lower_first}: face #{half}'s form spans {u_knots:?}");
            // Each half is a zone of the sphere 2c high, halved: pi r 2c.
            let want = pi * r * 2.0 * c;
            assert!(rel(meshed[fi].area, want) < 1e-2, "{lower_first}: face #{half}'s mesh has area {} of {want}", meshed[fi].area);
        }
    }
}

/// A circle edge between two vertices its file's uncertainty does not tell apart is the short arc
/// between them, not a full turn (the engine assembly's water pump housing has a 20.4 um arc
/// between two vertices in a part whose uncertainty is 20.5 um). A can whose wall is
/// split by two rulings 0.6 um apart, under the default 1 um: read as turns, the sliver of wall
/// between them wound once round the can with no second bound and the can came in as surfaces.
#[test]
fn a_circle_edge_between_two_vertices_closer_than_the_tolerance_is_the_arc_between_them() {
    let (r, h) = (5.0f64, 10.0f64);
    let delta = 0.6e-3 / r;
    let (c, s) = (r * delta.cos(), r * delta.sin());
    let text = format!(
        "{HEAD}
#1=CARTESIAN_POINT('',(0.,0.,0.));
#2=DIRECTION('',(0.,0.,1.));
#3=DIRECTION('',(1.,0.,0.));
#5=AXIS2_PLACEMENT_3D('',#1,#2,#3);
#6=CARTESIAN_POINT('',(0.,0.,{h:?}));
#7=AXIS2_PLACEMENT_3D('',#6,#2,#3);
#10=CARTESIAN_POINT('',({r:?},0.,0.));
#11=CARTESIAN_POINT('',({c:?},{s:?},0.));
#12=CARTESIAN_POINT('',({r:?},0.,{h:?}));
#13=CARTESIAN_POINT('',({c:?},{s:?},{h:?}));
#14=VERTEX_POINT('',#10);
#15=VERTEX_POINT('',#11);
#16=VERTEX_POINT('',#12);
#17=VERTEX_POINT('',#13);
#20=CIRCLE('',#5,{r:?});
#21=CIRCLE('',#7,{r:?});
#22=VECTOR('',#2,1.);
#23=LINE('',#10,#22);
#24=LINE('',#11,#22);
#30=EDGE_CURVE('',#14,#15,#20,.T.);
#31=EDGE_CURVE('',#15,#14,#20,.T.);
#32=EDGE_CURVE('',#16,#17,#21,.T.);
#33=EDGE_CURVE('',#17,#16,#21,.T.);
#34=EDGE_CURVE('',#14,#16,#23,.T.);
#35=EDGE_CURVE('',#15,#17,#24,.T.);
#40=ORIENTED_EDGE('',*,*,#31,.F.);
#41=ORIENTED_EDGE('',*,*,#30,.F.);
#42=EDGE_LOOP('',(#40,#41));
#43=FACE_OUTER_BOUND('',#42,.T.);
#44=PLANE('',#5);
#45=ADVANCED_FACE('Bottom',(#43),#44,.F.);
#50=ORIENTED_EDGE('',*,*,#32,.T.);
#51=ORIENTED_EDGE('',*,*,#33,.T.);
#52=EDGE_LOOP('',(#50,#51));
#53=FACE_OUTER_BOUND('',#52,.T.);
#54=PLANE('',#7);
#55=ADVANCED_FACE('Top',(#53),#54,.T.);
#60=ORIENTED_EDGE('',*,*,#30,.T.);
#61=ORIENTED_EDGE('',*,*,#35,.T.);
#62=ORIENTED_EDGE('',*,*,#32,.F.);
#63=ORIENTED_EDGE('',*,*,#34,.F.);
#64=EDGE_LOOP('',(#60,#61,#62,#63));
#65=FACE_OUTER_BOUND('',#64,.T.);
#66=CYLINDRICAL_SURFACE('',#5,{r:?});
#67=ADVANCED_FACE('Sliver',(#65),#66,.T.);
#70=ORIENTED_EDGE('',*,*,#31,.T.);
#71=ORIENTED_EDGE('',*,*,#34,.T.);
#72=ORIENTED_EDGE('',*,*,#33,.F.);
#73=ORIENTED_EDGE('',*,*,#35,.F.);
#74=EDGE_LOOP('',(#70,#71,#72,#73));
#75=FACE_OUTER_BOUND('',#74,.T.);
#77=ADVANCED_FACE('Wall',(#75),#66,.T.);
#80=CLOSED_SHELL('',(#45,#55,#67,#77));
#81=MANIFOLD_SOLID_BREP('Can',#80);
#82=SHAPE_REPRESENTATION('',(#81),$);
{TAIL}"
    );
    let imp = step_import(&text).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let can = &imp.bodies[0];
    assert!(can.closed && can.refused.is_none() && !can.body.is_surface(), "{:?} {:?}", can.refused, imp.warnings);
    let pi = std::f64::consts::PI;
    let m = measure(&can.body).unwrap();
    assert!(rel(m.volume, pi * r * r * h) < 1e-6, "{}", m.volume);
    assert!(rel(m.area, 2.0 * pi * r * r + 2.0 * pi * r * h) < 1e-6, "{}", m.area);
}

/// A ring torus (R 10, r 3) about z, as a closed solid: with `tube` its whole turn about the axis
/// and its tube from `tube.0` for `tube.1` radians, the gap across the tube closed by a cone
/// through the two circles where the tube ends (face #53, its seam a tube arc, the cone #57);
/// without it the whole tube for `sweep` radians from `start` about the axis, closed by two discs
/// (its seam an arc of the outer equator).
fn torus_part(tube: Option<(f64, f64)>, (start, sweep): (f64, f64)) -> String {
    let (big, small) = (10.0, 3.0);
    let p = |x: f64, y: f64, z: f64| format!("({x:.17},{y:.17},{z:.17})");
    let head = format!(
        "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n#50=TOROIDAL_SURFACE('',#4,{big:?},{small:?});\n"
    );
    let tail = "#80=CLOSED_SHELL('',(#53,#57,#61));\n#81=MANIFOLD_SOLID_BREP('Torus part',#80);\n#82=SHAPE_REPRESENTATION('',(#81),$);\n";
    if let Some((v0, dv)) = tube {
        // The circles about the axis at the tube's two ends, and the cone through them from the
        // narrower one, its axis toward the wider; its sense outward from the tube.
        let v1 = v0 + dv;
        let ring = |v: f64| (big + small * v.cos(), small * v.sin());
        let ((ra, za), (rb, zb)) = (ring(v0), ring(v1));
        let ((r_near, z_near), (r_far, z_far)) = if ra < rb { ((ra, za), (rb, zb)) } else { ((rb, zb), (ra, za)) };
        let k = (z_far - z_near).signum();
        let semi = ((r_far - r_near) / (z_far - z_near).abs()).atan();
        // The cone's natural normal at u = 0, (1, 0, −k tan) in (radial, axial), against the way
        // out of the tube at the chord's middle.
        let (mx, mz) = (0.5 * (ra + rb) - big, 0.5 * (za + zb));
        let sense = if mx - k * semi.tan() * mz > 0.0 { "T" } else { "F" };
        format!(
            "{head}#5=DIRECTION('',(0.,0.,{k:.1}));
#6=CARTESIAN_POINT('',{});
#7=AXIS2_PLACEMENT_3D('',#6,#2,#3);
#8=CARTESIAN_POINT('',{});
#9=AXIS2_PLACEMENT_3D('',#8,#2,#3);
#10=VERTEX_POINT('',#11);
#11=CARTESIAN_POINT('',{});
#12=VERTEX_POINT('',#13);
#13=CARTESIAN_POINT('',{});
#30=CIRCLE('',#7,{ra:.17});
#31=CIRCLE('',#9,{rb:.17});
#40=EDGE_CURVE('',#10,#10,#30,.T.);
#41=EDGE_CURVE('',#12,#12,#31,.T.);
#14=CARTESIAN_POINT('',({big:?},0.,0.));
#15=DIRECTION('',(0.,-1.,0.));
#16=AXIS2_PLACEMENT_3D('',#14,#15,#3);
#32=CIRCLE('',#16,{small:?});
#42=EDGE_CURVE('',#10,#12,#32,.T.);
#200=ORIENTED_EDGE('',*,*,#40,.T.);
#201=ORIENTED_EDGE('',*,*,#41,.F.);
#204=ORIENTED_EDGE('',*,*,#42,.T.);
#205=ORIENTED_EDGE('',*,*,#42,.F.);
#51=EDGE_LOOP('',(#200,#204,#201,#205));
#54=FACE_OUTER_BOUND('',#51,.T.);
#53=ADVANCED_FACE('Tube',(#54),#50,.T.);
#202=ORIENTED_EDGE('',*,*,#40,.F.);
#203=ORIENTED_EDGE('',*,*,#41,.T.);
#56=EDGE_LOOP('',(#202));
#58=EDGE_LOOP('',(#203));
#59=FACE_OUTER_BOUND('',#56,.T.);
#60=FACE_BOUND('',#58,.T.);
#62=CARTESIAN_POINT('',{});
#63=AXIS2_PLACEMENT_3D('',#62,#5,#3);
#64=CONICAL_SURFACE('',#63,{r_near:.17},{semi:.17});
#57=ADVANCED_FACE('Gap',(#59,#60),#64,.{sense}.);
#80=CLOSED_SHELL('',(#53,#57));
#81=MANIFOLD_SOLID_BREP('Torus part',#80);
#82=SHAPE_REPRESENTATION('',(#81),$);
{TAIL}",
            p(0.0, 0.0, za),
            p(0.0, 0.0, zb),
            p(ra, 0.0, za),
            p(rb, 0.0, zb),
            p(0.0, 0.0, z_near),
        )
    } else {
        // A tube circle in each end's meridian plane, run with the tube angle, and the discs.
        let end = |u: f64| {
            let (c, s) = (u.cos(), u.sin());
            ((big * c, big * s), (c, s), (s, -c))
        };
        let (((x0, y0), (c0, s0), (a0x, a0y)), ((x1, y1), (c1, s1), (a1x, a1y))) = (end(start), end(start + sweep));
        format!(
            "{head}#6=CARTESIAN_POINT('',{});
#7=DIRECTION('',{});
#8=DIRECTION('',{});
#9=AXIS2_PLACEMENT_3D('',#6,#7,#8);
#14=CARTESIAN_POINT('',{});
#15=DIRECTION('',{});
#16=DIRECTION('',{});
#17=AXIS2_PLACEMENT_3D('',#14,#15,#16);
#10=VERTEX_POINT('',#11);
#11=CARTESIAN_POINT('',{});
#12=VERTEX_POINT('',#13);
#13=CARTESIAN_POINT('',{});
#30=CIRCLE('',#9,{small:?});
#31=CIRCLE('',#17,{small:?});
#40=EDGE_CURVE('',#10,#10,#30,.T.);
#41=EDGE_CURVE('',#12,#12,#31,.T.);
#32=CIRCLE('',#4,{:.17});
#43=EDGE_CURVE('',#10,#12,#32,.T.);
#200=ORIENTED_EDGE('',*,*,#40,.F.);
#201=ORIENTED_EDGE('',*,*,#41,.T.);
#204=ORIENTED_EDGE('',*,*,#43,.T.);
#205=ORIENTED_EDGE('',*,*,#43,.F.);
#51=EDGE_LOOP('',(#204,#201,#205,#200));
#54=FACE_OUTER_BOUND('',#51,.T.);
#53=ADVANCED_FACE('Tube',(#54),#50,.T.);
#202=ORIENTED_EDGE('',*,*,#40,.T.);
#56=EDGE_LOOP('',(#202));
#59=FACE_OUTER_BOUND('',#56,.T.);
#64=PLANE('',#9);
#57=ADVANCED_FACE('Start',(#59),#64,.T.);
#203=ORIENTED_EDGE('',*,*,#41,.F.);
#58=EDGE_LOOP('',(#203));
#60=FACE_OUTER_BOUND('',#58,.T.);
#65=DIRECTION('',{});
#66=AXIS2_PLACEMENT_3D('',#14,#65,#16);
#67=PLANE('',#66);
#61=ADVANCED_FACE('End',(#60),#67,.T.);
{tail}{TAIL}",
            p(x0, y0, 0.0),
            p(a0x, a0y, 0.0),
            p(c0, s0, 0.0),
            p(x1, y1, 0.0),
            p(a1x, a1y, 0.0),
            p(c1, s1, 0.0),
            p(x0 + small * c0, y0 + small * s0, 0.0),
            p(x1 + small * c1, y1 + small * s1, 0.0),
            big + small,
            p(-a1x, -a1y, 0.0),
        )
    }
}

/// A torus face round most of its tube off the tube's quarter lines (326° of the tube from
/// −113.8°, as an exhaust collector has it) needed a fifth quarter along the tube: its form ran
/// past a full turn of the tube and SMLib refused the solid. Its tube start moves to where the
/// face's tube begins: the same torus, a form of four quarters along the tube, and the solid with
/// its exact volume and area. A face round the whole tube (300° about the axis from 75°, which
/// turns its frame instead) keeps its tube start and its four quarters.
#[test]
fn a_torus_face_round_most_of_its_tube_is_formed_within_one_turn_of_it() {
    use std::f64::consts::PI;
    let (big, small) = (10.0f64, 3.0f64);
    let (v0, dv) = ((-113.8f64).to_radians(), 326f64.to_radians());
    for (tube, about) in [(Some((v0, dv)), (0.0, 2.0 * PI)), (None, (75f64.to_radians(), 300f64.to_radians()))] {
        let imp = step_import(&torus_part(tube, about)).unwrap();
        assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
        let b = &imp.bodies[0];
        assert!(b.closed && b.refused.is_none(), "{tube:?}: {:?} {:?}", b.refused, imp.warnings);
        let face = b.face_origin.iter().position(|f| *f == 53).unwrap();
        let record = b.body.record();
        let Some(solidcraft_geom::brep::Surface::RationalSpline { v_knots, .. }) = &record.faces[face].nurbs else { panic!("the tube's form") };
        assert!(v_knots[v_knots.len() - 1] - v_knots[0] <= 4.0, "{tube:?}: the tube's form spans {v_knots:?}");
        let m = measure(&b.body).unwrap();
        let (volume, area) = match tube {
            Some((v0, dv)) => {
                // The whole torus less the segment of the tube the cone cuts off, revolved
                // (Pappus); the tube's face and the cone's.
                let gap = 2.0 * PI - dv;
                let segment = 0.5 * small * small * (gap - gap.sin());
                let centroid = 4.0 * small * (0.5 * gap).sin().powi(3) / (3.0 * (gap - gap.sin()));
                let middle = v0 + dv + 0.5 * gap;
                let volume = 2.0 * PI * PI * big * small * small - segment * 2.0 * PI * (big + centroid * middle.cos());
                let ring = |v: f64| big + small * v.cos();
                let chord = 2.0 * small * (0.5 * gap).sin();
                let area = 2.0 * PI * small * (big * dv + small * ((v0 + dv).sin() - v0.sin())) + PI * (ring(v0) + ring(v0 + dv)) * chord;
                (volume, area)
            }
            None => {
                assert_eq!(v_knots[v_knots.len() - 1] - v_knots[0], 4.0, "a whole tube is four quarters");
                let sweep = about.1;
                (PI * small * small * big * sweep, sweep * 2.0 * PI * small * big + 2.0 * PI * small * small)
            }
        };
        assert!(rel(m.volume, volume) < 1e-6, "{tube:?}: volume {} vs {volume}", m.volume);
        assert!(rel(m.area, area) < 1e-6, "{tube:?}: area {} vs {area}", m.area);
    }
}
