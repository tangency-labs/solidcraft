//! Reviewed 13006 base subset from the public XT reference and V13 `X_T/X_B`.
//!
//! Onshape V13 exports expose the complete base layouts, including LIST's three
//! trailing fields which the observed iCAD V30 delta discards at End. The V30
//! prefix-only experiment was insufficient to establish this complete base.
//! Revision 3 adds intersection (38), chart (40), limit (41) and shared
//! geometry owner (141): 29 reviewed types and 228 base field groups.
//! No Siemens catalog is used to construct these definitions.
//! Onshape revision 4 and iCAD revision 5 cover trimmed curve (133): 30 types
//! and 240 field groups. V13 producer pairs validate the same complete layout
//! previously observed with an unchanged declaration in iCAD V30 input.
//! Onshape revision 5 adds seven `SP_CURVE` dependency layouts (37 types / 277
//! groups). Their embedded membership/layout is not claimed for iCAD.
//! Onshape revision 6 adds three B-spline surface dependencies (40 types / 327
//! groups) for rational periodic UV boundaries on bilinear sheets.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

const BASE_SHA256: &str = "2748e9f9c28fa32b59edb9e0b16fea7a976ad081f698dd3d098d9cbd17e08fbb";
const EMBEDDED_SHA256: &str = "29d58f5c4aba74913689b9034b0532a17601bb660fafc01d21692ef5330f4f8e";

fn metadata(id: &str, revision: u32, producer: &str, digest: &str) -> BuiltinProfileMetadata {
    BuiltinProfileMetadata {
        profile_id: id.to_owned(),
        revision,
        provider_schema: "13006".to_owned(),
        producer_scope: producer.to_owned(),
        coverage: BuiltinProfileCoverage::VerifiedSubset,
        evidence_manifest_sha256: None,
        profile_sha256: digest.to_owned(),
    }
}

/// Construct the reviewed Onshape V13 standard-schema subset.
///
/// # Errors
/// Returns a profile error if the compiled definitions are inconsistent.
pub fn onshape_sch13006() -> Result<BuiltinSchemaProfile, ParseError> {
    let mut definitions = definitions();
    definitions.extend(sp_curve_definitions());
    definitions.extend(bspline_surface_definitions());
    BuiltinSchemaProfile::new(metadata("onshape-sch13006-r6", 6, "Onshape V13", BASE_SHA256), vec!["SCH_1300000_13006".to_owned()], definitions)
}

/// Public XT reference pp. 39-43, 52-53, 116-118, checked with V13 producer pairs.
/// Embedded iCAD membership is not inferred; V30 reviews its use separately.
pub(crate) fn sp_curve_definitions() -> Vec<TypeDefinition> {
    use FieldType::{Character as C, Double as F, Integer as D, Logical as L, PointerIndex as P, ShortInteger as N, UnsignedByte as U};
    let field = |name: &str, field_type, pointer_class, element_count| FieldDefinition {
        name: name.to_owned(),
        field_type,
        pointer_class,
        element_count,
        transmitted: true,
    };
    let common = || {
        vec![
            field("local_id", D, 0, 0),
            field("annotations", P, 1019, 0),
            field("owner_ref", P, 1010, 0),
            field("next_curve", P, 1008, 0),
            field("previous_curve", P, 1008, 0),
            field("indirect_owner", P, 141, 0),
            field("orientation", C, 0, 0),
        ]
    };
    let mut spline = common();
    spline.extend([field("nurbs_ref", P, 136, 0), field("data_ref", P, 135, 0)]);
    let mut surface_curve = common();
    surface_curve.extend([
        field("surface_ref", P, 1006, 0),
        field("parameter_curve", P, 134, 0),
        field("original_curve", P, 1008, 0),
        field("original_tolerance", F, 0, 0),
    ]);
    [
        (45, vec![field("vertices", F, 0, 1)]),
        (127, vec![field("multiplicities", N, 0, 1)]),
        (128, vec![field("knots", F, 0, 1)]),
        (134, spline),
        (135, vec![field("self_intersection_state", U, 0, 0), field("analytic_form", P, 163, 0)]),
        (
            136,
            vec![
                field("degree", N, 0, 0),
                field("control_count", D, 0, 0),
                field("vertex_dimension", N, 0, 0),
                field("knot_count", D, 0, 0),
                field("knot_kind", U, 0, 0),
                field("periodic", L, 0, 0),
                field("closed", L, 0, 0),
                field("rational", L, 0, 0),
                field("curve_form", U, 0, 0),
                field("control_vertices", P, 45, 0),
                field("multiplicities", P, 127, 0),
                field("knots", P, 128, 0),
            ],
        ),
        (137, surface_curve),
    ]
    .into_iter()
    .map(|(node_type, fields)| {
        TypeDefinition::from_fields(
            node_type,
            format!("sch13006_type_{node_type}"),
            "Reviewed V13 surface-parametric curve dependency",
            fields,
            SchemaSource::Base,
        )
    })
    .collect()
}

