//! Portable material libraries; no network, external resources or design links.
use crate::{IoError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcraft_doc::{Document, UserMaterial, appearance::Look, valid_density};
use std::collections::BTreeMap;

pub const FORMAT: &str = "solidcraft-library/2";
pub const V1_FORMAT: &str = "solidcraft-library/1";
pub const MAX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 1024;
/// Bound source inspection even when many assignments reuse the same definition.
pub const MAX_SOURCE_REFERENCES: usize = 100_000;
fn invalid(message: impl Into<String>) -> IoError {
    IoError::Invalid(message.into())
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value == value.trim() && value.len() <= max && !value.chars().any(char::is_control)
}
fn identifier(value: &str) -> bool {
    value.len() <= 64
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Information only: never fetched or treated as a local resource path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}
impl Default for Provenance {
    fn default() -> Self {
        Self { license: "LicenseRef-User-Provided".into(), author: None, source_url: None }
    }
}
impl Provenance {
    fn validate(&self) -> Result<()> {
        if !text(&self.license, 128)
            || self.author.as_ref().is_some_and(|s| !text(s, 256))
            || self.source_url.as_ref().is_some_and(|s| !text(s, 2048))
        {
            return Err(invalid("invalid library provenance (license 128, author 256, source URL 2048 bytes)"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryLook {
    pub name: String,
    pub color: [u8; 3],
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbr: Option<solidcraft_doc::pbr_appearance::PbrParameters>,
}
impl LibraryLook {
    pub fn from_look(look: &Look) -> Result<Self> {
        let out = Self { name: look.name.clone(), color: look.color, opacity: look.opacity, pbr: look.pbr };
        out.validate()?;
        Ok(out)
    }
    pub fn to_look(&self) -> Look {
        Look { name: self.name.clone(), color: self.color, opacity: self.opacity, pbr: self.pbr }
    }
    fn validate(&self) -> Result<()> {
        if !text(&self.name, 128) || !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err(invalid("library appearance needs a 1–128 byte name and finite opacity 0–1"));
        }
        self.to_look().opaque_surface().map_err(|e| invalid(e.to_string()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryMaterial {
    pub name: String,
    pub density_g_cm3: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<LibraryLook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical_card: Option<solidcraft_doc::physical_material::ValidatedCard>,
}
impl LibraryMaterial {
    pub fn from_material(material: &UserMaterial) -> Result<Self> {
        Ok(Self {
            name: material.name.clone(),
            density_g_cm3: material.density,
            appearance: material.appearance.as_ref().map(LibraryLook::from_look).transpose()?,
            physical_card: material.physical_card.clone(),
        })
    }
    pub fn to_material(&self, name: String) -> UserMaterial {
        UserMaterial {
            name,
            density: self.density_g_cm3,
            appearance: self.appearance.as_ref().map(LibraryLook::to_look),
            physical_card: self.physical_card.clone(),
        }
    }
    fn validate(&self) -> Result<()> {
        if !text(&self.name, 128) || !valid_density(self.density_g_cm3) {
            return Err(invalid("invalid library material name or density"));
        }
        if let Some(look) = &self.appearance {
            look.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case", deny_unknown_fields)]
pub enum Definition {
    Appearance(LibraryLook),
    Material(LibraryMaterial),
}
impl Definition {
    fn requires_v2(&self) -> bool {
        match self {
            Self::Appearance(look) => look.pbr.is_some(),
            Self::Material(material) => material.physical_card.is_some() || material.appearance.as_ref().is_some_and(|l| l.pbr.is_some()),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Appearance(l) => &l.name,
            Self::Material(m) => &m.name,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Appearance(l) => l.validate(),
            Self::Material(m) => m.validate(),
        }
    }
    fn kind(&self) -> &str {
        match self {
            Self::Appearance(_) => "appearance",
            Self::Material(_) => "material",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub revision: String,
    pub provenance: Provenance,
    pub definition: Definition,
}
impl Entry {
    /// SHA-256 of compact serde JSON `[provenance, definition]`. Absent v2 fields
    /// are omitted, preserving the exact v1 scalar writer contract.
    pub fn content_revision(&self) -> Result<String> {
        self.provenance.validate()?;
        self.definition.validate()?;
        let bytes = serde_json::to_vec(&(&self.provenance, &self.definition)).map_err(|e| invalid(e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
    pub fn new(pack_id: &str, definition: Definition, provenance: Provenance) -> Result<Self> {
        if !identifier(pack_id) {
            return Err(invalid("invalid library identifier"));
        }
        let mut out = Self { id: String::new(), revision: String::new(), provenance, definition };
        out.revision = out.content_revision()?;
        let prefix = out.revision.get(..16).ok_or_else(|| invalid("invalid library revision"))?;
        out.id = format!("{pack_id}/{}-{prefix}", out.definition.kind());
        Ok(out)
    }
    fn validate(&self, pack_id: &str) -> Result<()> {
        let (pack, local) = self.id.split_once('/').ok_or_else(|| invalid("entry ID must be qualified by its library ID"))?;
        if pack != pack_id || !identifier(local) || self.revision != self.content_revision()? {
            return Err(invalid("invalid entry identity or content revision"));
        }
        Ok(())
    }
}

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
                let revision = entry.content_revision().map_err(serde::de::Error::custom)?;
                if entry.revision != revision || entry.id.len() > 129 {
                    return Err(serde::de::Error::custom("invalid material library revision or identity"));
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
