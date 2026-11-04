// SPDX-License-Identifier: Apache-2.0
//! Admitted configuration documents and the configuration tables they hold.

use cadmpeg_core::CodecError;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

pub(crate) mod table;

cadmpeg_core::named_optional_field!(
    deserialize_active_row,
    DesignConfigurationActiveRow,
    "active_row"
);

/// JSON configuration document stored in a Fusion design-configuration entry.
#[derive(Debug, PartialEq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DesignConfigurationWire")]
pub(crate) struct DesignConfiguration {
    entry_name: String,
    identity_scope: cadmpeg_ir::ids::IdentityComponent,
    payload: Map<String, Value>,
    content: ConfigurationContent,
    active_row: Option<DesignConfigurationActiveRow>,
}

#[cfg(test)]
thread_local! {
    static CONFIGURATION_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DesignConfiguration {
    fn clone(&self) -> Self {
        CONFIGURATION_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            entry_name: self.entry_name.clone(),
            identity_scope: self.identity_scope.clone(),
            payload: self.payload.clone(),
            content: self.content.clone(),
            active_row: self.active_row.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ConfigurationContent {
    Table(table::AdmittedTables),
    Rule,
}

/// Active row that the Design `DesignConfigurationRoot` record names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DesignConfigurationActiveRow {
    /// Indexed-record index of the root record in its Design `BulkStream`.
    pub(crate) record_index: u32,
    /// Id of the table that holds the active row.
    pub(crate) table_id: String,
    /// Id of the active row.
    pub(crate) row_id: String,
}

/// Serialized configuration identity, document, and active row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DesignConfigurationWire {
    id: String,
    entry_name: String,
    kind: DesignConfigurationKind,
    payload: Value,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_active_row"
    )]
    active_row: Option<DesignConfigurationActiveRow>,
}

struct ConfigurationIdentity<'a>(&'a DesignConfiguration);

impl std::fmt::Display for ConfigurationIdentity<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write;

        write!(formatter, "f3d:{}:entry#", self.0.identity_scope.as_str())?;
        for character in self.0.entry_name.chars() {
            if matches!(character, ':' | '#' | '%') || character.is_whitespace() {
                let mut bytes = [0; 4];
                for byte in character.encode_utf8(&mut bytes).as_bytes() {
                    write!(formatter, "%{byte:02X}")?;
                }
            } else {
                formatter.write_char(character)?;
            }
        }
        Ok(())
    }
}

impl Serialize for ConfigurationIdentity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Serialize for DesignConfiguration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4 + usize::from(self.active_row.is_some())))?;
        map.serialize_entry("id", &ConfigurationIdentity(self))?;
        map.serialize_entry("entry_name", &self.entry_name)?;
        map.serialize_entry("kind", &self.kind())?;
        map.serialize_entry("payload", &self.payload)?;
        if let Some(active_row) = &self.active_row {
            map.serialize_entry("active_row", active_row)?;
        }
        map.end()
    }
}

/// Native Fusion design-configuration entry family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DesignConfigurationKind {
    Table,
    Rule,
}

impl DesignConfiguration {
    pub(crate) fn try_new(
        entry_name: String,
        kind: DesignConfigurationKind,
        payload: Map<String, Value>,
    ) -> Result<Self, CodecError> {
        let extension = match kind {
            DesignConfigurationKind::Table => ".dsgcfg",
            DesignConfigurationKind::Rule => ".dsgcfgrule",
        };
        if !entry_name.ends_with(extension) {
            return Err(CodecError::malformed(format_args!(
                "configuration.entry_name must end with {extension} for {kind:?}"
            )));
        }
        let content = match kind {
            DesignConfigurationKind::Table => {
                ConfigurationContent::Table(table::AdmittedTables::admit(&payload))
            }
            DesignConfigurationKind::Rule => ConfigurationContent::Rule,
        };
        Ok(Self {
            entry_name,
            identity_scope: cadmpeg_ir::identity_component!("configuration"),
            payload,
            content,
            active_row: None,
        })
    }

