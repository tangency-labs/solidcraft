//! Exact non-embedded keys from older iCAD releases.
//!
//! A standard key transmits no schema, so each profile declares complete
//! layouts for the node types seen under that key. They are the reviewed 13006
//! declarations with the field edits that iCAD streams of the same schema
//! revision carry in their embedded schema; the two 13006 keys use them
//! unchanged. No embedded 28002 stream contains LIMIT (41); it uses the edit
//! carried from 28101 on, which the 28002 inputs decode with. External catalogs
//! are used only for subsequent comparison.

use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, ErrorDetails, ErrorKind, FieldDefinition, FieldType, ParseError,
    SchemaSource, TypeDefinition,
};

/// One step of an observed embedded edit sequence, applied to a base layout.
#[derive(Clone, Copy)]
enum Edit {
    Keep(usize),
    Drop,
    Insert(&'static str, FieldType, u16),
}

use Edit::{Drop, Keep};

const fn pointer(name: &'static str, class: u16) -> Edit {
    Edit::Insert(name, FieldType::PointerIndex, class)
}

const fn integer(name: &'static str) -> Edit {
    Edit::Insert(name, FieldType::Integer, 0)
}

// Embedded 19008 streams carry the same BODY edit as 20000.
const BODY_20000: &[Edit] =
    &[Keep(23), integer("index_origin"), pointer("index_values", 82), pointer("node_id_values", 82), pointer("schema_values", 82)];

// Embedded 26105 streams carry the same BODY edit as 28002.
const BODY_28002: &[Edit] = &[
    Keep(13),
    Drop,
    pointer("container_ref", 1040),
    Keep(9),
    pointer("boundary_mesh", 1006),
    integer("index_origin"),
    pointer("index_values", 82),
    pointer("node_id_values", 82),
    pointer("schema_values", 82),
    pointer("child_body", 12),
    integer("min_local_id"),
];

const BODY_28101: &[Edit] = &[
    Keep(6),
    pointer("construction_mesh", 1006),
    pointer("construction_polyline", 1008),
    Keep(7),
    Drop,
    pointer("container_ref", 1040),
    Keep(6),
    pointer("boundary_mesh", 1006),
    pointer("boundary_polyline", 1008),
    Keep(3),
    integer("index_origin"),
    pointer("index_values", 82),
    pointer("node_id_values", 82),
    pointer("schema_values", 82),
    pointer("child_body", 12),
    integer("min_local_id"),
];

const BODY_32001: &[Edit] = &[
    Keep(6),
    pointer("construction_mesh", 1006),
    pointer("construction_polyline", 1008),
    Keep(7),
    Drop,
    pointer("container_ref", 1040),
    Keep(6),
    pointer("boundary_mesh", 1006),
    pointer("boundary_polyline", 1008),
    Keep(3),
    integer("index_origin"),
    pointer("index_values", 82),
    pointer("node_id_values", 82),
    pointer("schema_values", 82),
    pointer("child_body", 12),
    integer("min_local_id"),
    pointer("mesh_offset_ref", 206),
];

const BODY_33103: &[Edit] = &[
    Keep(3),
    pointer("construction_lattice", 222),
    Keep(3),
    pointer("construction_mesh", 1006),
    pointer("construction_polyline", 1008),
    Keep(7),
    Drop,
    pointer("container_ref", 1040),
    Keep(6),
    pointer("boundary_mesh", 1006),
    pointer("boundary_polyline", 1008),
    Keep(3),
    integer("index_origin"),
    pointer("index_values", 82),
    pointer("node_id_values", 82),
    pointer("schema_values", 82),
    pointer("child_body", 12),
    integer("min_local_id"),
    pointer("mesh_offset_ref", 206),
];

const REGION: &[Edit] = &[Keep(7), pointer("owner_ref", 12)];

const INTERSECTION: &[Edit] = &[Keep(11), pointer("intersection_data", 204)];

const LIMIT: &[Edit] = &[Keep(1), Edit::Insert("limit_state", FieldType::Character, 0), Keep(1)];

// The last three base fields are dropped where the embedded sequence ends.
const LIST: &[Edit] = &[
    Keep(1),
    Edit::Insert("entry_kind", FieldType::UnsignedByte, 0),
    Edit::Insert("transmission_flag", FieldType::Logical, 0),
    Keep(3),
    Drop,
    Keep(2),
    Drop,
    integer("cursor_index"),
    pointer("cursor_block", 1012),
    Keep(1),
    Drop,
    Drop,
    Drop,
];

const POINTER_BLOCK: &[Edit] = &[Keep(1), integer("index_origin"), Keep(2)];

pub(crate) struct ProfileSpec {
    pub key: &'static str,
    pub id: &'static str,
    pub sha256: &'static str,
    schema: &'static str,
    types: &'static [u16],
    edits: &'static [(u16, &'static [Edit])],
}

