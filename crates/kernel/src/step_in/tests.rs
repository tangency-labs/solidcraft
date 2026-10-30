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
    let mesh = body.body.display_mesh(radius * 1e-3).unwrap();
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

#[test]
fn non_antipodal_seam_sphere_does_not_panic() {
    let text = seam_sphere_step(Vec3::ZERO, 2.0, Vec3::Z, Vec3::X, 0.0, false, 2.5);
    let result = std::panic::catch_unwind(|| step_import(&text));
    assert!(result.is_ok());
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
    let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 0.01, snap: 0.01 };
    let mut items: Vec<_> = ex.entities.iter().filter(|(_, e)| e.name() == "MANIFOLD_SOLID_BREP").map(|(id, _)| *id).collect();
    items.sort();
    let start = std::time::Instant::now();
    let solids: Vec<_> = items.iter().map(|id| guard("benchmark read", || brep::read_item(&cx, *id).map_err(KernelError::Failed)).unwrap()).collect();
    let read = start.elapsed();
    let start = std::time::Instant::now();
    for solid in solids {
        std::hint::black_box(Body::new(solid.solid).unwrap().unmeshed_faces());
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
        let (a, b) = (a.body.display_mesh(0.05).unwrap(), b.body.display_mesh(0.05).unwrap());
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
    let units = DEFAULT_UNITS;
    let placement = mt::Matrix4::from_translation(mt::Vector3::new(10.0, 20.0, 30.0));
    // The second item is not in the file.
    let jobs: Vec<_> = [solid, 999_999_999, solid, solid]
        .into_iter()
        .map(|item| parallel::Job { source: parallel::Source::Item { item, units }, placement, check_mesh: true, tol: units.tol })
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
        reader.rep_bodies(60, &mt::Matrix4::from_scale(1.0), &[], "Budget", None, &mut out, 0);
        assert!(reader.oriented.is_empty() && reader.unreadable.is_empty(), "the walk only plans the solids");
        while reader.convert_next(workers == 1) {}
        let out = reader.remap(&out);
        assert_eq!(reader.bodies.len(), MAX_BODIES);
        assert_eq!(out, vec![MAX_BODIES - 2, MAX_BODIES - 1]);
        let (first, second) = (&reader.bodies[MAX_BODIES - 2], &reader.bodies[MAX_BODIES - 1]);
        assert_eq!((first.source_item, first.name.as_str()), (2001, "First"));
        assert_eq!((second.source_item, second.name.as_str()), (2002, "Second"));
        assert_eq!(first.geometry_item, second.geometry_item, "both occurrences reuse shell #100");
        assert_eq!(reader.oriented.len(), 1, "one prepared shell still consumes two body slots");
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
    // Pieces of one analytic surface are written as one face (volume and area above check
    // that nothing was lost).
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

/// Robustness seed 17: a thin three-quarter tube shelled with its bottom open. truck writes each
/// wall as three quarter-turn B-spline pieces; the export merges them into one 270° face and must
/// write that face's cylinder, not the first piece's quarter sheet. The seam direction came from
/// the mean direction of the face's vertices, which is zero at 0°, 90°, 180° and 270°, so the
/// merged face kept the quarter sheet and read back about 7% short with no warning.
#[test]
fn shelled_three_quarter_tube_round_trips() {
    let rect =
        Region2 { outer: Loop2::polygon(&[Vec2::new(10.0, 0.0), Vec2::new(12.5, 0.0), Vec2::new(12.5, 27.5), Vec2::new(10.0, 27.5)]), holes: vec![] };
    let tube = revolve(&Plane::XZ, &[rect], Vec2::ZERO, Vec2::Y, 1.5 * std::f64::consts::PI).unwrap().pop().unwrap();
    let b = crate::shell(&tube, &[Vec3::new(-7.182422893157272, 7.182422893245109, 0.0)], 1.0).unwrap();
    round_trip(&b);
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

/// Original fixtures: elementary basis surfaces plus offsets, with no external CAD files.
fn offset_geometry(definition: &str, offset: &str, len: f64, sense: bool, points: &[mt::Point3]) -> crate::Result<mt::Surface> {
    let text = format!(
        "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=AXIS2_PLACEMENT_3D('',#1,$,$);\n#3={definition};\n#4=OFFSET_SURFACE('',#3,{offset},.U.);\n{TAIL}"
    );
    let ex = p21::parse(&text).unwrap();
    let cx = Ctx { ex: &ex, len, ang: 1.0, tol: 1e-6, snap: 1e-6 };
    crate::guard("test offset surface", || geom::surface(&cx, 4, points, &[], sense, 0).map_err(KernelError::Failed))
}

#[test]
fn offset_surface_analytic_geometry_and_orientation() {
    use mt::{InnerSpace, MetricSpace, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let angle: f64 = 0.4;
    let cases = [
        ("PLANE('',#2)".to_string(), mt::Point3::new(2.0, 3.0, 1.0), mt::Vector3::unit_z()),
        ("CYLINDRICAL_SURFACE('',#2,5.)".to_string(), mt::Point3::new(6.0, 0.0, 2.0), mt::Vector3::unit_x()),
        ("SPHERICAL_SURFACE('',#2,5.)".to_string(), mt::Point3::new(6.0, 0.0, 0.0), mt::Vector3::unit_x()),
        ("TOROIDAL_SURFACE('',#2,10.,2.)".to_string(), mt::Point3::new(13.0, 0.0, 0.0), mt::Vector3::unit_x()),
        (
            format!("CONICAL_SURFACE('',#2,5.,{angle})"),
            mt::Point3::new(5.0 + 1.0 / angle.cos() + 2.0 * angle.tan(), 0.0, 2.0),
            mt::Vector3::new(angle.cos(), 0.0, -angle.sin()),
        ),
    ];
    for (definition, point, normal) in cases {
        for sense in [true, false] {
            let extent = [point, point + mt::Vector3::unit_z()];
            let surface = offset_geometry(&definition, "1.", 1.0, sense, &extent).unwrap();
            let uv = surface.search_nearest_parameter(point, None, 100).unwrap();
            assert!(surface.subs(uv.0, uv.1).distance(point) < 1e-6, "{definition}");
            assert!(surface.normal(uv.0, uv.1).dot(normal) * if sense { 1.0 } else { -1.0 } > 0.99, "{definition}, sense {sense}");
        }
    }
    let p = mt::Point3::new(40.0, 0.0, 2.0);
    let surface = offset_geometry("CYLINDRICAL_SURFACE('',#2,5.)", "-1.", 10.0, true, &[p, p + mt::Vector3::unit_z()]).unwrap();
    let uv = surface.search_nearest_parameter(p, None, 100).unwrap();
    assert!(surface.subs(uv.0, uv.1).distance(p) < 1e-6);
}

#[test]
fn offset_surfaces_nested_and_hostile() {
    let points = [mt::Point3::new(0.0, 0.0, 0.0), mt::Point3::new(0.0, 0.0, 1.0)];
    for (basis, offset) in [
        ("CYLINDRICAL_SURFACE('',#2,5.)", "-5."),
        ("SPHERICAL_SURFACE('',#2,5.)", "-6."),
        ("TOROIDAL_SURFACE('',#2,10.,2.)", "8."),
        ("TOROIDAL_SURFACE('',#2,10.,2.)", "-2."),
        ("CONICAL_SURFACE('',#2,5.,0.4)", "-10."),
        ("PLANE('',#2)", "1.E100"),
        ("PLANE('',#2)", "$"),
    ] {
        assert!(offset_geometry(basis, offset, 1.0, true, &points).is_err(), "{basis}, {offset}");
    }
    let text = format!("{HEAD}#1=OFFSET_SURFACE('',#1,1.,.F.);{TAIL}");
    let ex = p21::parse(&text).unwrap();
    let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 1e-6, snap: 1e-6 };
    assert!(geom::surface(&cx, 1, &points, &[], true, 0).unwrap_err().contains("nested too deeply"));
    let text = format!(
        "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));#2=AXIS2_PLACEMENT_3D('',#1,$,$);#3=CYLINDRICAL_SURFACE('',#2,5.);#4=OFFSET_SURFACE('',#3,-6.,.F.);#5=OFFSET_SURFACE('',#4,6.,.F.);{TAIL}"
    );
    let ex = p21::parse(&text).unwrap();
    let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 1e-6, snap: 1e-6 };
    assert!(geom::surface(&cx, 5, &points, &[], true, 0).is_err());
}

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

/// A hand-written unit cube brep (`#100`…) in a representation with the given unit and items.
fn cube_entities() -> String {
    let mut s = String::new();
    let pts = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)];
    for (i, (x, y, z)) in pts.iter().enumerate() {
        s += &format!("#{}=CARTESIAN_POINT('',({x}.,{y}.,{z}.));\n#{}=VERTEX_POINT('',#{});\n", 200 + i, 210 + i, 200 + i);
    }
    // 12 edges as lines (direction vector filler; trimmed by vertices).
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    s += "#290=DIRECTION('',(1.,0.,0.));\n#291=VECTOR('',#290,1.);\n";
    for (k, (a, b)) in edges.iter().enumerate() {
        s += &format!("#{}=LINE('',#{},#291);\n#{}=EDGE_CURVE('',#{},#{},#{},.T.);\n", 300 + k, 200 + a, 320 + k, 210 + a, 210 + b, 300 + k);
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
    crate::body::ORIENTED.with(|c| c.set(0));
    let imp = step_import_with_workers(&t, 1).unwrap();
    assert_eq!(crate::body::ORIENTED.with(|c| c.get()), 1, "one distinct part");
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
/// two parts in turn converts in a few steps of many solids (the first finds both parts'
/// orientations side by side on the workers, the next places every other instance), where
/// converting instance by instance took one step each.
#[test]
fn distinct_parts_convert_together_across_instances() {
    let n = 24;
    let t = instanced_assembly_of(2, n);
    let mut reading = StepReading::with_workers(&t, 8).unwrap();
    let mut steps = 0;
    while reading.step() {
        steps += 1;
    }
    assert_eq!(steps, 2, "both parts in the first step, every other instance in the second");
    let threaded = reading.finish().unwrap();
    assert_eq!(threaded.bodies.len(), n as usize);
    assert!(threaded.warnings.is_empty(), "{:?}", threaded.warnings);
    crate::body::ORIENTED.with(|c| c.set(0));
    let serial = step_import_with_workers(&t, 1).unwrap();
    assert_eq!(crate::body::ORIENTED.with(|c| c.get()), 1, "different products share exactly identical geometry");
    assert_same_import(&serial, &threaded);
    assert_eq!(threaded.bodies[1].name, "Cube1");
    assert_eq!(threaded.bodies[1].path, vec!["Assembly".to_string(), "Block1".to_string()]);
}

#[test]
fn exact_source_copies_restore_original_topology_and_reject_a_changed_coordinate() {
    let cube = box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap();
    let different = box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.0001)).unwrap();
    let text = step_export(&[&cube, &cube, &different], "Copies").unwrap();
    let imported = step_import_shared(&text).unwrap();
    assert_eq!(imported.bodies.len(), 3);
    assert_eq!(imported.bodies[0].geometry_item, imported.bodies[1].geometry_item);
    assert_ne!(imported.bodies[0].geometry_item, imported.bodies[2].geometry_item);
    assert_ne!(imported.bodies[0].face_origin, imported.bodies[1].face_origin);
    let source = StepSource::compact(text.clone(), &imported);
    assert!(matches!(source, StepSource::Shared(_)), "the repeated topology should be stored once");
    let reopened: StepSource = serde_json::from_str(&serde_json::to_string(&source).unwrap()).unwrap();
    // Expanding reads and verifies the library directly, independent of the recent-import cache.
    let expanded = reopened.expanded_text().unwrap();
    assert_eq!(p21::parse(&text).unwrap().entities, p21::parse(&expanded).unwrap().entities);
    let cold = StepReading::with_workers(&expanded, 1).unwrap().finish_shared().unwrap();
    assert_same_import(&imported, &cold);
    let selected = reopened.import_only(&[2]).unwrap();
    assert_eq!(selected.bodies.len(), 1);
    assert!(rel(measure(&selected.bodies[0].body).unwrap().volume, 24.0006) < 1e-9);
    let mut corrupt = serde_json::to_value(&source).unwrap();
    corrupt["copies"][0]["entities"][0][0] = serde_json::json!(0);
    let corrupt: StepSource = serde_json::from_value(corrupt).unwrap();
    assert!(corrupt.expanded_text().is_err());
}

