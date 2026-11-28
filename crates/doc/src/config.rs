//! Configurations: a table whose rows are variants of the design. Each column is a parameter
//! expression, feature suppression, body material or body appearance. Activating a row sets
//! those values in the design atomically, which then rebuilds as usual.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Every feature and joint in a timeline folder and its nested folders (by folder id).
    SuppressFolder(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigRow {
    /// Fixed for the row's life, from the design's id counter ([`Document::allocate_id`]): a rename
    /// keeps it. Undo can rewind the counter; external links also check [`ConfigTable::row_identity`].
    pub id: u64,
    pub name: String,
    /// One cell per column: expression, suppression, material name or appearance look.
    pub cells: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfigTable {
    pub columns: Vec<Column>,
    pub rows: Vec<ConfigRow>,
    /// Creation identities for rows made by this version. Unlike the numeric row id, a new
    /// identity is not reused after undo. Legacy rows have no entry and keep that absence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_identities: BTreeMap<u64, u64>,
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

/// A row of the table, by its id or by its name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowKey<'a> {
    Id(u64),
    Name(&'a str),
}

impl<'a> RowKey<'a> {
    /// A row as a command or a file names it: a whole number is its id, text its name, so a row
    /// named `"17"` is not row 17. Anything else names no row.
    pub fn read(v: &'a serde_json::Value) -> Option<Self> {
        v.as_u64().map(RowKey::Id).or_else(|| v.as_str().map(RowKey::Name))
    }
}

impl<'a> From<&'a str> for RowKey<'a> {
    fn from(name: &'a str) -> Self {
        RowKey::Name(name)
    }
}

impl From<u64> for RowKey<'_> {
    fn from(id: u64) -> Self {
        RowKey::Id(id)
    }
}

impl std::fmt::Display for RowKey<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RowKey::Id(id) => write!(f, "{id}"),
            RowKey::Name(name) => f.write_str(name),
        }
    }
}

