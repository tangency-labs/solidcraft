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
      <Property name="Constraints" type="Sketcher::PropertyConstraintList"><ConstraintList count="7">
        <Constrain Type="1" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="2" Second="1" SecondPos="1" Third="-2000"/>
        <Constrain Type="2" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="6" Value="40" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="11" Value="3" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="2" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
        <Constrain Type="12" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="2" FirstPos="0" Second="3" SecondPos="0" Third="-2000"/>
        <Constrain Type="2" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="2" FirstPos="3" Second="3" SecondPos="3" Third="-2000"/>
        <Constrain Type="999" Value="0" IsDriving="1" IsActive="1" IsInVirtualSpace="0" First="0" FirstPos="0" Second="-2000" SecondPos="0" Third="-2000"/>
      </ConstraintList></Property>
      <Property name="Geometry" type="Part::PropertyGeometryList"><GeometryList count="4">
        <Geometry type="Part::GeomLineSegment" id="1"><LineSegment StartX="0" StartY="0" EndX="40" EndY="0"/><Construction value="0"/></Geometry>
        <Geometry type="Part::GeomLineSegment" id="2"><LineSegment StartX="40" StartY="0" EndX="40" EndY="20"/><Construction value="1"/></Geometry>
        <Geometry type="Part::GeomCircle" id="3"><Circle CenterX="10" CenterY="10" Radius="3"/><Construction value="0"/></Geometry>
        <Geometry type="Part::GeomCircle" id="4"><Circle CenterX="30" CenterY="10" Radius="3"/><Construction value="0"/></Geometry>
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
fn circle_centers_on_sketch_axes_retain_geometric_constraints() {
    for (axis, dimension_type, x, y) in [(-2, 8, 0, 12), (-1, 7, 12, 0)] {
        let xml = format!(
            r#"<Document><Objects><Object name="BoltHoles" type="Sketcher::SketchObject"/></Objects>
        <ObjectData><Object name="BoltHoles"><Properties>
          <Property name="Geometry"><GeometryList><Geometry type="Part::GeomCircle">
            <Circle CenterX="{x}" CenterY="{y}" Radius="1.875"/>
          </Geometry></GeometryList></Property>
          <Property name="Constraints"><ConstraintList>
            <Constrain Type="13" First="0" FirstPos="3" Second="{axis}" SecondPos="0"/>
            <Constrain Type="{dimension_type}" Value="12" First="-1" FirstPos="1" Second="0" SecondPos="3"/>
            <Constrain Type="18" Value="3.75" First="0"/>
          </ConstraintList></Property>
        </Properties></Object></ObjectData></Document>"#
        );
        let project = read_freecad_sketches(&archive(xml.as_bytes()), "bolt-holes.FCStd").unwrap();
        let source = &project.sketches[0];
        assert!(source.warnings.is_empty(), "the built-in sketch axis is available: {:?}", source.warnings);
        let mut sketch = source.sketch.clone();
        let CurveKind::Circle { c: center, .. } = sketch.curves[0].kind else { panic!("expected a circle") };
        if axis == -2 {
            assert!(matches!(sketch.constraints[0].kind, ConstraintKind::VerticalPoints { p: 0, q } if q == center));
        } else {
            assert!(matches!(sketch.constraints[0].kind, ConstraintKind::HorizontalPoints { p: 0, q } if q == center));
        }
        assert!(!sketch.constraints[0].kind.is_dimension(), "point-on-axis must not become an editable zero dimension");
        assert_eq!(sketch.curves.len(), 1, "axis alignment needs no helper geometry");
        sketch.points[center].pos = Vec2::new(7.0, 18.0);
        let solved = solidcraft_sketch::solve(&mut sketch);
        assert!(solved.ok(), "{solved:?}");
        assert!((sketch.point(center).unwrap() - Vec2::new(f64::from(x), f64::from(y))).len() < 1e-7);
        assert!((sketch.radius(0).unwrap() - 1.875).abs() < 1e-7);

        if axis == -2 {
            let projected = xml.replace("Second=\"-2\"", "Second=\"-3\"");
            let project = read_freecad_sketches(&archive(projected.as_bytes()), "external.FCStd").unwrap();
            assert!(project.sketches[0].warnings.iter().any(|warning| warning.detail.contains("external geometry reference -3")));
        }
    }
}

