//! Imports kept as records build the bodies a STEP import builds: the step-conformance parts
//! (cones, cylinders, a dome, a NURBS torus ring, a washer, a sheet) read by the STEP reader into
//! records, then made into bodies from those records alone. Records from a file are hostile: a
//! broken one fails its import with the reason, never a panic.

use super::super::{IDENTITY, StepNode, step_import};
use super::{ImportRecords, RecordBody, RecordMesh, SharedRecords, records_import, records_noted, records_text};
use crate::measure;
use solidcore_record::brep::{self, Brep};

/// The records the STEP reader assembles from `text`, kept as an import.
fn kept(text: &str) -> ImportRecords {
    let ex = solidcore_step::p21::parse(text).unwrap();
    let read = solidcore_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibImportTrims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let bodies: Vec<RecordBody> = read
        .solids
        .into_iter()
        .map(|s| RecordBody {
            name: s.name,
            path: Vec::new(),
            instance: None,
            color: None,
            paint: None,
            hidden: false,
            placement: None,
            brep: Some(s.brep),
            face_origin: s.face_origin,
            sheet: s.sheet,
            copy_of: None,
            mesh: None,
        })
        .collect();
    let tree = vec![StepNode {
        name: "part".into(),
        transform: IDENTITY,
        bodies: (0..bodies.len()).collect(),
        items: Vec::new(),
        left_out: Vec::new(),
        children: Vec::new(),
    }];
    ImportRecords::new(bodies, tree, Vec::new(), String::new())
}

#[test]
fn records_build_the_bodies_the_step_text_builds() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    for part in ["cone_point", "cylinder_seam", "dome", "dome_nurbs", "torus_ring_nurbs", "washer", "grooved_shaft", "sheet_disc"] {
        let text = std::fs::read_to_string(dir.join(format!("{part}.step"))).unwrap();
        let from_step = step_import(&text).unwrap();
        // Only parts the reader reads whole compare: the records are what it assembled.
        if from_step.bodies.iter().any(|b| b.refused.is_some()) {
            continue;
        }
        let from_records = records_import(&kept(&text)).unwrap_or_else(|e| panic!("{part}: {e}"));
        assert_eq!(from_records.bodies.len(), from_step.bodies.len(), "{part}");
        for (r, s) in from_records.bodies.iter().zip(&from_step.bodies) {
            let (mr, ms) = (measure(&r.body).unwrap(), measure(&s.body).unwrap());
            assert_eq!((mr.faces, r.closed, &r.face_origin, r.file_faces), (ms.faces, s.closed, &s.face_origin, s.file_faces), "{part}");
            assert!((mr.volume - ms.volume).abs() <= 1e-9 * ms.volume.abs().max(1.0), "{part}: {} vs {}", mr.volume, ms.volume);
        }
    }
}

#[test]
fn a_record_placed_moves_its_body() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let text = std::fs::read_to_string(dir.join("washer.step")).unwrap();
    let mut records = kept(&text);
    let at = records_import(&records).unwrap();
    // Column-major: the last column is the move.
    records.bodies[0].placement = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, 0.0, 0.0, 1.0]]);
    let moved = records_import(&records).unwrap();
    let (a, b) = (measure(&at.bodies[0].body).unwrap(), measure(&moved.bodies[0].body).unwrap());
    assert!((b.centroid.x - a.centroid.x - 5.0).abs() < 1e-9 && (b.volume - a.volume).abs() < 1e-9, "{a:?} {b:?}");
}

/// A stored tree's left-out solids are kept at their places among the node's solids, and a body
/// left out now takes the place it had between them: with a solid left out first (item 42), the
/// broken record after it is at place 1 and the good one, at place 2, comes in.
#[test]
fn a_stored_trees_left_out_solids_are_kept_where_they_are() {
    use super::super::LeftOut;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut r = kept(&std::fs::read_to_string(dir.join("cone_point.step")).unwrap());
    let mut broken = r.bodies[0].clone();
    broken.face_origin.pop();
    r.bodies.insert(0, broken);
    r.tree[0].bodies = vec![0, 1];
    r.tree[0].left_out = vec![LeftOut { at: 0, item: 42 }];
    let imp = records_import(&r).unwrap();
    assert_eq!(imp.bodies.len(), 1);
    assert_eq!(imp.tree[0].bodies, vec![0]);
    assert_eq!(imp.tree[0].left_out, vec![LeftOut { at: 0, item: 42 }, LeftOut { at: 1, item: 0 }]);
}