#[test]
fn coincident_but_distinct_topology_is_not_a_source_copy() {
    // Both graphs have the same recursively expanded values. Only one reuses its point, so
    // their topology is not interchangeable even though a structural digest can match them.
    let text = format!(
        "{HEAD}#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(0.,0.,0.));\n\
        #3=EDGE_CURVE('',#1,#1,#1,.T.);#4=EDGE_CURVE('',#1,#2,#1,.T.);\n\
        #5=MANIFOLD_SOLID_BREP('Same',#3);#6=MANIFOLD_SOLID_BREP('Same',#4);{TAIL}"
    );
    let ex = p21::parse(&text).unwrap();
    assert!(sharing::find(&ex).copies.is_empty());
}

#[test]
fn matching_source_coordinates_with_different_units_keep_different_geometry() {
    let text = instanced_assembly_of(2, 2).replace(
        "#1110=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#1120,#1115),#20);",
        "#1110=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#1120,#1115),#30);",
    );
    let imported = StepReading::with_workers(&text, 1).unwrap().finish().unwrap();
    assert_eq!(imported.bodies.len(), 2);
    assert_ne!(imported.bodies[0].geometry_item, imported.bodies[1].geometry_item);
    assert!(rel(measure(&imported.bodies[0].body).unwrap().volume, 1000.) < 1e-9);
    assert!(rel(measure(&imported.bodies[1].body).unwrap().volume, 1.) < 1e-9);
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
    let a = first.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap();
    let b = second.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap();
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

#[test]
fn unsupported_face_is_left_out_and_reported() {
    let mut t = String::from(HEAD);
    let cube = cube_entities().replace("#410=PLANE('',#409);", "#410=OFFSET_SURFACE('',#409,1.,.F.);");
    t += &cube;
    t += "#60=ADVANCED_BREP_SHAPE_REPRESENTATION('',(#101),#20);\n";
    t += &context(20, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert_eq!(imp.bodies.len(), 1);
    assert!(!imp.bodies[0].closed);
    assert_eq!(imp.bodies[0].body.face_count(), 5);
    assert!(imp.warnings.iter().any(|w| w.contains("OFFSET_SURFACE")), "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("open body")), "{:?}", imp.warnings);
}

/// A flat square sheet `size` mm across in millimetres (declared accuracy 1e-6 mm) whose right
/// edge is a straight spline ending `gap` mm beside the corner vertex it runs to, as exporters
/// write two curves meeting at a vertex between them. `stray` adds an unused vertex that far
/// out, which makes the model look that large.
fn gapped_sheet(size: f64, gap: f64, stray: Option<f64>) -> String {
    let mut t = String::from(HEAD);
    for (k, (x, y)) in [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)].iter().enumerate() {
        t += &format!("#{}=CARTESIAN_POINT('',({x:.6},{y:.6},0.));\n#{}=VERTEX_POINT('',#{});\n", 20 + k, 30 + k, 20 + k);
    }
    if let Some(far) = stray {
        t += &format!("#38=CARTESIAN_POINT('',({far:.6},0.,0.));\n#39=VERTEX_POINT('',#38);\n");
    }
    t += &format!(
        "#46=CARTESIAN_POINT('',({:.6},{size:.6},0.));\n#47=B_SPLINE_CURVE_WITH_KNOTS('',1,(#21,#46),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);\n",
        size + gap
    );
    t += "#40=DIRECTION('',(1.,0.,0.));\n#41=VECTOR('',#40,1.);\n#50=LINE('',#20,#41);\n#52=LINE('',#22,#41);\n#53=LINE('',#23,#41);\n";
    for (k, curve) in [50, 47, 52, 53].iter().enumerate() {
        t += &format!(
            "#{}=EDGE_CURVE('',#{},#{},#{curve},.T.);\n#{}=ORIENTED_EDGE('',*,*,#{},.T.);\n",
            60 + k,
            30 + k,
            30 + (k + 1) % 4,
            70 + k,
            60 + k
        );
    }
    t += "#80=EDGE_LOOP('',(#70,#71,#72,#73));\n#81=FACE_OUTER_BOUND('',#80,.T.);\n#90=CARTESIAN_POINT('',(0.,0.,0.));\n#91=DIRECTION('',(0.,0.,1.));\n";
    t += "#92=AXIS2_PLACEMENT_3D('',#90,#91,#40);\n#93=PLANE('',#92);\n#82=ADVANCED_FACE('',(#81),#93,.T.);\n";
    t += "#110=OPEN_SHELL('',(#82));\n#111=SHELL_BASED_SURFACE_MODEL('Sheet',(#110));\n#112=SHAPE_REPRESENTATION('',(#111),#10);\n";
    t += &context(10, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    t += TAIL;
    t
}

#[test]
fn a_vertex_off_its_curve_by_a_small_share_of_the_model_is_moved_onto_it() {
    // 0.19 mm on a 1 m sheet: 190 000 times the file's declared accuracy, 1.3e-4 of its size.
    let imp = step_import(&gapped_sheet(1000.0, 0.19, None)).unwrap();
    assert!(!imp.warnings.iter().any(|w| w.contains("off their curve")), "{:?}", imp.warnings);
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let body = &imp.bodies[0].body;
    assert_eq!(body.face_count(), 1);
    // Every edge meets its vertices exactly: the spline's end was moved onto the corner.
    use mt::{BoundedCurve as _, MetricSpace as _};
    for e in body.solid.edge_iter() {
        let c = e.curve();
        assert!(c.front().distance(e.front().point()) < 1e-9 && c.back().distance(e.back().point()) < 1e-9);
    }
    let m = measure(body).unwrap();
    assert!(rel(m.area, 1e6) < 1e-4, "area {}", m.area);
}

#[test]
fn a_vertex_further_off_its_curve_than_the_model_allows_leaves_the_face_out() {
    // 0.19 mm on a 10 mm sheet is 1.3 % of the part; and a stray point a kilometre out cannot
    // stretch the limit past 1 mm.
    for text in [gapped_sheet(10.0, 0.19, None), gapped_sheet(1000.0, 2.0, Some(1e6))] {
        let warnings = match step_import(&text) {
            Ok(imp) => {
                assert!(imp.bodies.is_empty());
                imp.warnings.join("\n")
            }
            Err(e) => e.to_string(),
        };
        assert!(warnings.contains("off their curve"), "{warnings}");
    }
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

#[test]
fn split_band_edges_are_seams() {
    // A quarter pipe (torus R 10, r 3) whose face wraps around the tube with a seam edge: it is
    // split for the kernel, and the split edge is drawn as a seam.
    let mut t = String::from(HEAD);
    t += "#1=CARTESIAN_POINT('',(0.,0.,0.));\n#2=DIRECTION('',(0.,0.,1.));\n#3=DIRECTION('',(1.,0.,0.));\n#4=AXIS2_PLACEMENT_3D('',#1,#2,#3);\n";
    t += "#5=TOROIDAL_SURFACE('',#4,10.,3.);\n";
    t += "#10=CARTESIAN_POINT('',(13.,0.,0.));\n#11=CARTESIAN_POINT('',(0.,13.,0.));\n#12=VERTEX_POINT('',#10);\n#13=VERTEX_POINT('',#11);\n";
    // Tube circles at azimuth 0 (plane XZ) and 90 degrees (plane YZ), the seam around Z.
    t += "#20=CARTESIAN_POINT('',(10.,0.,0.));\n#21=DIRECTION('',(0.,-1.,0.));\n#22=AXIS2_PLACEMENT_3D('',#20,#21,#3);\n#23=CIRCLE('',#22,3.);\n";
    t += "#24=CARTESIAN_POINT('',(0.,10.,0.));\n#25=DIRECTION('',(1.,0.,0.));\n#26=DIRECTION('',(0.,1.,0.));\n#27=AXIS2_PLACEMENT_3D('',#24,#25,#26);\n#28=CIRCLE('',#27,3.);\n";
    t += "#29=CIRCLE('',#4,13.);\n";
    t += "#30=EDGE_CURVE('',#12,#12,#23,.T.);\n#31=EDGE_CURVE('',#13,#13,#28,.T.);\n#32=EDGE_CURVE('',#12,#13,#29,.T.);\n";
    t += "#40=ORIENTED_EDGE('',*,*,#30,.T.);\n#41=ORIENTED_EDGE('',*,*,#32,.T.);\n#42=ORIENTED_EDGE('',*,*,#31,.F.);\n#43=ORIENTED_EDGE('',*,*,#32,.F.);\n";
    t += "#44=EDGE_LOOP('',(#40,#41,#42,#43));\n#45=FACE_OUTER_BOUND('',#44,.T.);\n#46=ADVANCED_FACE('',(#45),#5,.F.);\n";
    t += "#47=PLANE('',#22);\n#48=ORIENTED_EDGE('',*,*,#30,.F.);\n#49=EDGE_LOOP('',(#48));\n#50=FACE_OUTER_BOUND('',#49,.T.);\n#51=ADVANCED_FACE('',(#50),#47,.T.);\n";
    t += "#52=PLANE('',#27);\n#53=ORIENTED_EDGE('',*,*,#31,.T.);\n#54=EDGE_LOOP('',(#53));\n#55=FACE_OUTER_BOUND('',#54,.T.);\n#56=ADVANCED_FACE('',(#55),#52,.T.);\n";
    t += "#60=CLOSED_SHELL('',(#46,#51,#56));\n#61=MANIFOLD_SOLID_BREP('Pipe',#60);\n#62=SHAPE_REPRESENTATION('',(#61),$);\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    let ib = &imp.bodies[0];
    assert_eq!(ib.file_faces, 3);
    assert_eq!(ib.body.face_count(), 4, "{:?}", imp.warnings);
    // The two halves of the tube face make a quarter torus: area π²·R·r.
    let curved: f64 = ib.body.faces(1e-3).unwrap().iter().filter(|f| f.plane_normal.is_none()).map(|f| f.area).sum();
    let want = std::f64::consts::PI.powi(2) * 30.0;
    assert!(rel(curved, want) < 2e-3, "{curved} vs {want}");
    let m = ib.body.display_mesh(ib.body.size() * 1e-3).unwrap();
    // Seams: the file's own seam (inside the tube face) and the split edge.
    assert!(m.seams.iter().filter(|s| **s).count() >= 2, "{:?}", m.seams);
}

/// A five-sided tube on a B-spline surface closed around (radius `r`, height `h`), as one band
/// face bounded by ring edges `off` mm outside the surface, with a seam edge up one corner used
/// both ways; plus the two flat ends on the planes the rings lie in. The file declares
/// `uncertainty` mm of uncertainty of its own, which is what tells a reader how far the rings
/// may lie off the surface.
///
/// The part's size, the rings' off-surface distance and the declared uncertainty are separate on
/// purpose: a real file's slop stays a few micrometres whether the part is 20 mm or 2 m across
/// (see `off_surface_budget_follows_the_file_not_the_part_size`).
#[derive(Clone, Copy)]
struct RingEdgeTube {
    /// Radius of the surface (mm).
    r: f64,
    /// Height of the tube (mm).
    h: f64,
    /// How far outside the surface the ring edges run (mm).
    off: f64,
    /// The uncertainty the file declares for itself (mm).
    uncertainty: f64,
}

impl RingEdgeTube {
    /// A 20 mm part with a few micrometres of slop and a micrometre of declared uncertainty:
    /// what a well-behaved writer puts out.
    fn small() -> RingEdgeTube {
        RingEdgeTube { r: 10.0, h: 8.0, off: 0.004, uncertainty: 1e-3 }
    }

    /// The same part, and the same slop, scaled to about 2 m across.
    fn large() -> RingEdgeTube {
        RingEdgeTube { r: 1000.0, h: 800.0, ..RingEdgeTube::small() }
    }

    /// The volume the closed pentagon prism encloses (mm³).
    fn volume(&self) -> f64 {
        2.5 * self.r * self.r * (std::f64::consts::TAU / 5.0).sin() * self.h
    }

    /// The area of the five sides and the two five-sided ends together (mm²).
    fn area(&self) -> f64 {
        let side = 2.0 * self.r * (std::f64::consts::TAU / 10.0).sin();
        5.0 * side * self.h + 5.0 * side * self.r * (std::f64::consts::TAU / 10.0).cos()
    }

    fn step(&self) -> String {
        ring_edge_tube_step(self.r, self.h, self.off, self.uncertainty)
    }
}

fn ring_edge_tube_step(r: f64, h: f64, off: f64, uncertainty: f64) -> String {
    let corner = |k: usize, radius: f64| {
        let a = std::f64::consts::TAU * (k % 5) as f64 / 5.0;
        (radius * a.cos(), radius * a.sin())
    };
    let mut t = String::from(HEAD);
    // Control net: the corners around (the first repeated to close it) x the two ends.
    let mut rows = Vec::new();
    for k in 0..=5 {
        let (x, y) = corner(k, r);
        let id = 100 + 2 * k as u64;
        t += &format!("#{id}=CARTESIAN_POINT('',({x:.9},{y:.9},0.));\n#{}=CARTESIAN_POINT('',({x:.9},{y:.9},{h:.9}));\n", id + 1);
        rows.push(format!("(#{id},#{})", id + 1));
    }
    t += &format!(
        "#200=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,({}),.UNSPECIFIED.,.T.,.F.,.F.,(2,1,1,1,1,2),(2,2),(0.,1.,2.,3.,4.,5.),(0.,1.),.UNSPECIFIED.);\n",
        rows.join(",")
    );
    // The rings, `off` mm outside the surface: each is one closed polyline edge.
    for k in 0..5u64 {
        let (x, y) = corner(k as usize, r + off);
        t += &format!("#{}=CARTESIAN_POINT('',({x:.9},{y:.9},0.));\n#{}=CARTESIAN_POINT('',({x:.9},{y:.9},{h:.9}));\n", 300 + k, 310 + k);
    }
    t += "#320=VERTEX_POINT('',#300);\n#321=VERTEX_POINT('',#310);\n";
    t += "#330=POLYLINE('',(#300,#301,#302,#303,#304,#300));\n#331=POLYLINE('',(#310,#311,#312,#313,#314,#310));\n";
    t += "#340=DIRECTION('',(0.,0.,1.));\n#341=VECTOR('',#340,1.);\n#342=LINE('',#300,#341);\n";
    t += "#350=EDGE_CURVE('',#320,#320,#330,.T.);\n#351=EDGE_CURVE('',#321,#321,#331,.T.);\n#352=EDGE_CURVE('',#320,#321,#342,.T.);\n";
    // The band's loop: around the bottom ring, up the seam, back around the top, down the seam.
    t += "#360=ORIENTED_EDGE('',*,*,#350,.T.);\n#361=ORIENTED_EDGE('',*,*,#352,.T.);\n#362=ORIENTED_EDGE('',*,*,#351,.F.);\n#363=ORIENTED_EDGE('',*,*,#352,.F.);\n";
    t += "#364=EDGE_LOOP('',(#360,#361,#362,#363));\n#365=FACE_OUTER_BOUND('',#364,.T.);\n#366=ADVANCED_FACE('',(#365),#200,.T.);\n";
    // The two ends, on the planes the rings lie in.
    t += "#370=CARTESIAN_POINT('',(0.,0.,0.));\n#371=DIRECTION('',(1.,0.,0.));\n#372=AXIS2_PLACEMENT_3D('',#370,#340,#371);\n#373=PLANE('',#372);\n";
    t += "#374=ORIENTED_EDGE('',*,*,#350,.F.);\n#375=EDGE_LOOP('',(#374));\n#376=FACE_OUTER_BOUND('',#375,.T.);\n#377=ADVANCED_FACE('',(#376),#373,.F.);\n";
    t += &format!("#380=CARTESIAN_POINT('',(0.,0.,{h:.9}));\n#381=AXIS2_PLACEMENT_3D('',#380,#340,#371);\n#382=PLANE('',#381);\n");
    t += "#383=ORIENTED_EDGE('',*,*,#351,.T.);\n#384=EDGE_LOOP('',(#383));\n#385=FACE_OUTER_BOUND('',#384,.T.);\n#386=ADVANCED_FACE('',(#385),#382,.T.);\n";
    t += "#390=CLOSED_SHELL('',(#366,#377,#386));\n#391=MANIFOLD_SOLID_BREP('Tube',#390);\n#392=SHAPE_REPRESENTATION('',(#391),#20);\n";
    // What the file declares about its own numbers. This, and not the part's size, is how far a
    // reader may let the ring edges lie off the surface.
    t += "#20=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#21))GLOBAL_UNIT_ASSIGNED_CONTEXT((#22,#23))REPRESENTATION_CONTEXT('',''));\n";
    t += &format!("#21=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({uncertainty:.9}),#22,'','');\n");
    t += "#22=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#23=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n";
    t += TAIL;
    t
}

/// Every check the split band must pass, at whatever size the part is: the band comes in as two
/// faces, every face has triangles at every chord tolerance from coarse to fine, and the four
/// faces together cover the tube once and close it.
///
/// Every tolerance here is a fraction of the body, so the same mesh is asked for at every scale;
/// the rings' off-surface distance is the one thing that stays absolute.
fn assert_ring_edge_tube_meshes(tube: &RingEdgeTube) {
    let imp = step_import(&tube.step()).unwrap();
    assert!(!imp.warnings.iter().any(|w| w.contains("kept whole")), "{:?}", imp.warnings);
    let ib = &imp.bodies[0];
    assert_eq!(ib.file_faces, 3);
    assert_eq!(ib.body.face_count(), 4, "the band face is split in two: {:?}", imp.warnings);
    assert_eq!(ib.body.unmeshed_faces(), 0, "{:?}", imp.warnings);
    let size = ib.body.size();
    // Every face is meshed, and the four together cover the tube once: five sides of
    // 2*R*sin(36 deg) by the height, plus the two five-sided ends.
    let m = ib.body.display_mesh(size * 2e-3).unwrap();
    let mut area = vec![0.0f64; ib.body.face_count()];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
        if let Some(sum) = area.get_mut(*f as usize) {
            *sum += 0.5 * (b - a).cross(c - a).len();
        }
    }
    let side = 2.0 * tube.r * (std::f64::consts::TAU / 10.0).sin();
    let want = tube.area();
    assert!(rel(area.iter().sum::<f64>(), want) < 5e-3, "{area:?} sums to {} not {want}", area.iter().sum::<f64>());
    // Neither half of the split is a sliver: the split line runs from ring to ring.
    assert!(area.iter().all(|a| *a > 0.2 * side * tube.h), "a face of the split tube is nearly empty: {area:?}");
    // The body closes: its volume is the pentagon prism's. `measure` tessellates far finer than
    // any display does, and the planner used to give up on a boundary running off its surface
    // once the chord tolerance came near the gap. One face without a plan costs the whole body
    // its plan, and the reference mesher cannot mesh a face on a closed B-spline surface at all,
    // so both halves of the band came out with no triangles: the volume was the top cap's alone.
    let vol = measure(&ib.body).unwrap().volume;
    let want_vol = tube.volume();
    assert!(rel(vol, want_vol) < 1e-3, "the split band does not close the body: {vol} not {want_vol}");
    // Every face keeps its triangles at every tolerance, coarse to fine.
    for frac in [2e-3, 4e-4, 4e-5, 1.1e-5, 4e-6] {
        let t = size * frac;
        let plan = ib.body.display_plan(t).unwrap_or_else(|e| panic!("no display plan at tol {t}: {e}"));
        let bare: Vec<u32> = plan.faces.iter().filter(|f| f.triangles.is_empty()).map(|f| f.face).collect();
        assert!(bare.is_empty(), "faces {bare:?} have no triangles at tol {t}");
    }
    // What the viewport and a snapshot actually draw: the display tolerance with a facet angle.
    // The angle divides an edge wherever it kinks, far below the chord tolerance, which is what
    // used to crowd the boundary against a corner of the surface and leave the band undrawn.
    let tol = (size * 1e-3).max(1e-3);
    let shown = ib.body.reference_display_mesh_angular(tol, crate::DISPLAY_ANGLE).unwrap();
    let mut drawn = vec![0.0f64; ib.body.face_count()];
    for (t, f) in shown.triangles.iter().zip(&shown.tri_face) {
        let (a, b, c) = (shown.positions[t[0] as usize], shown.positions[t[1] as usize], shown.positions[t[2] as usize]);
        if let Some(sum) = drawn.get_mut(*f as usize) {
            *sum += 0.5 * (b - a).cross(c - a).len();
        }
    }
    assert!(drawn.iter().all(|a| *a > 0.2 * side * tube.h), "a face of the split tube is not drawn: {drawn:?}");
    assert!(rel(drawn.iter().sum::<f64>(), want) < 5e-3, "{drawn:?} sums to {} not {want}", drawn.iter().sum::<f64>());
}

/// A band face is split although its ring edges run a little off its surface. A file writes
/// edges and surfaces to its own uncertainty, so the two disagree by a few micrometres; the
/// split line lies on the surface and can only reach the surface points below the rings.
#[test]
fn band_faces_split_when_ring_edges_run_off_their_surface() {
    assert_ring_edge_tube_meshes(&RingEdgeTube::small());
}

/// The very same fixture, the very same few micrometres of slop in the file, scaled to a part
/// about 2 m across: it meshes the same way (#1721).
#[test]
fn band_faces_split_when_a_scaled_copy_runs_off_its_surface() {
    assert_ring_edge_tube_meshes(&RingEdgeTube::large());
}

/// How far a boundary may lie off its surface is a hundredth of the part, at every chord
/// tolerance, and more only where the file declares more. A writer's declared accuracy is what it
/// aimed for, not a bound its edges keep (a robot assembly declaring 0.01 µm has edges 10 to
/// 30 µm off), so it never tightens the bound; and it must not depend on the mesh asked for, or a
/// face accepted while the model is far away would drop out when the user zooms in (#1721).
#[test]
fn off_surface_budget_is_a_hundredth_of_the_part_at_every_chord_tolerance() {
    let accepted = |tube: &RingEdgeTube| -> bool {
        let imp = step_import(&tube.step()).unwrap();
        let ib = &imp.bodies[0];
        let size = ib.body.size();
        [2e-3, 4e-6].iter().all(|fraction| ib.body.display_plan(size * fraction).is_ok())
    };
    // A few micrometres, whatever the file declares, at either size.
    for uncertainty in [1e-3, 1e-5] {
        for tube in [RingEdgeTube { uncertainty, ..RingEdgeTube::small() }, RingEdgeTube { uncertainty, ..RingEdgeTube::large() }] {
            assert!(accepted(&tube), "a part {} mm across, {} mm off on {uncertainty} declared", 2.0 * tube.r, tube.off);
        }
    }
    // Within a hundredth of the part: a 20 mm part takes 0.15 mm, a 2 m part 2 mm; past it, no.
    assert!(accepted(&RingEdgeTube { off: 0.15, ..RingEdgeTube::small() }));
    assert!(accepted(&RingEdgeTube { off: 2.0, ..RingEdgeTube::large() }));
    assert!(!accepted(&RingEdgeTube { off: 2.0, ..RingEdgeTube::small() }), "a 20 mm part accepts a boundary 2 mm off its surface");
    assert!(!accepted(&RingEdgeTube { off: 100.0, ..RingEdgeTube::large() }), "a 2 m part accepts a boundary 100 mm off its surface");
    // A file declaring more raises it: half a millimetre on a 20 mm part, refused on a declared
    // micrometre, accepted on a declared 60 µm (ten times which is 0.6 mm).
    let tight = RingEdgeTube { off: 0.5, ..RingEdgeTube::small() };
    assert!(!accepted(&tight));
    assert!(accepted(&RingEdgeTube { uncertainty: 0.06, ..tight }));
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
/// way; reading them with truck's profile-first normal turned faces inside out.
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
    // The same face with the opposite sense is inside out.
    let flipped = t.replace("#36=ADVANCED_FACE('',(#35),#8,.T.);", "#36=ADVANCED_FACE('',(#35),#8,.F.);");
    let bad = step_import(&flipped).unwrap();
    let v = measure(&bad.bodies[0].body).map(|m| m.volume).unwrap_or(0.0);
    assert!(!bad.warnings.is_empty() || rel(v, want) > 1e-2, "{v}");
}

/// A tube revolved about a line it touches (a horn torus, as a tight sweep makes) is written as
/// one rational B-spline face whose loop passes the pole twice, with no zero-length edge there:
/// no TOROIDAL_SURFACE (it would be degenerate) and no surface of revolution.
#[test]
fn horn_torus_written_as_one_spline_face() {
    let prof = Region2 { outer: Loop2::circle(Vec2::new(5.0, 0.0), 5.0), holes: vec![] };
    let b = revolve(&Plane::XZ, &[prof], Vec2::ZERO, Vec2::Y, std::f64::consts::FRAC_PI_2).unwrap().pop().unwrap();
    let text = step_export(&[&b], "t").unwrap();
    assert!(!text.contains("TOROIDAL_SURFACE") && !text.contains("SURFACE_OF_REVOLUTION"));
    assert_eq!(text.matches("ADVANCED_FACE(").count(), 3, "{text}");
    let imp = round_trip(&b);
    let v = std::f64::consts::PI * 25.0 * std::f64::consts::TAU * 5.0 / 4.0;
    assert!(rel(measure(&imp.bodies[0].body).unwrap().volume, v) < 1e-3);
}

/// Booleans and blends build the same body every time, so an export is byte-identical from
/// run to run (truck used to emit a boolean's faces in hash order of their addresses). The
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

/// A slot plate with a round on its top outline (issue #612): the extruded arc ends and the
/// rounds along the straight edges are lone B-spline sheets, written as cylinders bounded by
/// their faces. The rounds' sheets reach far past their edges, and a reader bounding the part by
/// its surfaces saw a 60 mm part as 130 mm long.
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
    assert!(!text.contains("B_SPLINE_SURFACE"), "{text}");
    assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 4);
    assert_eq!(text.matches("TOROIDAL_SURFACE(").count(), 2);
    // No point of the file (surface placements included) lies outside the part.
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
/// (issue #1198). The other end's half turn is still written as a cylinder (its corners alone
/// point both ways: the seam comes from points along its edges). The faces the ramp crosses keep
/// their B-spline sheets, cut down to the faces: the rounds' sheets reached 35 mm past both ends,
/// and readers bounding the part by its surfaces saw a 60 mm part as 130 mm long.
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
    // Both ends' walls and the three rounds along the sides it leaves whole; the two the ramp
    // crosses stay sheets.
    assert_eq!(text.matches("CYLINDRICAL_SURFACE(").count(), 5, "{text}");
    assert_eq!(text.matches("B_SPLINE_SURFACE(").count(), 2, "{text}");
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
    let cone = crate::extrude_tapered(&Plane::XY.offset(5.0), &disc, 3.2, -1.0, -FRAC_PI_4).unwrap();
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

/// Knot multiplicities are expanded into one value each before the knot count is compared with
/// the control points, so the sum has to be checked first: 1,000,000 knots of multiplicity 64
/// against two control points asked for a 500 MB vector (20,000,000 for 10 GB) only to reject
/// it afterwards, and an allocation failure aborts the process where an error does not.
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
        let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 1e-6, snap: 1e-6 };
        geom::curve(&cx, 3, 0)
    };
    let error = read(&curve(1_000_000, 64)).unwrap_err();
    // This diagnostic comes from the check before expansion; wall-clock time depends on CI load.
    assert!(error.contains("knot multiplicities do not match"), "{error}");
    // A degree-1 curve with two control points needs exactly four knot values.
    assert!(read(&curve(2, 2)).is_ok());
    assert!(read(&curve(4, 1)).is_ok());
    assert!(read(&curve(2, 3)).is_err());
}

