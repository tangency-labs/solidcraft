//! Base-schema provider boundary.

use std::collections::BTreeMap;

use crate::{ErrorDetails, ErrorKind, ParseError};

use super::{BuiltinProfileCoverage, SchemaKey, TypeDefinition};

/// Origin metadata retained by a schema provider without changing field provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaProviderProvenance<'a> {
    /// Definitions were supplied by a caller-managed provider.
    CallerSupplied,
    /// Definitions came from one exact compiled built-in profile.
    Builtin {
        /// Stable project-owned profile identifier.
        profile_id: &'a str,
        /// Monotonic profile revision.
        profile_revision: u32,
        /// Exact schema key selected from the profile allowlist.
        schema_key: &'a str,
        /// Reviewed completeness claim for the selected profile.
        coverage: BuiltinProfileCoverage,
        /// Declared SHA-256 of the reviewed profile source.
        profile_sha256: &'a str,
    },
}

/// Owned provider provenance retained by a successfully parsed document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SchemaProviderResolution {
    /// Definitions were supplied by the caller-selected provider path.
    CallerSupplied,
    /// Definitions came from one exact compiled built-in profile.
    Builtin {
        /// Stable project-owned profile identifier.
        profile_id: String,
        /// Monotonic profile revision.
        profile_revision: u32,
        /// Exact schema key selected from the profile allowlist.
        schema_key: String,
        /// Reviewed completeness claim for the selected profile.
        coverage: BuiltinProfileCoverage,
        /// Declared SHA-256 of the reviewed profile source.
        profile_sha256: String,
    },
}

impl SchemaProviderResolution {
    /// Return the stable provider-kind name used by bindings and reports.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::CallerSupplied => "caller_supplied",
            Self::Builtin { .. } => "builtin",
        }
    }
}

impl From<SchemaProviderProvenance<'_>> for SchemaProviderResolution {
    fn from(value: SchemaProviderProvenance<'_>) -> Self {
        match value {
            SchemaProviderProvenance::CallerSupplied => Self::CallerSupplied,
            SchemaProviderProvenance::Builtin { profile_id, profile_revision, schema_key, coverage, profile_sha256 } => Self::Builtin {
                profile_id: profile_id.to_owned(),
                profile_revision,
                schema_key: schema_key.to_owned(),
                coverage,
                profile_sha256: profile_sha256.to_owned(),
            },
        }
    }
}

/// Knowledge of one node type in an exact standard/base schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaTypeLookup<'a> {
    /// The type exists and its complete base definition is available.
    Defined(&'a TypeDefinition),
    /// The type exists, but its base definition has not been established.
    PresentUnsupported,
    /// The type is confirmed absent from this base schema.
    Absent,
    /// Whether the type exists in this base schema is not known.
    Unknown,
}

/// Supplies exact standard/base definitions without prescribing storage.
pub trait SchemaProvider {
    /// Return whether the named provider schema is loaded.
    fn contains_schema(&self, schema: &str) -> bool;

    /// Return one type from a loaded schema, or `None` when that type is absent.
    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition>;

    /// Distinguish unavailable definitions from confirmed base-type absence.
    ///
    /// The default preserves the complete-catalog contract of existing providers.
    /// Partial providers must override this method: only `Absent` authorizes a
    /// complete embedded definition; `Unknown` and `PresentUnsupported` cannot
    /// be interpreted as an empty base.
    fn lookup_type(&self, schema: &str, node_type: u16) -> SchemaTypeLookup<'_> {
        match self.type_definition(schema, node_type) {
            Some(definition) => SchemaTypeLookup::Defined(definition),
            None if self.contains_schema(schema) => SchemaTypeLookup::Absent,
            None => SchemaTypeLookup::Unknown,
        }
    }

    /// Return whether this provider may be used for the complete transmit schema key.
    ///
    /// Caller-managed catalogs are selected by their provider-schema component. A
    /// built-in provider overrides this method to require an exact allowlisted key.
    fn supports_schema_key(&self, schema_key: &SchemaKey) -> bool {
        self.contains_schema(schema_key.provider_schema())
    }

    /// Return provider origin metadata independently of per-type `SchemaSource`.
    fn provenance(&self) -> SchemaProviderProvenance<'_> {
        SchemaProviderProvenance::CallerSupplied
    }
}

/// Resolve base membership before either embedded decoder consumes any bytes.
pub(crate) fn embedded_base_definition<'a, P: SchemaProvider>(
    provider: &'a P,
    schema_key: &SchemaKey,
    node_type: u16,
    offset: usize,
) -> Result<Option<&'a TypeDefinition>, ParseError> {
    let (kind, message) = match provider.lookup_type(schema_key.provider_schema(), node_type) {
        SchemaTypeLookup::Defined(definition) => return Ok(Some(definition)),
        SchemaTypeLookup::Absent => return Ok(None),
        SchemaTypeLookup::PresentUnsupported => {
            (ErrorKind::UnsupportedBaseSchemaType, "embedded schema requires a known base type whose definition is unavailable")
        }
        SchemaTypeLookup::Unknown => (ErrorKind::UnknownBaseSchemaType, "embedded schema cannot be decoded without confirmed base-type membership"),
    };
    Err(ParseError::new(kind, offset, message, ErrorDetails::SchemaLookup { schema: schema_key.provider_schema().to_owned(), node_type }))
}

