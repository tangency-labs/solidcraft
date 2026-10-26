//! Recoverable design management, carried by the same durable queue as ordinary saves.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Destination, Error, Manifest, PendingSave, Published, Result};

pub const LIFECYCLE: &str = "lifecycle/1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Rename { file: String },
    Delete,
    Restore { commit: String, file: String },
}

impl Change {
    pub fn validate(&self, destination: &Destination) -> Result<()> {
        destination.validate()?;
        match self {
            Self::Rename { file } => {
                let target = Destination { file: file.clone(), ..destination.clone() };
                target.validate()?;
                if file == &destination.file
                    || file.starts_with(&format!("{}/", destination.file))
                    || destination.file.starts_with(&format!("{file}/"))
                {
                    return Err(Error::Invalid("choose a different design path outside the current design folder".into()));
                }
            }
            Self::Restore { commit, file } => {
                check_commit(commit)?;
                Destination { file: file.clone(), ..destination.clone() }.validate()?;
            }
            Self::Delete => {}
        }
        Ok(())
    }

    pub fn destination(&self, source: &Destination) -> Destination {
        match self {
            Self::Rename { file } => Destination { file: file.clone(), ..source.clone() },
            _ => source.clone(),
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Rename { .. } => "Rename",
            Self::Delete => "Move to Trash",
            Self::Restore { .. } => "Restore",
        }
    }
}

pub fn check_commit(commit: &str) -> Result<()> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid("invalid Git commit".into()));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revision {
    pub commit: String,
    pub file: String,
    pub date: String,
    #[serde(default)]
    pub timestamp: i64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    pub revision: Option<String>,
    pub revisions: Vec<Revision>,
    #[serde(default)]
    pub queued: bool,
    #[serde(default)]
    pub deleted: bool,
}

impl Revision {
    pub fn validate(&self, destination: &Destination) -> Result<()> {
        check_commit(&self.commit)?;
        Destination { file: self.file.clone(), ..destination.clone() }.validate()?;
        if self.date.len() > 64 || self.message.len() > 1024 || self.date.chars().chain(self.message.chars()).any(char::is_control) {
            return Err(Error::Invalid("invalid revision description".into()));
        }
        Ok(())
    }
}

/// A confirmed management revision can update only the binding it was captured against.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Managed {
    pub source: Destination,
    pub previous: Option<String>,
    pub revision: String,
    pub change: Change,
    pub destination: Destination,
}

impl Managed {
    pub fn confirmed(save: &PendingSave, published: &Published) -> Result<Option<Self>> {
        let Some(change) = save.manifest.change()? else { return Ok(None) };
        change.validate(&save.destination)?;
        if published.conflict || published.destination != change.destination(&save.destination) || published.revision != save.id {
            return Err(Error::Conflict);
        }
        Ok(Some(Self {
            source: save.destination.clone(),
            previous: save.expected_revision.clone(),
            revision: save.id.clone(),
            change,
            destination: published.destination.clone(),
        }))
    }
}

impl Manifest {
    pub fn change(&self) -> Result<Option<Change>> {
        self.extra.get("lifecycle").map(|value| serde_json::from_value(value.clone()).map_err(|e| Error::Invalid(e.to_string()))).transpose()
    }

    pub fn deleted(&self) -> bool {
        self.extra.get("lifecycle").and_then(|v| v.get("kind")).and_then(Value::as_str) == Some("delete")
    }

    pub fn moved(&self) -> bool {
        self.deleted() && self.extra.contains_key("moved_to")
    }

    pub fn set_change(&mut self, change: &Change) -> Result<()> {
        if !self.required.iter().any(|c| c == LIFECYCLE) {
            self.required.push(LIFECYCLE.into());
        }
        self.extra.insert("lifecycle".into(), serde_json::to_value(change).map_err(|e| Error::Invalid(e.to_string()))?);
        self.extra.remove("moved_to");
        Ok(())
    }

    /// Previous paths remain within this project; bound traversal and reject cycles.
    pub fn history_paths(&self, destination: &Destination) -> Result<Vec<String>> {
        let mut paths = vec![destination.file.clone()];
        if let Some(value) = self.extra.get("previous_paths") {
            let old = value.as_array().ok_or_else(|| Error::Invalid("invalid design history paths".into()))?;
            if old.len() > 16 {
                return Err(Error::Invalid("too many design history paths".into()));
            }
            for file in old {
                let file = file.as_str().ok_or_else(|| Error::Invalid("invalid design history path".into()))?;
                Destination { file: file.into(), ..destination.clone() }.validate()?;
                if !paths.iter().any(|p| p == file) {
                    paths.push(file.into());
                }
            }
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;
    #[test]
    fn lifecycle_requires_an_explicit_capability_and_rejects_hostile_renames() {
        let mut snapshot = Snapshot::prepare(b"{}".to_vec(), Default::default()).unwrap();
        snapshot.manifest.set_change(&Change::Delete).unwrap();
        assert!(Manifest::read(&snapshot.manifest.write().unwrap()).unwrap().deleted());
        snapshot.manifest.required.retain(|c| c != LIFECYCLE);
        assert!(snapshot.manifest.write().is_err());
        let source = Destination { connection: "github".into(), project: "Workshop".into(), file: "parts/bracket".into() };
        for file in ["../escape", "parts/bracket", "parts/bracket/child", "parts"] {
            assert!(Change::Rename { file: file.into() }.validate(&source).is_err());
        }
        assert!(Change::Rename { file: "parts/renamed bracket".into() }.validate(&source).is_ok());
    }
}
