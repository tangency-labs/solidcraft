//! Exact iCAD embedded keys qualified with authored fixtures and retained streams.
//! Declarations come from the public XT reference and existing producer profiles;
//! external catalogs are used only for subsequent comparison and membership checks.
//! Version changes to BODY, REGION, INTERSECTION, LIMIT and the list types arrive
//! as embedded edits of each stream; no profile declares them.

use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldDefinition, FieldType, ParseError, SchemaSource, TypeDefinition,
};

pub(crate) struct ProfileSpec {
    pub key: &'static str,
    pub id: &'static str,
    pub revision: u32,
    pub sha256: &'static str,
    extra_types: &'static [u16],
}

pub(crate) const PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        key: "SCH_1500137_15003_13006",
        id: "icad-1500137-15003-13006-r1",
        revision: 1,
        sha256: "dcb4bb70654e9d1bf091b1310b2068872b56304f0b820435a3910bcec366a9e8",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_1500245_15003_13006",
        id: "icad-1500245-15003-13006-r1",
        revision: 1,
        sha256: "4179d5785e61e89926dfb54e0bc5eed7f0fb7cdec7300de0936ef678ba4d0f4c",
        extra_types: &[56, 59, 68],
    },
    ProfileSpec {
        key: "SCH_1700223_16100_13006",
        id: "icad-1700223-16100-13006-r1",
        revision: 1,
        sha256: "a27d8216055fe16425438c854482ec8dfac5e6f2e0aea1e1b7ce3605bc6ac633",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_1700256_16100_13006",
        id: "icad-1700256-16100-13006-r1",
        revision: 1,
        sha256: "30fc3f141639819bb6c76a4585056c7b274a6103d6c3924f71c448603796e802",
        extra_types: &[45, 68, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_1901261_19008_13006",
        id: "icad-1901261-19008-13006-r1",
        revision: 1,
        sha256: "f8913a60054dd7f29049a46c4b57ec6e7be048a2c112230453d31fe8587b6e22",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_1901315_19008_13006",
        id: "icad-1901315-19008-13006-r1",
        revision: 1,
        sha256: "781de5125a27ee855bd377a456df9f21d2a71099ec231cbd1d67a86b0c637f96",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_2100293_20000_13006",
        id: "icad-2100293-20000-13006-r2",
        revision: 2,
        sha256: "52f7f986db1d2851aaf5c4a9938a5ade7a3c3daaa008cc8811247556a059f31f",
        extra_types: &[45, 56, 59, 60, 124, 125, 126, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_2100311_20000_13006",
        id: "icad-2100311-20000-13006-r1",
        revision: 1,
        sha256: "09ac64c00702f713b5ef2eaccffe003ce6e073a1b2694dd4184178cbd9399dff",
        extra_types: &[45, 56, 59, 124, 125, 126, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_2401260_20000_13006",
        id: "icad-2401260-20000-13006-r1",
        revision: 1,
        sha256: "d89bae93d6a526d92c022933d332b88b9c7083561b0e7cecec79c7a94e24a149",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_2601246_26105_13006",
        id: "icad-2601246-26105-13006-r1",
        revision: 1,
        sha256: "bc85f903ff495ab14dcce7e0528e28ad0f23197d60493e68f2b525ea321c024a",
        extra_types: &[45, 56, 60, 124, 125, 126, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_2800188_28002_13006",
        id: "icad-2800188-28002-13006-r1",
        revision: 1,
        sha256: "83f469624fd9e0a636850444e728bd65b7599669e74fa6816972c9a0f19a0976",
        extra_types: &[],
    },
    ProfileSpec {
        key: "SCH_2901199_28101_13006",
        id: "icad-2901199-28101-13006-r1",
        revision: 1,
        sha256: "1d834f8db3223aa77e005f9ac394d29c0c4d4d018d74dc4098293ae783031627",
        extra_types: &[45, 56, 59, 68, 124, 125, 126, 127, 128, 134, 135, 136],
    },
    ProfileSpec {
        key: "SCH_3200152_32001_13006",
        id: "icad-3200152-32001-13006-r1",
        revision: 1,
        sha256: "b6d919827de77377bc8a96581f25c1654bf257e5d0f3c126ba1090850b623413",
        extra_types: &[45, 68, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_3200252_32001_13006",
        id: "icad-3200252-32001-13006-r1",
        revision: 1,
        sha256: "141ddd0f43c9a5c8bac32395171352899adca081f7b17fb77f9fd625e2495172",
        extra_types: &[45, 56, 59, 68, 124, 125, 126, 127, 128, 134, 135, 136, 137],
    },
    ProfileSpec {
        key: "SCH_3301231_33103_13006",
        id: "icad-3301231-33103-13006-r2",
        revision: 2,
        sha256: "cc98a11221746512e9429ae85dd6c354b2c557fb7fe3959c7d13151220936baf",
        extra_types: &[45, 56, 59, 60, 68, 124, 125, 126, 127, 128, 134, 135, 136, 137],
    },
];

impl ProfileSpec {
    pub(crate) fn definitions(&self) -> Vec<TypeDefinition> {
        base_definitions(self.extra_types)
    }
}

/// The reviewed 13006 subset, TORUS (54) and the listed separately reviewed types.
pub(super) fn base_definitions(extra_types: &[u16]) -> Vec<TypeDefinition> {
    let mut definitions = super::sch13006_definitions();
    definitions.extend(super::onshape_current_definitions().into_iter().filter(|d| d.node_type == 54));
    definitions.extend(
        super::sch13006_sp_curve_definitions()
            .into_iter()
            .chain(super::sch13006_bspline_surface_definitions())
            .chain(surface_definitions())
            .filter(|d| extra_types.contains(&d.node_type)),
    );
    definitions
}

/// Construct the reviewed iCAD embedded profiles, one per exact key.
///
/// Each profile claims one exact key and its separately qualified base subset.
/// `SPUN_SURF` (68) is retained as explicitly unsupported B-Rep geometry.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_legacy_13006() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new_embedded(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: spec.revision,
                    provider_schema: "13006".into(),
                    producer_scope: "Legacy iCAD extracted Parasolid streams".into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                spec.definitions(),
                vec![],
                vec![204],
            )
        })
        .collect()
}