#[test]
fn a_broken_record_is_left_out_with_the_reason_and_the_others_still_come_in() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let good = kept(&std::fs::read_to_string(dir.join("cone_point.step")).unwrap());
    let with_broken = |break_it: &dyn Fn(&mut RecordBody)| {
        let mut r = good.clone();
        let mut broken = r.bodies[0].clone();
        break_it(&mut broken);
        r.bodies.insert(0, broken);
        r.tree[0].bodies = vec![0, 1];
        let imp = records_import(&r).unwrap();
        // The good body comes in, and the tree names it where it now is.
        assert_eq!(imp.bodies.len(), 1);
        assert_eq!(imp.tree[0].bodies, vec![0]);
        imp.warnings.join("; ")
    };
    assert!(
        with_broken(&|b| {
            b.face_origin.pop();
        })
        .contains("face origins")
    );
    assert!(with_broken(&|b| b.brep.as_mut().unwrap().edges[0].vertices[0] = 999).contains("invalid"));
    assert!(with_broken(&|b| b.placement = Some([[f64::NAN; 4]; 4])).contains("not finite"));
    // A face origin that cannot be numbered from 1 (hostile: the file's own faces never reach it).
    assert!(with_broken(&|b| b.face_origin[0] = u64::MAX).contains("face origin out of range"));
    // A product naming a body the import does not have refuses the whole import.
    let mut r = good;
    r.tree[0].bodies.push(7);
    assert!(records_import(&r).unwrap_err().to_string().contains("names a body"));
    // A tree of more products than an assembly the STEP reader reads, however shallow, refuses
    // it too; one of as many as it reads still imports.
    r.tree[0].bodies.pop();
    let leaf = StepNode { bodies: Vec::new(), children: Vec::new(), ..r.tree[0].clone() };
    r.tree.resize(super::super::MAX_VISITS, leaf);
    assert!(records_import(&r).is_ok());
    r.tree.push(r.tree[1].clone());
    assert!(records_import(&r).unwrap_err().to_string().contains("more than"));
}

/// A record body's colour is its body's, and each face is known by its record body and its face
/// in the file, as a STEP import's faces are known by their entities (from 1).
#[test]
fn a_record_body_has_its_colour_and_face_sources() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut r = kept(&std::fs::read_to_string(dir.join("cylinder_seam.step")).unwrap());
    r.bodies[0].color = Some([0.2, 0.6, 0.8]);
    let imp = records_import(&r).unwrap();
    let body = &imp.bodies[0].body;
    assert_eq!(body.color(), Some([0.2, 0.6, 0.8]));
    let sources = body.face_sources().expect("face sources");
    assert_eq!(sources.len(), body.face_count());
    for (s, origin) in sources.iter().zip(&imp.bodies[0].face_origin) {
        assert!(matches!(s, Some(crate::FaceSource::Imported { item: 1, face, .. }) if *face == origin + 1), "{s:?}");
    }
}

/// A square sheet: one planar face in an open shell of a surface model.
const SQUARE_SHEET: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','hand','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n\
#1=CARTESIAN_POINT('',(0.,0.,0.));#2=CARTESIAN_POINT('',(10.,0.,0.));#3=CARTESIAN_POINT('',(10.,10.,0.));#4=CARTESIAN_POINT('',(0.,10.,0.));\n\
#5=VERTEX_POINT('',#1);#6=VERTEX_POINT('',#2);#7=VERTEX_POINT('',#3);#8=VERTEX_POINT('',#4);\n\
#9=DIRECTION('',(1.,0.,0.));#10=DIRECTION('',(0.,1.,0.));#11=DIRECTION('',(-1.,0.,0.));#12=DIRECTION('',(0.,-1.,0.));\n\
#13=VECTOR('',#9,1.);#14=VECTOR('',#10,1.);#15=VECTOR('',#11,1.);#16=VECTOR('',#12,1.);\n\
#17=LINE('',#1,#13);#18=LINE('',#2,#14);#19=LINE('',#3,#15);#20=LINE('',#4,#16);\n\
#21=EDGE_CURVE('',#5,#6,#17,.T.);#22=EDGE_CURVE('',#6,#7,#18,.T.);#23=EDGE_CURVE('',#7,#8,#19,.T.);#24=EDGE_CURVE('',#8,#5,#20,.T.);\n\
#25=ORIENTED_EDGE('',*,*,#21,.T.);#26=ORIENTED_EDGE('',*,*,#22,.T.);#27=ORIENTED_EDGE('',*,*,#23,.T.);#28=ORIENTED_EDGE('',*,*,#24,.T.);\n\
#29=EDGE_LOOP('',(#25,#26,#27,#28));#30=FACE_OUTER_BOUND('',#29,.T.);\n\
#31=DIRECTION('',(0.,0.,1.));#32=AXIS2_PLACEMENT_3D('',#1,#31,#9);#33=PLANE('',#32);\n\
#34=ADVANCED_FACE('',(#30),#33,.T.);#35=OPEN_SHELL('',(#34));#36=SHELL_BASED_SURFACE_MODEL('Sheet',(#35));\n\
#37=SHAPE_REPRESENTATION('',(#36),$);\nENDSEC;\nEND-ISO-10303-21;\n";

/// A record whose shells bound no region is an open body, whatever its `sheet` flag says.
#[test]
fn an_open_record_marked_closed_is_still_open() {
    let mut r = kept(SQUARE_SHEET);
    assert!(r.bodies[0].sheet && r.bodies[0].brep.as_ref().unwrap().regions.len() == 1, "the reader reads the square as a sheet");
    r.bodies[0].sheet = false;
    let imp = records_import(&r).unwrap();
    assert!(!imp.bodies[0].closed);
    assert_eq!(imp.report.solids, 0);
}

