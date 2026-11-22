//! Imports kept as records build the bodies a STEP import builds: the step-conformance parts
//! (cones, cylinders, a dome, a NURBS torus ring, a washer, a sheet) read by the STEP reader into
//! records, then made into bodies from those records alone. Records from a file are hostile: a
//! broken one fails its import with the reason, never a panic.

use super::super::{IDENTITY, StepNode, step_import};
use super::{ImportRecords, RecordBody, records_import, records_noted};
use crate::measure;

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
    ImportRecords { bodies, tree, warnings: Vec::new(), system: String::new() }
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