/// Validate the current built-in scope even for streams with no data records.
/// User-field layouts have not yet been verified for compiled profiles.
pub(crate) fn validate_builtin_input<P: SchemaProvider>(
    provider: &P,
    schema_key: &SchemaKey,
    user_field_size: u8,
    offset: usize,
) -> Result<(), ParseError> {
    if matches!(provider.provenance(), SchemaProviderProvenance::Builtin { .. }) {
        if !provider.supports_schema_key(schema_key) {
            // No node has been read at this header-time check.
            return Err(unavailable_schema_error(provider, schema_key, 0, offset));
        }
        if user_field_size != 0 {
            return Err(ParseError::new(
                ErrorKind::UnsupportedUserFields,
                offset,
                "built-in profiles currently require zero user fields",
                ErrorDetails::InvalidLength { field: "user_field_size", value: i64::from(user_field_size) },
            ));
        }
    }
    Ok(())
}

pub(crate) fn unavailable_schema_error<P: SchemaProvider>(provider: &P, schema_key: &SchemaKey, node_type: u16, offset: usize) -> ParseError {
    match provider.provenance() {
        SchemaProviderProvenance::CallerSupplied => ParseError::new(
            ErrorKind::MissingBaseSchema,
            offset,
            "required schema catalog is not loaded",
            ErrorDetails::SchemaLookup { schema: schema_key.provider_schema().to_owned(), node_type },
        ),
        SchemaProviderProvenance::Builtin { profile_id, profile_revision, schema_key: selected_schema_key, .. } => ParseError::new(
            ErrorKind::UnsupportedBuiltinSchemaKey,
            offset,
            "built-in profile does not allow the requested complete schema key",
            ErrorDetails::BuiltinProfileLookup {
                profile_id: profile_id.to_owned(),
                profile_revision,
                selected_schema_key: selected_schema_key.to_owned(),
                requested_schema_key: schema_key.raw().to_owned(),
                node_type,
            },
        ),
    }
}

pub(crate) fn missing_type_definition_error<P: SchemaProvider>(provider: &P, schema_key: &SchemaKey, node_type: u16, offset: usize) -> ParseError {
    match provider.provenance() {
        SchemaProviderProvenance::CallerSupplied => ParseError::new(
            ErrorKind::MissingSchemaType,
            offset,
            "standard schema does not define the requested node type",
            ErrorDetails::SchemaLookup { schema: schema_key.provider_schema().to_owned(), node_type },
        ),
        SchemaProviderProvenance::Builtin { profile_id, profile_revision, schema_key: selected_schema_key, .. } => ParseError::new(
            ErrorKind::BuiltinProfileUncoveredType,
            offset,
            "selected built-in profile has no reviewed definition for the node type",
            ErrorDetails::BuiltinProfileLookup {
                profile_id: profile_id.to_owned(),
                profile_revision,
                selected_schema_key: selected_schema_key.to_owned(),
                requested_schema_key: schema_key.raw().to_owned(),
                node_type,
            },
        ),
    }
}

/// Deterministic provider used by callers, tests, and future file loaders.
#[derive(Debug, Clone, Default)]
pub struct InMemorySchemaProvider {
    schemas: BTreeMap<String, BTreeMap<u16, TypeDefinition>>,
}

impl InMemorySchemaProvider {
    /// Create an empty provider.
    #[must_use]
    pub const fn new() -> Self {
        Self { schemas: BTreeMap::new() }
    }

    /// Mark a schema catalog as loaded, even when it contains no definitions.
    pub fn add_schema(&mut self, schema: impl Into<String>) {
        self.schemas.entry(schema.into()).or_default();
    }

    /// Insert or replace one definition in a loaded schema catalog.
    pub fn insert(&mut self, schema: impl Into<String>, definition: TypeDefinition) -> Option<TypeDefinition> {
        self.schemas.entry(schema.into()).or_default().insert(definition.node_type, definition)
    }
}

impl SchemaProvider for InMemorySchemaProvider {
    fn contains_schema(&self, schema: &str) -> bool {
        self.schemas.contains_key(schema)
    }

    fn type_definition(&self, schema: &str, node_type: u16) -> Option<&TypeDefinition> {
        self.schemas.get(schema).and_then(|definitions| definitions.get(&node_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::SchemaSource;

    #[test]
    fn distinguishes_unloaded_schema_from_absent_type() {
        let mut provider = InMemorySchemaProvider::new();
        assert!(!provider.contains_schema("13006"));

        provider.add_schema("13006");
        assert!(provider.contains_schema("13006"));
        assert!(provider.type_definition("13006", 12).is_none());

        provider.insert("13006", TypeDefinition::from_fields(12, "BODY", "Body", Vec::new(), SchemaSource::Base));
        assert_eq!(provider.type_definition("13006", 12).map(|definition| definition.name.as_str()), Some("BODY"));
    }
}