/// A spline written over a parameter range of a few millionths (as if in metres of a part drawn
/// in millimetres: piecewise cubic Bézier over 5.4e-6, as a STEP part of a humanoid robot
/// writes its edges) is read over a range of 1. Over the file's own range the kernel took knots
/// a millionth apart for one, so trimming an edge to its vertices dropped a control point and
/// left a curve no exact store could keep (every reopen of that design imported it again).
#[test]
fn a_spline_over_a_tiny_parameter_range_is_read_over_one() {
    use mt::{BoundedCurve, Cut, MetricSpace, ParametricCurve};
    let a = 1.8024e-6;
    let knots = [0.0, a, 3.0304e-6, 3.8749e-6, 4.7103e-6, 5.0653e-6, 5.4274e-6];
    let n = 3 * (knots.len() - 1) + 1;
    let points: String = (0..n).map(|i| format!("#{}=CARTESIAN_POINT('',({}.,{}.,0.));\n", 10 + i, i, (i * i) % 7)).collect();
    let refs: Vec<String> = (0..n).map(|i| format!("#{}", 10 + i)).collect();
    let mults: Vec<String> = (0..knots.len()).map(|i| if i == 0 || i + 1 == knots.len() { "4".into() } else { "3".into() }).collect();
    let text = format!(
        "{HEAD}{points}#3=B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,({}),({}),.UNSPECIFIED.);\n{TAIL}",
        refs.join(","),
        mults.join(","),
        knots.iter().map(|k| format!("{k:e}")).collect::<Vec<_>>().join(",")
    );
    let ex = p21::parse(&text).unwrap();
    let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 1e-6, snap: 1e-6 };
    let Ok(geom::CurveGeo::BSpline(c)) = geom::curve(&cx, 3, 0) else { panic!("a B-spline") };
    let (t0, t1) = c.range_tuple();
    assert_eq!((t0, t1), (0.0, 1.0));
    // The same curve: its ends are the first and last control points.
    assert!(c.subs(t0).distance(mt::Point3::new(0.0, 0.0, 0.0)) < 1e-12);
    assert!(c.subs(t1).distance(mt::Point3::new((n - 1) as f64, ((n - 1) * (n - 1) % 7) as f64, 0.0)) < 1e-12);
    // Cut between knots, as an edge is trimmed to its vertices: both pieces keep clamped ends
    // and every control point, and meet where the curve was cut.
    for t in [0.3, 0.33, 0.9, 0.95] {
        let mut head = c.clone();
        let tail = head.cut(t);
        for piece in [&head, &tail] {
            let k: Vec<f64> = piece.knot_vec().iter().copied().collect();
            let d = piece.degree();
            assert_eq!(piece.control_points().len() + d + 1, k.len());
            assert!(k.iter().take(d + 1).all(|x| *x == k[0]) && k.iter().rev().take(d + 1).all(|x| Some(x) == k.last()), "{t}: {k:?}");
        }
        assert!(head.subs(head.range_tuple().1).distance(c.subs(t)) < 1e-9);
        assert!(tail.subs(tail.range_tuple().0).distance(c.subs(t)) < 1e-9);
    }
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
    assert!(imp.warnings.iter().any(|w| w.contains("boundary loops")), "{:?}", imp.warnings);
    assert!(start.elapsed() < std::time::Duration::from_secs(10), "took {:?}", start.elapsed());
}

