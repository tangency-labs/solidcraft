//! Document-local effective schema registry.

use std::collections::BTreeMap;

use crate::{ErrorDetails, ErrorKind, ParseError};

use super::{
    SchemaCoverageReport, SchemaKey, SchemaLimits, SchemaProvider, SchemaResolution, SchemaSource, decode_embedded_schema, embedded_base_definition,
    missing_type_definition_error, unavailable_schema_error,
};

/// Effective definitions resolved at the first occurrence of each node type.
#[derive(Debug, Clone)]
pub struct EffectiveSchemaRegistry {
    entries: BTreeMap<u16, SchemaResolution>,
    max_schema_types: usize,
}

impl EffectiveSchemaRegistry {
    /// Create an empty registry with an explicit retained-type bound.
    #[must_use]
    pub const fn new(max_schema_types: usize) -> Self {
        Self { entries: BTreeMap::new(), max_schema_types }
    }

    /// Return one resolved definition.
    #[must_use]
    pub fn get(&self, node_type: u16) -> Option<&SchemaResolution> {
        self.entries.get(&node_type)
    }

    /// Return the number of resolved types.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether no type has been resolved.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over resolved definitions in ascending node-type order.
    pub fn resolutions(&self) -> impl Iterator<Item = &SchemaResolution> {
        self.entries.values()
    }

    /// Insert one independently decoded resolution.
    ///
    /// # Errors
    ///
    /// Returns an invalid-limit, limit-exceeded, or duplicate-type error.
    pub fn insert(&mut self, resolution: SchemaResolution) -> Result<(), ParseError> {
        if self.max_schema_types == 0 {
            return Err(ParseError::invalid_limit("max_schema_types", 0));
        }
        let node_type = resolution.definition.node_type;
        if self.entries.contains_key(&node_type) {
            return Err(ParseError::new(
                ErrorKind::DuplicateSchemaType,
                resolution.byte_range.start,
                "node type already has an effective schema definition",
                ErrorDetails::NodeType { node_type },
            ));
        }
        let new_count = self.entries.len().saturating_add(1);
        if new_count > self.max_schema_types {
            return Err(ParseError::limit(resolution.byte_range.start, "schema_types", new_count, self.max_schema_types));
        }
        self.entries.insert(node_type, resolution);
        Ok(())
    }

    /// Resolve and register the first occurrence of one node type.
    ///
    /// Standard schema keys consume no file bytes and require a provider type.
    /// Embedded-base keys consume a full, delta, or unchanged marker blob.
    ///
    /// # Errors
    ///
    /// Returns a structured lookup, schema decoding, duplicate, or limit error.
    pub fn resolve_first<P: SchemaProvider>(
        &mut self,
        data: &[u8],
        offset: usize,
        node_type: u16,
        schema_key: &SchemaKey,
        provider: &P,
        limits: SchemaLimits,
    ) -> Result<usize, ParseError> {
        let provider_schema = schema_key.provider_schema();
        if !provider.supports_schema_key(schema_key) {
            return Err(unavailable_schema_error(provider, schema_key, node_type, offset));
        }

        let resolution = if schema_key.base().is_some() {
            decode_embedded_schema(data, offset, node_type, embedded_base_definition(provider, schema_key, node_type, offset)?, limits)?
        } else {
            let definition = provider
                .type_definition(provider_schema, node_type)
                .ok_or_else(|| missing_type_definition_error(provider, schema_key, node_type, offset))?;
            if definition.fields.len() > limits.max_fields_per_type {
                return Err(ParseError::limit(offset, "schema_fields_per_type", definition.fields.len(), limits.max_fields_per_type));
            }
            let mut definition = definition.clone();
            definition.source = SchemaSource::Base;
            SchemaResolution { definition, raw_schema: Vec::new(), byte_range: offset..offset, edits: Vec::new() }
        };
        let consumed = resolution.consumed();
        self.insert(resolution)?;
        Ok(consumed)
    }

