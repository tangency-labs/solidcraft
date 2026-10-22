//! A project is a portable folder, independent of its repository connection.
//! CAD entries are folders containing document.json and manifest.json; ordinary documents
//! (for example docs/README.md) remain ordinary Git files. Assets are shared within a project.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, FORMAT, MAX_MANIFEST_BYTES, Result};

pub const PROJECT_MARKER: &str = "solidcraft-project.json";

/// Paths must have the same meaning on Unix, Windows, in Git trees, and in browser storage.
pub fn check_path(path: &str) -> Result<()> {
    if path.len() > 512 || path.chars().any(|c| c.is_control() || "\\:*?<>|\"".contains(c)) {
        return Err(Error::Invalid("invalid project path".into()));
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or_default().to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"]
                .iter()
                .any(|prefix| stem.strip_prefix(prefix).is_some_and(|s| s.len() == 1 && matches!(s.as_bytes().first(), Some(b'1'..=b'9'))));
        if part.is_empty() || part.len() > 128 || part == "." || part.eq_ignore_ascii_case(".git") || part.ends_with(['.', ' ']) || device {
            return Err(Error::Invalid("invalid project path component".into()));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub required: Vec<String>,
    pub name: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Project {
    pub fn new(name: String) -> Result<Self> {
        let project = Self { format: FORMAT.into(), required: vec!["projects/1".into()], name, extra: BTreeMap::new() };
        project.write()?;
        Ok(project)
    }

    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(Error::Invalid("project marker too large".into()));
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
        crate::snapshot::check_envelope(&value, &["projects/1"])?;
        let project: Self = serde_json::from_value(value).map_err(|e| Error::Invalid(e.to_string()))?;
        if project.name.trim().is_empty() || project.name.len() > 256 || project.name.chars().any(char::is_control) {
            return Err(Error::Invalid("choose a project name of at most 256 bytes".into()));
        }
        Ok(project)
    }

    pub fn write(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| Error::Invalid(e.to_string()))?;
        Self::read(&bytes)?;
        Ok(bytes)
    }
}

/// An editor binding and every queued save carry the full destination; a global default
/// affects newly created designs only. Reconnecting another repo never redirects a save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub connection: String,
    pub project: String,
    pub file: String,
}

impl Destination {
    pub fn validate(&self) -> Result<()> {
        if self.connection.is_empty()
            || self.connection.len() > 128
            || !self.connection.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(Error::Invalid("invalid repository connection".into()));
        }
        check_path(&self.project)?;
        check_path(&self.file)?;
        if self.file.split('/').any(|s| matches!(s, "objects" | PROJECT_MARKER | "manifest.json" | "document.json")) {
            return Err(Error::Invalid("reserved project path".into()));
        }
        Ok(())
    }

    pub fn design_path(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("{}/{}", self.project, self.file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folders_are_portable_and_keep_optional_metadata() {
        for path in ["../outside", "/absolute", "C:/absolute", "a/.git/config", "a\\b", "a//b", "a/NUL.txt", "a/end.", "a/end ", "a\n"] {
            assert!(check_path(path).is_err(), "{path}");
        }
        for path in ["projects/My robot", "parts/bracket", "assemblies/drive", "docs/README.md"] {
            check_path(path).unwrap();
        }
        let mut project = Project::new("My robot".into()).unwrap();
        project.extra.insert("description".into(), Value::String("Editable project".into()));
        assert_eq!(Project::read(&project.write().unwrap()).unwrap(), project);
        assert!(matches!(Project::read(br#"{"format":"solidcraft-cloud/2"}"#), Err(Error::UpgradeRequired(_))));
        assert!(matches!(Project::read(br#"{"format":"solidcraft-cloud/1","required":["projects/2"]}"#), Err(Error::UpgradeRequired(_))));
    }
}