impl ConfigTable {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.rows.is_empty() && self.rules.is_empty()
    }

    /// The row `key` names: the one with that id, or with that name.
    pub fn row(&self, key: RowKey<'_>) -> Option<&ConfigRow> {
        self.rows.iter().find(|r| match key {
            RowKey::Id(id) => r.id == id,
            RowKey::Name(name) => r.name == name,
        })
    }

    /// A row's persistent creation identity, absent for a legacy row.
    pub fn row_identity(&self, id: u64) -> Option<u64> {
        self.row_identities.get(&id).copied()
    }

    /// The active row, when the table still has it.
    pub fn active_row(&self) -> Option<&ConfigRow> {
        self.row(RowKey::Name(self.active.as_deref()?))
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
    /// Lookup a timeline folder by its persistent id.
    pub fn timeline_group(&self, id: u64) -> Option<&crate::TimelineGroup> {
        self.timeline_groups.iter().find(|group| group.id == id)
    }

    /// All folder ids in the subtrees rooted at `roots`, walking nested folders iteratively.
    pub fn timeline_group_subtree(&self, roots: &[u64]) -> BTreeSet<u64> {
        let mut children = BTreeMap::<u64, Vec<u64>>::new();
        for group in &self.timeline_groups {
            if let Some(parent) = group.parent {
                children.entry(parent).or_default().push(group.id);
            }
        }
        let mut pending = roots.to_vec();
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) || self.timeline_group(id).is_none() {
                continue;
            }
            if let Some(kids) = children.get(&id) {
                pending.extend(kids.iter().copied());
            }
        }
        visited
    }

    /// Features (in timeline order) and joints (in assembly order) held by folder `id` and its nested folders.
    pub fn timeline_group_contents(&self, id: u64) -> (Vec<u64>, Vec<u64>) {
        let subtree = self.timeline_group_subtree(&[id]);
        if subtree.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let mut feature_ids = BTreeSet::new();
        let mut joint_ids = BTreeSet::new();
        for group in &self.timeline_groups {
            if subtree.contains(&group.id) {
                feature_ids.extend(group.features.iter().copied());
                joint_ids.extend(group.joints.iter().copied());
            }
        }
        let features = self.features.iter().filter(|f| feature_ids.contains(&f.id)).map(|f| f.id).collect();
        let joints = self.assembly.joints.iter().filter(|j| joint_ids.contains(&j.id)).map(|j| j.id).collect();
        (features, joints)
    }

    /// Remove configuration table columns targeting any deleted/ungrouped timeline folder in `ids`.
    pub fn remove_configuration_folders(&mut self, ids: &[u64]) {
        if ids.is_empty() {
            return;
        }
        let gone: BTreeSet<u64> = ids.iter().copied().collect();
        let keep: Vec<bool> = self.configs.columns.iter().map(|c| !matches!(c, Column::SuppressFolder(id) if gone.contains(id))).collect();
        if keep.contains(&false) {
            let mut k = keep.iter();
            self.configs.columns.retain(|_| k.next().copied().unwrap_or(true));
            for r in &mut self.configs.rows {
                let mut k = keep.iter();
                r.cells.retain(|_| k.next().copied().unwrap_or(true));
            }
        }
    }

    fn folder_suppressed(&self, id: u64) -> Option<bool> {
        self.timeline_group(id)?;
        let (features, joints) = self.timeline_group_contents(id);
        let has_contents = !features.is_empty() || !joints.is_empty();
        let all_suppressed = has_contents
            && features.iter().all(|fid| self.feature(*fid).is_some_and(|f| f.suppressed))
            && joints.iter().all(|jid| self.assembly.joints.iter().any(|j| j.id == *jid && j.suppressed));
        Some(all_suppressed)
    }

    fn folder_matches(&self, expected: &Document, id: u64) -> bool {
        if self.timeline_group(id).is_none() || expected.timeline_group(id).is_none() {
            return false;
        }
        let (features, joints) = self.timeline_group_contents(id);
        features.into_iter().all(|fid| self.feature(fid).map(|f| f.suppressed) == expected.feature(fid).map(|f| f.suppressed))
            && joints.into_iter().all(|jid| {
                self.assembly.joints.iter().find(|j| j.id == jid).map(|j| j.suppressed)
                    == expected.assembly.joints.iter().find(|j| j.id == jid).map(|j| j.suppressed)
            })
    }

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
            Column::Param(n) if self.text_param(n).is_some() => self.text_param(n).map(|p| p.text.clone()),
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
            Column::SuppressFolder(id) => self.folder_suppressed(*id).map(|on| on.to_string()),
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
            Column::SuppressFolder(id) => {
                let unique = self
                    .timeline_group(*id)
                    .filter(|group| self.timeline_groups.iter().filter(|other| other.name == group.name).count() == 1)
                    .map(|group| group.name.as_str());
                format!("suppress_folder:{}", unique.map(str::to_string).unwrap_or_else(|| id.to_string()))
            }
        }
    }

    /// Add a row to the configuration table, with an id of its own; at most [`MAX_ROWS`] rows.
    pub fn add_configuration_row(&mut self, name: String, cells: Vec<String>) -> Result<u64> {
        if self.configs.rows.len() >= MAX_ROWS {
            return Err(DocError::Invalid("too many rows".into()));
        }
        let in_use: BTreeSet<_> = self.configs.row_identities.values().copied().collect();
        let identity = crate::ids::IdSource::from_entropy().next(&in_use)?;
        let id = self.allocate_id()?;
        self.configs.rows.push(ConfigRow { id, name, cells });
        self.configs.row_identities.insert(id, identity);
        Ok(id)
    }

    /// Set the design to a row's values (all or nothing).
    pub fn apply_configuration<'a>(&mut self, row: impl Into<RowKey<'a>>) -> Result<()> {
        self.apply_configuration_with_trace(row).map(|_| ())
    }

    pub fn apply_configuration_with_trace<'a>(&mut self, row: impl Into<RowKey<'a>>) -> Result<Vec<RuleTrace>> {
        let key = row.into();
        let r = self.configs.row(key).cloned().ok_or_else(|| DocError::Unknown(format!("configuration `{key}`")))?;
        let row = r.name.as_str();
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
                Column::SuppressFolder(id) => {
                    let on = suppression_cell(v)?;
                    if next.timeline_group(*id).is_none() {
                        return Err(DocError::Unknown(format!("configuration `{row}`: folder {id} is gone")));
                    }
                    let (features, joints) = next.timeline_group_contents(*id);
                    for fid in features {
                        if let Some(f) = next.feature_mut(fid) {
                            f.suppressed = on;
                        }
                    }
                    if !joints.is_empty() {
                        let joint_set: BTreeSet<u64> = joints.into_iter().collect();
                        for joint in &mut next.assembly.joints {
                            if joint_set.contains(&joint.id) {
                                joint.suppressed = on;
                            }
                        }
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
        let Some(r) = self.configs.active_row() else { return false };
        if r.cells.len() != self.configs.columns.len() {
            return false;
        }
        let expected_with_folders =
            (!self.configs.rules.is_empty() || self.configs.columns.iter().any(|column| matches!(column, Column::SuppressFolder(_)))).then(|| {
                let mut expected = self.clone();
                expected.apply_configuration(r.id).ok().map(|()| expected)
            });
        if !self.configs.rules.is_empty() {
            let Some(Some(expected)) = expected_with_folders.as_ref() else {
                return false;
            };
            return self.configs.columns.iter().all(|column| match column {
                Column::SuppressFolder(id) => self.folder_matches(expected, *id),
                _ => self.config_current(column) == expected.config_current(column),
            });
        }
        let expected = match expected_with_folders {
            Some(Some(expected)) => Some(expected),
            Some(None) => return false,
            None => None,
        };
        self.configs.columns.iter().zip(&r.cells).all(|(c, v)| match c {
            Column::Suppress(id) => suppression_cell(v).is_ok_and(|on| self.feature(*id).is_some_and(|f| f.suppressed == on)),
            Column::SuppressFolder(id) => expected.as_ref().is_some_and(|expected| self.folder_matches(expected, *id)),
            Column::Param(name) if self.text_param(name).is_some() => self.text_param(name).is_some_and(|p| p.text == crate::text_params::unquote(v)),
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
        d.configs.rows.push(ConfigRow { id: 1, name: "Reordered".into(), cells: vec!["b".into(), "2 mm".into()] });
        d.apply_configuration("Reordered").unwrap();
        assert!(d.configuration_matches());
        assert_eq!(d.param_values().0["a"].v, 2.0);
        d.configs.rows.push(ConfigRow { id: 2, name: "Cycle".into(), cells: vec!["b".into(), "a".into()] });
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
        d.configs.rows.push(ConfigRow { id: 3, name: "Default".into(), cells: vec!["1/".into()] });
        d.apply_configuration("Default").unwrap();
        assert!(d.configuration_matches());
        d.configs.rows.push(ConfigRow { id: 4, name: "Different error".into(), cells: vec!["missing + 1".into()] });
        let before = d.clone();
        assert!(d.apply_configuration("Different error").is_err());
        assert_eq!(d, before);
    }

    /// A row is found by its id or its name, and activating it by id names it active. A deleted
    /// row's id is never given out again (#14750).
    #[test]
    fn a_row_is_found_by_its_id_or_its_name() {
        let mut d = Document::new("A");
        d.set_param("w", "10 mm", None, None).unwrap();
        d.configs.columns.push(Column::Param("w".into()));
        let (small, large) = (d.allocate_id().unwrap(), d.allocate_id().unwrap());
        d.configs.rows.push(ConfigRow { id: small, name: "Small".into(), cells: vec!["10 mm".into()] });
        d.configs.rows.push(ConfigRow { id: large, name: "Large".into(), cells: vec!["40 mm".into()] });
        assert_eq!(d.configs.row(RowKey::Id(large)).map(|r| r.name.as_str()), Some("Large"));
        assert_eq!(d.configs.row("Small".into()).map(|r| r.id), Some(small));
        d.apply_configuration(large).unwrap();
        assert_eq!((d.configs.active.as_deref(), d.configs.active_row().map(|r| r.id)), (Some("Large"), Some(large)));
        assert!(d.configuration_matches());
        d.configs.rows.retain(|r| r.id != large);
        assert!(d.configs.active_row().is_none() && !d.configuration_matches());
        assert!(d.allocate_id().unwrap() > large, "a deleted row's id is never given again");
        let gone = d.apply_configuration(large).unwrap_err().to_string();
        assert!(gone.contains(&format!("configuration `{large}`")), "{gone}");
    }

    #[test]
    fn configuration_row_identities_survive_reopen_and_distinguish_reused_numeric_ids() {
        let mut before = Document::new("Family");
        before.set_param("L", "25 mm", None, None).unwrap();
        before.configs.columns.push(Column::Param("L".into()));
        let mut first = before.clone();
        let id = first.add_configuration_row("First".into(), vec!["30 mm".into()]).unwrap();
        let identity = first.configs.row_identity(id).unwrap();
        assert_ne!(identity, 0);
        let reopened = Document::from_json(&first.to_json()).unwrap();
        assert_eq!(reopened.configs.row_identity(id), Some(identity));
        // An old snapshot can reuse the numeric id, but cannot supply this creation identity.
        let replacement = before.add_configuration_row("Replacement".into(), vec!["80 mm".into()]).unwrap();
        assert_eq!(replacement, id);
        assert_ne!(before.configs.row_identity(replacement), Some(identity));
        let mut legacy = serde_json::to_value(&first.configs).unwrap();
        legacy.as_object_mut().unwrap().remove("row_identities");
        let legacy: ConfigTable = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.row_identity(id), None);
        assert_eq!(legacy.rows, first.configs.rows);
    }

    #[test]
    fn rows_switch_parameters() {
        let mut d = Document::new("A");
        d.set_param("w", "10 mm", None, None).unwrap();
        d.configs.columns.push(Column::Param("w".into()));
        d.configs.rows.push(ConfigRow { id: 5, name: "Small".into(), cells: vec!["10 mm".into()] });
        d.configs.rows.push(ConfigRow { id: 6, name: "Large".into(), cells: vec!["40 mm".into()] });
        d.apply_configuration("Large").unwrap();
        assert_eq!(d.config_current(&Column::Param("w".into())).as_deref(), Some("40 mm"));
        assert!(d.configuration_matches());
        d.set_param("w", "12 mm", None, None).unwrap();
        assert!(!d.configuration_matches());
        d.configs.rows.push(ConfigRow { id: 7, name: "Bad".into(), cells: vec!["1/".into()] });
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
            id: 8,
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

    fn test_box(d: &mut Document, name: &str) -> u64 {
        d.add_feature(
            crate::FeatureKind::Box {
                corner: ["0".into(), "0".into(), "0".into()],
                length: "10".into(),
                width: "10".into(),
                height: "10".into(),
                operation: crate::Operation::NewBody,
                centred: false,
            },
            Some(name),
        )
        .unwrap()
    }

    fn test_joint(id: u64, name: &str) -> crate::joints::Joint {
        use crate::joints::*;
        let origin = |occurrence| JointOrigin::new(occurrence, Snap::Frame { origin: [0.0, 0.0, 0.0].into(), z: [0.0, 0.0, 1.0].into(), x: None });
        Joint {
            id,
            name: name.into(),
            kind: JointKind::Rigid,
            a: origin(1),
            b: origin(2),
            values: Vec::new(),
            limits: Vec::new(),
            flip: false,
            offset: 0.0,
            offset_xy: [0.0, 0.0],
            angle: 0.0,
            suppressed: false,
            exprs: JointExprs::default(),
            captured: None,
            base_values: None,
            base_exprs: Vec::new(),
            constraint: None,
            dynamics: Vec::new(),
        }
    }

    #[test]
    fn suppress_folder_column_applies_nested_features_joints_matches_keys_and_cleans_up() {
        let mut d = Document::new("Folders");
        let f_outer = test_box(&mut d, "OuterBox");
        let f_inner = test_box(&mut d, "InnerBox");
        let j_outer = d.allocate_id().unwrap();
        let j_inner = d.allocate_id().unwrap();
        d.assembly.joints.push(test_joint(j_outer, "OuterJoint"));
        d.assembly.joints.push(test_joint(j_inner, "InnerJoint"));

        let g_parent = d.allocate_id().unwrap();
        let g_child = d.allocate_id().unwrap();
        let g_empty = d.allocate_id().unwrap();
        d.timeline_groups = vec![
            crate::TimelineGroup { id: g_parent, name: "Group".into(), parent: None, features: vec![f_outer], joints: vec![j_outer] },
            crate::TimelineGroup { id: g_child, name: "Nested".into(), parent: Some(g_parent), features: vec![f_inner], joints: vec![j_inner] },
            crate::TimelineGroup { id: g_empty, name: "Empty".into(), parent: None, features: vec![], joints: vec![] },
        ];

        let col_parent = Column::SuppressFolder(g_parent);
        let col_empty = Column::SuppressFolder(g_empty);
        let col_override = Column::Suppress(f_inner);
        assert_eq!(d.config_key(&col_parent), "suppress_folder:Group");
        assert_eq!(d.config_current(&col_parent).as_deref(), Some("false"));
        assert_eq!(d.config_current(&col_empty).as_deref(), Some("false"));

        // Overlapping parent folder + child feature override + empty folder.
        d.configs.columns = vec![col_parent.clone(), col_override.clone(), col_empty.clone()];
        let row_off = d.add_configuration_row("On".into(), vec!["false".into(), "false".into(), "false".into()]).unwrap();
        let row_sup = d.add_configuration_row("Suppressed".into(), vec!["true".into(), "false".into(), "true".into()]).unwrap();
        let _ = row_off;

        d.apply_configuration(row_sup).unwrap();
        assert!(d.feature(f_outer).unwrap().suppressed);
        assert!(!d.feature(f_inner).unwrap().suppressed, "later child column overrides nested feature");
        assert!(d.assembly.joints.iter().find(|j| j.id == j_outer).unwrap().suppressed);
        assert!(d.assembly.joints.iter().find(|j| j.id == j_inner).unwrap().suppressed);
        assert!(d.configuration_matches(), "matches immediately after apply despite overlapping override and empty folder");

        // Manual drift on a single nested joint or feature must make configuration_matches false even when config_current is still "false".
        d.apply_configuration("On").unwrap();
        assert!(d.configuration_matches());
        d.feature_mut(f_inner).unwrap().suppressed = true;
        assert_eq!(d.config_current(&col_parent).as_deref(), Some("false"));
        assert!(!d.configuration_matches(), "mixed folder drift is detected as modified");

        // Duplicate folder names fall back to numeric id keys.
        let g_dup = d.allocate_id().unwrap();
        d.timeline_groups.push(crate::TimelineGroup { id: g_dup, name: "Group".into(), parent: None, features: vec![], joints: vec![] });
        assert_eq!(d.config_key(&Column::SuppressFolder(g_parent)), format!("suppress_folder:{g_parent}"));
        assert_eq!(d.config_key(&Column::SuppressFolder(g_dup)), format!("suppress_folder:{g_dup}"));

        // Cleaning up a folder removes its column and cells from every row.
        d.remove_configuration_folders(&[g_parent]);
        assert_eq!(d.configs.columns, vec![col_override, col_empty]);
        assert!(d.configs.rows.iter().all(|r| r.cells.len() == 2));
    }
}
