//! Validated project-owned built-in schema profiles and exact-key selection.

use std::collections::{BTreeMap, BTreeSet};

use crate::{ErrorDetails, ErrorKind, ParseError};

use super::{FieldType, SchemaKey, SchemaProvider, SchemaProviderProvenance, SchemaSource, SchemaTypeLookup, TypeDefinition, WireDecodeClass};

const MAX_PROFILE_ID_BYTES: usize = 128;
const MAX_PRODUCER_SCOPE_BYTES: usize = 128;
const MAX_FIELDS_PER_TYPE: usize = 4_096;

/// Declared completeness of one project-owned built-in profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinProfileCoverage {
    /// Only node types proven by the recorded evidence are supplied.
    VerifiedSubset,
}

impl BuiltinProfileCoverage {
    /// Return the stable value used by bindings and reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedSubset => "verified_subset",
        }
    }
}

/// Reviewed provenance declared by one compiled built-in profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinProfileMetadata {
    /// Stable project-owned identifier, for example `onshape-sch30000-r1`.
    pub profile_id: String,
    /// Monotonic profile revision, beginning at one.
    pub revision: u32,
    /// Numeric provider-schema component used for type lookup.
    pub provider_schema: String,
    /// Producer scope supported by the evidence, for example `Onshape`.
    pub producer_scope: String,
    /// Whether the profile is complete or covers only reviewed types.
    pub coverage: BuiltinProfileCoverage,
    /// Optional digest of the evidence manifest used during promotion.
    pub evidence_manifest_sha256: Option<String>,
    /// Digest of the reviewed canonical profile source.
    pub profile_sha256: String,
}

/// One validated built-in profile, independent of caller-owned schema catalogs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSchemaProfile {
    metadata: BuiltinProfileMetadata,
    accepted_schema_keys: Vec<SchemaKey>,
    definitions: BTreeMap<u16, TypeDefinition>,
    unsupported_base_types: BTreeSet<u16>,
    absent_base_types: BTreeSet<u16>,
}

impl BuiltinSchemaProfile {
    /// Validate and construct one built-in profile.
    ///
    /// The supplied SHA-256 values are promotion metadata produced by developer
    /// tooling. The core validates their representation and retains them; it does
    /// not read a source file or contact an external service to recompute them.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for invalid metadata, duplicate
    /// definitions or keys, embedded-base keys, or unsafe field layouts.
    pub fn new(metadata: BuiltinProfileMetadata, accepted_schema_keys: Vec<String>, definitions: Vec<TypeDefinition>) -> Result<Self, ParseError> {
        validate_metadata(&metadata)?;
        let accepted_schema_keys = validate_schema_keys(&metadata, accepted_schema_keys, false)?;
        let definitions = validate_definitions(definitions)?;
        Ok(Self { metadata, accepted_schema_keys, definitions, unsupported_base_types: BTreeSet::new(), absent_base_types: BTreeSet::new() })
    }

    /// Construct a partial base profile for explicitly allowlisted embedded keys.
    ///
    /// Missing definitions are unknown unless listed as present-but-unsupported
    /// or confirmed absent. Only confirmed absence permits a full embedded
    /// declaration. This constructor does not register the profile for default
    /// parsing or establish B-Rep roles for its effective definitions.
    ///
    /// # Errors
    ///
    /// Rejects non-embedded keys, invalid definitions, duplicate declarations,
    /// and contradictory base-type membership.
    pub fn new_embedded(
        metadata: BuiltinProfileMetadata,
        accepted_schema_keys: Vec<String>,
        definitions: Vec<TypeDefinition>,
        unsupported_base_types: Vec<u16>,
        absent_base_types: Vec<u16>,
    ) -> Result<Self, ParseError> {
        validate_metadata(&metadata)?;
        let accepted_schema_keys = validate_schema_keys(&metadata, accepted_schema_keys, true)?;
        let definitions = validate_definitions(definitions)?;
        let mut declared: BTreeSet<_> = definitions.keys().copied().collect();
        for node_type in unsupported_base_types.iter().chain(&absent_base_types) {
            if *node_type <= 1 || !declared.insert(*node_type) {
                return Err(invalid_profile(
                    "base_type_membership",
                    node_type.to_string(),
                    "base membership must be unique, non-contradictory, and greater than one",
                ));
            }
        }
        Ok(Self {
            metadata,
            accepted_schema_keys,
            definitions,
            unsupported_base_types: unsupported_base_types.into_iter().collect(),
            absent_base_types: absent_base_types.into_iter().collect(),
        })
    }