/// A copy of a record body is that body's part placed again: the same faces and volume where its
/// own placement puts it, and the same file item and face sources, so the design knows both as one
/// part, and the import's report counts it once. The copy carries no record of its own.
#[test]
fn a_copy_is_the_same_part_placed_again() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut records = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    let mut copy = records.bodies[0].clone();
    copy.brep = None;
    copy.face_origin = Vec::new();
    copy.copy_of = Some(0);
    copy.placement = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 40.0, 0.0, 1.0]]);
    records.bodies.push(copy);
    if let Some(root) = records.tree.first_mut() {
        root.bodies = vec![0, 1];
    }
    let imp = records_import(&records).unwrap();
    assert_eq!(imp.bodies.len(), 2, "{:?}", imp.warnings);
    let (a, b) = (&imp.bodies[0], &imp.bodies[1]);
    assert_eq!(a.source_item, b.source_item);
    assert_eq!(a.face_origin, b.face_origin);
    assert_eq!(a.body.face_sources(), b.body.face_sources());
    let (ma, mb) = (measure(&a.body).unwrap(), measure(&b.body).unwrap());
    assert!((mb.centroid.y - ma.centroid.y - 40.0).abs() < 1e-9 && (mb.volume - ma.volume).abs() < 1e-9, "{ma:?} {mb:?}");
    assert_eq!(ma.faces, mb.faces);
    assert_eq!((imp.report.parts, imp.report.solids), (1, 1), "one part, placed twice");
}

/// A copy of a body that comes after it, of itself, or of another copy is left out with the
/// reason, and the original still comes in. Records from a file are hostile.
#[test]
fn a_copy_of_nothing_it_can_use_is_left_out_with_why() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let records = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    let copy = |of: usize| RecordBody { brep: None, face_origin: Vec::new(), copy_of: Some(of), ..records.bodies[0].clone() };
    for (bodies, why) in [
        (vec![copy(1), records.bodies[0].clone()], "does not come before it"),
        (vec![records.bodies[0].clone(), copy(1)], "does not come before it"),
        (vec![records.bodies[0].clone(), copy(0), copy(1)], "itself a copy"),
        (vec![records.bodies[0].clone(), copy(usize::MAX)], "does not come before it"),
    ] {
        let n = bodies.len();
        let r = ImportRecords { bodies, tree: Vec::new(), ..records.clone() };
        let imp = records_import(&r).unwrap();
        assert!(imp.warnings.iter().any(|w| w.contains(why)), "{why}: {:?}", imp.warnings);
        assert!(!imp.bodies.is_empty() && imp.bodies.len() < n, "{why}: {}", imp.bodies.len());
    }
    // A copy whose original was refused is refused too.
    let mut broken = records.bodies[0].clone();
    if let Some(b) = broken.brep.as_mut() {
        b.edges.clear();
    }
    let r = ImportRecords { bodies: vec![broken, copy(0)], tree: Vec::new(), ..records.clone() };
    let imp = records_import(&r).unwrap();
    assert!(imp.bodies.is_empty() && imp.warnings.iter().any(|w| w.contains("was not imported")), "{:?}", imp.warnings);
    // A copy of an original whose own placement is refused still comes in where it is placed,
    // the part counted once.
    let mut unplaced = records.bodies[0].clone();
    unplaced.placement = Some([[f64::NAN; 4]; 4]);
    let r = ImportRecords { bodies: vec![unplaced, copy(0)], tree: Vec::new(), ..records.clone() };
    let imp = records_import(&r).unwrap();
    assert_eq!(imp.bodies.len(), 1, "{:?}", imp.warnings);
    assert!(imp.warnings.iter().any(|w| w.contains("not finite")), "{:?}", imp.warnings);
    assert_eq!((imp.report.parts, imp.report.solids), (1, 1));
}

/// A record body's paint names faces by their face in the file; the body's paint has them on its
/// own faces. Colours and opacities from a file are hostile: each comes in within 0..1, the body's
/// own colour too, and a colour on a face the body does not have is dropped.
#[test]
fn a_record_bodys_paint_is_on_its_own_faces_within_range() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut records = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    let file_face = records.bodies[0].face_origin[2];
    records.bodies[0].paint = Some(crate::Paint {
        opacity: f32::NAN,
        faces: vec![
            crate::FacePaint { face: file_face as usize, color: [2.0, -1.0, 0.5], opacity: 0.25, name: Some("Accent".into()) },
            crate::FacePaint { face: usize::MAX, color: [0.0; 3], opacity: 1.0, name: None },
        ],
        name: Some("Body".into()),
        layer: None,
    });
    records.bodies[0].color = Some([-1.0, 0.5, 2.0]);
    let imp = records_import(&records).unwrap();
    let body = &imp.bodies[0];
    assert_eq!((body.color, body.body.color()), (Some([0.0, 0.5, 1.0]), Some([0.0, 0.5, 1.0])));
    let paint = body.body.paint().expect("paint");
    assert_eq!((paint.opacity, paint.name.as_deref()), (1.0, Some("Body")));
    assert_eq!(paint.faces.len(), 1, "{paint:?}");
    let f = &paint.faces[0];
    assert_eq!((body.face_origin[f.face], f.color, f.opacity, f.name.as_deref()), (file_face, [1.0, 0.0, 0.5], 0.25, Some("Accent")));
}

/// Notes about a file found after its import was made (its stated volume, say) are file issues
/// of the import, after the records' own warnings, as an import of the noted records has them:
/// the design's evaluation of those records shows them without making the bodies again.
#[test]
fn notes_about_the_file_are_its_imports_file_issues() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut records = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    records.warnings = vec!["read: a loss".into()];
    let mut broken = records.bodies[0].clone();
    broken.face_origin.pop();
    records.bodies.push(broken);
    let imp = records_import(&records).unwrap();
    let note = "part 'washer': the file states its volume as 1 mm³".to_string();
    let (noted, noted_imp) = records_noted(records, &imp, vec![note.clone()]).unwrap();
    assert_eq!(noted.warnings, vec!["read: a loss".to_string(), note.clone()]);
    let rebuilt = records_import(&noted).unwrap();
    assert_eq!(noted_imp.report, rebuilt.report);
    assert_eq!(noted_imp.warnings, rebuilt.warnings);
    assert_eq!(noted_imp.report.issues[1].detail, note);
    assert_eq!(noted_imp.report.issues[1].kind, super::super::IssueKind::File);
    // The evaluation of the noted records finds this import.
    let shared = super::records_import_shared(&noted).unwrap();
    assert!(std::sync::Arc::ptr_eq(&shared, &noted_imp));
}

