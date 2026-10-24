//! Configurations: a table whose rows are variants of the design. Each column is a parameter
//! expression, feature suppression, body material or body appearance. Activating a row sets
//! those values in the design atomically, which then rebuilds as usual.

use serde::{Deserialize, Serialize};

use crate::{DEFAULT_MATERIAL, DocError, Document, Result};

#[path = "config_rules.rs"]
mod rules;
pub use rules::*;
#[path = "config_styles.rs"]
mod styles;
pub use styles::*;
#[path = "config_certificates.rs"]
mod certificates;
pub use certificates::*;
#[path = "config_clone.rs"]
mod clone;
pub use clone::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    /// A parameter's expression.
    Param(String),
    /// A feature's suppression (by feature id).
    Suppress(u64),
    /// A body's physical material (library name, or Default to clear).
    Material(String),
    /// A body's appearance (library name, serialized Look, or empty to clear).
    Appearance(String),
    /// Shared component definition's direct appearance.
    ComponentAppearance(u64),
    /// A certified semantic face lineage in a shared definition.
    FaceAppearance(ConfigFaceTarget),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigRow {
    pub name: String,
    /// One cell per column: expression, suppression, material name or appearance look.
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigTable {
    pub columns: Vec<Column>,
    pub rows: Vec<ConfigRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ConfigRule>,
}

pub const MAX_COLUMNS: usize = 500;
pub const MAX_ROWS: usize = 2000;

pub fn suppression_cell(value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(DocError::Invalid("suppression must be true or false".into())),
    }
}

impl ConfigTable {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.rows.is_empty() && self.rules.is_empty()
    }
}

/// Parse an appearance cell. Empty/Default follows the body's material; named library looks
/// and JSON Look objects support both convenient editing and lossless custom colours.
pub fn appearance_cell(value: &str) -> Result<Option<crate::appearance::Look>> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("default") {
        return Ok(None);
    }
    if value.len() > 4096 {
        return Err(DocError::Invalid("appearance value too long".into()));
    }
    if let Some(look) = crate::appearance::library(value) {
        return Ok(Some(look));
    }
    let look: crate::appearance::Look =
        serde_json::from_str(value).map_err(|_| DocError::Invalid(format!("unknown configuration appearance `{value}`")))?;
    if look.name.chars().count() > 128 || !look.opacity.is_finite() || !(0.0..=1.0).contains(&look.opacity) {
        return Err(DocError::Invalid("appearance requires a name up to 128 characters and opacity in 0..1".into()));
    }
    Ok(Some(look))
}

impl Document {
    /// Parse a physical material cell (a built-in material or one defined in this design),
    /// returning None for the default material.
    pub fn material_cell(&self, value: &str) -> Result<Option<String>> {
        let name = self.material_name(value).ok_or_else(|| DocError::Invalid(format!("unknown configuration material `{value}`")))?;
        Ok((name != DEFAULT_MATERIAL).then_some(name))
    }

    /// Freeze design-unit-sensitive configuration literals when changing display units.
    /// Explicit units and nonliteral expressions retain their source text.
    pub fn preserve_configuration_units(&mut self, previous_unit: &str, kept_parameters: &[String]) {
        let feature_lengths: Vec<String> = self
            .features
            .iter()
            .flat_map(|feature| {
                feature
                    .param_names
                    .iter()
                    .zip(feature.kind.inputs())
                    .filter(|(_, (_, _, kind))| *kind == crate::expr::Kind::Length)
                    .map(|(name, _)| name.clone())
            })
            .collect();
        let lengths: std::collections::BTreeSet<String> = feature_lengths.into_iter().chain(kept_parameters.iter().cloned()).collect();
        for row in &mut self.configs.rows {
            for (column, cell) in self.configs.columns.iter().zip(&mut row.cells) {
                if matches!(column,Column::Param(name) if lengths.contains(name)) && cell.trim().parse::<f64>().is_ok() {
                    *cell = format!("{} {previous_unit}", cell.trim());
                }
            }
        }
        for rule in &mut self.configs.rules {
            rule.preserve_length_units(&lengths, previous_unit);
        }
    }

    /// A column's current value in the design.
    pub fn config_current(&self, c: &Column) -> Option<String> {
        match c {
            Column::Param(n) => self.all_param_exprs().into_iter().find(|(m, _, _)| m == n).map(|(_, e, kind)| {
                // Feature literals are stored in base units, unlike newly typed bare
                // row cells. Capture their unit explicitly before editing in an inch design.
                if self.param(n).is_none() && kind == crate::expr::Kind::Length && self.units != "mm" && e.trim().parse::<f64>().is_ok() {
                    format!("{} mm", e.trim())
                } else {
                    e
                }
            }),
            Column::Suppress(id) => self.feature(*id).map(|f| f.suppressed.to_string()),
            Column::Material(body) => Some(self.materials.get(body).cloned().unwrap_or_else(|| DEFAULT_MATERIAL.into())),
            Column::Appearance(body) => Some(self.appearances.bodies.get(body).and_then(|look| serde_json::to_string(look).ok()).unwrap_or_default()),
            Column::ComponentAppearance(id) => {
                Some(self.appearances.components.get(id).and_then(|look| serde_json::to_string(look).ok()).unwrap_or_default())
            }
            Column::FaceAppearance(target) => {
                Some(self.config_face_look(target).and_then(|look| serde_json::to_string(&look).ok()).unwrap_or_default())
            }
        }
    }