/// A board-like assembly for issue #883: `SOLIDCRAFT_STEP_PCB_FIXTURE=path` writes it (with
/// `SOLIDCRAFT_STEP_PCB_COMPONENTS` and `SOLIDCRAFT_STEP_PCB_PINS`; 600 and 15 give 52 MB). Every component is
/// its own product with a package body and its own pins (nothing shared), the worst case for
/// a large import.
#[test]
#[ignore = "manual fixture generator for large STEP imports"]
fn generate_board_assembly_fixture() {
    use crate::step_out::{ExportBody, ExportProduct, StepHeader, step_export_products};
    let count: usize = solidcraft_geom::env::var("STEP_PCB_COMPONENTS").map(|s| s.parse().unwrap()).unwrap_or(500);
    let pins: usize = solidcraft_geom::env::var("STEP_PCB_PINS").map(|s| s.parse().unwrap()).unwrap_or(16);
    let mut owned: Vec<Vec<Body>> = Vec::new();
    for c in 0..count {
        let w = 4.0 + (c % 7) as f64 * 0.37;
        let d = 3.0 + (c % 5) as f64 * 0.29;
        let mut bodies = vec![fillet_box(w, d, 1.2 + (c % 3) as f64 * 0.1)];
        for p in 0..pins {
            let side = if p % 2 == 0 { -0.6 } else { d + 0.6 };
            let x = 0.3 + (p / 2) as f64 * (w - 0.6) / (pins / 2).max(1) as f64 + c as f64 * 1e-4;
            bodies.push(cylinder(Vec3::new(x, side, -1.5), Vec3::Z, 0.25, 2.0 + (p % 3) as f64 * 0.01).unwrap());
        }
        owned.push(bodies);
    }
    let mut products: Vec<ExportProduct> = owned
        .iter()
        .enumerate()
        .map(|(c, bodies)| ExportProduct {
            name: format!("U{c}"),
            bodies: bodies.iter().enumerate().map(|(k, b)| ExportBody { name: format!("U{c}_{k}"), body: b, color: Some([0.2, 0.2, 0.2]) }).collect(),
            children: Vec::new(),
        })
        .collect();
    let side = (count as f64).sqrt().ceil() as usize;
    let children = (0..count)
        .map(|c| {
            let (x, y) = ((c % side) as f64 * 12.0, (c / side) as f64 * 10.0);
            (c, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, 1.6, 1.0]], format!("U{c}"))
        })
        .collect();
    products.push(ExportProduct { name: "Board".into(), bodies: Vec::new(), children });
    let root = products.len() - 1;
    let text = step_export_products(&products, root, &StepHeader { file_name: "board.step".into(), ..Default::default() }).unwrap();
    let path = solidcraft_geom::env::var("STEP_PCB_FIXTURE").unwrap_or_else(|| "board.step".into());
    std::fs::write(&path, &text).unwrap();
    println!("wrote {path}: {} bytes, {} components", text.len(), count);
}