/// Check both display and export, including welded topology: a successful partial display is
/// not a successful mesh of a closed imported solid.
fn assert_closed_import_mesh(b: &Brep, size: f64, chord: f64) {
    use solidcore_tess::{MeshSpec, MeshTolerance};
    let plans = [
        solidcore_tess::plan(b, &MeshTolerance::display(chord), size).unwrap(),
        solidcore_tess::plan(b, &MeshTolerance::chord(chord), size).unwrap(),
        solidcore_tess::mesh(b, &MeshSpec::chord(chord), size, None).unwrap(),
    ];
    for (plan, stats) in plans {
        assert!(stats.unmeshed.is_empty(), "{stats:?}");
        assert!(stats.lenient.is_empty(), "{stats:?}");
        assert!(stats.edges_capped.is_empty(), "{stats:?}");
        assert!(stats.over_tolerance.is_empty(), "{stats:?}");
        let (mesh, refused) = plan.welded().unwrap();
        assert!(refused.is_empty(), "{refused:?}");
        assert!(!mesh.triangles.is_empty());
        let mut sides: std::collections::BTreeMap<(u32, u32), (usize, i32)> = Default::default();
        for t in &mesh.triangles {
            assert!(t[0] != t[1] && t[1] != t[2] && t[2] != t[0], "{t:?}");
            for k in 0..3 {
                let (a, z) = (t[k], t[(k + 1) % 3]);
                let s = sides.entry((a.min(z), a.max(z))).or_default();
                s.0 += 1;
                s.1 += if a < z { 1 } else { -1 };
            }
        }
        assert!(sides.values().all(|s| *s == (2, 0)), "every mesh side has two opposing uses");
        assert!((0..b.faces.len()).all(|f| mesh.tri_face.contains(&(f as u32))), "every imported face is meshed");
    }
}

/// Write a synthetic solid with the requested STEP context accuracy, then import it through
/// the production trim provider. The optional top is the same spline as the import-only
/// regression in solidcore-step: S(u,v) = (u, v + 0.016 u(1-u), 1).
fn coarse_accuracy_import(record: &Brep, accuracy: f64, spline_top: Option<usize>) -> Brep {
    use solidcore_step::p21::Param;
    use solidcore_step::write::{GeomWriter, Writer, topology};
    let mut w = Writer::new();
    let written = topology::brep(&mut GeomWriter::new(&mut w), record, "coarse accuracy").unwrap();
    let spline = spline_top.map(|_| {
        let surface = brep::Surface::Spline {
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![-0.016, -0.016, 1.0, 1.0],
            points: [(0.0, 0.0), (0.5, 0.008), (1.0, 0.0)].into_iter().map(|(x, bump)| vec![[x, -0.016 + bump, 1.0], [x, 1.0 + bump, 1.0]]).collect(),
        };
        let (surface, _) = topology::surface(&surface).unwrap();
        GeomWriter::new(&mut w).surface(&surface)
    });
    let empty = || Param::Str(String::new());
    let mm = w.add_complex(&[
        ("LENGTH_UNIT", vec![]),
        ("NAMED_UNIT", vec![Param::Derived]),
        ("SI_UNIT", vec![Param::Enum("MILLI".into()), Param::Enum("METRE".into())]),
    ]);
    let uncertainty = w.add(
        "UNCERTAINTY_MEASURE_WITH_UNIT",
        &[Param::Typed("LENGTH_MEASURE".into(), vec![Param::Real(accuracy)]), Param::Ref(mm), Param::Str("distance_accuracy_value".into()), empty()],
    );
    let context = w.add_complex(&[
        ("GEOMETRIC_REPRESENTATION_CONTEXT", vec![Param::Int(3)]),
        ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", vec![Param::List(vec![Param::Ref(uncertainty)])]),
        ("GLOBAL_UNIT_ASSIGNED_CONTEXT", vec![Param::List(vec![Param::Ref(mm)])]),
        ("REPRESENTATION_CONTEXT", vec![empty(), empty()]),
    ]);
    w.add(
        "ADVANCED_BREP_SHAPE_REPRESENTATION",
        &[empty(), Param::List(written.solids.iter().copied().map(Param::Ref).collect()), Param::Ref(context)],
    );
    let mut ex = solidcore_step::p21::parse(&w.file("coarse accuracy", "AUTOMOTIVE_DESIGN")).unwrap();
    if let (Some(top), Some(surface)) = (spline_top, spline) {
        // Replace just the top's support; its edges still bound the unit square in 3D. The
        // reader constructs pcurves from those edges, as in the STEP unit regression.
        ex.entities.get_mut(&written.faces[top]).unwrap().records[0].params[2] = Param::Ref(surface);
    }
    let read = solidcore_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibTrims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    assert_eq!(read.solids.len(), 1);
    let solid = read.solids.into_iter().next().unwrap();
    assert!(!solid.sheet);
    assert_eq!(solid.brep.tolerance, accuracy);
    solidcore_ops::record_check::validate(&solid.brep).unwrap();
    solidcore_ops::record_check::agreement(&solid.brep).unwrap();
    solid.brep
}