pub(crate) const PROFILES: &[ProfileSpec] = &[
    ProfileSpec {
        key: "SCH_1300218_13006",
        id: "icad-1300218-13006-r1",
        sha256: "928962c3f7f5a87013f6af6e45a443d040ff9a967b0887f959366b9bd2774b82",
        schema: "13006",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 50, 51, 70, 74, 79, 80, 81, 82, 141],
        edits: &[],
    },
    ProfileSpec {
        key: "SCH_1302234_13006",
        id: "icad-1302234-13006-r1",
        sha256: "29708f25ceea002ff57f2ebcada3a8bd25ffb6ef1f908ff678b45c3c7abc9f54",
        schema: "13006",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 50, 51, 70, 74, 79, 80, 81, 82],
        edits: &[],
    },
    ProfileSpec {
        key: "SCH_1500000_15003",
        id: "icad-1500000-15003-r1",
        sha256: "d3f7b3909050da2834e34c7f1ccf6b17f2f9cf16048053634dd7fc61506ad313",
        schema: "15003",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 50, 51, 52, 54, 68, 70, 74, 79, 80, 81, 82, 83, 141],
        edits: &[(70, LIST)],
    },
    ProfileSpec {
        key: "SCH_1700000_16100",
        id: "icad-1700000-16100-r1",
        sha256: "e9b55fd29420174f205353b6171c40140d70c067d25e9ea5854b014c468c5580",
        schema: "16100",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 50, 51, 70, 74, 79, 80, 81, 82],
        edits: &[(70, LIST)],
    },
    ProfileSpec {
        key: "SCH_1901000_19008",
        id: "icad-1901000-19008-r1",
        sha256: "c273c545b4b69f176457113d9bb1e158f170a39a4cd8babfbd687e9e0c325ba0",
        schema: "19008",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 50, 51, 54, 70, 74, 79, 80, 81, 82],
        edits: &[(12, BODY_20000), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_2401000_20000",
        id: "icad-2401000-20000-r1",
        sha256: "002dcea4ae6ff5bf1a817744ce4e83c06f58f8955f7a100c011048484a865a04",
        schema: "20000",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 50, 51, 70, 74, 79, 80, 81, 82],
        edits: &[(12, BODY_20000), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_2601000_26105",
        id: "icad-2601000-26105-r1",
        sha256: "f2717a3ccb97d6e836bb36e981671974b0d2fd0a586c9b42e3fcbe77fe36976d",
        schema: "26105",
        types: &[
            12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 56, 60, 70, 74, 79, 80, 81, 82, 83, 124, 125, 126, 127, 128, 133,
            134, 135, 136, 137, 141,
        ],
        edits: &[(12, BODY_28002), (19, REGION), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_2800000_28002",
        id: "icad-2800000-28002-r1",
        sha256: "a1693181a2af88fd8456155296f7ffefca2a71fdf14366a9866ab9c88e6531b7",
        schema: "28002",
        types: &[
            12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 45, 50, 51, 52, 53, 54, 56, 59, 70, 74, 79, 80, 81, 82, 83, 124, 125, 126,
            127, 128, 133, 134, 135, 136, 137, 141,
        ],
        edits: &[(12, BODY_28002), (19, REGION), (41, LIMIT), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_2901000_28101",
        id: "icad-2901000-28101-r1",
        sha256: "3e62cae9a159faf2364f85206fcc2bf1580eb68b4763b446fed1864010df19de",
        schema: "28101",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 50, 51, 52, 70, 74, 79, 80, 81, 82, 141],
        edits: &[(12, BODY_28101), (19, REGION), (41, LIMIT), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_3200000_32001",
        id: "icad-3200000-32001-r1",
        sha256: "5911102568cc1e2215b3bc93e0e43146a382b747394aca8661527557b3220edf",
        schema: "32001",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 32, 38, 40, 41, 50, 51, 52, 53, 70, 74, 79, 80, 81, 82, 83, 84, 133, 141, 204],
        edits: &[(12, BODY_32001), (19, REGION), (38, INTERSECTION), (41, LIMIT), (70, LIST), (74, POINTER_BLOCK)],
    },
    ProfileSpec {
        key: "SCH_3301000_33103",
        id: "icad-3301000-33103-r1",
        sha256: "29c38621bc5d937dddd0550bc52a4f3fcbc71616a5dd8774881b66aa5bcf030f",
        schema: "33103",
        types: &[12, 13, 14, 15, 16, 17, 18, 19, 29, 30, 31, 38, 40, 41, 50, 51, 52, 70, 74, 79, 80, 81, 82, 141, 204],
        edits: &[(12, BODY_33103), (19, REGION), (38, INTERSECTION), (41, LIMIT), (70, LIST), (74, POINTER_BLOCK)],
    },
];