    /// Summarize resolved type and field provenance.
    #[must_use]
    pub fn coverage(&self) -> SchemaCoverageReport {
        SchemaCoverageReport::from_resolutions(self.entries.values())
    }
}

impl Default for EffectiveSchemaRegistry {
    fn default() -> Self {
        Self::new(SchemaLimits::default().max_schema_types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FieldDefinition, FieldType, InMemorySchemaProvider, TypeDefinition};

    fn base_definition(node_type: u16) -> TypeDefinition {
        TypeDefinition::from_fields(
            node_type,
            "BASE_TYPE",
            "Base type",
            vec![FieldDefinition { name: "value".to_owned(), field_type: FieldType::Integer, pointer_class: 0, element_count: 0, transmitted: true }],
            SchemaSource::Base,
        )
    }

    #[test]
    fn resolves_standard_schema_without_consuming_node_bytes() {
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("36001", base_definition(12));
        let key = SchemaKey::parse("SCH_3700000_36001");
        assert!(key.is_ok());
        let mut registry = EffectiveSchemaRegistry::default();

        if let Ok(key) = key {
            assert_eq!(registry.resolve_first(&[0, 12], 0, 12, &key, &provider, SchemaLimits::default()), Ok(0));
        }
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.coverage().base_count, 1);
    }

    #[test]
    fn resolves_embedded_unchanged_and_full_types_from_loaded_base() {
        let mut provider = InMemorySchemaProvider::new();
        provider.insert("13006", base_definition(12));
        let key = SchemaKey::parse("SCH_3000310_30000_13006");
        assert!(key.is_ok());
        let mut registry = EffectiveSchemaRegistry::default();

        if let Ok(key) = key {
            assert_eq!(registry.resolve_first(&[0xff], 0, 12, &key, &provider, SchemaLimits::default()), Ok(1));

            let mut full = vec![0, 3];
            full.extend_from_slice(b"NEW");
            full.push(3);
            full.extend_from_slice(b"New");
            assert_eq!(registry.resolve_first(&full, 0, 204, &key, &provider, SchemaLimits::default()), Ok(full.len()));
        }
        let report = registry.coverage();
        assert_eq!(report.node_types, [12, 204]);
        assert_eq!(report.unchanged_count, 1);
        assert_eq!(report.full_count, 1);
    }

    #[test]
    fn distinguishes_missing_catalog_type_and_duplicate_registration() {
        let provider = InMemorySchemaProvider::new();
        let standard = SchemaKey::parse("SCH_3700000_36001");
        assert!(standard.is_ok());
        let mut registry = EffectiveSchemaRegistry::default();
        if let Ok(standard) = standard {
            let missing_catalog = registry.resolve_first(&[], 0, 12, &standard, &provider, SchemaLimits::default()).err();
            assert_eq!(missing_catalog.as_ref().map(ParseError::kind), Some(ErrorKind::MissingBaseSchema));
        }

        let mut provider = InMemorySchemaProvider::new();
        provider.add_schema("36001");
        let standard = SchemaKey::parse("SCH_3700000_36001");
        if let Ok(standard) = standard {
            let missing_type = registry.resolve_first(&[], 0, 12, &standard, &provider, SchemaLimits::default()).err();
            assert_eq!(missing_type.as_ref().map(ParseError::kind), Some(ErrorKind::MissingSchemaType));
        }

        provider.insert("36001", base_definition(12));
        let standard = SchemaKey::parse("SCH_3700000_36001");
        if let Ok(standard) = standard {
            assert!(registry.resolve_first(&[], 0, 12, &standard, &provider, SchemaLimits::default()).is_ok());
            let duplicate = registry.resolve_first(&[], 0, 12, &standard, &provider, SchemaLimits::default()).err();
            assert_eq!(duplicate.as_ref().map(ParseError::kind), Some(ErrorKind::DuplicateSchemaType));
        }
    }
}
