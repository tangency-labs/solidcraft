//! Exact Onshape 37.1 compound-geometry subset over the reviewed 13006 base.

use crate::schema::{FieldDefinition, FieldType, SchemaSource, TypeDefinition};
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError};

pub(crate) const PROFILE_SHA256: &str = "3f9489b24874ca7857e48d8daf106bcf821f612b49e0d60650b20e132e04abaa";

/// Construct the exact Onshape 37.1 analytic, NURBS and curve-wrapper subset.
///
/// This profile does not select other modeller builds or the non-embedded V37 key.
///
/// # Errors
/// Returns a profile error for inconsistent compiled definitions.
pub fn onshape_sch37102_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "onshape-sch37102-13006-r3".into(),
            revision: 3,
            provider_schema: "13006".into(),
            producer_scope: "Onshape 37.1.212 compound solids and NURBS sheets".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3701212_37102_13006".into()],
        definitions(),
        vec![],
        // Membership-only audit of the complete 13006 type table: these types
        // are absent. Their complete definitions must come from the input.
        // 176 retains the multi-part transmit block; it adds no assembly roles.
        vec![176, 204],
    )
}

pub(crate) fn definitions() -> Vec<TypeDefinition> {
    // Explicit membership: future 13006 additions do not expand this profile.
    let types = [12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 50, 51, 52, 53, 70, 74, 79, 80, 81, 82, 83, 84, 98, 133, 141];
    let mut definitions: Vec<_> = super::sch13006_definitions().into_iter().filter(|d| types.contains(&d.node_type)).collect();
    // Current-key producer pairs exercise these reviewed V13 base definitions.
    // Membership remains explicit rather than inheriting future V13 additions.
    definitions.extend(super::sch13006_sp_curve_definitions().into_iter().filter(|d| [45, 127, 128, 134, 135, 136, 137].contains(&d.node_type)));
    definitions.extend(super::sch13006_bspline_surface_definitions().into_iter().filter(|d| [124, 125, 126].contains(&d.node_type)));
    definitions.push(torus());
    definitions
}

/// Public XT Format Reference, April 2008, pp. 60-61, 116-118: TORUS (54).
/// Membership and the unchanged declaration are checked with producer pairs.
/// These project-owned names also identify copied fields after embedded edits.
fn torus() -> TypeDefinition {
    use FieldType::{Character as C, Double as F, Integer as D, PointerIndex as P, Vector as V};
    let fields = [
        ("local_id", D, 0),
        ("annotations", P, 1019),
        ("owner_ref", P, 1007),
        ("next_surface", P, 1006),
        ("previous_surface", P, 1006),
        ("indirect_owner", P, 141),
        ("orientation", C, 0),
        ("center", V, 0),
        ("axis", V, 0),
        ("major_radius", F, 0),
        ("minor_radius", F, 0),
        ("x_direction", V, 0),
    ]
    .into_iter()
    .map(|(name, field_type, pointer_class)| FieldDefinition { name: name.into(), field_type, pointer_class, element_count: 0, transmitted: true })
    .collect();
    TypeDefinition::from_fields(54, "sch13006_type_54", "Reviewed TORUS base layout", fields, SchemaSource::Base)
}