    /// A column's display key, prefixed by its kind to distinguish targets with equal names.
    pub fn config_key(&self, c: &Column) -> String {
        match c {
            Column::Param(n) => format!("param:{n}"),
            Column::Suppress(id) => format!("suppress:{}", self.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string())),
            Column::Material(body) => format!("material:{body}"),
            Column::Appearance(body) => format!("appearance:{body}"),
            Column::ComponentAppearance(id) => format!("component_appearance:{id}"),
            Column::FaceAppearance(target) => format!("face_appearance:{}", target.key()),
        }
    }

    /// Set the design to a row's values (all or nothing).
    pub fn apply_configuration(&mut self, row: &str) -> Result<()> {
        self.apply_configuration_with_trace(row).map(|_| ())
    }

    pub fn apply_configuration_with_trace(&mut self, row: &str) -> Result<Vec<RuleTrace>> {
        let r = self.configs.rows.iter().find(|r| r.name == row).cloned().ok_or_else(|| DocError::Unknown(format!("configuration `{row}`")))?;
        if self.configs.columns.len() > MAX_COLUMNS || self.configs.rows.len() > MAX_ROWS || r.cells.len() != self.configs.columns.len() {
            return Err(DocError::Invalid("configuration table exceeds limits or has incomplete rows".into()));
        }
        let mut next = self.clone();
        let changes: Vec<(String, String)> = self
            .configs
            .columns
            .iter()
            .zip(&r.cells)
            .filter_map(|(c, v)| match c {
                Column::Param(name) => Some((name.clone(), v.clone())),
                _ => None,
            })
            .collect();
        next.change_params_atomic(&changes).map_err(|e| DocError::Invalid(format!("configuration `{row}`: {e}")))?;
        for (c, v) in self.configs.columns.iter().zip(&r.cells) {
            if v.len() > 4096 {
                return Err(DocError::Invalid("configuration value too long".into()));
            }
            match c {
                Column::Param(_) => {}
                Column::Suppress(id) => {
                    let on = suppression_cell(v)?;
                    let f = next.feature_mut(*id).ok_or_else(|| DocError::Unknown(format!("configuration `{row}`: feature {id} is gone")))?;
                    f.suppressed = on;
                }
                Column::Material(body) => match self.material_cell(v)? {
                    Some(material) => {
                        next.materials.insert(body.clone(), material);
                    }
                    None => {
                        next.materials.remove(body);
                    }
                },
                Column::Appearance(body) => match appearance_cell(v)? {
                    Some(look) => {
                        next.appearances.bodies.insert(body.clone(), look);
                    }
                    None => {
                        next.appearances.bodies.remove(body);
                    }
                },
                Column::ComponentAppearance(id) => {
                    if *id == 0 || !next.components.iter().any(|component| component.id == *id) {
                        return Err(DocError::Unknown(format!("component {id} is gone")));
                    }
                    match appearance_cell(v)? {
                        Some(look) => {
                            next.appearances.components.insert(*id, look);
                        }
                        None => {
                            next.appearances.components.remove(id);
                        }
                    }
                }
                Column::FaceAppearance(target) => {
                    next.certify_config_face(target)?;
                    next.appearances.faces.retain(|face| !target.matches(face));
                    if let Some(look) = appearance_cell(v)? {
                        next.appearances.faces.push(crate::appearance::FaceLook {
                            body: target.body.clone(),
                            point: target.point,
                            look,
                            name: Some(target.name.clone()),
                            strict_name: true,
                            certificate: target.certificate.clone(),
                        });
                    }
                }
            }
        }
        next.configs.active = Some(row.to_string());
        let trace = next.evaluate_configuration_rules()?;
        *self = next;
        Ok(trace)
    }

    /// The active row's values all hold in the design.
    pub fn configuration_matches(&self) -> bool {
        let Some(r) = self.configs.active.as_ref().and_then(|a| self.configs.rows.iter().find(|r| r.name == *a)) else { return false };
        if r.cells.len() != self.configs.columns.len() {
            return false;
        }
        if !self.configs.rules.is_empty() {
            let mut expected = self.clone();
            if expected.apply_configuration(&r.name).is_err() {
                return false;
            }
            return self.configs.columns.iter().all(|column| self.config_current(column) == expected.config_current(column));
        }
        self.configs.columns.iter().zip(&r.cells).all(|(c, v)| match c {
            Column::Suppress(id) => suppression_cell(v).is_ok_and(|on| self.feature(*id).is_some_and(|f| f.suppressed == on)),
            Column::Param(name) => {
                let kind = self
                    .features
                    .iter()
                    .find_map(|f| f.param_names.iter().position(|n| n == name).and_then(|k| f.kind.inputs().get(k).map(|input| input.2)));
                let expected = kind.map_or_else(|| v.trim().to_string(), |kind| self.with_design_unit(v, kind));
                self.config_current(c)
                    .is_some_and(|current| current.chars().filter(|c| !c.is_whitespace()).eq(expected.chars().filter(|c| !c.is_whitespace())))
            }
            Column::Material(body) => self.material_cell(v).is_ok_and(|m| self.materials.get(body) == m.as_ref()),
            Column::Appearance(body) => appearance_cell(v).is_ok_and(|look| self.appearances.bodies.get(body) == look.as_ref()),
            Column::ComponentAppearance(id) => appearance_cell(v).is_ok_and(|look| self.appearances.components.get(id) == look.as_ref()),
            Column::FaceAppearance(target) => appearance_cell(v).is_ok_and(|look| self.config_face_look(target) == look),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_use_final_graph_and_missing_targets_never_reappear() {
        let mut d = Document::new("A");
        d.set_param("a", "1 mm", None, None).unwrap();
        d.set_param("b", "a", None, None).unwrap();
        d.configs.columns = vec![Column::Param("a".into()), Column::Param("b".into())];
        d.configs.rows.push(ConfigRow { name: "Reordered".into(), cells: vec!["b".into(), "2 mm".into()] });
        d.apply_configuration("Reordered").unwrap();
        assert!(d.configuration_matches());
        assert_eq!(d.param_values().0["a"].v, 2.0);
        d.configs.rows.push(ConfigRow { name: "Cycle".into(), cells: vec!["b".into(), "a".into()] });
        let before = d.clone();
        assert!(d.apply_configuration("Cycle").is_err());
        assert_eq!(d, before);
        d.params.retain(|p| p.name != "a");
        let before = d.clone();
        assert!(d.apply_configuration("Reordered").is_err());
        assert_eq!(d, before);
    }

    #[test]
    fn bulk_changes_reject_new_errors_in_unchanged_dependents() {
        let mut d = Document::new("A");
        d.set_param("length", "2 mm", None, None).unwrap();
        d.set_param("twice", "length * 2", None, None).unwrap();
        let before = d.clone();
        assert!(d.change_params_atomic(&[("length".into(), "1 / 0".into())]).is_err());
        assert_eq!(d, before);
        assert!(d.change_params_atomic(&[("length".into(), "3 mm".into()), ("length".into(), "4 mm".into())]).is_err());
        assert_eq!(d, before);
    }

    #[test]
    fn unchanged_erroneous_configured_parameter_is_a_noop_but_changed_error_rejects() {
        let mut d = Document::new("Existing broken design");
        d.set_param("width", "10 mm", None, None).unwrap();
        d.params[0].expr = "1/".into();
        d.configs.columns = vec![Column::Param("width".into())];
        d.configs.rows.push(ConfigRow { name: "Default".into(), cells: vec!["1/".into()] });
        d.apply_configuration("Default").unwrap();
        assert!(d.configuration_matches());
        d.configs.rows.push(ConfigRow { name: "Different error".into(), cells: vec!["missing + 1".into()] });
        let before = d.clone();
        assert!(d.apply_configuration("Different error").is_err());
        assert_eq!(d, before);
    }

    #[test]
    fn rows_switch_parameters() {
        let mut d = Document::new("A");
        d.set_param("w", "10 mm", None, None).unwrap();
        d.configs.columns.push(Column::Param("w".into()));
        d.configs.rows.push(ConfigRow { name: "Small".into(), cells: vec!["10 mm".into()] });
        d.configs.rows.push(ConfigRow { name: "Large".into(), cells: vec!["40 mm".into()] });
        d.apply_configuration("Large").unwrap();
        assert_eq!(d.config_current(&Column::Param("w".into())).as_deref(), Some("40 mm"));
        assert!(d.configuration_matches());
        d.set_param("w", "12 mm", None, None).unwrap();
        assert!(!d.configuration_matches());
        d.configs.rows.push(ConfigRow { name: "Bad".into(), cells: vec!["1/".into()] });
        let before = d.clone();
        assert!(d.apply_configuration("Bad").is_err());
        assert_eq!(d, before, "all or nothing");
    }

    #[test]
    fn invalid_saved_appearance_rolls_back_material_and_parameters() {
        let mut d = Document::new("A");
        d.set_param("width", "10 mm", None, None).unwrap();
        d.configs.columns = vec![Column::Param("width".into()), Column::Material("Body".into()), Column::Appearance("Body".into())];
        d.configs.rows.push(ConfigRow {
            name: "Broken".into(),
            cells: vec!["20 mm".into(), "Steel".into(), r#"{"name":"Invalid","color":[1,2,3],"opacity":2}"#.into()],
        });
        let before = d.clone();
        assert!(d.apply_configuration("Broken").is_err());
        assert_eq!(d, before);
        assert!(appearance_cell(r#"{"name":"Invalid","color":[1,2,3],"opacity":1e999}"#).is_err());
        assert!(appearance_cell(&"a".repeat(4097)).is_err());
        assert!(appearance_cell("default").unwrap().is_none());
    }
}