#[test]
fn symmetry_about_freecad_sketch_axes_keeps_circle_centers_and_line_endpoints() {
    let xml = r#"<Document><Objects><Object name="Symmetric" type="Sketcher::SketchObject"/></Objects>
      <ObjectData><Object name="Symmetric"><Properties>
        <Property name="Geometry"><GeometryList>
          <Geometry type="Part::GeomCircle"><Circle CenterX="10" CenterY="10" Radius="1.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="-10" CenterY="10" Radius="1.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="10" CenterY="-10" Radius="1.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="-10" CenterY="-10" Radius="1.5"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="10" StartY="0" EndX="10" EndY="5"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="-10" StartY="5" EndX="-10" EndY="0"/></Geometry>
        </GeometryList></Property>
        <Property name="Constraints"><ConstraintList>
          <Constrain Type="14" First="1" FirstPos="3" Second="0" SecondPos="3" Third="-2" ThirdPos="0"/>
          <Constrain Type="14" First="3" FirstPos="3" Second="2" SecondPos="3" Third="-2" ThirdPos="0"/>
          <Constrain Type="14" First="1" FirstPos="3" Second="3" SecondPos="3" Third="-1" ThirdPos="0"/>
          <Constrain Type="14" First="4" FirstPos="2" Second="5" SecondPos="1" Third="-2" ThirdPos="0"/>
        </ConstraintList></Property>
      </Properties></Object></ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "symmetry.FCStd").unwrap();
    let source = &project.sketches[0];
    assert!(source.warnings.is_empty(), "sketch axes are available symmetry references: {:?}", source.warnings);
    assert_eq!(source.source_curves.len(), 6, "linked axis helpers do not become FreeCAD source geometry");
    let (Some(x), Some(y)) = (source.sketch.sketch_axis_curve("x"), source.sketch.sketch_axis_curve("y")) else {
        panic!("both linked sketch axes were created")
    };
    let axes = source
        .sketch
        .constraints
        .iter()
        .filter_map(|constraint| match &constraint.kind {
            ConstraintKind::Symmetric { l, .. } => Some(*l),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(axes, [y, y, x, y]);
    assert!(source.sketch.curves[x].construction && source.sketch.curves[y].construction);
}

#[test]
fn symmetry_about_the_sketch_origin_keeps_circle_centers_opposite() {
    let xml = r#"<Document><Objects><Object name="Sketch" type="Sketcher::SketchObject"/></Objects>
      <ObjectData><Object name="Sketch"><Properties>
        <Property name="Geometry"><GeometryList>
          <Geometry type="Part::GeomCircle"><Circle CenterX="57" CenterY="0" Radius="9.25"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="-57" CenterY="0" Radius="9.25"/></Geometry>
        </GeometryList></Property>
        <Property name="Constraints"><ConstraintList>
          <Constrain Type="13" First="0" FirstPos="3" Second="-1" SecondPos="0"/>
          <Constrain Type="18" Value="18.5" First="0"/>
          <Constrain Type="18" Value="18.5" First="1"/>
          <Constrain Type="14" First="1" FirstPos="3" Second="0" SecondPos="3" Third="-1" ThirdPos="1"/>
          <Constrain Type="7" Value="114" First="1" FirstPos="3" Second="0" SecondPos="3"/>
        </ConstraintList></Property>
      </Properties></Object></ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "origin-symmetry.FCStd").unwrap();
    let source = &project.sketches[0];
    assert!(source.warnings.is_empty(), "the sketch origin is an available symmetry center: {:?}", source.warnings);
    assert_eq!(source.source_curves.len(), 2, "the construction line is not FreeCAD source geometry");
    let CurveKind::Circle { c: right, .. } = source.sketch.curves[0].kind else { panic!("expected a circle") };
    let CurveKind::Circle { c: left, .. } = source.sketch.curves[1].kind else { panic!("expected a circle") };
    let midpoint = source
        .sketch
        .constraints
        .iter()
        .find_map(|constraint| match &constraint.kind {
            ConstraintKind::Midpoint { p: 0, l } => Some(*l),
            _ => None,
        })
        .expect("origin-centered midpoint constraint");
    assert!(source.sketch.curves[midpoint].construction);
    assert!(matches!(source.sketch.curves[midpoint].kind, CurveKind::Line { a, b } if a == left && b == right));

    let mut sketch = source.sketch.clone();
    sketch.points[right].pos = Vec2::new(65.0, 2.0);
    sketch.points[left].pos = Vec2::new(-53.0, -3.0);
    let solved = solidcraft_sketch::solve(&mut sketch);
    assert!(solved.ok(), "{solved:?}");
    assert!((sketch.point(right).unwrap() - Vec2::new(57.0, 0.0)).len() < 1e-6);
    assert!((sketch.point(left).unwrap() - Vec2::new(-57.0, 0.0)).len() < 1e-6);
}

