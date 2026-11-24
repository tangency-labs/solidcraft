//! A library entry and its rules: what it holds, its identity and its revision. The single-file
//! catalog and the library folder (`crate::library`) both read and write entries through these.
use crate::{IoError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solidcraft_doc::{UserMaterial, appearance::Look, valid_density};

pub(crate) fn invalid(message: impl Into<String>) -> IoError {
    IoError::Invalid(message.into())
}
pub(crate) fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && value == value.trim() && value.len() <= max && !value.chars().any(char::is_control)
}
pub(crate) fn identifier(value: &str) -> bool {
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
    pub(crate) fn validate(&self) -> Result<()> {
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
        Look { name: self.name.clone(), color: self.color, opacity: self.opacity, pbr: self.pbr, grain: None }
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
    pub(crate) fn requires_v2(&self) -> bool {
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
    pub(crate) fn kind(&self) -> &str {
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
    /// An entry under an id the library chose: the id stays when the content changes, only the
    /// revision follows it. Only the library module allocates ids (`crate::library`).
    pub(crate) fn with_id(pack_id: &str, local_id: &str, definition: Definition, provenance: Provenance) -> Result<Self> {
        if !identifier(pack_id) || !identifier(local_id) {
            return Err(invalid("invalid library or entry identifier"));
        }
        let mut out = Self { id: format!("{pack_id}/{local_id}"), revision: String::new(), provenance, definition };
        out.revision = out.content_revision()?;
        Ok(out)
    }
    pub(crate) fn validate(&self, pack_id: &str) -> Result<()> {
        let (pack, local) = self.id.split_once('/').ok_or_else(|| invalid("entry ID must be qualified by its library ID"))?;
        if pack != pack_id || !identifier(local) || self.revision != self.content_revision()? {
            return Err(invalid("invalid entry identity or content revision"));
        }
        Ok(())
    }
}