fn fillet_box(w: f64, d: f64, h: f64) -> Body {
    let b = box_solid(Vec3::ZERO, Vec3::new(w, d, h)).unwrap();
    let vertical = [Vec3::new(0.0, 0.0, h / 2.0), Vec3::new(w, 0.0, h / 2.0), Vec3::new(w, d, h / 2.0), Vec3::new(0.0, d, h / 2.0)];
    let top = [Vec3::new(w / 2.0, 0.0, h), Vec3::new(w, d / 2.0, h), Vec3::new(w / 2.0, d, h), Vec3::new(0.0, d / 2.0, h)];
    let b = fillet(&b, &vertical, 0.3).unwrap_or(b);
    fillet(&b, &top, 0.1).unwrap_or(b)
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

/// A face that runs all the way around a closed (but not periodic) B-spline surface and carries
/// a zero-width slit of its own: a pair of curves a hundredth of a micrometre apart between the
/// same two vertices, which the loop walks up and straight back down. STEP writers leave such a
/// slit where two surfaces were trimmed apart. It stands away from the surface's join, so the
/// band splitter leaves the face whole and the mesher has to draw it; the boundary then runs
/// along itself in (u, v). That used to fail the planner, and with it the whole body's plan, so
/// the body fell back to the reference mesher, which drew bands folded over themselves and left
/// holes in others.
#[test]
fn band_face_with_a_zero_width_slit_is_still_meshed() {
    const R: f64 = 10.0;
    const H: f64 = 8.0;
    /// How far the slit's two sides stand apart (a hundredth of a micrometre).
    const LENS: f64 = 1.0e-5;
    let corner = |k: usize, r: f64| {
        let a = std::f64::consts::TAU * (k % 5) as f64 / 5.0;
        (r * a.cos(), r * a.sin())
    };
    let mut t = String::from(HEAD);
    // Control net: the five corners around (the first repeated to close it) × the two ends.
    let mut rows = Vec::new();
    for k in 0..=5 {
        let (x, y) = corner(k, R);
        let id = 100 + 2 * k as u64;
        t += &format!("#{id}=CARTESIAN_POINT('',({x:.12},{y:.12},0.));\n#{}=CARTESIAN_POINT('',({x:.12},{y:.12},{H:.1}));\n", id + 1);
        rows.push(format!("(#{id},#{})", id + 1));
    }
    t += &format!(
        "#200=B_SPLINE_SURFACE_WITH_KNOTS('',1,1,({}),.UNSPECIFIED.,.T.,.F.,.F.,(2,1,1,1,1,2),(2,2),(0.,1.,2.,3.,4.,5.),(0.,1.),.UNSPECIFIED.);\n",
        rows.join(",")
    );
    // The seam runs up corner 0; the slit stands at corner 2, where each ring is cut in two.
    let (mx, my) = corner(2, R + LENS);
    t += &format!("#210=CARTESIAN_POINT('',({mx:.12},{my:.12},{:.1}));\n", H * 0.5);
    t += "#300=VERTEX_POINT('',#100);\n#301=VERTEX_POINT('',#101);\n#302=VERTEX_POINT('',#104);\n#303=VERTEX_POINT('',#105);\n";
    t += "#310=DIRECTION('',(0.,0.,1.));\n#311=VECTOR('',#310,1.);\n#312=LINE('',#100,#311);\n";
    // The slit's two sides: straight up corner 2, and back down a hair outside it.
    t += "#313=POLYLINE('',(#104,#105));\n#314=POLYLINE('',(#105,#210,#104));\n";
    t += "#320=POLYLINE('',(#100,#102,#104));\n#321=POLYLINE('',(#104,#106,#108,#100));\n";
    t += "#322=POLYLINE('',(#101,#103,#105));\n#323=POLYLINE('',(#105,#107,#109,#101));\n";
    t += "#330=EDGE_CURVE('',#300,#301,#312,.T.);\n";
    t += "#331=EDGE_CURVE('',#302,#303,#313,.T.);\n#332=EDGE_CURVE('',#303,#302,#314,.T.);\n";
    t += "#333=EDGE_CURVE('',#300,#302,#320,.T.);\n#334=EDGE_CURVE('',#302,#300,#321,.T.);\n";
    t += "#335=EDGE_CURVE('',#301,#303,#322,.T.);\n#336=EDGE_CURVE('',#303,#301,#323,.T.);\n";
    // The band: round the bottom to the slit, up one side of it and down the other, on round the
    // bottom, up the seam, back along the top to the slit, down it and up again, on to the seam.
    t += "#340=ORIENTED_EDGE('',*,*,#333,.T.);\n#341=ORIENTED_EDGE('',*,*,#331,.T.);\n#342=ORIENTED_EDGE('',*,*,#332,.T.);\n";
    t += "#343=ORIENTED_EDGE('',*,*,#334,.T.);\n#344=ORIENTED_EDGE('',*,*,#330,.T.);\n#345=ORIENTED_EDGE('',*,*,#336,.F.);\n";
    t += "#346=ORIENTED_EDGE('',*,*,#331,.F.);\n#347=ORIENTED_EDGE('',*,*,#332,.F.);\n#348=ORIENTED_EDGE('',*,*,#335,.F.);\n";
    t += "#349=ORIENTED_EDGE('',*,*,#330,.F.);\n";
    t += "#350=EDGE_LOOP('',(#340,#341,#342,#343,#344,#345,#346,#347,#348,#349));\n#351=FACE_OUTER_BOUND('',#350,.T.);\n";
    t += "#352=ADVANCED_FACE('',(#351),#200,.T.);\n";
    // The two ends, on the planes the rings lie in.
    t += "#360=CARTESIAN_POINT('',(0.,0.,0.));\n#361=DIRECTION('',(1.,0.,0.));\n#362=AXIS2_PLACEMENT_3D('',#360,#310,#361);\n#363=PLANE('',#362);\n";
    t += "#364=ORIENTED_EDGE('',*,*,#334,.F.);\n#365=ORIENTED_EDGE('',*,*,#333,.F.);\n#366=EDGE_LOOP('',(#364,#365));\n";
    t += "#367=FACE_OUTER_BOUND('',#366,.T.);\n#368=ADVANCED_FACE('',(#367),#363,.F.);\n";
    t += &format!("#370=CARTESIAN_POINT('',(0.,0.,{H:.1}));\n#371=AXIS2_PLACEMENT_3D('',#370,#310,#361);\n#372=PLANE('',#371);\n");
    t += "#373=ORIENTED_EDGE('',*,*,#335,.T.);\n#374=ORIENTED_EDGE('',*,*,#336,.T.);\n#375=EDGE_LOOP('',(#373,#374));\n";
    t += "#376=FACE_OUTER_BOUND('',#375,.T.);\n#377=ADVANCED_FACE('',(#376),#372,.T.);\n";
    t += "#390=CLOSED_SHELL('',(#352,#368,#377));\n#391=MANIFOLD_SOLID_BREP('Slit tube',#390);\n#392=SHAPE_REPRESENTATION('',(#391),#20);\n";
    t += "#20=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#21))GLOBAL_UNIT_ASSIGNED_CONTEXT((#22,#23))REPRESENTATION_CONTEXT('',''));\n";
    t += "#21=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-3),#22,'','');\n";
    t += "#22=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#23=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let ib = &imp.bodies[0];
    assert_eq!(ib.file_faces, 3);
    assert_eq!(ib.body.unmeshed_faces(), 0);
    // Every face is planned: one face the planner gives up on costs the whole body its plan.
    let tol = 0.05;
    assert!(ib.body.display_plan(tol).is_ok(), "{:?}", ib.body.display_plan(tol).err());
    // The mesh covers the tube once: five sides of 2·R·sin(36°) by the height, plus the ends.
    let m = ib.body.display_mesh(tol).unwrap();
    let mut area = vec![0.0f64; ib.body.face_count()];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
        if let Some(sum) = area.get_mut(*f as usize) {
            *sum += 0.5 * (b - a).cross(c - a).len();
        }
    }
    let side = 2.0 * R * (std::f64::consts::TAU / 10.0).sin();
    let want = 5.0 * side * H + 5.0 * side * R * (std::f64::consts::TAU / 10.0).cos();
    assert!(rel(area.iter().sum::<f64>(), want) < 5e-3, "{area:?} sums to {} not {want}", area.iter().sum::<f64>());
    // The band itself is drawn once over: not folded over itself, and with no gap left in it.
    let band: f64 = (0..ib.body.face_count()).filter(|i| ib.face_origin.get(*i) == Some(&352)).map(|i| area[i]).sum();
    assert!(rel(band, 5.0 * side * H) < 5e-3, "the band covers {band} not {}", 5.0 * side * H);
}