#[test]
fn symmetry_about_another_circles_center_keeps_both_centers_opposite() {
    let xml = r#"<Document><Objects><Object name="Sketch013" type="Sketcher::SketchObject"/></Objects>
      <ObjectData><Object name="Sketch013"><Properties>
        <Property name="Geometry"><GeometryList>
          <Geometry type="Part::GeomCircle"><Circle CenterX="0" CenterY="0" Radius="7.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="5" CenterY="5" Radius="2.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="-5" CenterY="-5" Radius="2.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="15.5563491861040468" CenterY="-15.5563491861040468" Radius="2.5"/></Geometry>
          <Geometry type="Part::GeomCircle"><Circle CenterX="-15.5563491861040468" CenterY="15.5563491861040468" Radius="2.5"/></Geometry>
        </GeometryList></Property>
        <Property name="Constraints"><ConstraintList>
          <Constrain Type="1" First="0" FirstPos="3" Second="-1" SecondPos="1"/>
          <Constrain Type="14" First="3" FirstPos="3" Second="4" SecondPos="3" Third="0" ThirdPos="3"/>
        </ConstraintList></Property>
      </Properties></Object></ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "circle-center-symmetry.FCStd").unwrap();
    let source = &project.sketches[0];
    assert!(source.warnings.is_empty(), "a circle center is a valid symmetry point: {:?}", source.warnings);
    assert_eq!(source.source_curves.len(), 5);
    assert_eq!(source.sketch.curves.len(), 6, "the symmetry helper is a construction line");
    let CurveKind::Circle { c: center, .. } = source.sketch.curves[0].kind else { panic!("expected center circle") };
    let CurveKind::Circle { c: first, .. } = source.sketch.curves[3].kind else { panic!("expected first circle") };
    let CurveKind::Circle { c: second, .. } = source.sketch.curves[4].kind else { panic!("expected second circle") };
    let ConstraintKind::Midpoint { p, l } = source.sketch.constraints[1].kind else { panic!("expected midpoint symmetry") };
    assert_eq!(p, center);
    assert!(source.sketch.curves[l].construction);
    assert!(matches!(source.sketch.curves[l].kind, CurveKind::Line { a, b } if a == first && b == second));
}

#[test]
fn basic_operations_retain_profiles_dependencies_dimensions_and_axes() {
    let xml = include_str!("operations_fixture.xml");
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "operations.FCStd").unwrap();
    assert_eq!(project.operations.len(), 4);
    let pocket = project.operations.iter().find(|op| op.source_name == "Pocket").unwrap();
    assert_eq!(pocket.base.as_deref(), Some("Pad"));
    assert_eq!(pocket.profile, "PocketProfile");
    assert!(pocket.blockers.is_empty());
    assert!(matches!(pocket.settings, Some(FreeCadOperationSettings::Extrude { length: 4.0, cut: true, .. })));
    let revolve = project.operations.iter().find(|op| op.source_name == "Revolution").unwrap();
    assert!(matches!(revolve.settings, Some(FreeCadOperationSettings::Revolve { angle: 270.0, axis: FreeCadAxis::SketchY, .. })));
    let profile = project.sketches.iter().find(|sketch| sketch.source_name == "PocketProfile").unwrap();
    assert!((profile.plane.origin.z - 10.0).abs() < 1e-9);

    let unsupported = xml.replace(
        "<Property name=\"Length\"><Float value=\"4\"/></Property>",
        "<Property name=\"Length\"><Float value=\"4\"/></Property><Property name=\"SideType\"><Integer value=\"99\"/></Property>",
    );
    let project = read_freecad_sketches(&archive(unsupported.as_bytes()), "unsupported.FCStd").unwrap();
    let pocket = project.operations.iter().find(|op| op.source_name == "Pocket").unwrap();
    assert!(!pocket.blockers.is_empty());
}