impl ProfileSpec {
    pub(crate) fn definitions(&self) -> Result<Vec<TypeDefinition>, ParseError> {
        let mut pool = super::icad_legacy::base_definitions(self.types);
        pool.push(intersection_data());
        self.types
            .iter()
            .map(|node_type| {
                let base = pool.iter().find(|d| d.node_type == *node_type).ok_or_else(|| invalid(*node_type))?;
                match self.edits.iter().find(|(kind, _)| kind == node_type) {
                    Some((_, edits)) => apply(base, edits),
                    None => Ok(base.clone()),
                }
            })
            .collect()
    }
}

fn invalid(node_type: u16) -> ParseError {
    ParseError::new(
        ErrorKind::InvalidBuiltinProfile,
        0,
        "standard profile layout does not follow its reviewed base definition",
        ErrorDetails::InvalidText { field: "node_type", value: node_type.to_string() },
    )
}

fn apply(base: &TypeDefinition, edits: &[Edit]) -> Result<TypeDefinition, ParseError> {
    let mut source = base.fields.iter();
    let mut fields = Vec::new();
    for edit in edits {
        match *edit {
            Edit::Keep(count) => {
                for _ in 0..count {
                    fields.push(source.next().ok_or_else(|| invalid(base.node_type))?.clone());
                }
            }
            Edit::Drop => {
                source.next().ok_or_else(|| invalid(base.node_type))?;
            }
            Edit::Insert(name, field_type, pointer_class) => {
                fields.push(FieldDefinition { name: name.into(), field_type, pointer_class, element_count: 0, transmitted: true })
            }
        }
    }
    if source.next().is_some() {
        return Err(invalid(base.node_type));
    }
    Ok(TypeDefinition::from_fields(base.node_type, base.name.clone(), base.description.clone(), fields, SchemaSource::Base))
}

/// Embedded 32001 and 33103 streams declare this type in full. No UV
/// semantics are assigned to its values.
fn intersection_data() -> TypeDefinition {
    let field =
        |name: &str, field_type, element_count| FieldDefinition { name: name.into(), field_type, pointer_class: 0, element_count, transmitted: true };
    TypeDefinition::from_fields(
        204,
        "icad_type_204",
        "Reviewed iCAD intersection data",
        vec![field("uv_kind", FieldType::UnsignedByte, 0), field("values", FieldType::Double, 1)],
        SchemaSource::Base,
    )
}

/// Construct the reviewed non-embedded iCAD profiles, one per exact key.
///
/// Each profile claims one exact key and only the node types checked under it.
///
/// # Errors
/// Returns an error if the compiled metadata or definitions are inconsistent.
pub fn icad_legacy_standard() -> Result<Vec<BuiltinSchemaProfile>, ParseError> {
    PROFILES
        .iter()
        .map(|spec| {
            BuiltinSchemaProfile::new(
                BuiltinProfileMetadata {
                    profile_id: spec.id.into(),
                    revision: 1,
                    provider_schema: spec.schema.into(),
                    producer_scope: "Legacy iCAD extracted Parasolid streams".into(),
                    coverage: BuiltinProfileCoverage::VerifiedSubset,
                    evidence_manifest_sha256: None,
                    profile_sha256: spec.sha256.into(),
                },
                vec![spec.key.into()],
                spec.definitions()?,
            )
        })
        .collect()
}
