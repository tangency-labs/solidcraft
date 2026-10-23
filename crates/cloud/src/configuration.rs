//! Versioned, machine-local connection metadata. Credentials live in a separate host store.
use serde_json::{Value, json};

use crate::{Error, Result, Settings};

pub const MAX_SETTINGS_BYTES: usize = 1 << 20;
const FORMAT: &str = "solidcraft-connections/1";

/// Retain optional fields understood by a newer compatible client when changing settings.
pub struct Configuration(Value);

impl Default for Configuration {
    fn default() -> Self {
        Self(json!({"format": FORMAT, "settings": Settings::default()}))
    }
}

impl Configuration {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SETTINGS_BYTES {
            return Err(Error::Storage("connection settings are too large".into()));
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
        if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(Error::UpgradeRequired("connection settings format".into()));
        }
        let config = Self(value);
        config.settings()?;
        Ok(config)
    }

    pub fn settings(&self) -> Result<Settings> {
        let settings: Settings = serde_json::from_value(self.0.get("settings").cloned().unwrap_or(Value::Null))
            .map_err(|_| Error::UpgradeRequired("connection settings fields".into()))?;
        settings.validate()?;
        Ok(settings)
    }

    pub fn update(&mut self, settings: &Settings) -> Result<Vec<u8>> {
        settings.validate()?;
        let mut next = serde_json::to_value(settings).map_err(|e| Error::Invalid(e.to_string()))?;
        let previous = self.0.get("settings").ok_or_else(|| Error::Invalid("missing connection settings".into()))?;
        // Preserve optional per-connection fields by stable id, never by array position.
        if let Some(connections) = next.get_mut("connections").and_then(Value::as_array_mut) {
            for connection in connections {
                if let Some(old) = previous.get("connections").and_then(Value::as_array).and_then(|entries| {
                    entries.iter().find(|old| old.get("id").and_then(Value::as_str) == connection.get("id").and_then(Value::as_str))
                }) {
                    merge_optional(connection, old);
                }
            }
        }
        merge_optional(&mut next, previous);
        let mut updated = self.0.clone();
        updated["settings"] = next;
        let bytes = serde_json::to_vec_pretty(&updated).map_err(|e| Error::Invalid(e.to_string()))?;
        if bytes.len() > MAX_SETTINGS_BYTES {
            return Err(Error::Storage("connection settings are too large".into()));
        }
        self.0 = updated;
        Ok(bytes)
    }
}

fn merge_optional(next: &mut Value, previous: &Value) {
    if let (Some(next), Some(previous)) = (next.as_object_mut(), previous.as_object()) {
        for (key, value) in previous {
            next.entry(key).or_insert_with(|| value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_versions_and_optional_fields_survive_updates() {
        let mut config = Configuration::read(br#"{"format":"solidcraft-connections/1","note":7,"settings":{"extra":true,"connections":[{"id":"first","credential":"","note":"keep"},{"id":"second","credential":"","note":"other"}]}}"#).unwrap();
        let mut settings = config.settings().unwrap();
        settings.connections.reverse();
        settings.connections[0].name = "Renamed".into();
        let bytes = config.update(&settings).unwrap();
        let saved: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["note"], 7);
        assert_eq!(saved["settings"]["extra"], true);
        assert_eq!(saved["settings"]["connections"][0]["note"], "other");
        assert_eq!(saved["settings"]["connections"][1]["note"], "keep");
        assert_eq!(Configuration::read(&bytes).unwrap().settings().unwrap(), settings);
        assert!(matches!(Configuration::read(br#"{"format":"solidcraft-connections/2","settings":{}}"#), Err(Error::UpgradeRequired(_))));
        assert!(matches!(
            Configuration::read(br#"{"format":"solidcraft-connections/1","settings":{"connections":[{"authentication":"future"}]}}"#),
            Err(Error::UpgradeRequired(_))
        ));
    }
}