#[test]
fn mirror_and_circular_pattern_retain_their_saved_sources_and_axes() {
    let xml = r#"<Document><Objects>
      <Object name="Body" type="PartDesign::Body"/>
      <Object name="Sketch" type="Sketcher::SketchObject"/>
      <Object name="YZ_Plane" type="App::Plane"/>
      <Object name="Pad" type="PartDesign::Pad"/>
      <Object name="Mirrored" type="PartDesign::Mirrored"/>
      <Object name="Mirrored001" type="PartDesign::Mirrored"/>
      <Object name="PolarPattern" type="PartDesign::PolarPattern"/>
    </Objects><ObjectData>
      <Object name="Body"><Properties><Property name="AllowCompound"><Bool value="true"/></Property><Property name="Group"><LinkList>
        <Link value="Sketch"/><Link value="Pad"/><Link value="Mirrored"/><Link value="Mirrored001"/><Link value="PolarPattern"/>
      </LinkList></Property></Properties></Object>
      <Object name="Sketch"><Properties><Property name="Geometry"><GeometryList>
        <Geometry type="Part::GeomCircle"><Circle CenterX="10" CenterY="0" Radius="2"/></Geometry>
      </GeometryList></Property>
      <Property name="Placement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
      </Properties></Object>
      <Object name="YZ_Plane"><Properties><Property name="Placement">
        <PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.5" Q1="0.5" Q2="0.5" Q3="0.5"/>
      </Property></Properties></Object>
      <Object name="Pad"><Properties>
        <Property name="Profile"><LinkSub value="Sketch"/></Property>
        <Property name="Length"><Float value="5"/></Property>
      </Properties></Object>
      <Object name="Mirrored"><Properties>
        <Property name="BaseFeature"><Link value="Pad"/></Property>
        <Property name="Originals"><LinkList><Link value="Pad"/></LinkList></Property>
        <Property name="MirrorPlane"><LinkSub value="YZ_Plane"/></Property>
        <Property name="TransformMode"><Integer value="1"/></Property>
      </Properties></Object>
      <Object name="Mirrored001"><Properties>
        <Property name="BaseFeature"><Link value="Pad"/></Property>
        <Property name="Originals"><LinkList><Link value="Pad"/></LinkList></Property>
        <Property name="MirrorPlane"><LinkSub value="Sketch"><Sub value="V_Axis"/></LinkSub></Property>
        <Property name="TransformMode"><Integer value="0"/></Property>
      </Properties></Object>
      <Object name="PolarPattern"><Properties>
        <Property name="BaseFeature"><Link value="Mirrored"/></Property>
        <Property name="Originals"><LinkList><Link value="Mirrored"/></LinkList></Property>
        <Property name="Axis"><LinkSub value="Sketch"><Sub value="N_Axis"/></LinkSub></Property>
        <Property name="Occurrences"><Integer value="4"/></Property>
        <Property name="Angle"><Float value="360"/></Property>
        <Property name="Mode"><Integer value="0"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "transforms.FCStd").unwrap();
    assert_eq!(project.operations.len(), 4);
    let mirror = project.operations.iter().find(|operation| operation.source_name == "Mirrored").unwrap();
    assert!(mirror.blockers.is_empty(), "saved mirror plane is available: {:?}", mirror.blockers);
    assert!(mirror.body_allows_compound);
    assert!(matches!(mirror.settings, Some(FreeCadOperationSettings::Mirror { ref original, whole_body: true, .. }) if original == "Pad"));
    let sketch_axis_mirror = project.operations.iter().find(|operation| operation.source_name == "Mirrored001").unwrap();
    assert!(sketch_axis_mirror.blockers.is_empty());
    assert!(
        matches!(sketch_axis_mirror.settings, Some(FreeCadOperationSettings::Mirror { plane, whole_body: false, .. }) if plane.normal().dot(Vec3::X) > 1.0 - 1e-9)
    );
    let pattern = project.operations.iter().find(|operation| operation.source_name == "PolarPattern").unwrap();
    assert!(pattern.blockers.is_empty(), "saved pattern axis is available: {:?}", pattern.blockers);
    assert!(
        matches!(pattern.settings, Some(FreeCadOperationSettings::PolarPattern { count: 4, angle, ref original, .. }) if angle == 360.0 && original == "Mirrored")
    );
}

#[test]
fn negative_one_sided_extrusion_lengths_reverse_the_saved_direction() {
    let xml = include_str!("operations_fixture.xml")
        .replace("<Property name=\"Length\"><Float value=\"10\"/></Property>", "<Property name=\"Length\"><Float value=\"-10\"/></Property>")
        .replace("<Property name=\"Length\"><Float value=\"4\"/></Property>", "<Property name=\"Length\"><Float value=\"-4\"/></Property>");
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "negative-extrusion.FCStd").unwrap();
    let pad = project.operations.iter().find(|op| op.source_name == "Pad").unwrap();
    assert!(pad.blockers.is_empty(), "the negative pad length is a valid saved FreeCAD extent");
    assert!(matches!(pad.settings, Some(FreeCadOperationSettings::Extrude { length: 10.0, reversed: true, cut: false, .. })));
    let pocket = project.operations.iter().find(|op| op.source_name == "Pocket").unwrap();
    assert!(pocket.blockers.is_empty(), "the negative pocket length is a valid saved FreeCAD extent");
    assert!(matches!(pocket.settings, Some(FreeCadOperationSettings::Extrude { length: 4.0, reversed: true, cut: true, .. })));
}

#[test]
fn skipped_fillets_report_the_saved_radius_and_source_selection() {
    let xml = r#"<Document><Objects>
      <Object name="Pad" type="PartDesign::Pad"/>
      <Object name="FilletEdges" type="PartDesign::Fillet"/>
      <Object name="FilletFaces" type="PartDesign::Fillet"/>
      <Object name="FilletAll" type="PartDesign::Fillet"/>
    </Objects><ObjectData>
      <Object name="Pad"><Properties><Property name="Label"><String value="Support Pad"/></Property></Properties></Object>
      <Object name="FilletEdges"><Properties>
        <Property name="Radius"><Float value="2"/></Property>
        <Property name="Base"><LinkSub value="Pad" count="3"><Sub value="Edge20"/><Sub value="Edge25"/><Sub value="Edge12"/></LinkSub></Property>
      </Properties></Object>
      <Object name="FilletFaces"><Properties>
        <Property name="Radius"><Float value="1.5"/></Property>
        <Property name="Base"><LinkSub value="Pad" count="2"><Sub value="Face2"/><Sub value="Face12"/></LinkSub></Property>
      </Properties></Object>
      <Object name="FilletAll"><Properties>
        <Property name="Radius"><Float value="3"/></Property>
        <Property name="Base"><LinkSub value="Pad" count="0"/></Property>
        <Property name="UseAllEdges"><Bool value="true"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "fillet-selection.FCStd").unwrap();
    let detail = |name: &str| project.operations.iter().find(|op| op.source_name == name).unwrap().blockers[0].detail.clone();
    let edges = detail("FilletEdges");
    assert!(edges.contains("Saved fillet radius: 2 mm") && edges.contains("`Support Pad` (`Pad`)"));
    assert!(edges.contains("Selected FreeCAD edges: `Edge20`, `Edge25`, `Edge12`"));
    let faces = detail("FilletFaces");
    assert!(faces.contains("Saved fillet radius: 1.5 mm") && faces.contains("Selected FreeCAD faces: `Face2`, `Face12`"));
    assert!(faces.contains("does not provide a separate edge list"));
    let all = detail("FilletAll");
    assert!(all.contains("Saved fillet radius: 3 mm") && all.contains("all edges of that base operation"));
}