    /// Return reviewed profile metadata.
    #[must_use]
    pub const fn metadata(&self) -> &BuiltinProfileMetadata {
        &self.metadata
    }

    /// Iterate over exact accepted transmit schema keys in lexical order.
    #[must_use]
    pub fn accepted_schema_keys(&self) -> impl ExactSizeIterator<Item = &SchemaKey> {
        self.accepted_schema_keys.iter()
    }

    /// Iterate over reviewed definitions in ascending node-type order.
    #[must_use]
    pub fn definitions(&self) -> impl ExactSizeIterator<Item = &TypeDefinition> {
        self.definitions.values()
    }

    /// Return one reviewed definition.
    #[must_use]
    pub fn definition(&self, node_type: u16) -> Option<&TypeDefinition> {
        self.definitions.get(&node_type)
    }

    /// Iterate over confirmed base types whose definitions remain unsupported.
    #[must_use]
    pub fn unsupported_base_types(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.unsupported_base_types.iter().copied()
    }

    /// Iterate over node types confirmed absent from the selected base schema.
    #[must_use]
    pub fn absent_base_types(&self) -> impl ExactSizeIterator<Item = u16> + '_ {
        self.absent_base_types.iter().copied()
    }

    /// Return whether the complete key is explicitly allowlisted.
    #[must_use]
    pub fn accepts_schema_key(&self, schema_key: &SchemaKey) -> bool {
        self.accepted_schema_keys.binary_search_by(|candidate| candidate.raw().cmp(schema_key.raw())).is_ok()
    }
}

/// Registry of reviewed profiles indexed only by stable ID and exact schema key.
#[derive(Debug, Clone, Default)]
pub struct BuiltinProfileRegistry {
    profiles: Vec<BuiltinSchemaProfile>,
    profile_indices: BTreeMap<String, usize>,
    schema_key_indices: BTreeMap<String, usize>,
}

