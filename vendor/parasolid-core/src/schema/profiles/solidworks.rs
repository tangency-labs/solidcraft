//! Exact `SolidWorks` 2026 partition subset; deltas remain unsupported.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str = "5e05a32681cfa8bf4a3fb029124eb6fb2cf6e34e021f7ed7b60c3df654a9c480";

/// Construct the exact `SolidWorks` 2026 partition subset.
///
/// Deltas use the same header key but contain unsupported update records.
/// This profile does not establish a model's final saved configuration state.
///
/// # Errors
/// Returns a profile error for inconsistent compiled definitions.
pub fn solidworks_sch37102_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::sch13006_sp_curve_definitions());
    definitions.extend(super::sch13006_bspline_surface_definitions());
    definitions.push(world());
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "solidworks-sch37102-13006-r1".into(),
            revision: 1,
            provider_schema: "13006".into(),
            producer_scope: "SolidWorks 2026 partition".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3701229_37102_13006".into()],
        definitions,
        vec![],
        vec![],
    )
}

/// Public XT Format Reference, April 2008, pp. 82-83 and 118: WORLD (101).
/// Eleven base fields; indexed-transmit fields in the modern input are
/// embedded additions, not guessed members of the 13006 base.
pub(super) fn world() -> TypeDefinition {
    use FieldType::{Integer as D, Logical as L, PointerIndex as P};
    let fields = [
        ("assembly_head", P, 10),
        ("attribute_head", P, 81),
        ("body_head", P, 12),
        ("transform_head", P, 100),
        ("surface_head", P, 1006),
        ("curve_head", P, 1008),
        ("point_head", P, 29),
        ("alive", L, 0),
        ("attribute_definition_head", P, 80),
        ("highest_id", D, 0),
        ("current_id", D, 0),
    ]
    .into_iter()
    .map(|(name, field_type, pointer_class)| FieldDefinition { name: name.into(), field_type, pointer_class, element_count: 0, transmitted: true })
    .collect();
    TypeDefinition::from_fields(101, "sch13006_type_101", "Reviewed WORLD base layout", fields, SchemaSource::Base)
}