/// Public XT reference (April 2008), printed pp. 62-66 and 74-76.
/// Only the observed keys admit these 13006 declarations.
fn surface_definitions() -> Vec<TypeDefinition> {
    use FieldType::{Character as C, Double as F, Integer as D, Logical as L, PointerIndex as P, ShortInteger as N, Vector as V};
    let field = |name: &str, field_type, pointer_class, element_count| FieldDefinition {
        name: name.into(),
        field_type,
        pointer_class,
        element_count,
        transmitted: true,
    };
    let common = || {
        vec![
            field("local_id", D, 0, 0),
            field("annotations", P, 1019, 0),
            field("owner_ref", P, 1007, 0),
            field("next_surface", P, 1006, 0),
            field("previous_surface", P, 1006, 0),
            field("indirect_owner", P, 141, 0),
            field("orientation", C, 0, 0),
        ]
    };
    let mut blend = common();
    blend.extend([
        field("blend_kind", C, 0, 0),
        field("support_surfaces", P, 1006, 2),
        field("spine_curve", P, 1008, 0),
        field("offsets", F, 0, 2),
        field("weights", F, 0, 2),
        field("boundary_surfaces", P, 1006, 2),
        field("start_limit", P, 41, 0),
        field("end_limit", P, 41, 0),
    ]);
    let mut boundary = common();
    boundary.extend([field("boundary_index", N, 0, 0), field("blend_surface", P, 1006, 0)]);
    let mut offset = common();
    offset.extend([
        field("check_state", C, 0, 0),
        field("true_offset", L, 0, 0),
        field("basis_surface", P, 1006, 0),
        field("offset", F, 0, 0),
        field("scale", F, 0, 0),
    ]);
    let mut spun = common();
    spun.extend([
        field("profile_curve", P, 1008, 0),
        field("axis_origin", V, 0, 0),
        field("axis_direction", V, 0, 0),
        field("start_point", V, 0, 0),
        field("end_point", V, 0, 0),
        field("start_parameter", F, 0, 0),
        field("end_parameter", F, 0, 0),
        field("x_direction", V, 0, 0),
        field("scale", F, 0, 0),
    ]);
    [(56, blend), (59, boundary), (60, offset), (68, spun)]
        .into_iter()
        .map(|(kind, fields)| {
            TypeDefinition::from_fields(kind, format!("sch13006_type_{kind}"), "Reviewed legacy iCAD surface", fields, SchemaSource::Base)
        })
        .collect()
}
