use std::io::{Cursor, Write};

use super::*;

const DOCUMENT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Document SchemaVersion="4">
  <Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="Fixture Project"/></Property></Properties>
  <Objects Count="4">
    <Object type="App::Part" name="Part" id="1"/>
    <Object type="PartDesign::Body" name="Body" id="2"/>
    <Object type="Sketcher::SketchObject" name="Profile" id="3"/>
    <Object type="Sketcher::SketchObject" name="Loose" id="4"/>
  </Objects>
  <ObjectData Count="4">
    <Object name="Part"><Properties Count="2">
      <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Body"/></LinkList></Property>
      <Property name="Label" type="App::PropertyString"><String value="Bracket Part"/></Property>
    </Properties></Object>
    <Object name="Body"><Properties Count="2">
      <Property name="Group" type="App::PropertyLinkList"><LinkList count="1"><Link value="Profile"/></LinkList></Property>
      <Property name="Label" type="App::PropertyString"><String value="Main Body"/></Property>
    </Properties></Object>
    <Object name="Profile"><Properties Count="3">
      <Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="5">
        <Constrain Type="1" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="2" Second="1" SecondPos="1" Third="-2000"/>
        <Constrain Type="2" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="6" Value="40" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="11" Value="3" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="2" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="999" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
      </ConstraintList></Property>
      <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="3">
        <Geometry type="Part::GeomLineSegment" id="1"><LineSegment StartX="0" StartY="0" EndX="40" EndY="0"/><Construction value="0"/></Geometry>
        <Geometry type="Part::GeomLineSegment" id="2"><LineSegment StartX="40" StartY="0" EndX="40" EndY="20"/><Construction value="1"/></Geometry>
        <Geometry type="Part::GeomCircle" id="3"><Circle CenterX="10" CenterY="10" Radius="3"/><Construction value="0"/></Geometry>
      </GeometryList></Property>
      <Property name="Label" type="App::PropertyString"><String value="Mounting Profile"/></Property>
    </Properties></Object>
    <Object name="Loose"><Properties Count="2">
      <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="1">
        <Geometry type="Part::GeomArcOfCircle" id="1"><ArcOfCircle CenterX="5" CenterY="5" Radius="2" StartAngle="0" EndAngle="1.5707963267948966" AngleXU="0"/><Construction value="0"/></Geometry>
      </GeometryList></Property>
      <Property name="Label" type="App::PropertyString"><String value="Loose Sketch"/></Property>
    </Properties></Object>
  </ObjectData>
</Document>"#;

fn archive(xml: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Document.xml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(xml).unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn hierarchy_geometry_and_constraints_are_imported() {
    let project = read_freecad_sketches(&archive(DOCUMENT.as_bytes()), "fixture.FCStd").unwrap();
    assert_eq!(project.label, "Fixture Project");
    assert_eq!(project.nodes.len(), 2, "the part and the ungrouped sketch are roots");
    let part = &project.nodes[0];
    assert_eq!((part.label.as_str(), part.kind), ("Bracket Part", FreeCadNodeKind::Part));
    assert_eq!(part.children[0].label, "Main Body");
    let profile_node = &part.children[0].children[0];
    assert_eq!((profile_node.label.as_str(), profile_node.kind), ("Mounting Profile", FreeCadNodeKind::Sketch));
    let info = profile_node.sketch.as_ref().unwrap();
    assert_eq!((info.geometry, info.constraints), (3, 4));
    assert!(info.warnings.iter().any(|w| w.contains("type 999")));

    let profile = project.sketches.iter().find(|s| s.source_name == "Profile").unwrap();
    assert_eq!(profile.path, ["Bracket Part", "Main Body", "Mounting Profile"]);
    assert_eq!(profile.sketch.curves.len(), 3);
    assert!(profile.sketch.curves[1].construction);
    assert!(matches!(profile.sketch.constraints[0].kind, ConstraintKind::Coincident { .. }));
    assert!(matches!(profile.sketch.constraints[2].kind, ConstraintKind::Length { value: 40.0, .. }));
    assert!(matches!(profile.sketch.constraints[3].kind, ConstraintKind::Radius { value: 3.0, .. }));

    let loose = project.sketches.iter().find(|s| s.source_name == "Loose").unwrap();
    assert!(matches!(loose.sketch.curves[0].kind, CurveKind::Arc { .. }));
}

#[test]
fn malformed_and_missing_documents_are_errors() {
    assert!(read_freecad_sketches(b"not a zip", "bad.FCStd").is_err());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("Other.xml", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"<x/>").unwrap();
    let no_document = zip.finish().unwrap().into_inner();
    assert!(read_freecad_sketches(&no_document, "missing.FCStd").is_err());
    assert!(read_freecad_sketches(&archive(b"<Document><broken>"), "broken.FCStd").is_err());
}

#[test]
fn unsupported_geometry_is_a_warning_not_a_failed_file() {
    let xml = DOCUMENT.replace(
        "Part::GeomArcOfCircle\" id=\"1\"><ArcOfCircle CenterX=\"5\" CenterY=\"5\" Radius=\"2\" StartAngle=\"0\" EndAngle=\"1.5707963267948966\" AngleXU=\"0\"/>",
        "Part::GeomBSplineCurve\" id=\"1\"><BSplineCurve/>",
    );
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "unsupported.FCStd").unwrap();
    let loose = project.sketches.iter().find(|s| s.source_name == "Loose").unwrap();
    assert!(loose.sketch.curves.is_empty());
    assert!(loose.warnings.iter().any(|w| w.contains("GeomBSplineCurve")));
}