impl BuiltinProfileRegistry {
    /// Return the compiled profiles eligible for default exact-key selection.
    ///
    /// `Default` and `new` remain caller-managed registries. This shared registry
    /// contains only profiles validated for runtime use.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for an inconsistent compiled table.
    pub fn compiled() -> Result<&'static Self, ParseError> {
        static REGISTRY: std::sync::OnceLock<Result<BuiltinProfileRegistry, ParseError>> = std::sync::OnceLock::new();
        REGISTRY
            .get_or_init(|| {
                let mut profiles = vec![
                    super::profiles::onshape_sch30000()?,
                    super::profiles::onshape_sch13006()?,
                    super::profiles::icad_sch30000_13006()?,
                    super::profiles::icad_sch34101_13006()?,
                    super::profiles::solidworks_sch37102_13006()?,
                    super::profiles::solidworks_sch35102_13006()?,
                    super::profiles::onshape_sch37102_13006()?,
                ];
                profiles.extend(super::profiles::icad_legacy_13006()?);
                profiles.extend(super::profiles::icad_legacy_standard()?);
                Self::new(profiles)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Validate and index all supplied profiles.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` when an ID or exact schema key is
    /// claimed by more than one profile.
    pub fn new(profiles: Vec<BuiltinSchemaProfile>) -> Result<Self, ParseError> {
        let mut registry = Self::default();
        for profile in profiles {
            registry.insert(profile)?;
        }
        Ok(registry)
    }

    /// Insert one validated profile without replacing an existing claim.
    ///
    /// # Errors
    ///
    /// Returns `schema.invalid_builtin_profile` for duplicate IDs or keys.
    pub fn insert(&mut self, profile: BuiltinSchemaProfile) -> Result<(), ParseError> {
        let profile_id = &profile.metadata.profile_id;
        if self.profile_indices.contains_key(profile_id) {
            return Err(invalid_profile("profile_id", profile_id, "built-in profile identifier is already registered"));
        }
        for schema_key in &profile.accepted_schema_keys {
            if self.schema_key_indices.contains_key(schema_key.raw()) {
                return Err(invalid_profile("schema_key", schema_key.raw(), "complete schema key is already claimed by another built-in profile"));
            }
        }

        let index = self.profiles.len();
        self.profile_indices.insert(profile_id.clone(), index);
        for schema_key in &profile.accepted_schema_keys {
            self.schema_key_indices.insert(schema_key.raw().to_owned(), index);
        }
        self.profiles.push(profile);
        Ok(())
    }

    /// Return the number of registered profiles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Return whether no real profile has been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Iterate over profiles in insertion order.
    #[must_use]
    pub fn profiles(&self) -> impl ExactSizeIterator<Item = &BuiltinSchemaProfile> {
        self.profiles.iter()
    }

    /// Return one profile by stable ID.
    #[must_use]
    pub fn profile(&self, profile_id: &str) -> Option<&BuiltinSchemaProfile> {
        self.profile_indices.get(profile_id).and_then(|index| self.profiles.get(*index))
    }

    /// Return the profile which explicitly accepts the complete schema key.
    #[must_use]
    pub fn profile_for_key(&self, schema_key: &SchemaKey) -> Option<&BuiltinSchemaProfile> {
        self.schema_key_indices.get(schema_key.raw()).and_then(|index| self.profiles.get(*index))
    }

    /// Return an exact-key provider for one allowlisted schema key.
    #[must_use]
    pub fn provider_for_key(&self, schema_key: &SchemaKey) -> Option<BuiltinSchemaProvider<'_>> {
        let profile = self.profile_for_key(schema_key)?;
        let selected_schema_key = profile.accepted_schema_keys.iter().find(|candidate| candidate.raw() == schema_key.raw())?.raw();
        Some(BuiltinSchemaProvider { profile, selected_schema_key })
    }
}

/// Borrowed provider selected from one exact built-in profile allowlist entry.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinSchemaProvider<'a> {
    profile: &'a BuiltinSchemaProfile,
    selected_schema_key: &'a str,
}

impl BuiltinSchemaProvider<'_> {
    /// Return the selected built-in profile.
    #[must_use]
    pub const fn profile(&self) -> &BuiltinSchemaProfile {
        self.profile
    }

    /// Return the exact complete key selected from the profile allowlist.
    #[must_use]
    pub const fn selected_schema_key(&self) -> &str {
        self.selected_schema_key
    }
}

impl SchemaProvider for BuiltinSchemaProvider<'_> {
    fn contains_schema(&self, schema: &str) -> bool {
        schema == self.profile.metadata.provider_schema
    }

    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition> {
        (schema == self.profile.metadata.provider_schema).then(|| self.profile.definition(node_type)).flatten()
    }

    fn lookup_type(&self, schema: &str, node_type: u16) -> SchemaTypeLookup<'_> {
        if !self.contains_schema(schema) {
            return SchemaTypeLookup::Unknown;
        }
        if let Some(definition) = self.profile.definition(node_type) {
            SchemaTypeLookup::Defined(definition)
        } else if self.profile.unsupported_base_types.contains(&node_type) {
            SchemaTypeLookup::PresentUnsupported
        } else if self.profile.absent_base_types.contains(&node_type) {
            SchemaTypeLookup::Absent
        } else {
            SchemaTypeLookup::Unknown
        }
    }

    fn supports_schema_key(&self, schema_key: &SchemaKey) -> bool {
        schema_key.raw() == self.selected_schema_key
            && self.profile.accepts_schema_key(schema_key)
            && self.contains_schema(schema_key.provider_schema())
    }

    fn provenance(&self) -> SchemaProviderProvenance<'_> {
        SchemaProviderProvenance::Builtin {
            profile_id: &self.profile.metadata.profile_id,
            profile_revision: self.profile.metadata.revision,
            schema_key: self.selected_schema_key,
            coverage: self.profile.metadata.coverage,
            profile_sha256: &self.profile.metadata.profile_sha256,
        }
    }
}

fn validate_metadata(metadata: &BuiltinProfileMetadata) -> Result<(), ParseError> {
    if metadata.profile_id.is_empty()
        || metadata.profile_id.len() > MAX_PROFILE_ID_BYTES
        || !metadata.profile_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid_profile("profile_id", &metadata.profile_id, "profile identifier must use 1-128 lowercase ASCII identifier bytes"));
    }
    if metadata.revision == 0 {
        return Err(invalid_profile("profile_revision", metadata.revision.to_string(), "profile revision must be greater than zero"));
    }
    if metadata.provider_schema.is_empty() || !metadata.provider_schema.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_profile("provider_schema", &metadata.provider_schema, "provider schema must contain only ASCII digits"));
    }
    if metadata.producer_scope.is_empty()
        || metadata.producer_scope.len() > MAX_PRODUCER_SCOPE_BYTES
        || !metadata.producer_scope.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' ')
    {
        return Err(invalid_profile("producer_scope", &metadata.producer_scope, "producer scope must use 1-128 printable ASCII bytes"));
    }
    validate_sha256("profile_sha256", &metadata.profile_sha256)?;
    if let Some(digest) = &metadata.evidence_manifest_sha256 {
        validate_sha256("evidence_manifest_sha256", digest)?;
    }
    Ok(())
}

fn validate_schema_keys(metadata: &BuiltinProfileMetadata, accepted_schema_keys: Vec<String>, embedded: bool) -> Result<Vec<SchemaKey>, ParseError> {
    if accepted_schema_keys.is_empty() {
        return Err(invalid_profile("schema_key", "", "built-in profile must accept at least one exact schema key"));
    }
    let mut parsed = Vec::with_capacity(accepted_schema_keys.len());
    for raw in accepted_schema_keys {
        let schema_key =
            SchemaKey::parse(&raw).map_err(|_| invalid_profile("schema_key", &raw, "built-in profile contains an invalid schema key"))?;
        if schema_key.base().is_some() != embedded {
            return Err(invalid_profile("schema_key", &raw, "schema key must match the constructor's standard or embedded mode"));
        }
        if schema_key.provider_schema() != metadata.provider_schema {
            return Err(invalid_profile("schema_key", &raw, "schema key provider component does not match profile metadata"));
        }
        parsed.push(schema_key);
    }
    parsed.sort_by(|left, right| left.raw().cmp(right.raw()));
    if parsed.windows(2).any(|pair| pair[0].raw() == pair[1].raw()) {
        return Err(invalid_profile(
            "schema_key",
            parsed.windows(2).find(|pair| pair[0].raw() == pair[1].raw()).map_or("", |pair| pair[0].raw()),
            "built-in profile contains a duplicate exact schema key",
        ));
    }
    Ok(parsed)
}

fn validate_definitions(definitions: Vec<TypeDefinition>) -> Result<BTreeMap<u16, TypeDefinition>, ParseError> {
    if definitions.is_empty() {
        return Err(invalid_profile("definitions", "0", "built-in profile must contain at least one reviewed type definition"));
    }
    let mut validated = BTreeMap::new();
    for definition in definitions {
        validate_definition(&definition)?;
        let node_type = definition.node_type;
        if validated.insert(node_type, definition).is_some() {
            return Err(invalid_profile("node_type", node_type.to_string(), "built-in profile contains a duplicate node type"));
        }
    }
    Ok(validated)
}

fn validate_definition(definition: &TypeDefinition) -> Result<(), ParseError> {
    if definition.node_type <= 1 {
        return Err(invalid_profile("node_type", definition.node_type.to_string(), "built-in node type must be greater than one"));
    }
    if definition.name.is_empty() {
        return Err(invalid_profile("node_name", "", "built-in node definition must have a project-owned name"));
    }
    if definition.source != SchemaSource::Base {
        return Err(invalid_profile("schema_source", definition.source.as_str(), "built-in definitions must enter the parser as base definitions"));
    }
    if definition.fields.len() > MAX_FIELDS_PER_TYPE {
        return Err(invalid_profile("fields_per_type", definition.fields.len().to_string(), "built-in definition exceeds the field-count bound"));
    }

    let variable_positions =
        definition.fields.iter().enumerate().filter_map(|(index, field)| (field.element_count == 1).then_some(index)).collect::<Vec<_>>();
    if variable_positions.len() > 1
        || variable_positions.first().is_some_and(|position| *position + 1 != definition.fields.len())
        || definition.variable == variable_positions.is_empty()
    {
        return Err(invalid_profile(
            "variable_field",
            definition.node_type.to_string(),
            "variable field must be the sole final field and match the type flag",
        ));
    }

    let mut field_names = BTreeSet::new();
    for (index, field) in definition.fields.iter().enumerate() {
        if field.name.is_empty() {
            return Err(invalid_profile("field_name", index.to_string(), "built-in field must have a project-owned name"));
        }
        if !field_names.insert(field.name.as_str()) {
            return Err(invalid_profile("field_name", &field.name, "built-in definition contains a duplicate field name"));
        }
        if field.pointer_class != 0 && field.field_type != FieldType::PointerIndex {
            return Err(invalid_profile("pointer_class", field.pointer_class.to_string(), "non-zero pointer class requires the pointer-index codec"));
        }
        if !field.transmitted && field.element_count != 1 {
            return Err(invalid_profile("field_transmitted", index.to_string(), "only a retained variable field may be non-transmitted"));
        }
        if field.transmitted && WireDecodeClass::from_field_type(field.field_type).is_none() {
            return Err(invalid_profile("field_type", field.field_type.code(), "opaque pointers may not be transmitted by a built-in profile"));
        }
    }
    Ok(())
}

fn validate_sha256(field: &'static str, value: &str) -> Result<(), ParseError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err(invalid_profile(field, value, "digest must be 64 lowercase hexadecimal SHA-256 characters"));
    }
    Ok(())
}

fn invalid_profile(field: &'static str, value: impl Into<String>, message: &'static str) -> ParseError {
    ParseError::new(ErrorKind::InvalidBuiltinProfile, 0, message, ErrorDetails::InvalidText { field, value: value.into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentLimits, ErrorDetails, ErrorKind, FieldDefinition, FieldValue, SchemaProviderResolution, parse_xb, parse_xt, write_xb};

    const SCHEMA_KEY: &str = "SCH_1000000_90000";
    const NEAR_SCHEMA_KEY: &str = "SCH_1000001_90000";

    fn metadata(profile_id: &str) -> BuiltinProfileMetadata {
        BuiltinProfileMetadata {
            profile_id: profile_id.to_owned(),
            revision: 1,
            provider_schema: "90000".to_owned(),
            producer_scope: "Synthetic tests".to_owned(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: "0".repeat(64),
        }
    }

    fn definition(node_type: u16) -> TypeDefinition {
        TypeDefinition::from_fields(
            node_type,
            format!("PROJECT_TYPE_{node_type}"),
            "Project-owned synthetic definition",
            vec![FieldDefinition {
                name: "field_0".to_owned(),
                field_type: FieldType::Integer,
                pointer_class: 0,
                element_count: 0,
                transmitted: true,
            }],
            SchemaSource::Base,
        )
    }

    fn profile_with(profile_id: &str, schema_key: &str, node_type: u16) -> Result<BuiltinSchemaProfile, ParseError> {
        BuiltinSchemaProfile::new(metadata(profile_id), vec![schema_key.to_owned()], vec![definition(node_type)])
    }

    fn registry() -> Result<BuiltinProfileRegistry, ParseError> {
        let profile = profile_with("synthetic-sch90000-r1", SCHEMA_KEY, 42)?;
        BuiltinProfileRegistry::new(vec![profile])
    }

    fn xt_payload(schema_key: &str, node_type: u16, value: i32) -> Vec<u8> {
        let modeller = ": TRANSMIT FILE created by modeller version 1000000";
        format!("T{} {}{} {}0 {node_type} 1 {value} 1 0 ", modeller.len(), modeller, schema_key.len(), schema_key,).into_bytes()
    }

    fn xb_payload(schema_key: &str, node_type: u16, value: i32) -> Vec<u8> {
        let modeller = b": TRANSMIT FILE created by modeller version 1000000";
        let mut output = b"PS\0\0".to_vec();
        output.extend_from_slice(&u16::try_from(modeller.len()).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(modeller);
        output.extend_from_slice(&i32::try_from(schema_key.len()).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(schema_key.as_bytes());
        output.extend_from_slice(&0_i32.to_be_bytes());
        output.extend_from_slice(&i16::try_from(node_type).unwrap_or_default().to_be_bytes());
        output.extend_from_slice(&2_i16.to_be_bytes());
        output.extend_from_slice(&value.to_be_bytes());
        output.extend_from_slice(&1_i16.to_be_bytes());
        output.extend_from_slice(&1_i16.to_be_bytes());
        output
    }

    #[test]
    fn indexes_profiles_by_stable_id_and_exact_key() {
        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        if let Ok(registry) = registry {
            assert_eq!(registry.len(), 1);
            assert!(registry.profile("synthetic-sch90000-r1").is_some());

            let exact = SchemaKey::parse(SCHEMA_KEY);
            assert!(exact.is_ok(), "{exact:?}");
            if let Ok(exact) = exact {
                let provider = registry.provider_for_key(&exact);
                assert!(provider.is_some());
                if let Some(provider) = provider {
                    assert_eq!(provider.selected_schema_key(), SCHEMA_KEY);
                    assert_eq!(provider.profile().metadata().coverage.as_str(), "verified_subset");
                    assert!(provider.supports_schema_key(&exact));
                    assert!(provider.type_definition("90000", 42).is_some());
                }
            }

            let near = SchemaKey::parse(NEAR_SCHEMA_KEY);
            assert!(near.is_ok(), "{near:?}");
            if let Ok(near) = near {
                assert!(registry.profile_for_key(&near).is_none());
                assert!(registry.provider_for_key(&near).is_none());
            }
        }
    }

    #[test]
    fn parses_synthetic_text_and_binary_with_one_builtin_profile() {
        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        let schema_key = SchemaKey::parse(SCHEMA_KEY);
        assert!(schema_key.is_ok(), "{schema_key:?}");
        if let (Ok(registry), Ok(schema_key)) = (registry, schema_key) {
            let provider = registry.provider_for_key(&schema_key);
            assert!(provider.is_some());
            if let Some(provider) = provider {
                let text_bytes = xt_payload(SCHEMA_KEY, 42, 7);
                let binary_bytes = xb_payload(SCHEMA_KEY, 42, 7);
                let text = parse_xt(&text_bytes, &provider, DocumentLimits::default());
                let binary = parse_xb(&binary_bytes, &provider, DocumentLimits::default());
                assert!(text.is_ok(), "{text:?}");
                assert!(binary.is_ok(), "{binary:?}");
                if let (Ok(text), Ok(binary)) = (text, binary) {
                    assert!(matches!(
                        &text.schema_provider,
                        SchemaProviderResolution::Builtin {
                            profile_id,
                            profile_revision: 1,
                            schema_key,
                            coverage: BuiltinProfileCoverage::VerifiedSubset,
                            profile_sha256,
                        } if profile_id == "synthetic-sch90000-r1"
                            && schema_key == SCHEMA_KEY
                            && profile_sha256 == &"0".repeat(64)
                    ));
                    assert_eq!(text.schema_provider, binary.schema_provider);
                    assert_eq!(text.nodes.len(), 1);
                    assert_eq!(binary.nodes.len(), 1);
                    assert_eq!(text.nodes[0].node_type, binary.nodes[0].node_type);
                    assert_eq!(text.nodes[0].index, binary.nodes[0].index);
                    assert_eq!(text.nodes[0].fields[0].values, [FieldValue::Integer(Some(7))]);
                    assert_eq!(binary.nodes[0].fields[0].values, [FieldValue::Integer(Some(7))]);
                    assert_eq!(write_xb(&binary), binary_bytes);
                }
            }
        }
    }

    #[test]
    fn rejects_near_keys_and_uncovered_types_without_guessing() {
        let registry = registry();
        let schema_key = SchemaKey::parse(SCHEMA_KEY);
        assert!(registry.is_ok(), "{registry:?}");
        assert!(schema_key.is_ok(), "{schema_key:?}");
        if let (Ok(registry), Ok(schema_key)) = (registry, schema_key) {
            let provider = registry.provider_for_key(&schema_key);
            assert!(provider.is_some());
            if let Some(provider) = provider {
                for near_error in [
                    parse_xt(&xt_payload(NEAR_SCHEMA_KEY, 42, 7), &provider, DocumentLimits::default()).err(),
                    parse_xb(&xb_payload(NEAR_SCHEMA_KEY, 42, 7), &provider, DocumentLimits::default()).err(),
                ] {
                    assert_eq!(near_error.as_ref().map(ParseError::kind), Some(ErrorKind::UnsupportedBuiltinSchemaKey));
                    assert!(matches!(
                        near_error.as_ref().map(ParseError::details),
                        Some(ErrorDetails::BuiltinProfileLookup {
                            profile_id,
                            profile_revision: 1,
                            selected_schema_key,
                            requested_schema_key,
                            node_type: 0,
                        }) if profile_id == "synthetic-sch90000-r1"
                            && selected_schema_key == SCHEMA_KEY
                            && requested_schema_key == NEAR_SCHEMA_KEY
                    ));
                }

                for error in [
                    parse_xt(&xt_payload(SCHEMA_KEY, 43, 7), &provider, DocumentLimits::default()).err(),
                    parse_xb(&xb_payload(SCHEMA_KEY, 43, 7), &provider, DocumentLimits::default()).err(),
                ] {
                    assert_eq!(error.as_ref().map(ParseError::kind), Some(ErrorKind::BuiltinProfileUncoveredType));
                    assert!(matches!(
                        error.as_ref().map(ParseError::details),
                        Some(ErrorDetails::BuiltinProfileLookup {
                            profile_id,
                            profile_revision: 1,
                            selected_schema_key,
                            requested_schema_key,
                            node_type: 43,
                        }) if profile_id == "synthetic-sch90000-r1"
                            && selected_schema_key == SCHEMA_KEY
                            && requested_schema_key == SCHEMA_KEY
                    ));
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_profiles_and_registry_collisions() {
        let mut invalid_metadata = metadata("synthetic-sch90000-r1");
        invalid_metadata.profile_sha256 = "not-a-digest".to_owned();
        let invalid_digest = BuiltinSchemaProfile::new(invalid_metadata, vec![SCHEMA_KEY.to_owned()], vec![definition(42)]);
        assert_eq!(invalid_digest.as_ref().err().map(ParseError::kind), Some(ErrorKind::InvalidBuiltinProfile));

        let embedded_key =
            BuiltinSchemaProfile::new(metadata("synthetic-sch90000-r1"), vec!["SCH_1000000_90001_90000".to_owned()], vec![definition(42)]);
        assert_eq!(embedded_key.as_ref().err().map(ParseError::kind), Some(ErrorKind::InvalidBuiltinProfile));

        let duplicate_type =
            BuiltinSchemaProfile::new(metadata("synthetic-sch90000-r1"), vec![SCHEMA_KEY.to_owned()], vec![definition(42), definition(42)]);
        assert_eq!(duplicate_type.as_ref().err().map(ParseError::kind), Some(ErrorKind::InvalidBuiltinProfile));

        let mut repeated_field = definition(42);
        repeated_field.fields.push(repeated_field.fields[0].clone());
        let duplicate_field = BuiltinSchemaProfile::new(metadata("synthetic-sch90000-r1"), vec![SCHEMA_KEY.to_owned()], vec![repeated_field]);
        assert_eq!(duplicate_field.as_ref().err().map(ParseError::kind), Some(ErrorKind::InvalidBuiltinProfile));

        let registry = registry();
        assert!(registry.is_ok(), "{registry:?}");
        if let Ok(mut registry) = registry {
            let duplicate_id = profile_with("synthetic-sch90000-r1", NEAR_SCHEMA_KEY, 43);
            assert!(duplicate_id.is_ok(), "{duplicate_id:?}");
            if let Ok(duplicate_id) = duplicate_id {
                assert_eq!(registry.insert(duplicate_id).err().map(|error| error.kind()), Some(ErrorKind::InvalidBuiltinProfile));
            }

            let duplicate_key = profile_with("synthetic-sch90000-r2", SCHEMA_KEY, 43);
            assert!(duplicate_key.is_ok(), "{duplicate_key:?}");
            if let Ok(duplicate_key) = duplicate_key {
                assert_eq!(registry.insert(duplicate_key).err().map(|error| error.kind()), Some(ErrorKind::InvalidBuiltinProfile));
            }
            assert_eq!(registry.len(), 1);
        }
    }
}