#[test]
fn skipped_polar_patterns_report_occurrences_angle_axis_and_sources() {
    let xml = r#"<Document><Objects>
      <Object name="Sketch" type="Sketcher::SketchObject"/>
      <Object name="Pad" type="PartDesign::Pad"/>
      <Object name="Pocket" type="PartDesign::Pocket"/>
      <Object name="PatternExtent" type="PartDesign::PolarPattern"/>
      <Object name="PatternSpacing" type="PartDesign::PolarPattern"/>
    </Objects><ObjectData>
      <Object name="Sketch"><Properties><Property name="Label"><String value="Hole layout"/></Property></Properties></Object>
      <Object name="Pad"><Properties><Property name="Label"><String value="Boss"/></Property></Properties></Object>
      <Object name="Pocket"><Properties><Property name="Label"><String value="Bolt holes"/></Property></Properties></Object>
      <Object name="PatternExtent"><Properties>
        <Property name="Occurrences"><Integer value="10"/></Property>
        <Property name="Angle"><Float value="360"/></Property>
        <Property name="Mode"><Integer value="0"/></Property>
        <Property name="Axis"><LinkSub value="Sketch"><Sub value="N_Axis"/></LinkSub></Property>
        <Property name="Originals"><LinkList><Link value="Pad"/><Link value="Pocket"/></LinkList></Property>
        <Property name="Reversed"><Bool value="false"/></Property>
        <Property name="TransformMode"><Integer value="0"/></Property>
      </Properties></Object>
      <Object name="PatternSpacing"><Properties>
        <Property name="Occurrences"><Integer value="5"/></Property>
        <Property name="Angle"><Float value="360"/></Property>
        <Property name="Offset"><Float value="45"/></Property>
        <Property name="Mode"><Integer value="1"/></Property>
        <Property name="Axis"><LinkSub value="Sketch"><Sub value="V_Axis"/></LinkSub></Property>
        <Property name="Originals"><LinkList><Link value="Pocket"/></LinkList></Property>
        <Property name="Reversed"><Bool value="true"/></Property>
        <Property name="TransformMode"><Integer value="1"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "polar-patterns.FCStd").unwrap();
    let detail = |name: &str| project.operations.iter().find(|op| op.source_name == name).unwrap().blockers[0].detail.clone();
    let extent = detail("PatternExtent");
    assert!(extent.contains("Saved occurrence count: 10, including the original feature"));
    assert!(extent.contains("Total pattern angle: 360° (overall angle mode)"));
    assert!(extent.contains("Nominal angular step: 36°"));
    assert!(extent.contains("normal (Z) axis of `Hole layout` (`Sketch`)"));
    assert!(extent.contains("`Boss` (`Pad`), `Bolt holes` (`Pocket`)"));
    assert!(extent.contains("Transform mode: selected features"));
    let spacing = detail("PatternSpacing");
    assert!(spacing.contains("Saved occurrence count: 5"));
    assert!(spacing.contains("Saved offset angle: 45°"));
    assert!(!spacing.contains("Total pattern angle: 360°"), "the inactive saved Angle must not be called the actual span");
    assert!(spacing.contains("vertical (Y) axis of `Hole layout` (`Sketch`)"));
    assert!(spacing.contains("Direction: reversed") && spacing.contains("Transform mode: whole base shape"));
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
    assert_eq!((info.geometry, info.constraints), (4, 6));
    let unknown = info.warnings.iter().find(|w| w.detail.contains("type 999")).unwrap();
    assert!(unknown.detail.contains("geometry 1 (line segment; FreeCAD index 0)"));

    let profile = project.sketches.iter().find(|s| s.source_name == "Profile").unwrap();
    assert_eq!(profile.path, ["Bracket Part", "Main Body", "Mounting Profile"]);
    assert_eq!(profile.parts, [("Part".into(), "Bracket Part".into())]);
    assert_eq!(profile.body, Some(("Body".into(), "Main Body".into())));
    assert_eq!(profile.sketch.curves.len(), 4);
    assert!(profile.sketch.curves[1].construction);
    assert!(matches!(profile.sketch.constraints[0].kind, ConstraintKind::Coincident { .. }));
    assert!(matches!(profile.sketch.constraints[2].kind, ConstraintKind::Length { value: 40.0, .. }));
    assert!(matches!(profile.sketch.constraints[3].kind, ConstraintKind::Radius { value: 3.0, .. }));
    assert!(matches!(profile.sketch.constraints[4].kind, ConstraintKind::Equal { .. }));
    assert!(matches!(profile.sketch.constraints[5].kind, ConstraintKind::HorizontalPoints { .. }));

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
    let warning = loose.warnings.iter().find(|w| w.detail.contains("GeomBSplineCurve")).unwrap();
    assert!(warning.summary.contains("B-spline curve"));
    assert!(warning.detail.contains("points, line segments, circles, circular arcs and ellipses"));
}

