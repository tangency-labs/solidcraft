//! Synthetic role checks; solid-body evidence is verified by the local M8.3 runner.
#[path = "support/sch30000.rs"]
mod support;

use parasolid_core::{
    BuiltinProfileRegistry, DocumentLimits, ErrorKind, FieldType, FieldValue, RawField, RawNode, SchemaKey, SchemaProviderResolution, XbDocument,
    brep::{CurveKind, SurfaceKind, map_xb_brep},
    parse_xb,
    schema::profiles::onshape_sch30000,
};
use support::{KEY, Result};

fn node(kind: u16, index: u32, overrides: Vec<(usize, FieldValue)>) -> Result<RawNode> {
    let profile = onshape_sch30000()?;
    let definition = profile.definitions().find(|d| d.node_type == kind).ok_or("missing type")?.clone();
    let mut fields = definition
        .fields
        .iter()
        .map(|d| {
            let value = match d.field_type {
                FieldType::Integer => FieldValue::Integer(Some(i32::try_from(index).unwrap_or(0) + 1000)),
                FieldType::PointerIndex => FieldValue::PointerIndex(0),
                FieldType::Double => FieldValue::Double(None),
                FieldType::Vector => FieldValue::Vector([Some(0.0); 3]),
                FieldType::Character => FieldValue::Character(b'+'),
                FieldType::UnsignedByte => FieldValue::UnsignedByte(0),
                _ => return Err("unexpected synthetic field type"),
            };
            Ok(RawField { definition: d.clone(), values: vec![value], byte_range: 0..0 })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (ordinal, value) in overrides {
        fields[ordinal].values = vec![value];
    }
    Ok(RawNode { node_type: kind, index, variable_length: None, definition, fields, first_schema: None, byte_range: 0..0, user_fields: Vec::new() })
}

fn vector(x: f64, y: f64, z: f64) -> FieldValue {
    FieldValue::Vector([Some(x), Some(y), Some(z)])
}

/// A wire with two vertices, two dummy fins and independent analytic field probes.
/// The fixture is synthetic; it does not extend the profile's real solid coverage.
fn document() -> Result<XbDocument> {
    use FieldValue::{Character as C, Double as D, PointerIndex as P, UnsignedByte as U};
    let nodes = vec![
        node(12, 1, vec![(9, D(Some(1e-6))), (10, D(Some(1e-8))), (16, U(2)), (24, P(2)), (25, P(6)), (26, P(4))])?,
        node(19, 2, vec![(2, P(1)), (5, P(3)), (6, C(b'V'))])?,
        node(13, 3, vec![(7, P(2))])?,
        node(18, 4, vec![(4, P(9)), (5, P(5)), (7, P(1))])?,
        node(29, 5, vec![(2, P(4)), (5, vector(1.0, 2.0, 3.0))])?,
        node(16, 6, vec![(3, P(7)), (6, P(11)), (9, P(1))])?,
        node(17, 7, vec![(4, P(4)), (5, P(8)), (6, P(6)), (9, C(b'+'))])?,
        node(17, 8, vec![(4, P(9)), (5, P(7)), (6, P(6)), (9, C(b'-'))])?,
        node(18, 9, vec![(5, P(10)), (7, P(1))])?,
        node(29, 10, vec![(2, P(9)), (5, vector(4.0, 6.0, 3.0))])?,
        node(30, 11, vec![(2, P(6)), (7, vector(1.0, 2.0, 3.0)), (8, vector(0.6, 0.8, 0.0))])?,
        node(31, 12, vec![(7, vector(3.0, 4.0, 5.0)), (8, vector(0.0, 0.0, 1.0)), (9, vector(1.0, 0.0, 0.0)), (10, D(Some(2.0)))])?,
        node(50, 13, vec![(7, vector(7.0, 8.0, 9.0)), (8, vector(0.0, 1.0, 0.0)), (9, vector(1.0, 0.0, 0.0))])?,
        node(51, 14, vec![(7, vector(10.0, 11.0, 12.0)), (8, vector(0.0, 0.0, 1.0)), (9, D(Some(3.0))), (10, vector(1.0, 0.0, 0.0))])?,
        node(
            32,
            15,
            vec![
                (6, C(b'-')),
                (7, vector(-1.25, 2.5, -3.75)),
                (8, vector(0.0, 1.0, 0.0)),
                (9, vector(0.0, 0.0, -1.0)),
                (10, D(Some(4.5))),
                (11, D(Some(2.25))),
            ],
        )?,
        node(53, 16, vec![(6, C(b'-')), (7, vector(-4.0, 5.0, -6.0)), (8, D(Some(1.75))), (9, vector(0.0, 1.0, 0.0)), (10, vector(0.0, 0.0, -1.0))])?,
    ];
    let registry = BuiltinProfileRegistry::new(vec![onshape_sch30000()?])?;
    let key = SchemaKey::parse(KEY)?;
    let provider = registry.provider_for_key(&key).ok_or("missing profile")?;
    Ok(parse_xb(&support::encode_nodes(&nodes)?, &provider, DocumentLimits::default())?)
}

#[test]
#[allow(clippy::float_cmp)] // Role transfer must preserve decoded values exactly.
fn maps_analytic_roles_and_preserves_source_provenance() -> Result<()> {
    let doc = document()?;
    let before = doc.nodes.clone();
    let model = map_xb_brep(&doc)?;
    assert!(model.complete && model.topology.valid);
    assert_eq!(model.points[0].position.to_array(), [1.0, 2.0, 3.0]);
    assert_eq!(model.vertices[0].source.node_id, Some(1004));
    assert_eq!(model.bodies[0].source.node_id, None); // BODY has a maximum id, not a node id.
    assert_eq!(model.vertices[0].source.type_name, "onshape_type_18");
    assert_eq!(model.edges[0].half_edges.len(), 2);
    assert!(matches!(model.curves[0].kind, CurveKind::Line {point, direction}
        if point.to_array() == [1.0,2.0,3.0] && direction.to_array() == [0.6,0.8,0.0]));
    assert!(matches!(model.curves[1].kind, CurveKind::Circle {center, normal, x_axis, radius}
        if center.to_array() == [3.0,4.0,5.0] && normal.to_array() == [0.0,0.0,1.0]
        && x_axis.to_array() == [1.0,0.0,0.0] && (radius-2.0).abs() < f64::EPSILON));
    assert!(matches!(model.surfaces[0].kind, SurfaceKind::Plane {point, normal, x_axis}
        if point.to_array() == [7.0,8.0,9.0] && normal.to_array() == [0.0,1.0,0.0]
        && x_axis.to_array() == [1.0,0.0,0.0]));
    assert!(matches!(model.surfaces[1].kind, SurfaceKind::Cylinder {point, axis, radius, x_axis}
        if point.to_array() == [10.0,11.0,12.0] && axis.to_array() == [0.0,0.0,1.0]
        && (radius-3.0).abs() < f64::EPSILON && x_axis.to_array() == [1.0,0.0,0.0]));
    assert_eq!(doc.nodes, before);
    assert!(matches!(model.curves[2].kind, CurveKind::Ellipse {center, normal, x_axis, major_radius, minor_radius}
        if center.to_array() == [-1.25, 2.5, -3.75] && normal.to_array() == [0.0, 1.0, 0.0]
        && x_axis.to_array() == [0.0, 0.0, -1.0] && major_radius == 4.5 && minor_radius == 2.25));
    assert!(matches!(model.surfaces[2].kind, SurfaceKind::Sphere {center, radius, axis, x_axis}
        if center.to_array() == [-4.0, 5.0, -6.0] && radius == 1.75
        && axis.to_array() == [0.0, 1.0, 0.0] && x_axis.to_array() == [0.0, 0.0, -1.0]));
    assert_eq!(model.curves[2].source.node_id, Some(1015));
    assert_eq!(model.surfaces[2].source.node_id, Some(1016));
    Ok(())
}

#[test]
#[allow(clippy::float_cmp)] // No arithmetic is performed by the role accessor.
fn builtin_mapping_uses_ordinals_instead_of_names_or_classes() -> Result<()> {
    let mut doc = document()?;
    for node in &mut doc.nodes {
        node.definition.name = "unrelated".to_owned();
        for field in &mut node.fields {
            field.definition.name = "unrelated".to_owned();
            field.definition.pointer_class = 0;
        }
    }
    let mapped = map_xb_brep(&doc)?;
    assert!(mapped.complete);
    assert_eq!(mapped.curves.len(), 3);
    assert_eq!(mapped.surfaces.len(), 3);
    assert_eq!(mapped.points[0].position.to_array(), [1.0, 2.0, 3.0]);
    assert_eq!(mapped.vertices[0].source.type_name, "unrelated");
    Ok(())
}

#[test]
fn never_reuses_ordinals_for_caller_or_another_profile() -> Result<()> {
    let mut caller = document()?;
    caller.schema_provider = SchemaProviderResolution::CallerSupplied;
    assert_eq!(map_xb_brep(&caller).err().map(|e| e.kind()), Some(ErrorKind::MissingBrepBody));
    for change in 0..5 {
        let mut doc = document()?;
        if let SchemaProviderResolution::Builtin { profile_id, profile_revision, schema_key, profile_sha256, .. } = &mut doc.schema_provider {
            match change {
                0 => *profile_id = "unreviewed".to_owned(),
                1 => *profile_revision = 1,
                2 => *schema_key = "SCH_3000001_30000".to_owned(),
                3 => *profile_sha256 = "0".repeat(64),
                _ => doc.schema_key = SchemaKey::parse("SCH_3000001_30000")?,
            }
        }
        assert_eq!(map_xb_brep(&doc).err().map(|e| e.kind()), Some(ErrorKind::InvalidBrepField));
    }
    Ok(())
}

#[test]
fn rejects_wrong_typed_references_broken_rings_and_negative_radii() -> Result<()> {
    for (index, ordinal, value, error) in [
        (6, 9, FieldValue::PointerIndex(5), ErrorKind::InvalidBrepTopology),
        (4, 7, FieldValue::PointerIndex(5), ErrorKind::InvalidBrepTopology),
        (4, 5, FieldValue::PointerIndex(1), ErrorKind::InvalidBrepReference),
        (7, 5, FieldValue::PointerIndex(999), ErrorKind::InvalidBrepReference),
        (12, 10, FieldValue::Double(Some(-1.0)), ErrorKind::InvalidGeometryParameter),
        (14, 9, FieldValue::Double(Some(-1.0)), ErrorKind::InvalidGeometryParameter),
        (15, 10, FieldValue::Double(Some(-1.0)), ErrorKind::InvalidGeometryParameter),
        (15, 11, FieldValue::Double(Some(0.0)), ErrorKind::InvalidGeometryParameter),
        (16, 8, FieldValue::Double(Some(-1.0)), ErrorKind::InvalidGeometryParameter),
        (15, 7, FieldValue::Vector([None; 3]), ErrorKind::InvalidGeometryParameter),
        (16, 9, FieldValue::Vector([None; 3]), ErrorKind::InvalidGeometryParameter),
    ] {
        let mut doc = document()?;
        let node = doc.nodes.iter_mut().find(|n| n.index == index).ok_or("missing synthetic node")?;
        node.fields[ordinal].values[0] = value;
        assert_eq!(map_xb_brep(&doc).err().map(|e| e.kind()), Some(error));
    }
    Ok(())
}
