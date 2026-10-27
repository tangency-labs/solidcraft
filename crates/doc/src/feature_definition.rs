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

/// The derived fields, each with its test that a change of it is only what evaluation or Update
/// Links write (`self` after, the argument before).
const DERIVED: [(&str, fn(&Feature, &Feature) -> bool); 2] = [("body_names", Feature::pinned_from), ("link_faces", Feature::relinked_from)];

impl Feature {
    /// What saved field `field` of this feature is.
    pub fn field_role(&self, field: &str) -> FieldRole {
        match field {
            "id" | "type" | "name" | "suppressed" => FieldRole::Identity,
            "param_names" | "edge_refs" | "edge_names" | "face_names" => FieldRole::Bookkeeping,
            _ if DERIVED.iter().any(|(k, _)| *k == field) => FieldRole::Derived,
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

    /// Whether derived field `field` differs from `before`'s only as evaluation and Update Links
    /// write it. False for any other field.
    pub fn only_written_back(&self, before: &Feature, field: &str) -> bool {
        DERIVED.iter().find(|(k, _)| *k == field).is_some_and(|(_, written_back)| written_back(self, before))
    }

    /// Pinning keeps every name that was set, and fills an empty slot only with a name
    /// evaluation hands out (`Body3`, or `Ribs (4)` after the first name): a person's own name
    /// there is a rename.
    fn pinned_from(&self, before: &Feature) -> bool {
        let given = |name: &str| name.trim().is_empty() || crate::is_counted_body_name(name) || crate::eval::split_piece_of(name).is_some();
        let kept = before.body_names.iter().enumerate().all(|(k, name)| name.trim().is_empty() || self.body_names.get(k) == Some(name));
        kept && self.body_names.iter().enumerate().all(|(k, name)| before.body_names.get(k).is_some_and(|b| !b.trim().is_empty()) || given(name))
    }

    /// Update Links writes a linked design's face names with its new STEP text.
    fn relinked_from(&self, before: &Feature) -> bool {
        matches!((&self.kind, &before.kind), (FeatureKind::Import { step: a, .. }, FeatureKind::Import { step: b, .. }) if a != b)
    }

    /// The heavy fields of this kind of feature, in the order `timeline.inspect` lists them.
    /// [`Feature::without_heavy`] and [`Feature::heavy_differs`] name the same fields.
    fn heavy_fields(&self) -> &'static [(&'static str, &'static str)] {
        match self.kind {
            FeatureKind::Sketch { .. } => &[("sketch", "the sketch geometry (sketch.inspect; edit it with the sketch commands)")],
            FeatureKind::Import { .. } => &[("step", "the imported STEP text"), ("components", "the imported product tree")],
            FeatureKind::MeshImport { .. } => &[("meshes", "the imported meshes")],
            _ => &[],
        }
    }

    /// This feature with its heavy fields emptied: cheap to copy and serialise. Compare those
    /// with [`Feature::heavy_differs`].
    pub(crate) fn without_heavy(&self) -> Feature {
        let kind = match &self.kind {
            FeatureKind::Sketch { plane, .. } => FeatureKind::Sketch { plane: plane.clone(), sketch: Default::default() },
            FeatureKind::Import { file, skip, .. } => {
                FeatureKind::Import { file: file.clone(), step: "".into(), components: Vec::new(), skip: skip.clone() }
            }
            FeatureKind::MeshImport { file, .. } => FeatureKind::MeshImport { file: file.clone(), meshes: Vec::new() },
            kind => kind.clone(),
        };
        let Feature {
            id,
            name,
            suppressed,
            body_names,
            param_names,
            component,
            edge_refs,
            edge_names,
            face_names,
            participants,
            shell_piece_order,
            kernel_piece_order,
            inserted,
            link_faces,
            kind: _,
        } = self;
        Feature {
            id: *id,
            name: name.clone(),
            suppressed: *suppressed,
            body_names: body_names.clone(),
            param_names: param_names.clone(),
            component: *component,
            edge_refs: edge_refs.clone(),
            edge_names: edge_names.clone(),
            face_names: face_names.clone(),
            participants: participants.clone(),
            shell_piece_order: *shell_piece_order,
            kernel_piece_order: *kernel_piece_order,
            inserted: *inserted,
            link_faces: link_faces.clone(),
            kind,
        }
    }

    /// The heavy fields that differ from `other`'s, compared where they are.
    pub(crate) fn heavy_differs(&self, other: &Feature) -> Vec<&'static str> {
        let fields = match (&self.kind, &other.kind) {
            (FeatureKind::Sketch { sketch: a, .. }, FeatureKind::Sketch { sketch: b, .. }) => vec![("sketch", a != b)],
            (FeatureKind::Import { step: a, components: c, .. }, FeatureKind::Import { step: b, components: d, .. }) => {
                vec![("step", !std::sync::Arc::ptr_eq(a, b) && a != b), ("components", c != d)]
            }
            (FeatureKind::MeshImport { meshes: a, .. }, FeatureKind::MeshImport { meshes: b, .. }) => vec![("meshes", a != b)],
            _ => Vec::new(),
        };
        fields.into_iter().filter_map(|(field, differs)| differs.then_some(field)).collect()
    }
}
