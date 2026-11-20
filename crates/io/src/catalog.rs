//! Portable material libraries; no network, external resources or design links.
//! This file is the single-file envelope; the entry rules both envelopes share are in [`entry`].
use crate::Result;
use serde::{Deserialize, Serialize};
use solidcraft_doc::Document;
use std::collections::BTreeMap;

mod entry;
pub use entry::*;

pub const FORMAT: &str = "solidcraft-library/2";
pub const V1_FORMAT: &str = "solidcraft-library/1";
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1024;
/// Bound source inspection even when many assignments reuse the same definition.
pub const MAX_SOURCE_REFERENCES: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format: String,
    pub id: String,
    pub name: String,
    #[serde(deserialize_with = "bounded_entries")]
    pub entries: Vec<Entry>,
}

fn bounded_entries<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<Vec<Entry>, D::Error> {
    struct Entries;
    impl<'de> serde::de::Visitor<'de> for Entries {
        type Value = Vec<Entry>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most 1024 material library entries")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            loop {
                if entries.len() == MAX_ENTRIES {
                    if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                        return Err(serde::de::Error::custom("material library exceeds 1024 entries"));
                    }
                    break;
                }
                let Some(entry) = sequence.next_element::<Entry>()? else { break };
                // The revision is checked once, by `Catalog::validate` after decoding.
                if entry.id.len() > 129 {
                    return Err(serde::de::Error::custom("invalid material library identity"));
                }
                entries.try_reserve(1).map_err(|_| serde::de::Error::custom("could not reserve material library entries"))?;
                entries.push(entry);
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_seq(Entries)
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.format.as_str(), FORMAT | V1_FORMAT) || !identifier(&self.id) || !text(&self.name, 128) || self.entries.len() > MAX_ENTRIES
        {
            return Err(invalid("unsupported or invalid library (v1/v2, 1024 entries, 128 byte name)"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &self.entries {
            if self.format == V1_FORMAT && entry.definition.requires_v2() {
                return Err(invalid("catalog v1 cannot preserve PBR or engineering cards; export refused"));
            }
            entry.validate(&self.id)?;
            if !ids.insert(&entry.id) {
                return Err(invalid("duplicate library entry identity"));
            }
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err(invalid("material library exceeds 4 MiB"));
        }
        // Keep the v1 wire schema strict, including newly recognized null fields.
        // This bounded raw preflight is not arbitrary JSON canonicalization.
        let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| invalid(format!("invalid library: {e}")))?;
        if raw.get("format").and_then(serde_json::Value::as_str) == Some(V1_FORMAT)
            && let Some(entries) = raw.get("entries").and_then(serde_json::Value::as_array)
        {
            if entries.len() > MAX_ENTRIES {
                return Err(invalid("library exceeds 1024 entries"));
            }
            for entry in entries {
                if let Some(value) = entry.get("definition").and_then(|d| d.get("value")) {
                    let has = |v: &serde_json::Value, key: &str| v.as_object().is_some_and(|o| o.contains_key(key));
                    if has(value, "pbr") || has(value, "physical_card") || value.get("appearance").is_some_and(|a| has(a, "pbr")) {
                        return Err(invalid("v1 definitions cannot contain v2 fields"));
                    }
                }
            }
        }
        drop(raw);
        let catalog: Self = serde_json::from_slice(bytes).map_err(|e| invalid(format!("invalid material library: {e}")))?;
        catalog.validate()?;
        Ok(catalog)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut budget = 4096usize;
        for entry in &self.entries {
            budget = budget.saturating_add(entry_encoded_bound(entry)?);
            if budget > MAX_BYTES {
                return Err(invalid("library exceeds 4 MiB encoded admission budget"));
            }
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("material library exceeds 4 MiB"));
        }
        Ok(bytes)
    }
    /// Change only the envelope version; reject any lossy downgrade.
    pub fn in_format(mut self, format: &str) -> Result<Self> {
        if !matches!(format, FORMAT | V1_FORMAT) {
            return Err(invalid("unsupported library format"));
        }
        self.format = format.into();
        self.validate()?;
        Ok(self)
    }
    pub fn read(path: &str) -> Result<Self> {
        // Check the VFS size first as well: its browser read clones the stored bytes.
        if crate::vfs::len(path).map_err(|e| invalid(e.to_string()))? > MAX_BYTES as u64 {
            return Err(invalid("material library exceeds 4 MiB"));
        }
        Self::decode(&crate::vfs::read(path, MAX_BYTES).map_err(|e| invalid(e.to_string()))?)
    }
    pub fn from_document(doc: &Document, id: &str, name: &str, provenance: Provenance) -> Result<Self> {
        if !identifier(id) || !text(name, 128) {
            return Err(invalid("invalid material library identifier or name"));
        }
        provenance.validate()?;
        let references = doc
            .appearances
            .bodies
            .len()
            .saturating_add(doc.appearances.components.len())
            .saturating_add(doc.appearances.faces.len())
            .saturating_add(doc.user_materials.len().saturating_mul(2));
        if references > MAX_SOURCE_REFERENCES {
            return Err(invalid("library export exceeds 100000 source references"));
        }
        let mut entries = BTreeMap::new();
        let mut budget = 4096usize;
        let mut insert = |definition| -> Result<()> {
            let entry = Entry::new(id, definition, provenance.clone())?;
            if let Some(previous) = entries.get(&entry.id) {
                if previous != &entry {
                    return Err(invalid("library content identifier collision"));
                }
            } else {
                if entries.len() >= MAX_ENTRIES {
                    return Err(invalid("material library exceeds 1024 unique entries"));
                }
                budget = budget.saturating_add(entry_encoded_bound(&entry)?);
                if budget > MAX_BYTES {
                    return Err(invalid("library exceeds 4 MiB encoded admission budget"));
                }
                budget = budget.saturating_add(entry_encoded_bound(&entry)?);
                if budget > MAX_BYTES {
                    return Err(invalid("library exceeds 4 MiB encoded admission budget"));
                }
                entries.insert(entry.id.clone(), entry);
            }
            Ok(())
        };
        for look in doc.appearances.bodies.values().chain(doc.appearances.components.values()).chain(doc.appearances.faces.iter().map(|f| &f.look)) {
            insert(Definition::Appearance(LibraryLook::from_look(look)?))?;
        }
        for material in &doc.user_materials {
            insert(Definition::Material(LibraryMaterial::from_material(material)?))?;
            if let Some(look) = &material.appearance {
                insert(Definition::Appearance(LibraryLook::from_look(look)?))?;
            }
        }
        let out = Self { format: FORMAT.into(), id: id.into(), name: name.into(), entries: entries.into_values().collect() };
        out.validate()?;
        Ok(out)
    }
}

/// Bound retained unique definitions and the final pretty writer before a full
/// catalog allocation. Each admitted entry is bounded independently; reserve
/// envelope text, four spaces per nested line, separators and a final newline.
fn entry_encoded_bound(entry: &Entry) -> Result<usize> {
    let bytes = serde_json::to_vec_pretty(entry).map_err(|e| invalid(e.to_string()))?;
    let lines = bytes.iter().filter(|b| **b == b'\n').count().saturating_add(1);
    Ok(bytes.len().saturating_add(lines.saturating_mul(4)).saturating_add(2))
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "catalog_v2_tests.rs"]
mod v2_tests;
