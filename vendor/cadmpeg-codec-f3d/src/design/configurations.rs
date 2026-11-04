// SPDX-License-Identifier: Apache-2.0
//! Decode and project Design configuration records.

use cadmpeg_core::container::ContainerRole;

use crate::container::ContainerScan;
use crate::records::configuration::table::{ConfigurationRow, ConfiguredValue};
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashSet};

mod json;

fn configuration_property_key(
    prefix: &'static str,
    suffix: &str,
) -> Result<cadmpeg_core::text::NonBlankString, CodecError> {
    cadmpeg_core::text::NonBlankString::new(format!("{prefix}{suffix}"))
        .ok_or_else(|| CodecError::malformed("configuration property key is blank"))
}

/// Configuration documents and the count of root records that no table
/// document receives.
#[derive(Debug, Default)]
pub(crate) struct DecodedConfigurations {
    pub(crate) documents: Vec<DesignConfiguration>,
    pub(crate) unbound_roots: usize,
}

/// Decode every JSON design-configuration table and rule entry, and attach the
/// active row of the Design root record to its table document.
pub(crate) fn decode_configurations(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<DecodedConfigurations, CodecError> {
    let mut documents = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, ContainerRole::DesignConfig))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let payload = json::parse_configuration_payload(ctx, &entry.name, bytes)?;
        let serde_json::Value::Object(payload) = payload else {
            return Err(CodecError::malformed(format_args!(
                "F3D configuration JSON must be an object: {}",
                entry.name
            )));
        };
        let kind = if entry.name.ends_with(".dsgcfgrule") {
            DesignConfigurationKind::Rule
        } else {
            DesignConfigurationKind::Table
        };
        documents.push(DesignConfiguration::try_new(
            entry.name.clone(),
            kind,
            payload,
        )?);
    }
    let mut names = HashSet::new();
    for document in &documents {
        if !names.insert(document.entry_name().as_str()) {
            return Err(CodecError::malformed(format_args!(
                "duplicate F3D configuration identity: {}",
                document.entry_name()
            )));
        }
    }
    let roots = crate::design::decode::configuration_root::decode_configuration_roots(scan)?;
    let unbound_roots = roots.unparsed + attach_roots(&mut documents, roots.rows)?;
    Ok(DecodedConfigurations {
        documents,
        unbound_roots,
    })
}

/// Attach the active row of the one root record to its table document, and
/// count the root records that no table document receives.
fn attach_roots(
    documents: &mut [DesignConfiguration],
    rows: Vec<crate::records::configuration::DesignConfigurationActiveRow>,
) -> Result<usize, CodecError> {
    if rows.len() > 1 {
        return Ok(rows.len());
    }
    let mut unbound = 0;
    for row in rows {
        match root_document(documents, &row.table_id) {
            Some(index) => documents[index].set_active_row(row)?,
            None => unbound += 1,
        }
    }
    Ok(unbound)
}

/// The table document that holds `table_id`; with no such document, the only
/// table document.
fn root_document(documents: &[DesignConfiguration], table_id: &str) -> Option<usize> {
    let table_documents = || {
        documents
            .iter()
            .enumerate()
            .filter(|(_, document)| document.kind() == DesignConfigurationKind::Table)
    };
    let mut holding = table_documents()
        .filter(|(_, document)| document.tables().iter().any(|table| table.id == table_id));
    let mut candidates = match holding.next() {
        Some(first) => std::iter::once(first).chain(holding).collect::<Vec<_>>(),
        None => table_documents().collect(),
    };
    let (index, _) = candidates.pop()?;
    candidates.is_empty().then_some(index)
}

/// Native and neutral records that configured objects name by indexed-record
/// index.
pub(crate) struct ConfigurationTargets<'a> {
    pub(crate) parameters: &'a [crate::records::parameters::DesignParameter],
    pub(crate) owners: &'a [crate::records::parameters::DesignParameterOwner],
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) neutral_parameters: &'a [cadmpeg_ir::features::DesignParameter],
    pub(crate) neutral_features: &'a [cadmpeg_ir::features::Feature],
}

/// The one value that every item repeats.
fn unique<T: PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

impl ConfigurationTargets<'_> {
    /// The neutral parameter of parameter record `record_index`, limited to
    /// `stream` when it is stated.
    fn parameter(
        &self,
        record_index: u32,
        stream: Option<&str>,
    ) -> Option<cadmpeg_ir::features::ParameterId> {
        let id = unique(
            self.parameters
                .iter()
                .filter(|parameter| {
                    parameter.record_index == record_index
                        && stream.is_none_or(|stream| {
                            crate::ids::native_stream(&parameter.id) == Some(stream)
                        })
                })
                .map(crate::ids::neutral_parameter_id),
        )?;
        self.neutral_parameters
            .iter()
            .any(|parameter| parameter.id == id)
            .then_some(id)
    }

    fn owned_parameter(
        &self,
        owner_record_index: u32,
    ) -> Option<cadmpeg_ir::features::ParameterId> {
        let (stream, parameter) = unique(
            self.owners
                .iter()
                .filter(|owner| owner.record_index() == owner_record_index)
                .map(|owner| {
                    (
                        crate::ids::native_stream(owner.id()),
                        owner.parameter_record_index(),
                    )
                }),
        )?;
        self.parameter(parameter, Some(stream?))
    }

    fn feature(&self, scope_record_index: u32) -> Option<&cadmpeg_ir::features::Feature> {
        let id = unique(
            self.scopes
                .iter()
                .filter(|scope| scope.record_index == scope_record_index)
                .map(crate::ids::neutral_feature_id),
        )?;
        self.neutral_features
            .iter()
            .find(|feature| feature.id == id)
    }
}

