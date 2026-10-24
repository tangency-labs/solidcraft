//! Exact iCAD V34 embedded key, using the independently reviewed 13006 subset.
//! No catalog field layouts are copied; unsupported base types remain unknown.
use crate::{BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ParseError, TypeDefinition};

pub(crate) const PROFILE_SHA256: &str = "595a8848c2b7730367bb41bbe75332c79c9038eb56accc7ee785ca9742a37de7";

/// Construct the reviewed base subset for iCAD SX V8L3 exported resources.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_sch34101_13006() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new_embedded(
        BuiltinProfileMetadata {
            profile_id: "icad-sch34101-13006-r3".into(),
            revision: 3,
            provider_schema: "13006".into(),
            producer_scope: "iCAD V34 extracted Parasolid streams".into(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.into(),
        },
        vec!["SCH_3401212_34101_13006".into()],
        definitions(),
        vec![],
        // Reuse only the existing 13006 membership audit: 204 is absent.
        vec![204],
    )
}

/// The reviewed subset with TORUS (54), as in revision 2, plus the blend,
/// spun-surface and SP-curve dependency types observed under this key.
pub(crate) fn definitions() -> Vec<TypeDefinition> {
    super::icad_legacy::base_definitions(&[45, 56, 68, 127, 128, 134, 135, 136, 137])
}
