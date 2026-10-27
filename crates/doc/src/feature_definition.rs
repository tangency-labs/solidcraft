//! What each saved field of a feature is (#5184, CE8): who the feature is, the names kept for its
//! inputs and picks, large payloads, the fields evaluation writes back, and the rest, which a
//! person sets. The one rule `timeline.inspect`, `timeline.edit` and the design compare read.

use serde_json::{Map, Value};

use crate::{DocError, Feature, FeatureKind, Result};

/// What a saved field of a feature is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldRole {
    /// Who the feature is (`id`, `type`, `name`, `suppressed`): their own commands change them.
    Identity,
    /// Kept for the inputs and picks, and worked out again when they change (`timeline.edit` does):
    /// the inputs' parameter names, and the picked edges' and faces' names.
    Bookkeeping,
    /// A large payload, left out of the definition: what it holds.
    Heavy(&'static str),
    /// In the definition, but evaluation or Update Links write it too: the bodies' names (pinned
    /// as they are named) and a linked design's face names.
    Derived,
    /// What a person sets.
    Input,
}

impl FieldRole {
    /// Whether a field of this role is in the definition (`timeline.inspect` shows it, and
    /// `timeline.edit` changes it).
    pub fn in_definition(self) -> bool {
        matches!(self, FieldRole::Input | FieldRole::Derived)
    }
}

/// A feature's definition: what `timeline.inspect` shows and `timeline.edit`'s `set` changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Definition {
    /// The input and derived fields, as saved.
    pub fields: Map<String, Value>,
    /// The heavy fields the feature has, left out: (field, what it holds).
    pub omitted: Vec<(&'static str, &'static str)>,
}

impl Feature {
    /// What saved field `field` of this feature is.
    pub fn field_role(&self, field: &str) -> FieldRole {
        match field {
            "id" | "type" | "name" | "suppressed" => FieldRole::Identity,
            "param_names" | "edge_refs" | "edge_names" | "face_names" => FieldRole::Bookkeeping,
            "body_names" | "link_faces" => FieldRole::Derived,
            _ => self.heavy_fields().iter().find(|(k, _)| *k == field).map_or(FieldRole::Input, |(_, what)| FieldRole::Heavy(what)),
        }
    }

    /// The feature's definition. Costs one serialisation of the feature, its heavy fields included.
    pub fn definition(&self) -> Result<Definition> {
        let mut fields = match serde_json::to_value(self).map_err(|e| DocError::Invalid(e.to_string()))? {
            Value::Object(o) => o,
            _ => Map::new(),
        };
        let omitted = self.heavy_fields().iter().filter(|(k, _)| fields.contains_key(*k)).copied().collect();
        fields.retain(|k, _| self.field_role(k).in_definition());
        Ok(Definition { fields, omitted })
    }

    /// The heavy fields of this kind of feature, in the order `timeline.inspect` lists them.
    fn heavy_fields(&self) -> &'static [(&'static str, &'static str)] {
        match self.kind {
            FeatureKind::Sketch { .. } => &[("sketch", "the sketch geometry (sketch.inspect; edit it with the sketch commands)")],
            FeatureKind::Import { .. } => &[("step", "the imported STEP text"), ("components", "the imported product tree")],
            FeatureKind::MeshImport { .. } => &[("meshes", "the imported meshes")],
            _ => &[],
        }
    }
}