#[test]
fn a_spline_face_imported_with_coarse_distance_accuracy_meshes() {
    let record = solidcore_ops::make_box([0.0; 3], [1.0; 3]).unwrap().to_record().unwrap().0;
    let top = record
        .faces
        .iter()
        .position(|f| {
            f.loops.iter().all(|l| match l {
                brep::Loop::Edges { edges } => {
                    edges.iter().all(|u| record.edges[u.edge as usize].vertices.iter().all(|v| record.vertices[*v as usize].point[2] == 1.0))
                }
                brep::Loop::Vertex { .. } => false,
            })
        })
        .unwrap();
    for accuracy in [0.01, 0.005] {
        let b = coarse_accuracy_import(&record, accuracy, Some(top));
        assert_eq!((b.regions.len(), b.shells.len(), b.faces.len(), b.edges.len(), b.vertices.len()), (2, 1, 6, 12, 8));
        assert!(b.faces.iter().any(|f| matches!(f.surface, brep::Surface::Spline { .. })));
        assert_closed_import_mesh(&b, 3f64.sqrt(), 1e-3);
    }
}

#[test]
fn a_small_cylinder_imported_with_coarse_distance_accuracy_meshes() {
    let (r, h) = (0.05, 0.1);
    let record = solidcore_ops::make_cylinder([0.0; 3], r, h).unwrap().to_record().unwrap().0;
    for accuracy in [0.01, 0.005] {
        let b = coarse_accuracy_import(&record, accuracy, None);
        assert_eq!((b.regions.len(), b.shells.len(), b.faces.len(), b.edges.len(), b.vertices.len()), (2, 1, 3, 3, 2));
        assert!(b.faces.iter().any(|f| matches!(f.surface, brep::Surface::Cylinder { .. })));
        assert_closed_import_mesh(&b, (8.0 * r * r + h * h).sqrt(), 1e-4);
    }
}

/// [`SharedRecords`] serialises identically to [`ImportRecords`] and short-circuits [`PartialEq`]
/// on [`std::sync::Arc::ptr_eq`], falling back to value comparison for distinct allocations.
#[test]
fn shared_records_wire_compatible_and_ptr_eq_short_circuit() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut raw = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    raw.warnings = vec!["note".into()];
    raw.system = "FreeCAD".into();
    let shared = SharedRecords::new(raw.clone());

    // Wire-compatibility: identical JSON bytes to raw `ImportRecords`.
    let raw_json = serde_json::to_string(&raw).unwrap();
    let shared_json = serde_json::to_string(&shared).unwrap();
    assert_eq!(shared_json, raw_json);
    let decoded: SharedRecords = serde_json::from_str(&raw_json).unwrap();
    assert_eq!(*decoded, raw);
    assert_eq!(shared, decoded);
    assert!(!SharedRecords::ptr_eq(&shared, &decoded));

    let mut changed = raw.clone();
    changed.system = "Rhino".into();
    assert_ne!(shared, SharedRecords::new(changed));

    // Deterministic proof of `Arc::ptr_eq` short-circuit: NaN != NaN under value comparison, so
    // `nan_a == nan_a_clone` can only hold when pointer equality short-circuits.
    let nan_body = RecordBody {
        name: "NaN".into(),
        path: Vec::new(),
        instance: None,
        color: None,
        paint: None,
        hidden: false,
        placement: None,
        brep: None,
        face_origin: Vec::new(),
        mesh: Some(RecordMesh { positions: vec![[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], triangles: vec![[0, 1, 2]] }),
        copy_of: None,
        sheet: false,
    };
    let nan_raw = ImportRecords::new(vec![nan_body], Vec::new(), Vec::new(), String::new());
    let nan_a = SharedRecords::new(nan_raw.clone());
    let nan_a_clone = nan_a.clone();
    let nan_b = SharedRecords::new(nan_raw);
    assert!(SharedRecords::ptr_eq(&nan_a, &nan_a_clone));
    assert_eq!(nan_a, nan_a_clone);
    assert_ne!(nan_a, nan_b);
}