/// A face so thin that its whole boundary is two chords: a lens between a straight edge and an
/// arc that bows away from it by a ten-thousandth of a millimetre. The edge sampler rightly
/// leaves each side as one segment, so the boundary comes to two nodes — too few to hold a
/// triangle. The planner used to give up on such a face, and one face it gives up on costs the
/// whole body its plan, so every other face fell back to the reference mesher. STEP writers
/// leave these slivers where two surfaces were trimmed apart.
#[test]
fn a_sliver_face_too_thin_to_triangulate_does_not_cost_the_body_its_plan() {
    /// The lens is shorter than one facet of the body it sits on: its whole boundary is then
    /// two chords, however finely the rest of the body is meshed.
    const L: f64 = 0.05;
    const H: f64 = 60.0;
    /// How far the arc bows away from the straight side, at its middle (above the file's own
    /// uncertainty, so the two sides are not the same curve).
    const BOW: f64 = 5.0e-3;
    let mut t = String::from(HEAD);
    // The lens lies in the xy plane: straight along y = 0, the arc bowing to y = BOW.
    t += &format!("#100=CARTESIAN_POINT('',(0.,0.,0.));\n#101=CARTESIAN_POINT('',(0.,0.,{H:.1}));\n");
    t += &format!("#102=CARTESIAN_POINT('',({L:.6},0.,0.));\n#103=CARTESIAN_POINT('',({L:.6},0.,{H:.1}));\n");
    // A degree-2 spline through the ends with this middle control point bows by BOW (half of it).
    t += &format!(
        "#104=CARTESIAN_POINT('',({:.6},{:.9},0.));\n#105=CARTESIAN_POINT('',({:.6},{:.9},{H:.1}));\n",
        L * 0.5,
        2.0 * BOW,
        L * 0.5,
        2.0 * BOW
    );
    t += "#110=VERTEX_POINT('',#100);\n#111=VERTEX_POINT('',#101);\n#112=VERTEX_POINT('',#102);\n#113=VERTEX_POINT('',#103);\n";
    t += "#114=DIRECTION('',(1.,0.,0.));\n#115=DIRECTION('',(0.,0.,1.));\n#116=DIRECTION('',(0.,1.,0.));\n";
    t += "#120=VECTOR('',#114,1.);\n#121=LINE('',#100,#120);\n#122=LINE('',#101,#120);\n";
    t += "#123=VECTOR('',#115,1.);\n#124=LINE('',#100,#123);\n#125=LINE('',#102,#123);\n";
    t += "#126=B_SPLINE_CURVE_WITH_KNOTS('',2,(#100,#104,#102),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\n";
    t += "#127=B_SPLINE_CURVE_WITH_KNOTS('',2,(#101,#105,#103),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);\n";
    t += "#130=EDGE_CURVE('',#110,#112,#121,.T.);\n#131=EDGE_CURVE('',#111,#113,#122,.T.);\n";
    t += "#132=EDGE_CURVE('',#110,#112,#126,.T.);\n#133=EDGE_CURVE('',#111,#113,#127,.T.);\n";
    t += "#134=EDGE_CURVE('',#110,#111,#124,.T.);\n#135=EDGE_CURVE('',#112,#113,#125,.T.);\n";
    // The four surfaces: the two lens ends, the flat side and the bowed side.
    t += "#140=AXIS2_PLACEMENT_3D('',#100,#115,#114);\n#141=PLANE('',#140);\n";
    t += &format!("#142=CARTESIAN_POINT('',(0.,0.,{H:.1}));\n#143=AXIS2_PLACEMENT_3D('',#142,#115,#114);\n#144=PLANE('',#143);\n");
    t += "#145=AXIS2_PLACEMENT_3D('',#100,#116,#114);\n#146=PLANE('',#145);\n";
    t += "#147=B_SPLINE_SURFACE_WITH_KNOTS('',2,1,((#100,#101),(#104,#105),(#102,#103)),.UNSPECIFIED.,.F.,.F.,.F.,(3,3),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.);\n";
    // The lens ends: two edges each, and so two nodes. Every edge of the shell is walked once
    // each way, and each face's ring turns anticlockwise seen from outside the solid.
    t += "#150=ORIENTED_EDGE('',*,*,#132,.T.);\n#151=ORIENTED_EDGE('',*,*,#130,.F.);\n#152=EDGE_LOOP('',(#150,#151));\n";
    t += "#153=FACE_OUTER_BOUND('',#152,.T.);\n#154=ADVANCED_FACE('',(#153),#141,.F.);\n";
    t += "#155=ORIENTED_EDGE('',*,*,#131,.T.);\n#156=ORIENTED_EDGE('',*,*,#133,.F.);\n#157=EDGE_LOOP('',(#155,#156));\n";
    t += "#158=FACE_OUTER_BOUND('',#157,.T.);\n#159=ADVANCED_FACE('',(#158),#144,.T.);\n";
    // The flat side and the bowed side, each a four-edge ring.
    t += "#160=ORIENTED_EDGE('',*,*,#130,.T.);\n#161=ORIENTED_EDGE('',*,*,#135,.T.);\n#162=ORIENTED_EDGE('',*,*,#131,.F.);\n#163=ORIENTED_EDGE('',*,*,#134,.F.);\n";
    t += "#164=EDGE_LOOP('',(#160,#161,#162,#163));\n#165=FACE_OUTER_BOUND('',#164,.T.);\n#166=ADVANCED_FACE('',(#165),#146,.F.);\n";
    t += "#170=ORIENTED_EDGE('',*,*,#134,.T.);\n#171=ORIENTED_EDGE('',*,*,#133,.T.);\n#172=ORIENTED_EDGE('',*,*,#135,.F.);\n#173=ORIENTED_EDGE('',*,*,#132,.F.);\n";
    t += "#174=EDGE_LOOP('',(#170,#171,#172,#173));\n#175=FACE_OUTER_BOUND('',#174,.T.);\n#176=ADVANCED_FACE('',(#175),#147,.F.);\n";
    t += "#180=CLOSED_SHELL('',(#154,#159,#166,#176));\n#181=MANIFOLD_SOLID_BREP('Sliver lens',#180);\n#182=SHAPE_REPRESENTATION('',(#181),#20);\n";
    t += "#20=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#21))GLOBAL_UNIT_ASSIGNED_CONTEXT((#22,#23))REPRESENTATION_CONTEXT('',''));\n";
    t += "#21=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-3),#22,'','');\n";
    t += "#22=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#23=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));\n";
    t += TAIL;
    let imp = step_import(&t).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    let ib = &imp.bodies[0];
    assert_eq!(ib.file_faces, 4);
    // Nothing is left unshown, and the body keeps its plan although one face cannot hold a
    // triangle until its boundary is sampled again.
    assert_eq!(ib.body.unmeshed_faces(), 0);
    let tol = 0.05;
    let plan = ib.body.display_plan(tol).expect("a sliver face must not cost the body its plan");
    for f in &plan.faces {
        assert!(!f.triangles.is_empty(), "face {} has no triangles", f.face);
    }
    // The sides carry the area: L by H flat, a shade more over the bow. The lens ends are
    // slivers, so what matters is that they are drawn at all, not how wide they come out.
    let m = ib.body.display_mesh(tol).unwrap();
    let mut area = vec![0.0f64; ib.body.face_count()];
    for (tri, f) in m.triangles.iter().zip(&m.tri_face) {
        if let Some([p, q, r]) = m.tri(tri)
            && let Some(sum) = area.get_mut(*f as usize)
        {
            *sum += 0.5 * (q - p).cross(r - p).len();
        }
    }
    let mut sides: Vec<f64> = area.iter().copied().filter(|a| *a > 0.1).collect();
    sides.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(sides.len(), 2, "two sides carry the area: {area:?}");
    // The flat side is exactly L by H; the bowed one is longer by the bow, within a tenth.
    assert!(rel(sides[0], L * H) < 1e-6, "the flat side covers {} not {}", sides[0], L * H);
    assert!((1.0..1.1).contains(&(sides[1] / (L * H))), "the bowed side covers {}", sides[1]);
    // Both lens ends are drawn, each a sliver no wider than its own bow.
    let ends: Vec<f64> = area.iter().copied().filter(|a| *a <= 0.1).collect();
    assert_eq!(ends.len(), 2, "two lens ends: {area:?}");
    for e in ends {
        assert!(e > 0.0 && e < L * BOW, "a lens end covers {e}, not a sliver under {}", L * BOW);
    }
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

/// A B-spline whose end knots repeat fewer than degree + 1 times (an unclamped knot vector,
/// as OpenCascade writes closed curves and surfaces) is defined only between knot `degree`
/// and knot `len - degree - 1`. Read over the whole knot range, the circle and the tube went
/// on past their domain, and the rational ones divided by zero at the outermost knots.
#[test]
fn periodic_knot_bsplines_are_read_on_their_domain() {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface};
    let (r, h) = (1.7, 1.5);
    let (tube, face) = periodic_knot_tube(100, 0.0, r, h);
    let (circle, row) = circle_net(10, 0.0, r, 0.0);
    let text = format!(
        "{HEAD}{tube}{circle}#30=(BOUNDED_CURVE()B_SPLINE_CURVE(2,({}),.CIRCULAR_ARC.,.T.,.F.)B_SPLINE_CURVE_WITH_KNOTS(({PERIODIC_MULTS}),({PERIODIC_KNOTS}),.UNSPECIFIED.)CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({CIRCLE_WEIGHTS}))REPRESENTATION_ITEM(''));\n{TAIL}",
        row.join(",")
    );
    let ex = p21::parse(&text).unwrap();
    let cx = Ctx { ex: &ex, len: 1.0, ang: 1.0, tol: 1e-6, snap: 1e-6 };
    let pi = std::f64::consts::PI;
    let on_tube = |p: mt::Point3| (p.x.hypot(p.y) - r).abs() < 1e-9 && p.z > -1e-9 && p.z < h + 1e-9;

    let surface_id = ex.get(face).and_then(|e| e.params().get(2)).and_then(Param::as_ref_id).unwrap();
    let surface = geom::bspline_surface(&cx, ex.get(surface_id).unwrap()).unwrap();
    let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() else { panic!("unbounded surface") };
    // Along the tube the file's range (h / r) is shorter than one: it is read over one.
    assert!(h / r < 1.0 && u0.abs() < 1e-12 && (u1 - 1.0).abs() < 1e-12, "u range {u0} .. {u1}");
    assert!((v0 + pi).abs() < 1e-9 && (v1 - pi).abs() < 1e-9, "v range {v0} .. {v1}");
    for i in 0..=8 {
        for j in 0..=16 {
            let (u, v) = (u0 + (u1 - u0) * i as f64 / 8.0, v0 + (v1 - v0) * j as f64 / 16.0);
            assert!(on_tube(surface.subs(u, v)), "S({u}, {v}) = {:?}", surface.subs(u, v));
        }
    }

    let geom::CurveGeo::Nurbs(curve) = geom::curve(&cx, 30, 0).unwrap() else { panic!("not a rational B-spline") };
    let (t0, t1) = curve.range_tuple();
    assert!((t0 + pi).abs() < 1e-9 && (t1 - pi).abs() < 1e-9, "curve range {t0} .. {t1}");
    for j in 0..=16 {
        let t = t0 + (t1 - t0) * j as f64 / 16.0;
        assert!(on_tube(curve.subs(t)), "C({t}) = {:?}", curve.subs(t));
    }
}

