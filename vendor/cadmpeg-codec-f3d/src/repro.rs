// SPDX-License-Identifier: Apache-2.0
//! Anonymous stream identities with the native record grammar.
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

pub(crate) fn redactions(ir: &CadIr) -> Result<BTreeMap<String, String>, CodecError> {
    let mut replacements = BTreeMap::new();
    let Some(namespace) = ir.native.namespace("f3d") else {
        return Ok(replacements);
    };
    let mut components = BTreeMap::new();
    let value = serde_json::to_value(ir).map_err(CodecError::malformed)?;
    for row in cadmpeg_ir::repro::rows(&value) {
        let Some(id) = cadmpeg_ir::repro::record(&value, &row)["id"].as_str() else {
            continue;
        };
        if let Some(scope) = id
            .strip_prefix("f3d:")
            .and_then(|tail| tail.split_once(':'))
            .map(|(scope, _)| scope)
        {
            if let Some(raw) = crate::ids::decode_identity_key_component(scope) {
                remember_scope(&raw, &mut components, &mut replacements);
            }
        }
    }
    drop(value);
    for records in namespace.arenas().values() {
        for record in records {
            let fields = record.fields().map_err(CodecError::from)?;
            for (field, value) in fields {
                let Some(text) = value.as_str() else {
                    continue;
                };
                if field == "stream" {
                    remember_scope(text, &mut components, &mut replacements);
                }
                if matches!(field.as_str(), "blob_name" | "source_brep") {
                    if let Some(name) = text.strip_prefix("BREP.") {
                        let anonymous = anonymous_component(name, &mut components);
                        replacements.insert(text.into(), format!("BREP.{anonymous}"));
                    }
                }
                if field == "source_namespace" {
                    if let Some(scope) = text.strip_prefix("f3d:") {
                        remember_scope(scope, &mut components, &mut replacements);
                    }
                }
            }
        }
    }
    Ok(replacements)
}

fn remember_scope(
    raw: &str,
    components: &mut BTreeMap<String, String>,
    replacements: &mut BTreeMap<String, String>,
) {
    let anonymous = raw
        .split('/')
        .map(|component| {
            let occurrence = component
                .strip_prefix("occurrence-")
                .is_some_and(|ordinal| ordinal.parse::<u64>().is_ok());
            if occurrence
                || matches!(
                    component,
                    "xref"
                        | "brep"
                        | "design"
                        | "model"
                        | "feature-input"
                        | "appearance"
                        | "tspline"
                        | "BulkStream.dat"
                        | "MetaStream.dat"
                )
            {
                component.into()
            } else {
                anonymous_component(component, components)
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    replacements.insert(raw.into(), anonymous.clone());
    replacements.insert(
        crate::ids::identity_key_component(raw),
        crate::ids::identity_key_component(&anonymous),
    );
    replacements.insert(
        format!("f3d:{}", crate::ids::identity_key_component(raw)),
        format!("f3d:{}", crate::ids::identity_key_component(&anonymous)),
    );
}

fn anonymous_component(raw: &str, components: &mut BTreeMap<String, String>) -> String {
    let ordinal = components.len();
    components
        .entry(raw.into())
        .or_insert_with(|| format!("repro{ordinal}"))
        .clone()
}

#[cfg(test)]
mod tests;