/// The zero-allocation streaming digest on [`ImportRecords`] matches
/// `text_key(&serde_json::to_string(r))` and SHA-256 of the serialised bytes bit-for-bit, caches
/// lazily, starts fresh on [`Clone`], and refreshes after [`ImportRecords::invalidate_digest`].
#[test]
fn streaming_digest_matches_serialized_text_key_and_sha256_bit_for_bit() {
    use sha2::{Digest, Sha256};

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let mut records = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    records.bodies[0].path = vec!["Asm".into(), "Washer".into()];
    records.bodies[0].instance = Some("Washer:1".into());
    records.bodies[0].color = Some([0.2, 0.4, 0.6]);
    records.bodies[0].paint = Some(crate::Paint {
        opacity: 0.9,
        faces: vec![crate::FacePaint { face: 0, color: [0.8, 0.1, 0.2], opacity: 0.75, name: Some("Red".into()) }],
        name: Some("Blue".into()),
        layer: None,
    });
    let mut copy = records.bodies[0].clone();
    copy.name = "WasherCopy".into();
    copy.brep = None;
    copy.face_origin = Vec::new();
    copy.copy_of = Some(0);
    copy.placement = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [10.0, 20.0, 30.0, 1.0]]);
    let mesh = RecordBody {
        name: "Tri".into(),
        path: Vec::new(),
        instance: None,
        color: None,
        paint: None,
        hidden: true,
        placement: None,
        brep: None,
        face_origin: Vec::new(),
        mesh: Some(RecordMesh { positions: vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [0.0, 5.0, 0.0]], triangles: vec![[0, 1, 2]] }),
        copy_of: None,
        sheet: false,
    };
    records.bodies.push(copy);
    records.bodies.push(mesh);
    records.warnings = vec!["first warning".into()];
    records.system = "FreeCAD".into();

    let text = records_text(&records).unwrap();
    let (want_hash, want_len) = super::super::text_key(&text);
    let want_sha = format!("{:x}", Sha256::digest(text.as_bytes()));

    let first = records.digest().unwrap();
    assert_eq!(first.hash, want_hash);
    assert_eq!(first.bytes, want_len);
    assert_eq!(first.sha256, want_sha);
    assert!(std::ptr::eq(first, records.digest().unwrap()), "subsequent calls return the cached digest");

    // Cloning does not carry a stale cache into a mutated clone.
    let mut cloned = records.clone();
    cloned.warnings.push("clone note".into());
    let cloned_text = records_text(&cloned).unwrap();
    let cloned_digest = cloned.digest().unwrap();
    assert_eq!((cloned_digest.hash, cloned_digest.bytes), super::super::text_key(&cloned_text));
    assert_eq!(cloned_digest.sha256, format!("{:x}", Sha256::digest(cloned_text.as_bytes())));
    assert_ne!(cloned_digest.sha256, want_sha);

    // Mutating in place and calling `invalidate_digest` recomputes accurately.
    records.warnings.push("mutated note".into());
    records.invalidate_digest();
    let mutated_text = records_text(&records).unwrap();
    let mutated_digest = records.digest().unwrap();
    assert_eq!((mutated_digest.hash, mutated_digest.bytes), super::super::text_key(&mutated_text));
    assert_eq!(mutated_digest.sha256, format!("{:x}", Sha256::digest(mutated_text.as_bytes())));
}

/// Parallel two-pass [`records_import`] preserves exact body ordering, geometry, transformed copy
/// placements, warning order, solid counts and product-tree renumbering.
#[test]
fn parallel_records_import_preserves_order_geometry_copies_and_placements() {
    use super::super::LeftOut;

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let washer_rec = kept(&std::fs::read_to_string(dir.join("washer.step")).unwrap());
    let cone_rec = kept(&std::fs::read_to_string(dir.join("cone_point.step")).unwrap());
    let cyl_rec = kept(&std::fs::read_to_string(dir.join("cylinder_seam.step")).unwrap());

    let washer_ref = measure(&records_import(&washer_rec).unwrap().bodies[0].body).unwrap();
    let cone_ref = measure(&records_import(&cone_rec).unwrap().bodies[0].body).unwrap();
    let cyl_ref = measure(&records_import(&cyl_rec).unwrap().bodies[0].body).unwrap();

    let translate = |dx: f64, dy: f64, dz: f64| [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [dx, dy, dz, 1.0]];
    // 90° CCW about Z followed by +25 mm along Y: (x, y, z) -> (-y, x + 25, z).
    let rot_z_90_y25 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 25.0, 0.0, 1.0]];

    let mut b0 = washer_rec.bodies[0].clone();
    b0.name = "Washer0".into();
    b0.color = Some([0.1, 0.2, 0.3]);

    let mut b1 = cone_rec.bodies[0].clone();
    b1.name = "Cone1".into();
    b1.placement = Some(translate(10.0, 0.0, 0.0));

    let mut b2_broken = washer_rec.bodies[0].clone();
    b2_broken.name = "Broken2".into();
    b2_broken.face_origin.pop();

    let mut b3_nan = cyl_rec.bodies[0].clone();
    b3_nan.name = "CylNan3".into();
    b3_nan.placement = Some([[f64::NAN; 4]; 4]);

    let b4_washer_copy =
        RecordBody { name: "WasherCopy4".into(), brep: None, face_origin: Vec::new(), copy_of: Some(0), placement: Some(rot_z_90_y25), ..b0.clone() };

    let b5_cyl_copy = RecordBody {
        name: "CylCopy5".into(),
        brep: None,
        face_origin: Vec::new(),
        copy_of: Some(3),
        placement: Some(translate(0.0, 0.0, 40.0)),
        ..cyl_rec.bodies[0].clone()
    };

    let b6_broken_copy =
        RecordBody { name: "BrokenCopy6".into(), brep: None, face_origin: Vec::new(), copy_of: Some(2), placement: None, ..b0.clone() };

    let b7_washer_id =
        RecordBody { name: "WasherIdentity7".into(), brep: None, face_origin: Vec::new(), copy_of: Some(0), placement: Some(IDENTITY), ..b0.clone() };

    let tree = vec![StepNode {
        name: "Asm".into(),
        transform: IDENTITY,
        bodies: (0..8).collect(),
        items: Vec::new(),
        left_out: Vec::new(),
        children: Vec::new(),
    }];
    let records = ImportRecords::new(
        vec![b0, b1, b2_broken, b3_nan, b4_washer_copy, b5_cyl_copy, b6_broken_copy, b7_washer_id],
        tree,
        Vec::new(),
        String::new(),
    );
    let imp = records_import(&records).unwrap();

    // Exact imported order and source items: [0, 1, 4, 5, 7].
    let names: Vec<&str> = imp.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["Washer0", "Cone1", "WasherCopy4", "CylCopy5", "WasherIdentity7"]);
    let items: Vec<u64> = imp.bodies.iter().map(|b| b.source_item).collect();
    assert_eq!(items, vec![0, 1, 0, 3, 0]);
    assert_eq!((imp.report.parts, imp.report.solids), (4, 3));

    // Exact warning order matches failing indices 2, 3, 6.
    assert_eq!(imp.warnings.len(), 3, "{:?}", imp.warnings);
    assert!(imp.warnings[0].contains("Broken2") && imp.warnings[0].contains("face origins"), "{:?}", imp.warnings);
    assert!(imp.warnings[1].contains("CylNan3") && imp.warnings[1].contains("not finite"), "{:?}", imp.warnings);
    assert!(imp.warnings[2].contains("BrokenCopy6") && imp.warnings[2].contains("was not imported"), "{:?}", imp.warnings);

    // Tree renumbering and left-out slots preserved deterministically.
    assert_eq!(imp.tree[0].bodies, vec![0, 1, 2, 3, 4]);
    assert_eq!(imp.tree[0].left_out, vec![LeftOut { at: 2, item: 2 }, LeftOut { at: 3, item: 3 }, LeftOut { at: 6, item: 6 }]);

    // Verify exact volumes and transformed centroids against single-body reference imports.
    let m0 = measure(&imp.bodies[0].body).unwrap();
    let m1 = measure(&imp.bodies[1].body).unwrap();
    let m2 = measure(&imp.bodies[2].body).unwrap();
    let m3 = measure(&imp.bodies[3].body).unwrap();
    let m4 = measure(&imp.bodies[4].body).unwrap();

    assert!((m0.volume - washer_ref.volume).abs() < 1e-9);
    assert!((m0.centroid.x - washer_ref.centroid.x).abs() < 1e-9);
    assert!((m1.volume - cone_ref.volume).abs() < 1e-9);
    assert!((m1.centroid.x - (cone_ref.centroid.x + 10.0)).abs() < 1e-9);
    assert!((m2.volume - washer_ref.volume).abs() < 1e-9);
    assert!((m2.centroid.x - (-washer_ref.centroid.y)).abs() < 1e-9);
    assert!((m2.centroid.y - (washer_ref.centroid.x + 25.0)).abs() < 1e-9);
    assert!((m2.centroid.z - washer_ref.centroid.z).abs() < 1e-9);
    assert!((m3.volume - cyl_ref.volume).abs() < 1e-9);
    assert!((m3.centroid.z - (cyl_ref.centroid.z + 40.0)).abs() < 1e-9);
    assert!((m4.volume - washer_ref.volume).abs() < 1e-9);
    assert!((m4.centroid.x - washer_ref.centroid.x).abs() < 1e-9);
    assert!((m4.centroid.y - washer_ref.centroid.y).abs() < 1e-9);
}

