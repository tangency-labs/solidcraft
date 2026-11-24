//! FreeCAD and Rhino files (read through cadmpeg) go the same way as STEP files: a new rigid
//! component named after the file (#1426), with the `up` and `names` options (#1312). A FreeCAD
//! file's feature history is rebuilt as features in that component unless `history: solids`.

use serde_json::{Value, json};
use std::io::{Cursor, Write};

use super::run;
use crate::*;

const BLOCK_FCSTD: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.FCStd");
const BLOCK_3DM: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/block.3dm");
const BRACKET_FCSTD: &[u8] = include_bytes!("../../../io/src/native_cad/fixtures/bracket.FCStd");

/// `bytes` saved as `name` in a fresh temp directory.
fn file(tag: &str, name: &str, bytes: &[u8]) -> (std::path::PathBuf, String) {
    let dir = std::env::temp_dir().join(format!("solidcraft-native-cad-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    (dir, path.to_string_lossy().to_string())
}

fn extent(s: &Session) -> ([f64; 3], [f64; 3]) {
    let b = crate::view::bounds(s).unwrap();
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
        let r = run(&mut s, "file.insert_step", json!({"path": path, "history": "solids"}));
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

/// A FreeCAD file's history comes in as features in a component named after the file, and stays
/// parametric: a sketch dimension drives the part. A second copy's body is numbered and its
/// features act on it. With `history: solids` the file's solids come in as one Import feature,
/// without the shapes FreeCAD keeps hidden.
#[test]
fn a_freecad_history_comes_in_as_editable_features() {
    let (dir, path) = file("hist", "bracket.FCStd", BRACKET_FCSTD);
    let expected = 7840.0 - 50.0 * std::f64::consts::PI;
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": path}));
    assert_eq!((r["history"].as_str(), r["features"].as_u64(), r["component_name"].as_str()), (Some("features"), Some(5), Some("bracket")), "{r}");
    let component = r["component"].as_u64().unwrap();
    assert!(s.doc.features.iter().all(|f| f.component == component), "every feature is in the file's component");
    assert_eq!(body_names(&s), ["Bracket"]);
    let v = volume(&mut s);
    assert!((v - expected).abs() < 0.05, "volume {v}");
    assert!(s.model.results.iter().all(|r| r.error.is_none()));

    // The rectangle's 40 mm dimension is a parameter: 50 mm adds 10 × 20 × 10 mm³.
    let param = s
        .doc
        .features
        .iter()
        .find_map(|f| match &f.kind {
            solidcraft_doc::FeatureKind::Sketch { sketch, .. } if f.name == "Profile" => {
                sketch.constraints.iter().find(|c| c.kind.value().is_some_and(|v| (v - 40.0).abs() < 1e-9)).and_then(|c| c.param.clone())
            }
            _ => None,
        })
        .expect("the 40 mm dimension");
    s.doc_mut().set_param(&param, "50 mm", None, None).unwrap();
    s.refresh();
    let wider = volume(&mut s);
    assert!((wider - (v + 2000.0)).abs() < 0.05, "volume {wider}");

    let mut d = Session::default();
    run(&mut d, "file.insert_step", json!({"path": path}));
    run(&mut d, "file.insert_step", json!({"path": path}));
    assert_eq!(body_names(&d), ["Bracket", "Bracket (2)"]);
    // Both files have sketches called Profile and HoleSketch: the second file's are numbered, not refused.
    let sketches: Vec<&str> =
        d.doc.features.iter().filter(|f| matches!(f.kind, solidcraft_doc::FeatureKind::Sketch { .. })).map(|f| f.name.as_str()).collect();
    assert_eq!(sketches, ["Profile", "HoleSketch", "Profile (2)", "HoleSketch (2)"]);
    assert!((volume(&mut d) - 2.0 * expected).abs() < 0.1, "each copy's pocket and fillet act on its own body");

    let mut d = Session::default();
    let r = run(&mut d, "file.insert_step", json!({"path": path, "history": "solids"}));
    assert_eq!(d.doc.feature(r["feature"].as_u64().unwrap()).unwrap().kind.type_name(), "BaseFeature");
    assert_eq!(body_names(&d).len(), 1, "the pad's, pocket's and tools' hidden shapes are not imported");
    assert!((volume(&mut d) - expected).abs() < 0.05);
    let e = d.execute("file.insert_step", &json!({"path": path, "history": "sketches"})).expect_err("an unknown history choice");
    assert!(e.to_string().contains("history"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A rebuilt history that cannot be inserted (here the design is at its feature limit) falls
/// back to the file's solids with a warning, and leaves the design as it was: no features,
/// parameters or component of the attempt stay behind.
#[test]
fn a_rebuilt_history_that_cannot_be_inserted_falls_back_to_solids() {
    let (dir, path) = file("fallback", "bracket.FCStd", BRACKET_FCSTD);
    let mut s = Session::default();
    let dummy = |id: u64| solidcraft_doc::Feature {
        id,
        name: format!("Dummy{id}"),
        suppressed: true,
        body_names: Vec::new(),
        param_names: Vec::new(),
        component: 0,
        edge_refs: Vec::new(),
        edge_names: Vec::new(),
        face_names: Vec::new(),
        participants: Vec::new(),
        shell_piece_order: false,
        kernel_piece_order: false,
        pattern_names_in_order: false,
        inserted: false,
        link_faces: Vec::new(),
        link_threads: Vec::new(),
        touching: solidcraft_doc::Touching::Separate,
        profile_frame: None,
        kind: solidcraft_doc::FeatureKind::Sketch {
            plane: solidcraft_doc::PlaneRef::Origin { name: "XY".into() },
            sketch: solidcraft_doc::sketch::Sketch::new(),
        },
    };
    // Room for the file's one Import feature, but not for the five features of its history.
    let made = solidcraft_doc::MAX_FEATURES - 3;
    {
        let doc = s.doc_mut();
        doc.features.extend((1..=made).map(|i| dummy(i as u64)));
        doc.next_id = made as u64 + 1;
    }
    let before = s.doc.features.len();
    let r = run(&mut s, "file.insert_step", json!({"path": path}));
    assert!(r["history"].is_null(), "the solids came in: {r}");
    let warnings: Vec<&str> = r["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(warnings.iter().any(|w| w.contains("feature history was not rebuilt")), "{warnings:?}");
    assert_eq!(s.doc.features.len(), before + 1, "only the file's Import feature was added");
    assert_eq!(s.doc.features.iter().filter(|f| !f.name.starts_with("Dummy")).count(), 1);
    assert_eq!(s.doc.feature(r["feature"].as_u64().unwrap()).unwrap().kind.type_name(), "BaseFeature");
    assert!(s.doc.params.iter().all(|p| !p.model), "the failed attempt's parameters are gone");
    assert_eq!(s.doc.components.len(), 1, "only the solids import made a component");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `reader` chooses how a FreeCAD or Rhino file's solids are read: by default (`solidcraft`)
/// cadmpeg's STEP writer restates them and the Import feature keeps that STEP text, as before the
/// records route; `cadmpeg` reads cadmpeg's model straight into records, with no STEP text. Both
/// bring the same block, through `file.insert_step` and `doc.open`; another word is refused.
#[test]
fn the_reader_option_chooses_the_step_route_or_the_records() {
    for (tag, name, bytes) in [("rdfc", "block.FCStd", BLOCK_FCSTD), ("rdrh", "block.3dm", BLOCK_3DM)] {
        let (dir, path) = file(tag, name, bytes);
        for (reader, records) in [(None, false), (Some("solidcraft"), false), (Some(" CADMPEG "), true)] {
            for cmd in ["file.insert_step", "doc.open"] {
                let mut s = Session::default();
                let mut p = json!({"path": path, "history": "solids"});
                if let Some(reader) = reader {
                    p["reader"] = json!(reader);
                }
                run(&mut s, cmd, p);
                let import = s.doc.features.iter().find_map(|f| match &f.kind {
                    crate::doc::FeatureKind::Import { step, records, .. } => Some((step.is_empty(), records.is_some())),
                    _ => None,
                });
                assert_eq!(import, Some((records, records)), "{name} {cmd} reader={reader:?}");
                assert!((volume(&mut s) - 6000.0).abs() < 1e-6, "{name} {cmd} reader={reader:?}");
            }
        }
        let mut s = Session::default();
        let e = s.execute("file.insert_step", &json!({"path": path, "reader": "other"})).expect_err("an unknown reader");
        assert!(e.to_string().contains("reader"), "{e}");
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
fn freecad_baseless_pocket_cannot_cut_an_unrelated_pad_in_the_same_source_body() {
    let xml = include_str!("../../../io/src/freecad_sketch/operations_fixture.xml")
        .replace("<Property name=\"BaseFeature\"><Link value=\"Pad\"/></Property>", "<Property name=\"BaseFeature\"><Link value=\"\"/></Property>");
    let (dir, path) = file("baseless-pocket-with-pad", "source.FCStd", &operations_archive(&xml));
    let mut session = Session::default();
    let imported =
        run(&mut session, "file.import_freecad_sketches", json!({"path":path,"sketches":[],"import_operations":true,"operations":["Pad","Pocket"]}));
    let operations = imported["operations"].as_array().unwrap();
    assert_eq!(operations[0]["status"], "imported", "{imported}");
    assert_eq!(operations[1]["status"], "needs_repair", "{imported}");
    let pocket = session.doc.feature(operations[1]["feature"].as_u64().unwrap()).unwrap();
    let solidcraft_doc::FeatureKind::Extrude { targets, .. } = &pocket.kind else { panic!("expected Pocket") };
    assert!(pocket.suppressed);
    assert!(targets.iter().all(|target| target.0 != "Plate"), "the source Body name is not a proven cut target");
    assert!((volume(&mut session) - 6000.0).abs() < 1e-6, "the Pad remains untouched");
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

/// Two separate 10 × 20 × 30 mm boxes, the second 100 mm along x, as manifold solid B-reps
/// (186) in an IGES file, as SolidCraft's IGES export writes them: two solids, 12000 mm³
/// together, spanning 0..110 × 0..20 × 0..30.
fn two_boxes_iges() -> Vec<u8> {
    use solidcore_record::Vec3;
    let a = crate::kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let b = crate::kernel::box_solid(Vec3::new(100.0, 0.0, 0.0), Vec3::new(110.0, 20.0, 30.0)).unwrap();
    let bodies = [
        crate::kernel::ExportBody { name: "A".into(), body: &a, color: None },
        crate::kernel::ExportBody { name: "B".into(), body: &b, color: None },
    ];
    crate::kernel::iges_export_bodies(&bodies, &Default::default()).unwrap().into_bytes()
}

/// `reader` chooses how an IGES file is read, as for a STEP file: by default (`solidcraft`)
/// SolidCraft's IGES reader restates it as STEP and the Import feature keeps that STEP text;
/// `cadmpeg` decodes it with cadmpeg's IGES decoder straight into records, with no STEP text.
/// Both bring the two boxes, through `file.insert_step` and `doc.open`.
#[test]
fn the_reader_option_chooses_the_iges_reader() {
    let (dir, path) = file("rdig", "boxes.igs", &two_boxes_iges());
    for (reader, records) in [(None, false), (Some("solidcraft"), false), (Some("cadmpeg"), true)] {
        for cmd in ["file.insert_step", "doc.open"] {
            let mut s = Session::default();
            let mut p = json!({"path": path});
            if let Some(reader) = reader {
                p["reader"] = json!(reader);
            }
            run(&mut s, cmd, p);
            let import = s.doc.features.iter().find_map(|f| match &f.kind {
                crate::doc::FeatureKind::Import { step, records, .. } => Some((step.is_empty(), records.is_some())),
                _ => None,
            });
            assert_eq!(import, Some((records, records)), "{cmd} reader={reader:?}");
            assert_eq!(s.model.state().bodies.len(), 2, "{cmd} reader={reader:?}");
            assert!((volume(&mut s) - 12000.0).abs() < 1e-3, "{cmd} reader={reader:?}");
            let (lo, hi) = extent(&s);
            assert!(near(lo, [0.0, 0.0, 0.0]) && near(hi, [110.0, 20.0, 30.0]), "{cmd} reader={reader:?}: {lo:?} {hi:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The program a STEP or IGES file names is read the same with either reader, so the window makes
/// the same stand-up offer: the reply's `system`, and `exporter` and `exporter_up`, which say a
/// SolidWorks file has Y up and SolidCraft's own Z up (another program says nothing). STEP: the
/// FILE_NAME preprocessor version and originating system, or FILE_DESCRIPTION, written here; IGES:
/// the global section's native system ID (field 5), written over SolidCraft's at the same width.
#[test]
fn either_reader_names_the_program_and_its_up_axis() {
    let dir = std::env::temp_dir().join(format!("solidcraft-native-cad-producer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = dir.join("box.step");
    let mut source = Session::default();
    run(&mut source, "solid.box", json!({"length": 10, "width": 40, "height": 5}));
    run(&mut source, "file.export", json!({"path": step.to_string_lossy()}));
    let text = std::fs::read_to_string(&step).unwrap();
    let (named, described) = ("'SolidCraft','SolidCraft'", "FILE_DESCRIPTION((");
    assert!(text.contains(named) && text.contains(described), "{text}");
    let iges = String::from_utf8(two_boxes_iges()).unwrap();
    let native = "10HSolidCraft,19H";
    assert!(iges.contains(native), "{iges}");
    let mut files = Vec::new();
    for (preprocessor, system, description, want) in [
        ("SolidCraft", "SolidCraft", "", ("SolidCraft", Some(("z", "SolidCraft")))),
        ("SwSTEP 2.0", "SolidWorks 2021", "", ("SolidWorks 2021", Some(("y", "SolidWorks")))),
        ("Example CAD", "Example CAD", "SolidWorks export", ("Example CAD", Some(("y", "SolidWorks")))),
        ("Example CAD", "Example CAD", "", ("Example CAD", None)),
    ] {
        let header = text.replacen(named, &format!("'{preprocessor}','{system}'"), 1);
        let header = if description.is_empty() { header } else { header.replacen(described, &format!("{described}'{description}',"), 1) };
        files.push(("step", header, want));
    }
    for (system, want) in [
        ("SolidCraft", ("SolidCraft", Some(("z", "SolidCraft")))),
        ("SolidWorks", ("SolidWorks", Some(("y", "SolidWorks")))),
        ("ExampleCAD", ("ExampleCAD", None)),
    ] {
        files.push(("igs", iges.replacen(native, &format!("10H{system},19H"), 1), want));
    }
    for (k, (ext, contents, (system, up))) in files.into_iter().enumerate() {
        let path = dir.join(format!("part{k}.{ext}"));
        std::fs::write(&path, contents).unwrap();
        for reader in ["solidcraft", "cadmpeg"] {
            for cmd in ["file.insert_step", "doc.open"] {
                let r = run(&mut Session::default(), cmd, json!({"path": path.to_string_lossy(), "reader": reader}));
                let got = r["exporter_up"].as_str().zip(r["exporter"].as_str());
                assert_eq!((r["system"].as_str(), got), (Some(system), up), "part{k}.{ext} {cmd} reader={reader}: {r}");
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// Rhino export through cadmpeg (`file.export` to `.3dm`), read back through the Rhino import
// above. Expected volumes and boxes come from the designs' dimensions.

/// The world box of every shown body, rounded to a micrometre, sorted.
fn shown_boxes(s: &Session) -> Vec<[i64; 6]> {
    let mut out: Vec<[i64; 6]> = s
        .world_state()
        .unwrap()
        .bodies
        .iter()
        .map(|b| {
            let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
            [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 1e3).round() as i64)
        })
        .collect();
    out.sort();
    out
}

fn um(b: [f64; 6]) -> [i64; 6] {
    b.map(|v| (v * 1e3).round() as i64)
}

/// The cylinder's exact volume, π · 6² · 15 = 540π mm³.
const CYLINDER_VOLUME_MM3: f64 = 1_696.460_032_938_488;

/// A loose 10 mm cube, a coloured cylinder (radius 6 mm, 15 mm high, at x = 80 mm), and a
/// `Bracket` component (a 20 × 20 × 10 mm base and a 10 × 10 × 10 mm block on it) placed twice,
/// the second copy 40 mm along Y.
fn parts() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-30, 0, 0], "body_name": "Loose"}));
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 15, "base": [80, 0, 0], "body_name": "Pin"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Pin"], "color": "#0040ff"}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Loose"], "color": "#ff8000"}));
    run(&mut s, "component.create", json!({"name": "Bracket"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 5, 10], "operation": "new", "body_name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.copy", json!({"component": "Bracket", "translate": [0, 40, 0]}));
    s
}

/// The design's solids that Rhino holds exactly: the cube and the bracket's boxes, twice.
fn exact_boxes() -> Vec<[i64; 6]> {
    let mut want = vec![
        um([-30.0, 0.0, 0.0, -20.0, 10.0, 10.0]),
        um([0.0, 0.0, 0.0, 20.0, 20.0, 10.0]),
        um([0.0, 40.0, 0.0, 20.0, 60.0, 10.0]),
        um([5.0, 5.0, 10.0, 15.0, 15.0, 20.0]),
        um([5.0, 45.0, 10.0, 15.0, 55.0, 20.0]),
    ];
    want.sort();
    want
}

/// `file.export` writes a `.3dm` file (any case) from the design: every box where the design shows
/// it, as an exact solid that both Rhino readers bring back with its volume, and the cylinder,
/// which cadmpeg's Rhino writer cannot hold exactly yet, as its mesh with a warning naming it.
/// The components are not kept, and a warning says so.
#[test]
fn file_export_writes_rhino_and_it_reads_back() {
    let (dir, _) = file("rhino-out", "unused", b"");
    let mut s = parts();
    let cylinder = s.world_state().unwrap().bodies.iter().find(|b| b.name.starts_with("Pin")).map(|b| b.name.clone()).unwrap();
    for name in ["parts.3dm", "PARTS.3DM"] {
        let path = dir.join(name).to_string_lossy().to_string();
        let out = run(&mut s, "file.export", json!({"path": path}));
        assert_eq!(out["format"], "Rhino", "{out}");
        let warnings = out["warnings"].to_string();
        assert!(warnings.contains(&format!("{cylinder}: written as its mesh only")), "{out}");
        assert!(warnings.contains("components and placements are not written"), "{out}");
        for reader in ["solidcraft", "cadmpeg"] {
            let mut back = Session::default();
            run(&mut back, "file.insert_step", json!({"path": path, "reader": reader}));
            // SolidCraft's own Rhino reader leaves the cylinder's mesh out; cadmpeg's brings it in
            // as a mesh body spanning the cylinder (radius 6 mm, 15 mm high, at x = 80 mm).
            let mut want = exact_boxes();
            let mut mesh_volume = 0.0;
            if reader == "cadmpeg" {
                let world = back.world_state().unwrap();
                let meshes: Vec<_> = world.bodies.iter().filter(|b| b.body.is_mesh()).collect();
                let [mesh] = meshes.as_slice() else { panic!("{name}: one mesh body, got {}", meshes.len()) };
                let m = solidcraft_kernel::measure(&mesh.body).unwrap();
                let bb = [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z];
                for (got, want) in bb.iter().zip([74.0, -6.0, 0.0, 86.0, 6.0, 15.0]) {
                    assert!((got - want).abs() < 0.01, "{name}: {bb:?}");
                }
                want.push(bb.map(|v| (v * 1e3).round() as i64));
                want.sort();
                // The cylinder's volume is π r² h = 540π mm³. Its mesh is a prism on a
                // polygon inscribed in the circle, at a chord tolerance of at most 2e-4 of the
                // body's size (its 22.6 mm diagonal: a sagitta s ≤ 0.0046 mm), so short of it by
                // less than (4/3) s / r ≈ 0.1%; 1% allows that, and the file's single-precision
                // points (≈1e-6 relative) cannot take it past π r² h by 1e-4.
                let exact = CYLINDER_VOLUME_MM3;
                assert!(
                    m.volume > 0.99 * exact && m.volume < exact * (1.0 + 1e-4),
                    "{name}: the cylinder's mesh holds {} mm³, not {exact}",
                    m.volume
                );
                mesh_volume = m.volume;
            }
            assert_eq!(shown_boxes(&back), want, "{name} {reader}");
            let v = volume(&mut back);
            assert!((v - (1000.0 + 2.0 * (4000.0 + 1000.0)) - mesh_volume).abs() < 1e-6, "{name} {reader}: {v}");
        }
    }
    // Chosen bodies only.
    let path = dir.join("loose.3dm").to_string_lossy().to_string();
    let out = run(&mut s, "file.export", json!({"path": path, "bodies": ["Loose"]}));
    assert!(out.get("warnings").is_none_or(|w| w.as_array().is_some_and(Vec::is_empty)), "{out}");
    let mut back = Session::default();
    run(&mut back, "file.insert_step", json!({"path": path}));
    assert_eq!(shown_boxes(&back), vec![um([-30.0, 0.0, 0.0, -20.0, 10.0, 10.0])]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `format: 3dm` names Rhino; `format: 3dm` under another format's extension is refused, while
/// another format named under a Rhino extension is written as asked; FreeCAD is refused with
/// why, leaving no file behind.
#[test]
fn rhino_and_freecad_export_check_their_format() {
    let (dir, _) = file("rhino-format", "unused", b"");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    let path = dir.join("block.3dm").to_string_lossy().to_string();
    assert_eq!(run(&mut s, "file.export", json!({"path": path, "format": "3dm"}))["format"], "Rhino");
    let step = dir.join("block.step").to_string_lossy().to_string();
    let e = s.execute("file.export", &json!({"path": step, "format": "3dm"})).unwrap_err().to_string();
    assert!(e.contains("disagrees with the path's extension"), "{e}");
    assert!(!std::path::Path::new(&step).exists());
    // The other way round, a format SolidCraft writes itself, named, is written whatever the
    // extension, as an explicit format always was: STEP under a Rhino name.
    let rhino = dir.join("part.3dm").to_string_lossy().to_string();
    assert_eq!(run(&mut s, "file.export", json!({"path": rhino, "format": "step"}))["format"], "Step");
    assert!(std::fs::read(&rhino).unwrap().starts_with(b"ISO-10303-21;"));
    let fcstd = dir.join("block.FCStd").to_string_lossy().to_string();
    let e = s.execute("file.export", &json!({"path": fcstd})).unwrap_err().to_string();
    assert!(e.contains("FreeCAD files cannot be exported yet"), "{e}");
    assert!(!std::path::Path::new(&fcstd).exists());
    // A file already there is left as it was by a refused export.
    std::fs::write(&fcstd, b"kept").unwrap();
    assert!(s.execute("file.export", &json!({"path": fcstd})).is_err());
    assert_eq!(std::fs::read(&fcstd).unwrap(), b"kept");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A body's colour: its own, else the one colour every face of it has.
fn colour_of(b: &solidcraft_kernel::Body) -> Option<[f32; 3]> {
    b.color().or_else(|| {
        let faces = &b.paint()?.faces;
        let first = faces.first()?.color;
        (faces.len() == b.face_count() && faces.iter().all(|f| f.color == first)).then_some(first)
    })
}

/// [`exact_boxes`] and the cylinder: radius 6 mm, 15 mm high at x = 80 mm.
fn all_boxes() -> Vec<[i64; 6]> {
    let mut want = exact_boxes();
    want.push(um([74.0, -6.0, 0.0, 86.0, 6.0, 15.0]));
    want.sort();
    want
}

/// `file.export` writes STEP and IGES with SolidCraft's own writers unless `writer: cadmpeg`
/// asks for cadmpeg's. Each file, by either writer, reads back through SolidCraft's reader (and a
/// STEP file through cadmpeg's too) with every body where the design shows it, the volume the
/// design's dimensions give and the cylinder in its colour. The default writes the same file as
/// `writer: solidcraft`, which is not cadmpeg's.
#[test]
fn file_export_writes_step_and_iges_with_either_writer() {
    let (dir, _) = file("step-writer", "unused", b"");
    let mut s = parts();
    let volume_want = 1000.0 + 2.0 * (4000.0 + 1000.0) + CYLINDER_VOLUME_MM3;
    let blue = [0.0, 64.0 / 255.0, 1.0];
    for ext in ["step", "igs"] {
        let mut texts = Vec::new();
        for writer in [None, Some("solidcraft"), Some(" CadMpeg ")] {
            let path = dir.join(format!("parts-{}.{ext}", writer.map_or("default", str::trim))).to_string_lossy().to_string();
            let mut p = json!({"path": path});
            if let Some(w) = writer {
                p["writer"] = json!(w);
            }
            let out = run(&mut s, "file.export", p);
            assert_eq!(out["format"], if ext == "step" { "Step" } else { "Iges" }, "{out}");
            let text = std::fs::read_to_string(&path).unwrap();
            // cadmpeg's writers name themselves in the file's header; SolidCraft's do not.
            assert_eq!(text.contains("cadmpeg"), writer.is_some_and(|w| w.trim().eq_ignore_ascii_case("cadmpeg")), "{ext} {writer:?}");
            texts.push(text);
            let readers: &[&str] = if ext == "step" { &["solidcraft", "cadmpeg"] } else { &["solidcraft"] };
            for reader in readers {
                let mut back = Session::default();
                run(&mut back, "file.insert_step", json!({"path": path, "reader": reader}));
                assert_eq!(shown_boxes(&back), all_boxes(), "{ext} {writer:?} {reader}");
                assert!((volume(&mut back) - volume_want).abs() < 1e-3, "{ext} {writer:?} {reader}");
                let world = back.world_state().unwrap();
                // Names need not survive every writer: the cylinder is the body with its box.
                let cylinder = world
                    .bodies
                    .iter()
                    .find(|b| {
                        let bb = solidcraft_kernel::measure(&b.body).unwrap().bbox;
                        [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(|v| (v * 1e3).round() as i64)
                            == um([74.0, -6.0, 0.0, 86.0, 6.0, 15.0])
                    })
                    .unwrap_or_else(|| panic!("{ext} {writer:?} {reader}: no cylinder body"));
                let colour = colour_of(&cylinder.body).unwrap_or_else(|| panic!("{ext} {writer:?} {reader}: the cylinder is not coloured"));
                for (got, want) in colour.iter().zip(blue) {
                    assert!((f64::from(*got) - want).abs() < 1e-3, "{ext} {writer:?} {reader}: the cylinder's colour: {colour:?}");
                }
                assert_eq!(
                    world
                        .bodies
                        .iter()
                        .filter(|b| colour_of(&b.body).is_some_and(|c| c.iter().zip(blue).all(|(g, w)| (f64::from(*g) - w).abs() < 1e-3)))
                        .count(),
                    1,
                    "{ext} {writer:?} {reader}: only the cylinder is blue"
                );
            }
        }
        // The default and `solidcraft` write the same file but for the header's time stamp.
        let body = |t: &str| t.split_once("DATA;").or_else(|| t.split_once("S      1")).map(|(_, b)| b.to_string()).unwrap_or_default();
        assert_eq!(body(&texts[0]).lines().count(), body(&texts[1]).lines().count(), "{ext}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `writer` is for STEP and IGES only, and takes solidcraft or cadmpeg; anything else is refused
/// before a file is written.
#[test]
fn the_writer_option_is_refused_where_it_does_not_apply() {
    let (dir, _) = file("step-writer-refused", "unused", b"");
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    for (name, writer) in
        [("a.stl", "cadmpeg"), ("a.3mf", "solidcraft"), ("a.usda", "cadmpeg"), ("a.3dm", "cadmpeg"), ("a.step", "other"), ("a.igs", "")]
    {
        let path = dir.join(name).to_string_lossy().to_string();
        let e = s.execute("file.export", &json!({"path": path, "writer": writer})).expect_err(name).to_string();
        assert!(e.contains("`writer`"), "{name}: {e}");
        assert!(!std::path::Path::new(&path).exists(), "{name}");
    }
    let path = dir.join("a.step").to_string_lossy().to_string();
    let e = s.execute("file.export", &json!({"path": path, "writer": 1})).expect_err("a number").to_string();
    assert!(e.contains("`writer`"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