#[test]
fn freecad_point_geometry_keeps_its_constraints_without_changing_the_profile() {
    let xml = r#"<Document><Objects>
      <Object name="Body" type="PartDesign::Body"/>
      <Object name="Sketch009" type="Sketcher::SketchObject"/>
      <Object name="Pad006" type="PartDesign::Pad"/>
    </Objects><ObjectData>
      <Object name="Body"><Properties><Property name="Group"><LinkList>
        <Link value="Sketch009"/><Link value="Pad006"/>
      </LinkList></Property></Properties></Object>
      <Object name="Sketch009"><Properties>
        <Property name="Label"><String value="Sketch010"/></Property>
        <Property name="Geometry"><GeometryList>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="0" EndX="30.4" EndY="0"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="30.4" StartY="0" EndX="30.4" EndY="20"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="30.4" StartY="20" EndX="0" EndY="20"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="0" StartY="20" EndX="0" EndY="0"/></Geometry>
          <Geometry type="Part::GeomPoint"><GeomPoint X="5" Y="0" Z="0"/><Construction value="0"/></Geometry>
          <Geometry type="Part::GeomLineSegment"><LineSegment StartX="5" StartY="-10" EndX="5" EndY="0"/><Construction value="1"/></Geometry>
        </GeometryList></Property>
        <Property name="Constraints"><ConstraintList>
          <Constrain Type="13" First="4" FirstPos="1" Second="0" SecondPos="0"/>
          <Constrain Type="1" First="5" FirstPos="2" Second="4" SecondPos="1"/>
          <Constrain Type="6" Value="25.4" First="0" FirstPos="2" Second="4" SecondPos="1"/>
        </ConstraintList></Property>
      </Properties></Object>
      <Object name="Pad006"><Properties>
        <Property name="Profile"><LinkSub value="Sketch009"/></Property>
        <Property name="Length"><Float value="10"/></Property>
        <Property name="Type"><Integer value="0"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "point-profile.FCStd").unwrap();
    let source = &project.sketches[0];
    assert!(source.warnings.is_empty(), "point and its constraints import: {:?}", source.warnings);
    assert_eq!(source.sketch.curves.len(), 5, "the point must not become a profile curve");
    assert_eq!(source.source_curves.len(), 6);
    assert!(source.source_curves[4].is_none());
    assert_eq!(source.sketch.constraints.len(), 3);
    let point = match source.sketch.constraints[0].kind {
        ConstraintKind::PointOnCurve { p, c: 0 } => p,
        ref kind => panic!("expected point on profile edge, got {kind:?}"),
    };
    assert_eq!(source.sketch.point(point), Some(Vec2::new(5.0, 0.0)));
    assert!(matches!(source.sketch.constraints[1].kind, ConstraintKind::Coincident { q, .. } if q == point));
    assert!(matches!(source.sketch.constraints[2].kind, ConstraintKind::Distance { q, value, .. } if q == point && value == 25.4));
    assert!(project.operations[0].blockers.is_empty(), "the pad remains importable");
}

#[test]
fn freecad_point_is_locatable_in_issue_previews() {
    let source = RawObject {
        geometry: vec![RawGeometry {
            type_name: "Part::GeomPoint".into(),
            construction: false,
            data: GeometryData::Point { position: Vec2::new(5.0, 0.0) },
        }],
        ..RawObject::default()
    };
    let picture = preview(&source, &mut 1);
    assert_eq!(picture.omitted, 0);
    assert_eq!(picture.curves[0].start, Some(Vec2::new(5.0, 0.0)));
    assert_eq!(picture.curves[0].points, [Vec2::new(5.0, 0.0)]);
}

