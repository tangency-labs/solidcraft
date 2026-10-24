//! Exact `SolidWorks` 2026 partition subset; deltas remain unsupported.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

/// Construct the exact `SolidWorks` 2026 partition subset.
///
/// Deltas use the same header key but contain unsupported update records.
/// This profile does not establish a model's final saved configuration state.
///
/// # Errors
/// Returns a profile error for inconsistent compiled definitions.
pub fn solidworks_sch35102_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::sch13006_sp_curve_definitions());
    definitions.extend(super::sch13006_bspline_surface_definitions());
    definitions.extend(super::onshape_current_definitions().into_iter().filter(|d| d.node_type == 54));
    definitions.push(vector_values());
    definitions.push(super::solidworks::world());
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "solidworks-sch35102-13006-r1".into(),
            revision: 1,
            provider_schema: "13006".into(),
            producer_scope: "SolidWorks 35.1 saved B-Rep partition".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: super::SOLIDWORKS_35102_PROFILE_SHA256.into(),
        },
        vec!["SCH_3501251_35102_13006".into()],
        definitions,
        vec![],
        vec![204],
    )
}

/// VECTOR_VALUES (86), public XT Format Reference, April 2008, pp. 111,117.
fn vector_values() -> TypeDefinition {
    TypeDefinition::from_fields(
        86,
        "sch13006_type_86",
        "XT vector attribute values",
        vec![FieldDefinition { name: "values".into(), field_type: FieldType::Vector, pointer_class: 0, element_count: 1, transmitted: true }],
        SchemaSource::Base,
    )
}