/// Construct the 13006 base subset for the reviewed iCAD V30 embedded key.
///
/// This accepts extracted neutral Parasolid streams, not iCAD containers.
/// Type 204 is confirmed absent from 13006 and uses its full embedded
/// declaration. Other types outside the reviewed base subset remain unknown.
/// Revision 6 adds TORUS (54) and the separately reviewed curve, blend,
/// offset and B-spline surface types observed under this key.
///
/// # Errors
/// Returns a profile error if the compiled definitions are inconsistent.
pub fn icad_sch30000_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new_embedded(
        metadata("icad-sch30000-13006-r6", 6, "iCAD V30 extracted Parasolid streams", EMBEDDED_SHA256),
        vec!["SCH_3000310_30000_13006".to_owned()],
        icad_sch30000_13006_definitions(),
        vec![],
        // Membership-only audit: the complete 13006 catalog header declares
        // a type-table upper bound of 184. No catalog field layouts are copied.
        // Keep this allowlist bounded to the independently tested new type.
        vec![204],
    )
}

pub(crate) fn icad_sch30000_13006_definitions() -> Vec<TypeDefinition> {
    super::icad_legacy::base_definitions(&[45, 56, 60, 124, 125, 126, 127, 128, 134, 135, 136, 137])
}

/// Public XT reference pp. 48-50, V13 producer pairs and V30 unchanged records.
fn trimmed_curve() -> TypeDefinition {
    use FieldType::{Character as C, Double as F, Integer as D, PointerIndex as P, Vector as V};
    let fields = [
        ("local_id", D, 0),
        ("annotations", P, 1019),
        ("owner_ref", P, 1010),
        ("next_curve", P, 1008),
        ("previous_curve", P, 1008),
        ("indirect_owner", P, 141),
        ("orientation", C, 0),
        ("basis_curve", P, 1008),
        ("start_point", V, 0),
        ("end_point", V, 0),
        ("start_parameter", F, 0),
        ("end_parameter", F, 0),
    ]
    .into_iter()
    .map(|(name, field_type, pointer_class)| FieldDefinition {
        name: name.to_owned(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    })
    .collect();
    TypeDefinition::from_fields(133, "sch13006_type_133", "Reviewed 13006 trimmed curve", fields, SchemaSource::Base)
}

#[allow(clippy::too_many_lines)] // Auditable wire order of the reviewed base subset.
pub(crate) fn definitions() -> Vec<TypeDefinition> {
    use FieldType::{
        Character as C, Double as F, Integer as D, IntersectionPoint as H, Logical as L, PointerIndex as P, UnsignedByte as U, Vector as V,
    };
    let field = |name: &str, field_type, pointer_class| FieldDefinition {
        name: name.to_owned(),
        field_type,
        pointer_class,
        element_count: 0,
        transmitted: true,
    };
    let array = |name: &str, field_type, pointer_class, element_count| FieldDefinition { element_count, ..field(name, field_type, pointer_class) };
    // Sharing only these explicitly reviewed unchanged structures avoids claiming
    // that future additions to the V30 table also existed in 13006.
    let types = [12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 50, 51, 53, 70, 74, 79, 80, 81, 82, 83, 84, 98];
    let mut definitions: Vec<_> = super::sch30000::base_definitions().into_iter().filter(|d| types.contains(&d.node_type)).collect();
    for definition in &mut definitions {
        definition.name = format!("sch13006_type_{}", definition.node_type);
        "Reviewed 13006 base subset".clone_into(&mut definition.description);
        match definition.node_type {
            // Public reference pp. 87-90; V13 has no V30 inserted mesh/index fields.
            12 => {
                definition.fields = vec![
                    field("max_local_id", D, 0),
                    field("annotations", P, 1019),
                    field("attribute_lists", P, 70),
                    field("construction_surfaces", P, 1006),
                    field("construction_curves", P, 1008),
                    field("construction_points", P, 29),
                    field("key_ref", P, 102),
                    field("size_precision", F, 0),
                    field("linear_precision", F, 0),
                    field("instances", P, 11),
                    field("next_body", P, 12),
                    field("previous_body", P, 12),
                    field("storage_state", U, 0),
                    field("legacy_owner", P, 101),
                    field("body_kind", U, 0),
                    field("geometry_state", U, 0),
                    field("legacy_shell", P, 13),
                    field("boundary_surfaces", P, 1006),
                    field("boundary_curves", P, 1008),
                    field("boundary_points", P, 29),
                    field("region_head", P, 19),
                    field("edge_head", P, 16),
                    field("vertex_head", P, 18),
                ];
            }
            // Public reference pp. 91-92; the eighth field is appended in V30.
            19 => definition.fields.truncate(7),
            // Public reference pp. 100-101; all 12 fields are transmitted in V13.
            70 => {
                definition.fields = vec![
                    field("local_id", D, 0),
                    field("owner_ref", P, 1013),
                    field("next_list", P, 70),
                    field("previous_list", P, 70),
                    field("entry_kind", D, 0),
                    field("entry_count", D, 0),
                    field("block_capacity", D, 0),
                    field("entry_size", D, 0),
                    field("block_head", P, 1012),
                    field("cursor_block", P, 1012),
                    field("cursor_index", D, 0),
                    field("transmission_flag", L, 0),
                ];
            }
            // Public reference pp. 101-102; V30 inserts index_origin before next.
            74 => {
                definition.fields.remove(1);
            }
            _ => {}
        }
    }
    // Public XT reference pp. 57-58. The radius is measured at origin, not
    // necessarily at a cap; preserve both signed half-angle components.
    // Revision 2 verifies this layout against new Onshape V13 cone exports.
    definitions.push(TypeDefinition::from_fields(
        52,
        "sch13006_type_52",
        "Reviewed 13006 cone",
        vec![
            field("local_id", D, 0),
            field("annotations", P, 1019),
            field("owner_ref", P, 1007),
            field("next_surface", P, 1006),
            field("previous_surface", P, 1006),
            field("indirect_owner", P, 141),
            field("orientation", C, 0),
            field("origin", V, 0),
            field("axis", V, 0),
            field("radius", F, 0),
            field("sin_half_angle", F, 0),
            field("cos_half_angle", F, 0),
            field("x_direction", V, 0),
        ],
        SchemaSource::Base,
    ));
    // Public XT reference pp. 44-48: intersection branch, chart and limits.
    // An hvec transmits only its three position coordinates, not its cached
    // surface parameters, tangent or curve parameter.
    definitions.extend([
        TypeDefinition::from_fields(
            38,
            "sch13006_type_38",
            "Reviewed 13006 surface intersection",
            vec![
                field("local_id", D, 0),
                field("annotations", P, 1019),
                field("owner_ref", P, 1010),
                field("next_curve", P, 1008),
                field("previous_curve", P, 1008),
                field("indirect_owner", P, 141),
                field("orientation", C, 0),
                array("supporting_surfaces", P, 1006, 2),
                field("chart_ref", P, 40),
                field("start_limit", P, 41),
                field("end_limit", P, 41),
            ],
            SchemaSource::Base,
        ),
        TypeDefinition::from_fields(
            40,
            "sch13006_type_40",
            "Reviewed 13006 intersection chart",
            vec![
                field("base_parameter", F, 0),
                field("base_scale", F, 0),
                field("chart_count", D, 0),
                field("chordal_error", F, 0),
                field("angular_error", F, 0),
                array("parameter_errors", F, 0, 2),
                array("sample_positions", H, 0, 1),
            ],
            SchemaSource::Base,
        ),
        TypeDefinition::from_fields(
            41,
            "sch13006_type_41",
            "Reviewed 13006 intersection limit",
            vec![field("limit_kind", C, 0), array("limit_positions", H, 0, 1)],
            SchemaSource::Base,
        ),
        // Public reference pp. 80-81: back-reference ring for shared geometry.
        TypeDefinition::from_fields(
            141,
            "sch13006_type_141",
            "Reviewed 13006 shared geometry owner",
            vec![
                field("referencing_geometry", P, 1003),
                field("next_owner", P, 141),
                field("previous_owner", P, 141),
                field("shared_geometry", P, 1003),
            ],
            SchemaSource::Base,
        ),
    ]);
    definitions.push(trimmed_curve());
    definitions
}

/// Public XT reference pp. 67-72 and V13 sheet-boundary producer pairs.
/// Auxiliary surface data is retained raw; only the NURBS definition maps to B-Rep.
#[allow(clippy::too_many_lines)]
pub(crate) fn bspline_surface_definitions() -> Vec<TypeDefinition> {
    use FieldType::{Character as C, Integer as D, Interval as I, Logical as L, PointerIndex as P, ShortInteger as N, UnsignedByte as U};
    let mut definitions = Vec::new();
    let field =
        |name: &str, field_type, pointer_class| FieldDefinition { name: name.into(), field_type, pointer_class, element_count: 0, transmitted: true };
    let surface = vec![
        field("local_id", D, 0),
        field("annotations", P, 1019),
        field("owner_ref", P, 1007),
        field("next_surface", P, 1006),
        field("previous_surface", P, 1006),
        field("indirect_owner", P, 141),
        field("orientation", C, 0),
        field("nurbs_ref", P, 126),
        field("data_ref", P, 125),
    ];
    let nurbs = vec![
        field("u_periodic", L, 0),
        field("v_periodic", L, 0),
        field("u_degree", N, 0),
        field("v_degree", N, 0),
        field("u_control_count", D, 0),
        field("v_control_count", D, 0),
        field("u_knot_kind", U, 0),
        field("v_knot_kind", U, 0),
        field("u_knot_count", D, 0),
        field("v_knot_count", D, 0),
        field("rational", L, 0),
        field("u_closed", L, 0),
        field("v_closed", L, 0),
        field("surface_form", U, 0),
        field("vertex_dimension", N, 0),
        field("control_vertices", P, 45),
        field("u_multiplicities", P, 127),
        field("v_multiplicities", P, 127),
        field("u_knots", P, 128),
        field("v_knots", P, 128),
    ];
    let mut data = vec![
        field("original_u_range", I, 0),
        field("original_v_range", I, 0),
        field("extended_u_range", I, 0),
        field("extended_v_range", I, 0),
        field("self_intersection_state", U, 0),
    ];
    for name in [
        "original_u_start",
        "original_u_end",
        "original_v_start",
        "original_v_end",
        "extended_u_start",
        "extended_u_end",
        "extended_v_start",
        "extended_v_end",
        "analytic_kind",
        "swept_kind",
        "spun_kind",
        "blend_kind",
    ] {
        data.push(field(name, C, 0));
    }
    for name in ["analytic_form", "swept_form", "spun_form", "blend_form"] {
        data.push(field(name, P, 0));
    }
    for (kind, fields) in [(124, surface), (125, data), (126, nurbs)] {
        definitions.push(TypeDefinition::from_fields(
            kind,
            format!("sch13006_type_{kind}"),
            "Reviewed V13 B-spline surface dependency",
            fields,
            SchemaSource::Base,
        ));
    }
    definitions
}
