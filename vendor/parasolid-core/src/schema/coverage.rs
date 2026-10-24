//! Schema-resolution coverage summaries.

use std::collections::BTreeMap;

use super::{SchemaResolution, SchemaSource};

/// Counts definitions by provenance without claiming node-stream coverage.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaCoverageReport {
    /// Sorted resolved node types.
    pub node_types: Vec<u16>,
    /// Total effective fields across all resolved definitions.
    pub field_count: usize,
    /// Definitions read directly from a standard/base schema.
    pub base_count: usize,
    /// Embedded `0xff` markers which reused base definitions.
    pub unchanged_count: usize,
    /// Embedded delta definitions.
    pub delta_count: usize,
    /// Embedded full definitions.
    pub full_count: usize,
}

impl SchemaCoverageReport {
    /// Build a deterministic report from `(node_type, source, field_count)` entries.
    #[must_use]
    pub fn from_entries(entries: &[(u16, SchemaSource, usize)]) -> Self {
        let mut report = Self::default();
        let unique = entries.iter().copied().map(|(node_type, source, field_count)| (node_type, (source, field_count))).collect::<BTreeMap<_, _>>();
        for (node_type, (source, field_count)) in unique {
            report.node_types.push(node_type);
            report.field_count += field_count;
            match source {
                SchemaSource::Base => report.base_count += 1,
                SchemaSource::EmbeddedUnchanged => report.unchanged_count += 1,
                SchemaSource::EmbeddedDelta => report.delta_count += 1,
                SchemaSource::EmbeddedFull => report.full_count += 1,
            }
        }
        report
    }

    /// Build a report from resolved schema objects.
    #[must_use]
    pub fn from_resolutions<'a>(resolutions: impl IntoIterator<Item = &'a SchemaResolution>) -> Self {
        let entries = resolutions
            .into_iter()
            .map(|resolution| (resolution.definition.node_type, resolution.definition.source, resolution.definition.fields.len()))
            .collect::<Vec<_>>();
        Self::from_entries(&entries)
    }

    /// Return the number of unique resolved node types.
    #[must_use]
    pub fn resolved_type_count(&self) -> usize {
        self.node_types.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_sources_and_sorts_unique_types() {
        let report = SchemaCoverageReport::from_entries(&[
            (204, SchemaSource::EmbeddedFull, 2),
            (12, SchemaSource::EmbeddedUnchanged, 8),
            (14, SchemaSource::EmbeddedDelta, 11),
            (29, SchemaSource::Base, 8),
        ]);

        assert_eq!(report.node_types, [12, 14, 29, 204]);
        assert_eq!(report.resolved_type_count(), 4);
        assert_eq!(report.field_count, 29);
        assert_eq!(report.base_count, 1);
        assert_eq!(report.unchanged_count, 1);
        assert_eq!(report.delta_count, 1);
        assert_eq!(report.full_count, 1);
    }
}
