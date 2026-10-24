//! Configuration graph remapping for independent component copies.
use super::{Column, ConfigTable};
use crate::{DocError, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct ConfigCloneMaps {
    pub features: BTreeMap<u64, u64>,
    pub components: BTreeMap<u64, u64>,
    pub bodies: BTreeMap<String, String>,
    pub parameters: BTreeMap<String, String>,
}

/// Simultaneous token-reference substitution, including maps whose values are other keys.
pub(super) fn remap_expression(source: &str, parameters: &BTreeMap<String, String>) -> String {
    let refs = crate::expr::references(source);
    let mut result = source.to_string();
    let mut replacements = Vec::new();
    for (index, reference) in refs.iter().enumerate() {
        if let Some(mapped) = parameters.get(reference) {
            let mut temporary = format!("_config_clone_{index}");
            while refs.contains(&temporary) || parameters.contains_key(&temporary) || parameters.values().any(|value| value == &temporary) {
                temporary.push('_');
            }
            result = crate::expr::rename_reference(&result, reference, &temporary);
            replacements.push((temporary, mapped));
        }
    }
    for (temporary, mapped) in replacements {
        result = crate::expr::rename_reference(&result, &temporary, mapped);
    }
    result
}

impl ConfigTable {
    /// Append mapped columns/cells/rules atomically; source targets and active row remain intact.
    pub fn clone_targets(&mut self, maps: &ConfigCloneMaps) -> Result<()> {
        if maps.features.values().chain(maps.components.values()).any(|id| *id == 0)
            || maps.parameters.values().any(|name| crate::Document::param_name_problem(name).is_some())
            || maps.bodies.values().any(|name| name.is_empty() || name.len() > 128)
        {
            return Err(DocError::Invalid("invalid configuration clone identity mapping".into()));
        }
        if self.columns.len() > super::MAX_COLUMNS
            || self.rows.len() > super::MAX_ROWS
            || self.rules.len() > super::MAX_RULES
            || self.rows.iter().any(|row| row.cells.len() != self.columns.len())
        {
            return Err(DocError::Invalid("cannot clone an incomplete configuration table".into()));
        }
        let mut next = self.clone();
        for (index, column) in self.columns.iter().enumerate() {
            if let Column::FaceAppearance(target) = column
                && maps.bodies.contains_key(&target.body)
                && !maps.features.contains_key(&target.origin_feature)
            {
                return Err(DocError::Invalid(format!(
                    "cannot copy configured face `{}`: imported foreign body has no certified cloned lineage",
                    target.name
                )));
            }
            let mapped = match column {
                Column::Param(name) => maps.parameters.get(name).cloned().map(Column::Param),
                Column::Suppress(id) => maps.features.get(id).copied().map(Column::Suppress),
                Column::Material(name) => maps.bodies.get(name).cloned().map(Column::Material),
                Column::Appearance(name) => maps.bodies.get(name).cloned().map(Column::Appearance),
                Column::ComponentAppearance(id) => maps.components.get(id).copied().map(Column::ComponentAppearance),
                Column::FaceAppearance(target) => {
                    maps.bodies.get(&target.body).zip(maps.features.get(&target.origin_feature)).map(|(body, origin)| {
                        let mut target = target.clone();
                        target.body = body.clone();
                        target.name = target
                            .name
                            .strip_prefix(&format!("F{}:", target.origin_feature))
                            .map(|role| format!("F{origin}:{role}"))
                            .unwrap_or_else(|| target.name.clone());
                        target.origin_feature = *origin;
                        if let Some(certificate) = &mut target.certificate {
                            certificate.remap_features(&maps.features);
                        }
                        Column::FaceAppearance(target)
                    })
                }
            };
            if let Some(mapped) = mapped {
                if mapped == *column {
                    continue;
                }
                if next.columns.iter().any(|column| {
                    *column == mapped
                        || matches!((column,&mapped),(Column::FaceAppearance(a),Column::FaceAppearance(b)) if a.body == b.body && a.name == b.name)
                }) {
                    return Err(DocError::Invalid("configuration copy target collides with an existing column".into()));
                }
                if next.columns.len() >= super::MAX_COLUMNS {
                    return Err(DocError::Invalid("configuration copy exceeds column limit".into()));
                }
                next.columns.push(mapped);
                for (source, destination) in self.rows.iter().zip(&mut next.rows) {
                    let value = source.cells.get(index).cloned().unwrap_or_default();
                    let value = if matches!(column, Column::Param(_)) { remap_expression(&value, &maps.parameters) } else { value };
                    if value.len() > 4096 {
                        return Err(DocError::Invalid("configuration copied value exceeds length limit".into()));
                    }
                    destination.cells.push(value);
                }
            }
        }
        // The ids taken so far, looked up rather than scanned: copies of rules whose ids share
        // their first 40 characters try one id after another.
        let mut taken: BTreeSet<String> = next.rules.iter().map(|rule| rule.id.clone()).collect();
        for rule in &self.rules {
            if let Some(mut copied) = rule.clone_for_targets(maps) {
                if next.rules.len() >= super::MAX_RULES {
                    return Err(DocError::Invalid("configuration copy exceeds rule limit".into()));
                }
                let stem: String = rule.id.chars().take(40).collect();
                copied.id = (1..=super::MAX_RULES + 1)
                    .map(|i| format!("{stem}_copy{i}"))
                    .find(|id| !taken.contains(id))
                    .ok_or_else(|| DocError::Invalid("no configuration rule ID available".into()))?;
                taken.insert(copied.id.clone());
                copied.name = format!("{} (copy)", rule.name.chars().take(120).collect::<String>());
                next.rules.push(copied);
            }
        }
        super::validate_rules_json(&serde_json::json!({"configs":{"rules":next.rules}}))?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigFaceTarget, ConfigRow, ConfigRule, RuleCondition, RuleEffect};
    #[test]
    fn clone_preserves_source_variations_and_remaps_only_typed_targets_and_tokens() {
        let mut table = ConfigTable {
            columns: vec![
                Column::Param("d1".into()),
                Column::Param("d2".into()),
                Column::Appearance("Old".into()),
                Column::ComponentAppearance(2),
                Column::FaceAppearance(ConfigFaceTarget {
                    body: "Old".into(),
                    origin_feature: 3,
                    name: "F3:+z".into(),
                    point: solidcraft_geom::Vec3::new(1., 1., 2.),
                    certificate: None,
                }),
            ],
            rows: vec![ConfigRow {
                name: "Default".into(),
                cells: vec!["d1 * 2 + d2".into(), "4 mm".into(), "d1".into(), String::new(), String::new()],
            }],
            active: Some("Default".into()),
            rules: vec![ConfigRule {
                id: "Rule1".into(),
                name: "Rule1".into(),
                enabled: true,
                length_unit: Some("mm".into()),
                condition: RuleCondition::Suppression { feature: 3, value: false },
                effect: RuleEffect::DriveParameter { parameter: "d1".into(), expression: "d2 * 2 + d1".into() },
            }],
        };
        let original = table.clone();
        let maps = ConfigCloneMaps {
            features: BTreeMap::from([(3, 7)]),
            components: BTreeMap::from([(2, 8)]),
            bodies: BTreeMap::from([("Old".into(), "New".into())]),
            parameters: BTreeMap::from([("d1".into(), "copy_d1".into()), ("d2".into(), "copy_d2".into())]),
        };
        table.clone_targets(&maps).unwrap();
        assert_eq!(&table.columns[..5], &original.columns);
        assert_eq!(&table.rows[0].cells[..5], &original.rows[0].cells);
        assert_eq!(table.columns[5], Column::Param("copy_d1".into()));
        assert_eq!(table.rows[0].cells[5], "copy_d1 * 2 + copy_d2");
        assert_eq!(table.rows[0].cells[7], "d1"); // appearance strings are never parameter expressions
        assert_eq!(table.columns[8], Column::ComponentAppearance(8));
        let Column::FaceAppearance(face) = &table.columns[9] else { panic!() };
        assert_eq!(face.name, "F7:+z");
        assert_eq!(face.body, "New");
        assert_eq!(table.rules[0], original.rules[0]);
        assert!(
            matches!(&table.rules[1].effect,RuleEffect::DriveParameter{parameter,expression} if parameter=="copy_d1" && expression=="copy_d2 * 2 + copy_d1")
        );
        assert!(matches!(table.rules[1].condition, RuleCondition::Suppression { feature: 7, .. }));
        assert_eq!(table.active, original.active);
        let chained = BTreeMap::from([("d1".into(), "d2".into()), ("d2".into(), "d3".into())]);
        assert_eq!(remap_expression("d1 * 2 + d2", &chained), "d2 * 2 + d3");
        let collision = ConfigCloneMaps { parameters: chained, ..Default::default() };
        let mut original_copy = original.clone();
        assert!(original_copy.clone_targets(&collision).is_err());
        assert_eq!(original_copy, original);
    }
    #[test]
    fn foreign_strict_lineage_failure_is_atomic() {
        let mut table = ConfigTable {
            columns: vec![
                Column::Param("width".into()),
                Column::FaceAppearance(ConfigFaceTarget {
                    body: "Foreign".into(),
                    origin_feature: 3,
                    name: "F3:+z".into(),
                    point: solidcraft_geom::Vec3::ZERO,
                    certificate: None,
                }),
            ],
            rows: vec![ConfigRow { name: "Default".into(), cells: vec!["2 mm".into(), String::new()] }],
            ..Default::default()
        };
        let original = table.clone();
        let maps = ConfigCloneMaps {
            bodies: BTreeMap::from([("Foreign".into(), "Copied".into())]),
            parameters: BTreeMap::from([("width".into(), "copied_width".into())]),
            ..Default::default()
        };
        assert!(table.clone_targets(&maps).unwrap_err().to_string().contains("certified"));
        assert_eq!(table, original);
    }
}
