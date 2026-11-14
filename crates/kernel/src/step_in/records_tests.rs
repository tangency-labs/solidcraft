//! Imports kept as records build the bodies a STEP import builds: the step-conformance parts
//! (cones, cylinders, a dome, a NURBS torus ring, a washer, a sheet) read by the STEP reader into
//! records, then made into bodies from those records alone. Records from a file are hostile: a
//! broken one fails its import with the reason, never a panic.

use super::super::{IDENTITY, StepNode, step_import};
use super::{ImportRecords, RecordBody, records_import};
use crate::measure;

/// The records the STEP reader assembles from `text`, kept as an import.
fn kept(text: &str) -> ImportRecords {
    let ex = solidcore_step::p21::parse(text).unwrap();
    let read = solidcore_step::topology::read_solids_with(&ex, &solidcraft_smlib::pcurves::SmlibTrims);
    assert!(read.failures.is_empty(), "{:?}", read.failures);
    let bodies: Vec<RecordBody> = read
        .solids
        .into_iter()
        .map(|s| RecordBody {
            name: s.name,
            path: Vec::new(),
            instance: None,
            color: None,
            hidden: false,
            placement: None,
            brep: s.brep,
            face_origin: s.face_origin,
            sheet: s.sheet,
        })
        .collect();
    let tree = vec![StepNode { name: "part".into(), transform: IDENTITY, bodies: (0..bodies.len()).collect(), children: Vec::new() }];
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
            assert_eq!((mr.faces, r.closed, &r.face_origin), (ms.faces, s.closed, &s.face_origin), "{part}");
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
    assert!(with_broken(&|b| b.brep.edges[0].vertices[0] = 999).contains("invalid"));
    assert!(with_broken(&|b| b.placement = Some([[f64::NAN; 4]; 4])).contains("not finite"));
    // A product naming a body the import does not have refuses the whole import.
    let mut r = good;
    r.tree[0].bodies.push(7);
    assert!(records_import(&r).unwrap_err().to_string().contains("names a body"));
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
    assert!(r.bodies[0].sheet && r.bodies[0].brep.regions.len() == 1, "the reader reads the square as a sheet");
    r.bodies[0].sheet = false;
    let imp = records_import(&r).unwrap();
    assert!(!imp.bodies[0].closed);
    assert_eq!(imp.report.solids, 0);
}