#[test]
fn native_records_from_bodies_round_trip_exactly_without_smlib() {
    use solidcore_record::Vec3;
    let paint = crate::Paint {
        opacity: 0.75,
        faces: vec![crate::FacePaint { face: 2, color: [0.1, 0.5, 0.9], opacity: 0.5, name: Some("Accent".into()) }],
        name: Some("ShellBlue".into()),
        layer: None,
    };
    let bx = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap().with_color(Some([0.2, 0.4, 0.6])).with_paint(Some(paint));
    let cyl = crate::cylinder(Vec3::new(40.0, 0.0, 0.0), Vec3::Z, 5.0, 12.0).unwrap();
    let inputs = [super::NativeRecordBody { name: "Plate".into(), body: &bx }, super::NativeRecordBody { name: "Pin".into(), body: &cyl }];
    let (records, imp) = super::records_from_bodies("Bracket", &inputs).unwrap();
    assert!(records.native);
    assert_eq!(records.bodies.len(), 2);
    assert_eq!(imp.bodies.len(), 2);
    assert_eq!(imp.tree.len(), 1);
    assert_eq!(imp.tree[0].name, "Bracket");
    assert_eq!(imp.tree[0].bodies, vec![0, 1]);
    assert_eq!(imp.tree[0].items, vec![0, 1]);
    assert_eq!((imp.report.parts, imp.report.solids, imp.report.single_root), (2, 2, true));
    // Cached lookup hits without re-importing.
    let shared = super::records_import_shared(&records).unwrap();
    assert!(std::sync::Arc::ptr_eq(&shared, &imp));
    // JSON round-trip and cold records_import restore identical records, face order, colour and paint.
    let text = records_text(&records).unwrap();
    let decoded: ImportRecords = serde_json::from_str(&text).unwrap();
    assert_eq!(decoded, *records);
    let cold = records_import(&decoded).unwrap();
    assert_eq!(cold.bodies.len(), imp.bodies.len());
    assert_eq!(cold.tree, imp.tree);
    assert_eq!(cold.report, imp.report);
    for (a, b) in imp.bodies.iter().zip(&cold.bodies) {
        assert_eq!(a.body.record(), b.body.record());
        assert_eq!(a.face_origin, b.face_origin);
        assert_eq!(a.body.color(), b.body.color());
        assert_eq!(a.body.paint(), b.body.paint());
        assert_eq!(a.body.face_sources(), b.body.face_sources());
        assert_eq!(a.closed, b.closed);
        assert!(!b.body.holds_kernel_form(), "native records restore without calling SMLib");
    }
    // Hostile native records are still validated structurally and geometrically.
    let mut broken = decoded.clone();
    broken.bodies[0].face_origin.pop();
    let imp_broken = records_import(&broken).unwrap();
    assert_eq!(imp_broken.bodies.len(), 1);
    assert!(imp_broken.warnings.iter().any(|w| w.contains("face origins")));

    let mut bad_idx = decoded.clone();
    bad_idx.bodies[0].brep.as_mut().unwrap().edges[0].vertices[0] = 999;
    let imp_bad = records_import(&bad_idx).unwrap();
    assert_eq!(imp_bad.bodies.len(), 1);
    assert!(imp_bad.warnings.iter().any(|w| w.contains("invalid")));

    // Structurally valid records whose geometry disagrees are refused identically with or without
    // `native: true`; the intact sibling body still restores without SMLib when `native: true`.
    let mut disagree_native = decoded;
    let bad_brep = disagree_native.bodies[0].brep.as_mut().unwrap();
    bad_brep.vertices[0].point[2] += 0.5;
    solidcore_ops::record_check::validate(bad_brep).expect("mutated record must remain structurally valid");
    let mut disagree_plain = disagree_native.clone();
    disagree_plain.native = false;
    let imp_disagree_native = records_import(&disagree_native).unwrap();
    let imp_disagree_plain = records_import(&disagree_plain).unwrap();
    assert_eq!(imp_disagree_native.bodies.len(), 1);
    assert_eq!(imp_disagree_plain.bodies.len(), 1);
    assert!(!imp_disagree_native.bodies[0].body.holds_kernel_form(), "good native sibling still restores without SMLib");
    assert_eq!(imp_disagree_native.warnings, imp_disagree_plain.warnings);
    assert_eq!(imp_disagree_native.report, imp_disagree_plain.report);
    assert!(imp_disagree_native.warnings.iter().any(|w| w.contains("geometry disagrees")));
}