/// Two such tubes far apart in one body: the body's size sets a chord tolerance coarse beside
/// the tubes, and meshing a face at it divided the surface's whole parameter box, whose
/// corners lay outside the domain. The division never converged there (#1374's recursion),
/// and the import grew by gigabytes without end.
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
    let m = measure(&imp.bodies[0].body).unwrap();
    let area = 2.0 * std::f64::consts::TAU * r * h;
    assert!(rel(m.area, area) < 0.02, "area {} (expected {area})", m.area);
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
            b.body.tessellate(0.01).unwrap(),
            b.body.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap(),
            b.body.display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap(),
        ] {
            assert!(!mesh.triangles.is_empty());
            assert!(mesh.bounds().diagonal() < 10.0, "sense={sense}: {:?}", mesh.bounds());
            assert!((mesh.measure().area - 42.0).abs() < 0.01, "sense={sense}: area {}", mesh.measure().area);
            assert!(mesh.normals.iter().all(|n| n.x > 0.99), "the outer loop determines the face normal");
        }
    }
}

#[test]
fn implicit_bound_can_still_describe_the_complement_of_a_patch() {
    let imp = step_import(&shallow_cylinder_patch(false, false)).unwrap();
    let mesh = imp.bodies[0].body.tessellate(0.1).unwrap();
    assert!(mesh.bounds().diagonal() > 5000.0, "without an explicit outer bound the loop is a hole");
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
        let mesh = body.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap();
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
            assert_eq!(imported.warnings, ["solid 'Strip': imported as an open body (the shell is not closed)"]);
            assert_eq!(imported.bodies.len(), 1);
            let body = &imported.bodies[0].body;
            assert_eq!(imported.bodies[0].file_faces, 1);
            let plan = body.display_plan(0.01).unwrap();
            for mesh in [
                plan.evaluate(),
                body.display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap(),
                body.reference_display_mesh_angular(0.01, crate::DISPLAY_ANGLE).unwrap(),
                body.tessellate(0.01).unwrap(),
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
    // (file, exact volume (mm³), box (min, max), kernel faces, bodies)
    #[rustfmt::skip]
    let parts: [(&str, &str, f64, [f64; 6], usize, usize); 17] = [
        ("cone_point", include_str!("../../../../tests/step-conformance/cone_point.step"), 1000.0 * std::f64::consts::PI, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 2, 1),
        ("cone_point_vertex", include_str!("../../../../tests/step-conformance/cone_point_vertex.step"), 1000.0 * std::f64::consts::PI, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 2, 1),
        ("cylinder", include_str!("../../../../tests/step-conformance/cylinder.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1),
        ("cylinder_inch", include_str!("../../../../tests/step-conformance/cylinder_inch.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1),
        ("cylinder_nurbs", include_str!("../../../../tests/step-conformance/cylinder_nurbs.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1),
        ("cylinder_placed", include_str!("../../../../tests/step-conformance/cylinder_placed.step"), 9424.778, [31.835, -33.165, 1.835, 65.485, 0.485, 35.485], 3, 1),
        ("cylinder_seam", include_str!("../../../../tests/step-conformance/cylinder_seam.step"), 9424.778, [-10.0, -10.0, 0.0, 10.0, 10.0, 30.0], 3, 1),
        ("cylinder_twice", include_str!("../../../../tests/step-conformance/cylinder_twice.step"), 18849.556, [-10.0, -10.0, 0.0, 90.0, 10.0, 30.0], 6, 2),
        ("dome", include_str!("../../../../tests/step-conformance/dome.step"), 16755.161, [-20.0, -20.0, 0.0, 20.0, 20.0, 20.0], 2, 1),
        ("dome_nurbs", include_str!("../../../../tests/step-conformance/dome_nurbs.step"), 16755.161, [-20.0, -20.0, 0.0, 20.0, 20.0, 20.0], 2, 1),
        ("far_cylinder", include_str!("../../../../tests/step-conformance/far_cylinder.step"), 9424.778, [99990.0, -10.0, 0.0, 100010.0, 10.0, 30.0], 3, 1),
        ("grooved_shaft", include_str!("../../../../tests/step-conformance/grooved_shaft.step"), 153532.251, [-10.0, -10.0, 0.0, 10.0, 10.0, 501.0], 753, 1),
        ("plate_100_holes", include_str!("../../../../tests/step-conformance/plate_100_holes.step"), 71588.321, [0.0, 0.0, 0.0, 100.0, 100.0, 10.0], 110, 1),
        ("sheet_disc", include_str!("../../../../tests/step-conformance/sheet_disc.step"), 193396443.755, [-1800.0, -1800.0, 0.0, 1800.0, 1800.0, 19.0], 3, 1),
        ("tiny_pin", include_str!("../../../../tests/step-conformance/tiny_pin.step"), std::f64::consts::FRAC_PI_8, [-0.25, -0.25, 0.0, 0.25, 0.25, 2.0], 3, 1),
        ("torus_ring_nurbs", include_str!("../../../../tests/step-conformance/torus_ring_nurbs.step"), 4934.802, [-25.0, -25.0, 0.0, 25.0, 25.0, 5.0], 2, 1),
        ("washer", include_str!("../../../../tests/step-conformance/washer.step"), 3534.292, [-20.0, -20.0, 0.0, 20.0, 20.0, 3.0], 4, 1),
    ];
    for (name, text, volume, bbox, faces, bodies) in parts {
        let imp = step_import(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(imp.warnings.is_empty(), "{name}: {:?}", imp.warnings);
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

/// #4598: a file whose bore rim runs back and forth across the bore (as main exported a moved
/// body's edge before) imports whole: the crossings of the rim's own pieces are left out of the
/// triangulation, not asserted on, so the face meshes a little off the exact rod.
#[test]
fn a_rim_that_runs_back_and_forth_imports_whole() {
    let imp = step_import(include_str!("../../../../tests/step-regressions/rod_doubled_edge.step")).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    let m = measure(&imp.bodies[0].body).unwrap();
    // The exact rod is 74 898.5 mm³; the rim's crossings left out shave a little off.
    assert!(rel(m.volume, 74_898.5) < 2e-3, "volume {}", m.volume);
    assert_eq!(m.merged.faces, 5);
}
