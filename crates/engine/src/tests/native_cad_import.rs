//! FreeCAD and Rhino files (read through cadmpeg) go the same way as STEP files: a new rigid
//! component named after the file (#1426), with the `up` and `names` options (#1312).

use serde_json::{Value, json};
use std::io::{Cursor, Write};

use super::run;
use crate::*;

const BLOCK_FCSTD: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.FCStd");
const BLOCK_3DM: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.3dm");

/// `bytes` saved as `name` in a fresh temp directory.
fn file(tag: &str, name: &str, bytes: &[u8]) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-native-cad-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (dir, path.to_string_lossy().to_string())
}

fn extent(s: &Session) -> ([f64; 3], [f64; 3]) {
    let b = crate::view::bounds(s);
    ([b.min.x, b.min.y, b.min.z], [b.max.x, b.max.y, b.max.z])
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6)
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

#[test]
fn freecad_and_rhino_files_come_in_as_a_component_named_after_the_file() {
    for (tag, name, bytes) in [("fc", "block.FCStd", BLOCK_FCSTD), ("rh", "block.3dm", BLOCK_3DM)] {
        let (dir, path) = file(tag, name, bytes);
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]}));
        let r = run(&mut s, "file.insert_step", json!({"path": path}));
        assert_eq!(r["component_name"], "block", "{name}: {r}");
        let feature = r["feature"].as_u64().unwrap();
        assert_eq!(s.doc.feature(feature).unwrap().kind.type_name(), "BaseFeature", "{name}");
        assert_eq!(s.doc.feature(feature).unwrap().component, r["component"].as_u64().unwrap(), "{name}");
        assert!((volume(&mut s) - (6000.0 + 125.0)).abs() < 1e-6, "{name}");
        // Rhino keeps the object's name; cadmpeg leaves FreeCAD's label out, so it is the file's.
        assert!(body_names(&s).iter().any(|n| n == "Block" || n == "block"), "{name}: {:?}", body_names(&s));
        assert_eq!(s.active_component, 0, "{name}: the active component is unchanged");

        // As written the block spans 10 × 20 × 30; up: y stands it up, (x, y, z) → (x, -z, y).
        let mut d = Session::default();
        let r = run(&mut d, "doc.open", json!({"path": path}));
        assert_eq!((r["features"].clone(), r["component_name"].clone()), (json!(1), json!("block")), "{name}: {r}");
        assert!(near(extent(&d).1, [10.0, 20.0, 30.0]), "{name}: {:?}", extent(&d));
        let mut d = Session::default();
        let r = run(&mut d, "file.insert_step", json!({"path": path, "up": "y"}));
        assert_eq!(r["stand_up"], r["occurrence"], "{name}: {r}");
        let (lo, hi) = extent(&d);
        assert!(near(lo, [0.0, -30.0, 0.0]) && near(hi, [10.0, 0.0, 20.0]), "{name}: stood up: {lo:?} {hi:?}");

        // Every names choice is taken, and the body is named something.
        for names in ["solid", "product", "instance"] {
            let mut d = Session::default();
            run(&mut d, "file.insert_step", json!({"path": path, "names": names}));
            let got = body_names(&d);
            assert!(got.len() == 1 && !got[0].is_empty(), "{name} names={names}: {got:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A broken file is refused with a message naming it, and the design is left as it was.
#[test]
fn a_broken_freecad_or_rhino_file_is_refused_cleanly() {
    let mut cut = BLOCK_FCSTD[..BLOCK_FCSTD.len() / 2].to_vec();
    cut.extend_from_slice(b"garbage");
    for (tag, name, bytes) in
        [("bad-fc", "broken.FCStd", cut.as_slice()), ("bad-rh", "broken.3dm", &BLOCK_3DM[..200]), ("acis", "cube.sat", b"700 0 1 0\n")]
    {
        let (dir, path) = file(tag, name, bytes);
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
        let before = s.doc.clone();
        let e: Result<Value> = s.execute("file.insert_step", &json!({"path": path}));
        let e = e.expect_err(name).to_string();
        assert!(!e.contains("internal"), "{name}: a clean error, not a caught panic: {e}");
        assert_eq!(s.doc, before, "{name}: the design is unchanged");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

fn sketch_archive() -> Vec<u8> {
    let xml = r#"<Document SchemaVersion="4">
      <Properties><Property name="Label"><String value="Sketch Project"/></Property></Properties>
      <Objects Count="4">
        <Object type="App::Part" name="Part"/><Object type="PartDesign::Body" name="Body"/>
        <Object type="Sketcher::SketchObject" name="Profile"/><Object type="Sketcher::SketchObject" name="Loose"/>
      </Objects>
      <ObjectData Count="4">
        <Object name="Part"><Properties><Property name="Group"><LinkList><Link value="Body"/></LinkList></Property><Property name="Label"><String value="Bracket"/></Property></Properties></Object>
        <Object name="Body"><Properties><Property name="Group"><LinkList><Link value="Profile"/></LinkList></Property><Property name="Label"><String value="Plate"/></Property></Properties></Object>
        <Object name="Profile"><Properties>
          <Property name="Constraints"><ConstraintList>
            <Constrain Type="1" Value="0" First="0" FirstPos="2" Second="1" SecondPos="1" Third="-2000" IsDriving="1" IsActive="1" IsInVirtualSpace="0"/>
            <Constrain Type="2" Value="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000" IsDriving="1" IsActive="1" IsInVirtualSpace="0"/>
            <Constrain Type="6" Value="20" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000" IsDriving="1" IsActive="1" IsInVirtualSpace="0"/>
          </ConstraintList></Property>
          <Property name="Geometry"><GeometryList>
            <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="20" EndY="0"/><Construction value="0"/></Geometry>
            <Geometry type="Part::GeomLineSegment"><LineSegment StartX="20" StartY="0" EndX="20" EndY="10"/><Construction value="0"/></Geometry>
          </GeometryList></Property>
          <Property name="Label"><String value="Mounting Profile"/></Property>
        </Properties></Object>
        <Object name="Loose"><Properties>
          <Property name="Geometry"><GeometryList><Geometry type="Part::GeomCircle"><Circle CenterX="5" CenterY="5" Radius="2"/><Construction value="0"/></Geometry></GeometryList></Property>
          <Property name="Label"><String value="Loose Sketch"/></Property>
        </Properties></Object>
      </ObjectData>
    </Document>"#;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Document.xml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

fn operations_archive(xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Document.xml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(xml.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn freecad_operations_import_dependencies_target_only_their_body_and_undo_together() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml");
    let (dir, path) = file("operations", "source.FCStd", &operations_archive(xml));
    let mut session = Session::default();
    run(&mut session, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Unrelated"}));
    let imported = run(
        &mut session,
        "file.import_freecad_sketches",
        json!({
            "path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]
        }),
    );
    let operations = imported["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 2, "the Pad dependency is included automatically");
    assert_eq!(operations[0]["source"], "Pad");
    assert!(operations.iter().all(|op| op["status"] == "imported"), "{operations:?}");
    let plate_component = session.doc.components.iter().find(|component| component.name == "Plate").unwrap().id;
    assert!(
        session.model.state().bodies.iter().any(|body| body.name == "Plate" && session.doc.model_body_component(body) == plate_component),
        "the imported FreeCAD Body owns its solid"
    );
    assert!(
        operations.iter().all(|op| session.doc.feature(op["feature"].as_u64().unwrap()).is_some_and(|feature| feature.component == plate_component)),
        "the Body component owns its operation history"
    );
    assert!(session.doc.browser_groups.is_empty(), "the Body component's standard folders hold solids and sketches directly");
    assert!((volume(&mut session) - 6900.0).abs() < 1e-6, "Pocket cuts Plate only, not the overlapping Unrelated body");
    assert_eq!(session.undo.len(), 2, "one undo step for the complete import");
    let id = operations[1]["feature"].as_u64().unwrap();
    assert!(!session.doc.feature(id).unwrap().param_names.is_empty(), "the imported feature has editable dimensions");
    run(&mut session, "edit.undo", json!({}));
    assert!((volume(&mut session) - 1000.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_negative_pad_length_imports_a_solid_in_the_saved_direction() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("<Property name=\"Length\"><Float value=\"10\"/></Property>", "<Property name=\"Length\"><Float value=\"-10\"/></Property>");
    let (dir, path) = file("negative-pad", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pad"]}));
    assert_eq!(imported["operations"][0]["status"], "imported", "{imported}");
    assert!((volume(&mut session) - 6000.0).abs() < 1e-6);
    let measured = run(&mut session, "inspect.measure", json!({}));
    assert!(
        (measured["bodies"][0]["center_of_mass"][2].as_f64().unwrap() + 5.0).abs() < 1e-6,
        "negative FreeCAD length extrudes below the sketch plane"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_mirror_of_a_starting_pad_joins_the_existing_body() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("</Objects>", "<Object name=\"MirrorPlane\" type=\"App::Plane\"/><Object name=\"Mirrored\" type=\"PartDesign::Mirrored\"/></Objects>")
        .replace("<Link value=\"Pad\"/><Link value=\"PocketProfile\"/>", "<Link value=\"Pad\"/><Link value=\"Mirrored\"/><Link value=\"PocketProfile\"/>")
        .replace("</ObjectData>", r#"<Object name="MirrorPlane"><Properties><Property name="Placement"><PropertyPlacement Px="30" Py="0" Pz="0" Q0="0.5" Q1="0.5" Q2="0.5" Q3="0.5"/></Property></Properties></Object>
          <Object name="Mirrored"><Properties>
            <Property name="BaseFeature"><Link value="Pad"/></Property>
            <Property name="Originals"><LinkList><Link value="Pad"/></LinkList></Property>
            <Property name="MirrorPlane"><LinkSub value="MirrorPlane"/></Property>
            <Property name="TransformMode"><Integer value="1"/></Property>
          </Properties></Object></ObjectData>"#);
    let (dir, path) = file("mirror-pad", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Mirrored"]}));
    assert!(imported["operations"].as_array().unwrap().iter().all(|operation| operation["status"] == "imported"), "{imported}");
    assert!(matches!(
        session.doc.feature(imported["operations"][1]["feature"].as_u64().unwrap()).unwrap().kind,
        solidcraft_doc::FeatureKind::Mirror { .. }
    ));
    assert_eq!(body_names(&session), ["Plate"]);
    assert!((volume(&mut session) - 12_000.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_compound_mirror_keeps_both_solids_as_targets_for_a_later_pocket() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("</Objects>", "<Object name=\"MirrorPlane\" type=\"App::Plane\"/><Object name=\"Mirrored\" type=\"PartDesign::Mirrored\"/></Objects>")
        .replace("<Object name=\"Plate\"><Properties><Property name=\"Group\">", "<Object name=\"Plate\"><Properties><Property name=\"AllowCompound\"><Bool value=\"true\"/></Property><Property name=\"Group\">")
        .replace("<Link value=\"Pad\"/><Link value=\"PocketProfile\"/>", "<Link value=\"Pad\"/><Link value=\"Mirrored\"/><Link value=\"PocketProfile\"/>")
        .replace("<Object name=\"Pocket\"><Properties>\n      <Property name=\"BaseFeature\"><Link value=\"Pad\"/></Property>",
            "<Object name=\"Pocket\"><Properties>\n      <Property name=\"BaseFeature\"><Link value=\"Mirrored\"/></Property>")
        .replace("</ObjectData>", r#"<Object name="MirrorPlane"><Properties><Property name="Placement"><PropertyPlacement Px="-5" Py="0" Pz="0" Q0="0.5" Q1="0.5" Q2="0.5" Q3="0.5"/></Property></Properties></Object>
          <Object name="Mirrored"><Properties>
            <Property name="BaseFeature"><Link value="Pad"/></Property>
            <Property name="Originals"><LinkList><Link value="Pad"/></LinkList></Property>
            <Property name="MirrorPlane"><LinkSub value="MirrorPlane"/></Property>
            <Property name="TransformMode"><Integer value="1"/></Property>
          </Properties></Object></ObjectData>"#);
    let (dir, path) = file("compound-mirror", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    assert!(imported["operations"].as_array().unwrap().iter().all(|operation| operation["status"] == "imported"), "{imported}");
    assert_eq!(body_names(&session).len(), 2, "the source FreeCAD Body permits both disconnected solids");
    let pocket = session.doc.feature(imported["operations"][2]["feature"].as_u64().unwrap()).unwrap();
    assert!(matches!(&pocket.kind, solidcraft_doc::FeatureKind::Extrude { targets, .. } if targets.len() == 2));
    assert!((volume(&mut session) - 11_900.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_circular_pattern_repeats_a_pocket_about_its_saved_axis() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("</Objects>", "<Object name=\"AxisSketch\" type=\"Sketcher::SketchObject\"/><Object name=\"PolarPattern\" type=\"PartDesign::PolarPattern\"/></Objects>")
        .replace("<Link value=\"PocketProfile\"/><Link value=\"Pocket\"/>", "<Link value=\"PocketProfile\"/><Link value=\"Pocket\"/><Link value=\"AxisSketch\"/><Link value=\"PolarPattern\"/>")
        .replace("</ObjectData>", r#"<Object name="AxisSketch"><Properties><Property name="Placement"><PropertyPlacement Px="15" Py="10" Pz="10" Q0="0" Q1="0" Q2="0" Q3="1"/></Property></Properties></Object>
          <Object name="PolarPattern"><Properties>
            <Property name="BaseFeature"><Link value="Pocket"/></Property>
            <Property name="Originals"><LinkList><Link value="Pocket"/></LinkList></Property>
            <Property name="Axis"><LinkSub value="AxisSketch"><Sub value="N_Axis"/></LinkSub></Property>
            <Property name="Occurrences"><Integer value="2"/></Property>
            <Property name="Angle"><Float value="180"/></Property>
            <Property name="Mode"><Integer value="0"/></Property>
            <Property name="TransformMode"><Integer value="0"/></Property>
          </Properties></Object></ObjectData>"#);
    let (dir, path) = file("pattern-pocket", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["PolarPattern"]}));
    assert!(imported["operations"].as_array().unwrap().iter().all(|operation| operation["status"] == "imported"), "{imported}");
    assert!(matches!(
        session.doc.feature(imported["operations"][2]["feature"].as_u64().unwrap()).unwrap().kind,
        solidcraft_doc::FeatureKind::Pattern { .. }
    ));
    assert_eq!(body_names(&session), ["Plate"]);
    assert!((volume(&mut session) - 5_800.0).abs() < 1e-5);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_compound_circular_pattern_keeps_disconnected_rollers() {
    let xml = r#"<Document><Objects>
      <Object name="Body" type="PartDesign::Body"/>
      <Object name="Sketch" type="Sketcher::SketchObject"/>
      <Object name="Pad004" type="PartDesign::Pad"/>
      <Object name="PolarPattern002" type="PartDesign::PolarPattern"/>
    </Objects><ObjectData>
      <Object name="Body"><Properties>
        <Property name="Label"><String value="Rollers"/></Property>
        <Property name="AllowCompound"><Bool value="true"/></Property>
        <Property name="Group"><LinkList><Link value="Sketch"/><Link value="Pad004"/><Link value="PolarPattern002"/></LinkList></Property>
      </Properties></Object>
      <Object name="Sketch"><Properties><Property name="Geometry"><GeometryList>
        <Geometry type="Part::GeomCircle"><Circle CenterX="10" CenterY="0" Radius="2"/></Geometry>
      </GeometryList></Property></Properties></Object>
      <Object name="Pad004"><Properties>
        <Property name="Profile"><LinkSub value="Sketch"/></Property>
        <Property name="Length"><Float value="5"/></Property>
      </Properties></Object>
      <Object name="PolarPattern002"><Properties>
        <Property name="BaseFeature"><Link value="Pad004"/></Property>
        <Property name="Originals"><LinkList><Link value="Pad004"/></LinkList></Property>
        <Property name="Axis"><LinkSub value="Sketch"><Sub value="N_Axis"/></LinkSub></Property>
        <Property name="Occurrences"><Integer value="4"/></Property>
        <Property name="Angle"><Float value="360"/></Property>
        <Property name="Mode"><Integer value="0"/></Property>
        <Property name="TransformMode"><Integer value="0"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let (dir, path) = file("compound-rollers", "source.FCStd", &operations_archive(xml));
    let mut session = Session::default();
    let imported = run(
        &mut session,
        "file.import_freecad_sketches",
        json!({"path":path,"sketches":[],"import_operations":true,"operations":["PolarPattern002"]}),
    );
    assert!(imported["operations"].as_array().unwrap().iter().all(|operation| operation["status"] == "imported"), "{imported}");
    assert_eq!(body_names(&session).len(), 4);
    let measured = run(&mut session, "inspect.measure", json!({}));
    let actual = measured["total"]["volume_mm3"].as_f64().unwrap();
    let expected = 80.0 * std::f64::consts::PI;
    assert!((actual - expected).abs() / expected < 2e-4, "expected four 2 mm radius × 5 mm cylinders; measured {measured}");
    let bodies = measured["bodies"].as_array().unwrap();
    for [x, y] in [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0], [0.0, -10.0]] {
        assert!(
            bodies.iter().any(|body| {
                let center = &body["center_of_mass"];
                (center[0].as_f64().unwrap() - x).abs() < 1e-6 && (center[1].as_f64().unwrap() - y).abs() < 1e-6
            }),
            "missing roller at ({x}, {y}): {measured}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_pocket_without_a_base_solid_explains_why_its_pattern_cannot_run() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("<Property name=\"BaseFeature\"><Link value=\"Pad\"/></Property>", "<Property name=\"BaseFeature\"><Link value=\"\"/></Property>");
    let (dir, path) = file("pocket-without-base", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    assert_eq!(imported["operations"][0]["status"], "needs_repair", "{imported}");
    let warnings = imported["operations"][0]["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|issue| issue["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("subtractive") && detail.contains("no preceding BaseFeature") && detail.contains("Plate"))),
        "{warnings:?}"
    );
    assert!(session.model.state().bodies.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_revolution_and_groove_keep_the_axis_sweep_and_dimensions() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml");
    let (dir, path) = file("revolve-operations", "source.FCStd", &operations_archive(xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Revolution"]}));
    assert_eq!(imported["operations"][0]["status"], "imported", "{imported}");
    let expected = std::f64::consts::PI * 75.0 * 15.0 * 0.75;
    let actual = volume(&mut session);
    assert!((actual - expected).abs() / expected < 2e-4, "expected {expected} mm³, measured {actual} mm³");
    let measured = run(&mut session, "inspect.measure", json!({}));
    let center = measured["bodies"][0]["center_of_mass"].as_array().unwrap();
    let radial_center = -3500.0 / (675.0 * std::f64::consts::PI);
    for (coordinate, expected) in center.iter().zip([radial_center, 7.5, radial_center]) {
        assert!((coordinate.as_f64().unwrap() - expected).abs() < 2e-4, "right-hand rotation about FreeCAD's +Y axis: {measured}");
    }
    run(&mut session, "edit.undo", json!({}));
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Groove"]}));
    assert!(imported["operations"].as_array().unwrap().iter().all(|op| op["status"] == "imported"), "{imported}");
    let expected_after_groove = expected - std::f64::consts::PI * 36.0 * 5.0 * 0.5;
    let actual = volume(&mut session);
    assert!((actual - expected_after_groove).abs() / expected_after_groove < 2e-4, "expected {expected_after_groove} mm³, measured {actual} mm³");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_face_attached_operation_uses_saved_independent_plane() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml");
    let xml = xml.replace("<Object name=\"PocketProfile\"><Properties>", "<Object name=\"PocketProfile\"><Properties><Property name=\"MapMode\"><Integer value=\"5\"/></Property><Property name=\"AttachmentSupport\"><LinkSubList><Link obj=\"Pad\" sub=\"Face6\"/></LinkSubList></Property>");
    let (dir, path) = file("independent-face-profile", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    let operations = imported["operations"].as_array().unwrap();
    assert_eq!(operations[0]["status"], "imported");
    assert_eq!(operations[1]["status"], "imported");
    let sketch_warnings =
        imported["imported"].as_array().unwrap().iter().find(|sketch| sketch["source"] == "PocketProfile").unwrap()["warnings"].as_array().unwrap();
    assert_eq!(sketch_warnings.len(), 1);
    assert_eq!(sketch_warnings[0]["severity"], "warning");
    assert!(sketch_warnings[0]["detail"].as_str().is_some_and(|detail| detail.contains("Pad") && detail.contains("Face6")));
    let feature = session.doc.feature(operations[1]["feature"].as_u64().unwrap()).unwrap();
    assert!(!feature.suppressed);
    let solidcraft_doc::FeatureKind::Extrude { sketch, .. } = &feature.kind else { panic!("expected native Pocket") };
    let plane = session.model.state().sketch(sketch.0).unwrap().plane;
    assert!((plane.origin.z - 10.0).abs() < 1e-9, "the face-attached sketch keeps its saved location");
    assert!((volume(&mut session) - 5900.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_sketch_with_empty_attachment_support_keeps_its_saved_plane_and_operation() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml").replace(
        "<Object name=\"PocketProfile\"><Properties>",
        "<Object name=\"PocketProfile\"><Properties><Property name=\"MapMode\"><Integer value=\"5\"/></Property><Property name=\"AttachmentSupport\"><LinkSubList count=\"0\"/></Property>",
    );
    let (dir, path) = file("detached-profile", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    assert!(imported["operations"].as_array().unwrap().iter().all(|op| op["status"] == "imported"), "{imported}");
    assert!(
        imported["imported"].as_array().unwrap().iter().find(|sketch| sketch["source"] == "PocketProfile").unwrap()["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!((volume(&mut session) - 5900.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_pad_with_only_second_side_length_imports_as_reverse_extrude() {
    let fixture = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml");
    for first in ["0", "-1"] {
        let xml = fixture.replace(
            "<Property name=\"Length\"><Float value=\"10\"/></Property>",
            &format!("<Property name=\"Length\"><Float value=\"{first}\"/></Property><Property name=\"Length2\"><Float value=\"8\"/></Property><Property name=\"SideType\"><Integer value=\"1\"/></Property>"),
        );
        let (dir, path) = file("second-side-pad", "source.FCStd", &operations_archive(&xml));
        let mut session = Session::default();
        let imported =
            run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pad"]}));
        assert_eq!(imported["operations"][0]["status"], "imported", "{imported}");
        let feature = session.doc.feature(imported["operations"][0]["feature"].as_u64().unwrap()).unwrap();
        assert!(matches!(&feature.kind, solidcraft_doc::FeatureKind::Extrude { extent, .. }
            if matches!(extent.direction, solidcraft_doc::Direction::Negative) && extent.distance2.is_none()));
        assert!((volume(&mut session) - 4800.0).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn freecad_skipped_operation_placeholders_survive_reopening_and_never_change_geometry() {
    let xml =
        include_str!("../../../io/src/freecad_sketch/operations_fixture.xml").replace("type=\"PartDesign::Pocket\"", "type=\"PartDesign::Fillet\"");
    let (dir, path) = file("placeholder-operations", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let before = session.doc.clone();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    let pad = imported["operations"][0]["feature"].as_u64().unwrap();
    let missing = imported["operations"][1]["feature"].as_u64().unwrap();
    assert_eq!(imported["operations"][1]["status"], "skipped");
    let feature = session.doc.feature(missing).unwrap();
    assert_eq!(feature.name, "[Not imported] Pocket");
    assert_eq!(feature.component, session.doc.feature(pad).unwrap().component);
    assert!(session.doc.feature_index(pad).unwrap() < session.doc.feature_index(missing).unwrap());
    assert!(session.model.result(missing).unwrap().warning.as_ref().unwrap().contains("was not imported"));
    assert!((volume(&mut session) - 6000.0).abs() < 1e-6);
    let reopened = solidcraft_doc::Document::from_json(&session.doc.to_json()).unwrap();
    assert_eq!(reopened.feature(missing), session.doc.feature(missing));
    let mut model = solidcraft_doc::Model::new();
    model.evaluate(&reopened);
    assert!(model.result(missing).unwrap().error.is_none());
    assert!(model.result(missing).unwrap().warning.is_some());
    assert_eq!(model.state().bodies.len(), 1);
    // Suppression cannot turn a placeholder into a modelling operation.
    run(&mut session, "timeline.suppress", json!({"feature":missing,"suppressed":true}));
    run(&mut session, "timeline.suppress", json!({"feature":missing,"suppressed":false}));
    assert!((volume(&mut session) - 6000.0).abs() < 1e-6);
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.doc, before, "the import, including placeholders, is one undo step");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_unsupported_history_blocks_dependents_and_cycles_do_not_change_the_design() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml");
    let unsupported = xml.replace("type=\"PartDesign::Pad\"", "type=\"PartDesign::Fillet\"");
    let (dir, path) = file("unsupported-operations", "source.FCStd", &operations_archive(&unsupported));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    assert_eq!(imported["operations"][0]["status"], "skipped");
    assert_eq!(imported["operations"][1]["status"], "needs_repair");
    let warnings = imported["operations"][1]["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|issue| issue["summary"] == "The target body has not been created."
            && issue["detail"].as_str().is_some_and(|detail| detail.contains("Plate") && detail.contains("Pad") && detail.contains("PocketProfile"))),
        "the missing predecessor and target body must be named: {warnings:?}"
    );
    let placeholder = session.doc.feature(imported["operations"][0]["feature"].as_u64().unwrap()).unwrap();
    assert!(placeholder.name.starts_with("[Not imported]"));
    assert!(matches!(&placeholder.kind, solidcraft_doc::FeatureKind::ImportPlaceholder { source, source_type, detail }
        if source == "Pad" && source_type == "PartDesign::Fillet" && !detail.is_empty()));
    assert!(session.model.state().bodies.is_empty(), "skipping an unsupported predecessor must not enable a later cut");
    let plate_component = session.doc.components.iter().find(|component| component.name == "Plate").unwrap().id;
    assert!(
        session
            .doc
            .metadata
            .get("org.solidcraft.freecad.body_components")
            .and_then(|value| value.get("ids"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ids| ids.iter().any(|id| id.as_u64() == Some(plate_component)))
    );
    assert!(session.doc.browser_groups.is_empty(), "a missing solid leaves an empty Bodies folder in the Body component");
    assert!(
        imported["body_warnings"].as_array().unwrap().is_empty(),
        "a source Body without an additive starting operation need not contain a solid"
    );
    let _ = std::fs::remove_dir_all(dir);

    let cyclic = xml.replace(
        "<Object name=\"Pad\"><Properties>",
        "<Object name=\"Pad\"><Properties><Property name=\"BaseFeature\"><Link value=\"Pocket\"/></Property>",
    );
    let (dir, path) = file("cyclic-operations", "source.FCStd", &operations_archive(&cyclic));
    let before = session.doc.clone();
    let error = session
        .execute("file.import_freecad_sketches", &json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}))
        .unwrap_err();
    assert!(error.to_string().contains("cycle"), "{error}");
    assert_eq!(session.doc, before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_pad_after_missing_fillet_builds_on_the_available_body() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace(
            "<Object name=\"Pad\" type=\"PartDesign::Pad\"/>",
            "<Object name=\"Pad\" type=\"PartDesign::Pad\"/><Object name=\"Fillet\" type=\"PartDesign::Fillet\"/><Object name=\"Pad002\" type=\"PartDesign::Pad\"/>",
        )
        .replace(
            "<Link value=\"PadProfile\"/><Link value=\"Pad\"/><Link value=\"PocketProfile\"/>",
            "<Link value=\"PadProfile\"/><Link value=\"Pad\"/><Link value=\"Fillet\"/><Link value=\"Pad002\"/><Link value=\"PocketProfile\"/>",
        )
        .replace(
            "</ObjectData>",
            "<Object name=\"Fillet\"><Properties><Property name=\"BaseFeature\"><Link value=\"Pad\"/></Property></Properties></Object>\
             <Object name=\"Pad002\"><Properties><Property name=\"BaseFeature\"><Link value=\"Fillet\"/></Property>\
             <Property name=\"Profile\"><LinkSub value=\"PocketProfile\"/></Property>\
             <Property name=\"Length\"><Float value=\"4\"/></Property></Properties></Object></ObjectData>",
        );
    let (dir, path) = file("pad-after-fillet", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported = run(
        &mut session,
        "file.import_freecad_sketches",
        json!({
            "path": path, "sketches": [], "import_operations": true, "operations": ["Pad002"]
        }),
    );
    let operations = imported["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 3);
    assert_eq!(operations[0]["status"], "imported");
    assert_eq!(operations[1]["status"], "skipped");
    assert_eq!(operations[2]["status"], "imported", "{operations:?}");
    let pad = session.doc.feature(operations[2]["feature"].as_u64().unwrap()).unwrap();
    assert_eq!(pad.name, "Pad002");
    assert!(!pad.suppressed);
    assert!(operations[2]["warnings"].as_array().unwrap().iter().any(|w| w["severity"] == "warning"));
    assert!((volume(&mut session) - 6100.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_missing_base_feature_keeps_one_reserved_target_through_the_body_history() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("<Object name=\"Pad\" type=\"PartDesign::Pad\"/>", "<Object name=\"BaseFeature\" type=\"PartDesign::FeatureBase\"/><Object name=\"Pad\" type=\"PartDesign::Pad\"/>")
        .replace("<Link value=\"PadProfile\"/><Link value=\"Pad\"/>", "<Link value=\"BaseFeature\"/><Link value=\"PadProfile\"/><Link value=\"Pad\"/>")
        .replace("<Object name=\"Pad\"><Properties>", "<Object name=\"BaseFeature\"><Properties/></Object><Object name=\"Pad\"><Properties><Property name=\"BaseFeature\"><Link value=\"BaseFeature\"/></Property>");
    let (dir, path) = file("missing-base-feature", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pocket"]}));
    let operations = imported["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 3, "the unsupported base and Pad are included as dependencies");
    assert_eq!(operations[0]["status"], "skipped");
    for operation in &operations[1..] {
        assert_eq!(operation["status"], "needs_repair");
        let warnings = operation["warnings"].as_array().unwrap();
        assert!(
            !warnings.iter().any(|warning| warning["summary"] == "The target body could not be identified."),
            "the FreeCAD Body and preceding operation identify the target: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|warning| warning["summary"] == "The target body has not been created."),
            "the missing base still prevents evaluation: {warnings:?}"
        );
        let feature = session.doc.feature(operation["feature"].as_u64().unwrap()).unwrap();
        let solidcraft_doc::FeatureKind::Extrude { targets, .. } = &feature.kind else { panic!("expected native extrude") };
        assert_eq!(targets.as_slice(), &[solidcraft_doc::BodyName("Plate".into())]);
        assert!(feature.suppressed);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_body_with_failed_additive_operation_reports_missing_solid() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("<Property name=\"Length\"><Float value=\"10\"/></Property>", "<Property name=\"Length\"><Float value=\"0\"/></Property>");
    let (dir, path) = file("failed-pad", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pad"]}));
    let body_issue = imported["body_warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|warning| warning["summary"].as_str().is_some_and(|summary| summary.contains("Plate")))
        .unwrap();
    assert!(
        body_issue["detail"].as_str().is_some_and(|detail| detail.contains("PadProfile")
            && detail.contains("PartDesign::Pad")
            && detail.contains("additive starting operation")),
        "the missing solid explanation lists the failed source operation: {body_issue}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn freecad_sketches_can_be_inspected_and_selectively_imported_in_one_undo_step() {
    let bytes = sketch_archive();
    let (dir, path) = file("sketches", "source.FCStd", &bytes);
    let mut s = Session::default();
    let inspected = run(&mut s, "file.inspect_freecad_sketches", json!({"path": path}));
    assert_eq!((inspected["project"].clone(), inspected["sketches"].clone()), (json!("Sketch Project"), json!(2)));
    assert_eq!(inspected["nodes"][0]["label"], "Bracket");
    assert_eq!(inspected["nodes"][0]["children"][0]["label"], "Plate");
    assert_eq!(inspected["nodes"][0]["children"][0]["children"][0]["source_name"], "Profile");
    assert!(s.doc.features.is_empty() && s.undo.is_empty(), "inspection does not change the design");

    let imported = run(&mut s, "file.import_freecad_sketches", json!({"path": path, "sketches": ["Profile"]}));
    assert_eq!(imported["imported"].as_array().unwrap().len(), 1);
    assert_eq!(imported["imported"][0]["path"], json!(["Bracket", "Plate", "Mounting Profile"]));
    assert_eq!(s.doc.features.len(), 1);
    let feature = &s.doc.features[0];
    assert_eq!(feature.name, "Mounting Profile");
    assert_eq!(s.doc.components.iter().map(|component| component.name.as_str()).collect::<Vec<_>>(), ["Bracket", "Plate"]);
    assert!(s.doc.browser_groups.is_empty());
    assert_eq!(s.doc.components[1].parent, s.doc.components[0].id);
    assert_eq!(feature.component, s.doc.components[1].id);
    let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Origin { name }, sketch } = &feature.kind else {
        panic!("import did not make a sketch")
    };
    assert_eq!(name, "XY");
    assert_eq!((sketch.curves.len(), sketch.constraints.len()), (2, 3));
    let dimension = sketch.constraints.iter().find(|constraint| matches!(constraint.kind, solidcraft_sketch::ConstraintKind::Length { .. })).unwrap();
    let parameter = dimension.param.as_deref().expect("an imported driving dimension is editable");
    assert!(s.doc.param(parameter).is_some(), "the dimension's model parameter exists");
    assert_eq!(s.undo.len(), 1, "all selected sketches are one command and one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.is_empty() && s.doc.components.is_empty() && s.doc.browser_groups.is_empty());

    let imported =
        run(&mut s, "file.import_freecad_sketches", json!({"path": path, "sketches": ["Profile"], "organization": "flat", "naming": "body"}));
    assert_eq!(imported["organization"], "flat");
    assert_eq!(s.doc.features[0].name, "Plate — Mounting Profile");
    assert_eq!(s.doc.features[0].component, 0);
    assert!(s.doc.components.is_empty() && s.doc.browser_groups.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