#[test]
fn origin_plane_referenced_through_its_origin_is_not_a_face_attachment() {
    let xml = r#"<Document><Objects>
      <Object name="Body" type="PartDesign::Body"/><Object name="Origin" type="App::Origin"/>
      <Object name="XZ_Plane004" type="App::Plane"/><Object name="Sketch" type="Sketcher::SketchObject"/>
    </Objects><ObjectData>
      <Object name="Body"><Properties>
        <Property name="Group"><LinkList><Link value="Sketch"/></LinkList></Property>
        <Property name="Origin"><Link value="Origin"/></Property>
        <Property name="Placement"><PropertyPlacement Px="100" Py="0" Pz="5" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
      </Properties></Object>
      <Object name="Origin"><Properties>
        <Property name="OriginFeatures"><LinkList><Link value="XZ_Plane004"/></LinkList></Property>
      </Properties></Object>
      <Object name="XZ_Plane004"><Properties>
        <Property name="Label"><String value="XZ plane"/></Property>
      </Properties></Object>
      <Object name="Sketch"><Properties>
        <Property name="MapMode"><Integer value="5"/></Property>
        <Property name="AttachmentSupport"><LinkSubList><Link obj="Origin" sub="XZ_Plane004"/></LinkSubList></Property>
        <Property name="Placement"><PropertyPlacement Px="0" Py="0" Pz="0" Q0="0.7071067811865475" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property>
      </Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "origin-plane.FCStd").unwrap();
    let sketch = &project.sketches[0];
    assert!(!sketch.attachment_unresolved);
    assert!(sketch.warnings.is_empty());
    assert_eq!(sketch.plane_description, "Origin plane: XZ plane");
    assert!((sketch.plane.origin - Vec3::new(100.0, 0.0, 5.0)).len() < 1e-9);
    assert!((sketch.plane.y - Vec3::Z).len() < 1e-9);
    let unowned = xml.replace("<Link value=\"XZ_Plane004\"/>", "<Link value=\"UnrelatedPlane\"/>");
    let project = read_freecad_sketches(&archive(unowned.as_bytes()), "unowned-plane.FCStd").unwrap();
    assert!(!project.sketches[0].attachment_unresolved, "an explicitly named App::Plane retains the saved static plane");
    assert!(project.sketches[0].warnings.is_empty(), "an Origin membership mismatch does not affect the imported sketch plane");
    let dotted = xml.replace("sub=\"XZ_Plane004\"", "sub=\"XZ_Plane004.\"");
    let project = read_freecad_sketches(&archive(dotted.as_bytes()), "dotted-plane.FCStd").unwrap();
    assert!(!project.sketches[0].attachment_unresolved, "FreeCAD's trailing dot still names the Origin plane");
    assert!(project.sketches[0].warnings.is_empty());
    assert_eq!(project.sketches[0].plane_description, "Origin plane: XZ plane");
    let missing = xml.replace("sub=\"XZ_Plane004\"", "sub=\"MissingPlane\"");
    let project = read_freecad_sketches(&archive(missing.as_bytes()), "missing-plane.FCStd").unwrap();
    assert!(!project.sketches[0].attachment_unresolved, "a stale Origin link does not invalidate a saved sketch plane");
    assert!(project.sketches[0].warnings.is_empty());
    assert_eq!(project.sketches[0].plane_description, "Saved sketch plane (source Origin link unavailable)");
    assert_eq!(project.sketches[0].plane, sketch.plane);
    assert!(project.sketches[0].datum.is_none(), "a missing source plane must not be silently linked to another Origin feature");
    let no_placement = missing.replace(
        "<Property name=\"Placement\"><PropertyPlacement Px=\"0\" Py=\"0\" Pz=\"0\" Q0=\"0.7071067811865475\" Q1=\"0\" Q2=\"0\" Q3=\"0.7071067811865476\"/></Property>",
        "",
    );
    let project = read_freecad_sketches(&archive(no_placement.as_bytes()), "missing-plane-and-placement.FCStd").unwrap();
    assert!(project.sketches[0].attachment_unresolved, "without the source plane or a saved placement, the attachment needs repair");
    let detached = xml.replace("<Link obj=\"Origin\" sub=\"XZ_Plane004\"/>", "");
    let project = read_freecad_sketches(&archive(detached.as_bytes()), "detached-plane.FCStd").unwrap();
    assert!(!project.sketches[0].attachment_unresolved, "an empty support list leaves the saved sketch plane usable");
    assert!(project.sketches[0].warnings.is_empty());
    assert_eq!(project.sketches[0].plane_description, "Saved sketch plane (source support not recorded)");
}