#[test]
fn parallel_records_import_matches_serial_and_respects_cancellation() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/step-conformance");
    let parts = ["cone_point", "cylinder_seam", "dome", "washer"];
    let mut combined = ImportRecords::new(Vec::new(), Vec::new(), vec!["header note".into()], "test".into());
    let mut indices = Vec::new();
    for (idx, part) in parts.iter().enumerate() {
        let mut r = kept(&std::fs::read_to_string(dir.join(format!("{part}.step"))).unwrap());
        let mut orig = r.bodies.remove(0);
        orig.name = format!("{part}_{idx}");
        let orig_index = combined.bodies.len();
        combined.bodies.push(orig);
        indices.push(orig_index);
        // Add a placed copy of the original to exercise both parallel prep and copy ordering.
        let copy_index = combined.bodies.len();
        combined.bodies.push(RecordBody {
            name: format!("{part}_{idx}_copy"),
            path: Vec::new(),
            instance: Some(format!("inst_{idx}")),
            color: None,
            paint: None,
            hidden: false,
            placement: Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [50.0 * (idx as f64 + 1.0), 0.0, 0.0, 1.0]]),
            brep: None,
            face_origin: Vec::new(),
            sheet: false,
            copy_of: Some(orig_index),
            mesh: None,
        });
        indices.push(copy_index);
    }
    // Insert a broken record to verify deterministic warning and left-out placement.
    let mut broken = combined.bodies[0].clone();
    broken.name = "broken_mid".into();
    broken.face_origin.pop();
    let broken_idx = combined.bodies.len();
    combined.bodies.push(broken);
    indices.push(broken_idx);
    combined.tree.push(StepNode {
        name: "assembly".into(),
        transform: IDENTITY,
        bodies: indices,
        items: Vec::new(),
        left_out: Vec::new(),
        children: Vec::new(),
    });

    let serial = super::records_import_with_workers(&combined, 1).unwrap();
    let parallel = super::records_import_with_workers(&combined, 4).unwrap();
    assert_eq!(parallel.bodies.len(), serial.bodies.len());
    assert_eq!(parallel.warnings, serial.warnings);
    assert_eq!(parallel.tree, serial.tree);
    assert_eq!(parallel.report.parts, serial.report.parts);
    assert_eq!(parallel.report.solids, serial.report.solids);
    assert_eq!(parallel.report.issues, serial.report.issues);
    for (p, s) in parallel.bodies.iter().zip(&serial.bodies) {
        let (mp, ms) = (measure(&p.body).unwrap(), measure(&s.body).unwrap());
        assert_eq!((&p.name, p.source_item, p.closed, &p.face_origin, mp.faces), (&s.name, s.source_item, s.closed, &s.face_origin, ms.faces));
        assert_eq!(mp.volume.to_bits(), ms.volume.to_bits());
        assert_eq!(mp.area.to_bits(), ms.area.to_bits());
    }

    struct CancelWatch(AtomicBool);
    impl crate::watch::Watch for CancelWatch {
        fn cancelled(&self) -> bool {
            self.0.load(Ordering::Relaxed)
        }
        fn progress(&self, _: &'static str, _: usize, _: usize) {}
    }
    let watch = Arc::new(CancelWatch(AtomicBool::new(true)));
    let err = crate::watch::watching(watch, || super::records_import_with_workers(&combined, 4)).unwrap_err();
    assert!(err.to_string().contains("cancel"), "{err}");
}
