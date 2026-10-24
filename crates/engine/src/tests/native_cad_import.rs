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
    let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Origin { name }, sketch } = &feature.kind else {
        panic!("import did not make a sketch")
    };
    assert_eq!(name, "XY");
    assert_eq!((sketch.curves.len(), sketch.constraints.len()), (2, 3));
    assert_eq!(s.undo.len(), 1, "all selected sketches are one command and one undo step");
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