/// Project the rows of the one populated table into the neutral configuration
/// arena. Rule documents remain native because a rule is a selector, not a
/// model variant.
pub(crate) fn project_configurations(
    native: &[DesignConfiguration],
    targets: &ConfigurationTargets<'_>,
) -> Result<Vec<cadmpeg_ir::features::DesignConfiguration>, CodecError> {
    let mut populated = native.iter().flat_map(|document| {
        document
            .tables()
            .iter()
            .filter(|table| !table.rows.is_empty())
            .map(move |table| (document, table))
    });
    let first = populated.next();
    if populated.next().is_some() {
        return Err(CodecError::NotImplemented(
            "independent nonempty F3D configuration tables have no shared authored order".into(),
        ));
    }
    let mut projected = Vec::new();
    if let Some((document, table)) = first {
        let active_row = document
            .active_row()
            .filter(|row| row.table_id == table.id)
            .map(|row| row.row_id.as_str());
        for (ordinal, row) in table.rows.iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| {
                CodecError::Malformed("F3D configuration ordinal exceeds u32".into())
            })?;
            let active = active_row == Some(row.id.as_str());
            projected.push(project_row(document, row, ordinal, active, targets)?);
        }
    }
    for (rule, payload) in native.iter().filter_map(|rule| Some((rule, rule.rule()?))) {
        let Some(condition) = payload.get("when").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(target) = payload.get("activate").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let mut matches = projected
            .iter_mut()
            .filter(|configuration| configuration.name.as_deref() == Some(target));
        let Some(configuration) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let key = configuration_property_key("activation_rule:", rule.entry_name())?;
        configuration.properties.insert(key, condition.to_owned());
    }
    projected.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(projected)
}

fn project_row(
    document: &DesignConfiguration,
    row: &ConfigurationRow,
    ordinal: u32,
    active: bool,
    targets: &ConfigurationTargets<'_>,
) -> Result<cadmpeg_ir::features::DesignConfiguration, CodecError> {
    use cadmpeg_ir::features::{ConfigurationEvaluation, ConfigurationFeatureState};

    let mut configuration = cadmpeg_ir::features::DesignConfiguration {
        id: crate::ids::neutral_configuration_id(document.entry_name(), &row.id),
        ordinal,
        active,
        source_index: None,
        name: Some(row.name.clone()),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        bodies: None,
        native_ref: Some(document.id()),
    };
    for cell in &row.cells {
        let (parameter, expression) = match &cell.value {
            ConfiguredValue::Property { key, text } => {
                let key = configuration_property_key("property:", key)?;
                configuration.properties.insert(key, text.clone());
                continue;
            }
            ConfiguredValue::Feature {
                record_index,
                computed,
            } => {
                match targets.feature(*record_index) {
                    Some(feature) => {
                        let evaluation = if *computed {
                            ConfigurationEvaluation::Active {
                                outputs: cadmpeg_ir::features::DistinctMembers::default(),
                            }
                        } else {
                            ConfigurationEvaluation::Suppressed {}
                        };
                        configuration.feature_states.insert(
                            feature.id.clone(),
                            ConfigurationFeatureState {
                                evaluation,
                                dependencies: feature.dependencies.clone(),
                                definition: feature.evaluation.definition().clone(),
                            },
                        );
                    }
                    None => {
                        let key = configuration_property_key("suppressed:", &cell.column_id)?;
                        configuration
                            .properties
                            .insert(key, (!computed).to_string());
                    }
                }
                continue;
            }
            ConfiguredValue::Parameter {
                record_index,
                expression,
            } => (targets.parameter(*record_index, None), expression),
            ConfiguredValue::OwnedParameter {
                owner_record_index,
                expression,
            } => (targets.owned_parameter(*owner_record_index), expression),
        };
        match parameter {
            Some(parameter) => {
                configuration
                    .parameter_overrides
                    .insert(parameter, expression.clone());
            }
            None => {
                let key = configuration_property_key("parameter:", &cell.column_id)?;
                configuration.properties.insert(key, expression.clone());
            }
        }
    }
    Ok(configuration)
}

pub(crate) fn unresolved_configuration_parameter_override_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("parameter:"))
        .count()
}

pub(crate) fn unresolved_configuration_suppressed_feature_count(
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    projected
        .iter()
        .flat_map(|configuration| configuration.properties.keys())
        .filter(|key| key.as_str().starts_with("suppressed:"))
        .count()
}

pub(crate) fn unresolved_configuration_rule_count(
    native: &[DesignConfiguration],
    projected: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    native
        .iter()
        .filter(|rule| rule.rule().is_some_and(|payload| !payload.is_empty()))
        .filter(|rule| {
            !projected.iter().any(|configuration| {
                configuration.properties.keys().any(|key| {
                    key.as_str().strip_prefix("activation_rule:") == Some(rule.entry_name())
                })
            })
        })
        .count()
}

pub(crate) fn unresolved_configuration_member_count(native: &[DesignConfiguration]) -> usize {
    native
        .iter()
        .map(DesignConfiguration::unknown_member_count)
        .sum()
}

#[cfg(test)]
mod tests;