#[test]
fn planes_keep_saved_frames_parent_placements_and_independent_face_attachments() {
    let xml = r#"<Document><Objects>
      <Object name="Part" type="App::Part"/><Object name="Body" type="PartDesign::Body"/>
      <Object name="Datum" type="PartDesign::Plane"/><Object name="XZ" type="App::Plane"/>
      <Object name="Sketch" type="Sketcher::SketchObject"/><Object name="FaceSketch" type="Sketcher::SketchObject"/>
      <Object name="Solid" type="PartDesign::Pad"/>
    </Objects><ObjectData>
      <Object name="Part"><Properties>
        <Property name="Group"><LinkList><Link value="Body"/></LinkList></Property>
        <Property name="Placement"><PropertyPlacement Px="100" Py="0" Pz="0" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
      </Properties></Object>
      <Object name="Body"><Properties>
        <Property name="Group"><LinkList><Link value="Datum"/><Link value="Sketch"/><Link value="FaceSketch"/></LinkList></Property>
        <Property name="Placement"><PropertyPlacement Px="0" Py="0" Pz="5" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
      </Properties></Object>
      <Object name="Datum"><Properties>
        <Property name="Label"><String value="Mounting plane"/></Property>
        <Property name="Placement"><PropertyPlacement Px="0" Py="0" Pz="12" Q0="0.7071067811865475" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property>
      </Properties></Object>
      <Object name="Sketch"><Properties>
        <Property name="MapMode"><Integer value="5"/></Property>
        <Property name="AttachmentSupport"><LinkSubList><Link obj="Datum" sub=""/></LinkSubList></Property>
        <Property name="Placement"><PropertyPlacement Px="0" Py="-7" Pz="12" Q0="0.7071067811865475" Q1="0" Q2="0" Q3="0.7071067811865476"/></Property>
      </Properties></Object>
      <Object name="FaceSketch"><Properties>
        <Property name="MapMode"><Integer value="5"/></Property>
        <Property name="Support"><LinkSub value="Solid"><Sub value="Face2"/></LinkSub></Property>
        <Property name="Placement"><PropertyPlacement Px="0" Py="0" Pz="50" Q0="0" Q1="0" Q2="0" Q3="1"/></Property>
      </Properties></Object>
      <Object name="Solid"><Properties><Property name="Label"><String value="Mounting Pad"/></Property></Properties></Object>
    </ObjectData></Document>"#;
    let project = read_freecad_sketches(&archive(xml.as_bytes()), "planes.FCStd").unwrap();
    let sketch = project.sketches.iter().find(|s| s.source_name == "Sketch").unwrap();
    assert!((sketch.plane.origin - Vec3::new(100.0, -7.0, 17.0)).len() < 1e-9);
    assert!((sketch.plane.x - Vec3::X).len() < 1e-9);
    assert!((sketch.plane.y - Vec3::Z).len() < 1e-9, "FreeCAD XZ uses +Z, unlike SolidCraft's named XZ");
    assert!(sketch.warnings.is_empty());
    let (_, label, datum) = sketch.datum.as_ref().unwrap();
    assert_eq!(label, "Mounting plane");
    assert!((datum.origin - Vec3::new(100.0, 0.0, 17.0)).len() < 1e-9);
    let face = project.sketches.iter().find(|s| s.source_name == "FaceSketch").unwrap();
    assert!((face.plane.origin - Vec3::new(100.0, 0.0, 55.0)).len() < 1e-9);
    assert!(!face.attachment_unresolved);
    assert_eq!(face.warnings.len(), 1);
    assert_eq!(face.warnings[0].severity, FreeCadWarningSeverity::Warning);
    assert!(face.warnings[0].detail.contains("dynamically attached"));
    assert!(face.warnings[0].detail.contains("static, independent plane"));
    assert!(
        face.warnings[0].detail.contains("Mounting Pad") && face.warnings[0].detail.contains("Solid") && face.warnings[0].detail.contains("Face2")
    );
    assert_eq!(face.plane_description, "Saved sketch plane (independent of Solid Face2)");
    assert!(face.datum.is_none());

    let sketch_attached =
        xml.replace("<LinkSub value=\"Solid\"><Sub value=\"Face2\"/></LinkSub>", "<LinkSub value=\"Sketch\"><Sub value=\"\"/></LinkSub>");
    let project = read_freecad_sketches(&archive(sketch_attached.as_bytes()), "sketch-attached.FCStd").unwrap();
    let attached = project.sketches.iter().find(|s| s.source_name == "FaceSketch").unwrap();
    assert!(!attached.attachment_unresolved);
    assert_eq!(attached.warnings.len(), 1);
    assert_eq!(attached.warnings[0].severity, FreeCadWarningSeverity::Warning);
    assert!(attached.warnings[0].detail.contains("Sketch"));
    assert_eq!(attached.plane_description, "Saved sketch plane (independent of Sketch)");

    let invalid = xml.replace("Q0=\"0.7071067811865475\" Q1=\"0\" Q2=\"0\" Q3=\"0.7071067811865476\"", "Q0=\"0\" Q1=\"0\" Q2=\"0\" Q3=\"0\"");
    let project = read_freecad_sketches(&archive(invalid.as_bytes()), "invalid-plane.FCStd").unwrap();
    let sketch = project.sketches.iter().find(|s| s.source_name == "Sketch").unwrap();
    assert_eq!(sketch.plane, Plane::XY);
    assert!(sketch.warnings.iter().any(|w| w.detail.contains("invalid placement")));
}