    /// Attach the active row of a table document.
    pub(crate) fn set_active_row(
        &mut self,
        active_row: DesignConfigurationActiveRow,
    ) -> Result<(), CodecError> {
        if self.kind() == DesignConfigurationKind::Rule {
            return Err(CodecError::malformed(format_args!(
                "F3D configuration rule carries an active row: {}",
                self.entry_name
            )));
        }
        self.active_row = Some(active_row);
        Ok(())
    }

    pub(crate) fn id(&self) -> String {
        crate::ids::configuration_entry_id(&self.entry_name, &self.identity_scope)
    }

    pub(crate) fn entry_name(&self) -> &String {
        &self.entry_name
    }

    pub(crate) fn kind(&self) -> DesignConfigurationKind {
        match self.content {
            ConfigurationContent::Table(_) => DesignConfigurationKind::Table,
            ConfigurationContent::Rule => DesignConfigurationKind::Rule,
        }
    }

    /// Admitted tables in document order; rules hold none.
    pub(crate) fn tables(&self) -> &[table::ConfigurationTable] {
        match &self.content {
            ConfigurationContent::Table(admitted) => &admitted.tables,
            ConfigurationContent::Rule => &[],
        }
    }

    pub(crate) fn active_row(&self) -> Option<&DesignConfigurationActiveRow> {
        self.active_row.as_ref()
    }

    pub(crate) fn rule(&self) -> Option<&Map<String, Value>> {
        match self.content {
            ConfigurationContent::Rule => Some(&self.payload),
            ConfigurationContent::Table(_) => None,
        }
    }

    /// The complete source document.
    pub(crate) fn payload(&self) -> &Map<String, Value> {
        &self.payload
    }

    pub(crate) fn unknown_member_count(&self) -> usize {
        match &self.content {
            ConfigurationContent::Rule => self
                .payload
                .keys()
                .filter(|key| !matches!(key.as_str(), "when" | "activate"))
                .count(),
            ConfigurationContent::Table(admitted) => admitted.unassigned,
        }
    }
}

impl TryFrom<DesignConfigurationWire> for DesignConfiguration {
    type Error = String;
    fn try_from(wire: DesignConfigurationWire) -> Result<Self, String> {
        let suffix = format!(
            ":entry#{}",
            crate::ids::identity_key_component(&wire.entry_name)
        );
        let scope = wire
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.strip_suffix(&suffix))
            .ok_or_else(|| format!("configuration.id {} must identify entry_name", wire.id))?;
        let scope = cadmpeg_ir::ids::IdentityComponent::try_new(scope)
            .map_err(|error| format!("configuration.id {}: {error}", wire.id))?;
        let Value::Object(payload) = wire.payload else {
            return Err("payload must be an object".into());
        };
        let mut record = Self::try_new(wire.entry_name, wire.kind, payload)
            .map_err(|error| error.to_string())?;
        record.identity_scope = scope;
        if let Some(active_row) = wire.active_row {
            record
                .set_active_row(active_row)
                .map_err(|error| error.to_string())?;
        }
        Ok(record)
    }
}

#[cfg(test)]
impl From<DesignConfiguration> for DesignConfigurationWire {
    fn from(value: DesignConfiguration) -> Self {
        Self {
            id: value.id(),
            kind: value.kind(),
            payload: Value::Object(value.payload),
            entry_name: value.entry_name,
            active_row: value.active_row,
        }
    }
}

/// Encode the native source document.
pub(crate) fn encode_configuration_payload(
    configuration: &DesignConfiguration,
) -> Result<Vec<u8>, CodecError> {
    serde_json::to_vec(configuration.payload()).map_err(|error| {
        CodecError::malformed(format_args!(
            "cannot encode F3D configuration JSON {}: {error}",
            configuration.entry_name()
        ))
    })
}

#[cfg(test)]
mod tests;
