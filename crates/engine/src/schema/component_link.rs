//! Contracts and result types for the linked-component commands in `cmd/component_link.rs`:
//! `component.insert_design`, `component.update_links`, `component.relink`,
//! `component.break_link`, `component.links` and, from `cmd/component_extract.rs`,
//! `component.extract`, `component.copies` and `component.replace_with_link` (#12277).
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it and fails with the handler's message. Whether a component, a design file or a link
//! exists stays the handler's to check. A key the handler refuses when it is left out (a
//! component, a path) is optional in the type and required in the schema, so that the handler's
//! message, not the check's "is required", is what a caller sees.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::Vec3Geom as _;

use super::check::{located, whole};
use super::types::{not_blank_pattern, present};
use crate::cmd::LinkState;
use crate::params;

/// The most components `component.update_links` and `component.replace_with_link` read from a
/// list, as the handlers do.
const MAX_UPDATE: usize = 10_000;
const MAX_REPLACE: usize = 1_000;

/// A component by its id (a number) or its name, as `component_key` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentRef(pub String);

impl<'de> Deserialize<'de> for ComponentRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => Ok(ComponentRef(n.to_string())),
            Value::String(s) => Ok(ComponentRef(s)),
            _ => Err(D::Error::custom("must be a component id or name")),
        }
    }
}

impl JsonSchema for ComponentRef {
    fn schema_name() -> Cow<'static, str> {
        "ComponentRef".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A component's id (a number) or its name.", "type": ["number", "string"]})
    }
}

/// `key`, a component's id or name, with the handler's message for anything else, `null`
/// included.
pub(super) fn component_key<'de, D: Deserializer<'de>>(d: D, key: &str) -> Result<Option<ComponentRef>, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(n) => Ok(Some(ComponentRef(n.to_string()))),
        Value::String(s) => Ok(Some(ComponentRef(s))),
        _ => Err(D::Error::custom(whole(format!("`{key}` must be a component id or name")))),
    }
}

fn component<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    component_key(d, "component")
}

fn parent<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    component_key(d, "parent")
}

fn with<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ComponentRef>, D::Error> {
    component_key(d, "with")
}

/// A design file's path as the handlers read `path`: text with something in it, under 4096
/// bytes. `message` is the handler's, for anything else, `null` included.
fn design_path<'de, D: Deserializer<'de>>(d: D, message: &str) -> Result<Option<String>, D::Error> {
    match Value::deserialize(d)? {
        Value::String(s) if !s.trim().is_empty() && s.len() < 4096 => Ok(Some(s)),
        _ => Err(D::Error::custom(whole(message))),
    }
}

fn path_required<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    design_path(d, "`path` is required")
}

fn with_or_path<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    design_path(d, "`with` or `path` is required")
}

fn path_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": not_blank_pattern(), "maxLength": 4095})
}

/// Adds `key` to the schema's `required` list: for a key the type has optional so that the
/// handler, not the check, refuses it when it's left out.
pub(super) fn require(schema: &mut Schema, key: &str) {
    match schema.get_mut("required") {
        Some(Value::Array(required)) => {
            if !required.iter().any(|k| k == key) {
                required.push(key.into());
            }
        }
        _ => {
            schema.insert("required".into(), Value::Array(vec![key.into()]));
        }
    }
}

fn component_required(schema: &mut Schema) {
    require(schema, "component");
}

fn component_and_path_required(schema: &mut Schema) {
    require(schema, "component");
    require(schema, "path");
}

fn path_only_required(schema: &mut Schema) {
    require(schema, "path");
}

/// A component's `translate` as `component.insert_design` reads it: `[x, y, z]` or `{x, y, z}`
/// in mm, finite and under 1e9 in length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Translation(pub Vec3);

impl<'de> Deserialize<'de> for Translation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        params::vec3(&Value::deserialize(d)?)
            .filter(|t| t.is_finite() && t.len() < 1e9)
            .map(Translation)
            .ok_or_else(|| D::Error::custom(whole("`translate` must be [x, y, z] in mm")))
    }
}

impl JsonSchema for Translation {
    fn schema_name() -> Cow<'static, str> {
        "Translation".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let c = json!({"type": "number", "exclusiveMinimum": -1e9, "exclusiveMaximum": 1e9});
        json_schema!({
            "description": "A distance in mm, as [x, y, z] or {\"x\", \"y\", \"z\"}.",
            "anyOf": [
                {"type": "array", "items": c, "minItems": 3, "maxItems": 3},
                {"type": "object", "properties": {"x": c, "y": c, "z": c}, "required": ["x", "y", "z"]}
            ]
        })
    }
}

/// Insert a design file as a new component linked to it: its solid bodies as placed in that
/// design, and a copy of its design properties. The link remembers the file, so
/// `component.update_links` can read it again. One undo step. With `reuse`, a design already
/// linked gets another placement instead of another link.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = path_only_required)]
pub struct InsertDesignParams {
    // Optional in the type and required in the schema (`path_only_required`).
    /// The design (`.sc`) to link, by file path.
    #[serde(default, deserialize_with = "path_required", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
    /// The component's name. Default: the design's name.
    #[serde(default)]
    pub name: Option<String>,
    /// The component to put it in, by id or name. Default: the active one.
    #[serde(default, deserialize_with = "parent", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub parent: Option<ComponentRef>,
    /// Where to place it, in mm from the parent's origin. Default: [0, 0, 0], also with `reuse`.
    #[serde(default)]
    pub translate: Option<Translation>,
    /// Reuse an existing link to the same file when there is one, placing another occurrence of
    /// it. Default: false.
    #[serde(default)]
    pub reuse: Option<bool>,
}

/// What `component.insert_design` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InsertDesignResult {
    #[serde(flatten)]
    pub inserted: DesignInserted,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What an insertion made.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum DesignInserted {
    /// With `reuse`, a link to the file existed: only a new placement of its component.
    Reused {
        /// The new placement, by id.
        occurrence: u64,
        /// The linked component, by id.
        component: u64,
    },
    /// A new component, linked to the file.
    New {
        /// The new component, by id.
        component: u64,
        /// Its placement, by id.
        occurrence: Option<u64>,
        /// The component's name.
        name: String,
        /// The Import feature that holds the bodies, by id.
        feature: u64,
        /// The file as the link stores it (relative to this design's folder when it is saved).
        path: String,
        /// The bodies the link brought in.
        bodies: Vec<String>,
        /// The same bodies, as the commands that make bodies report them.
        created_bodies: Vec<String>,
        /// What the design file gave that did not come in (such as mesh bodies).
        warnings: Vec<String>,
    },
}

/// A list of components, as `component.update_links` reads `components`: one to 10000, each by
/// id or name.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentList(pub Vec<ComponentRef>);

impl<'de> Deserialize<'de> for ComponentList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let item = |v: Value| match v {
            Value::Number(n) => Ok(ComponentRef(n.to_string())),
            Value::String(s) => Ok(ComponentRef(s)),
            _ => Err(D::Error::custom(whole("`component` must be a component id or name"))),
        };
        match Value::deserialize(d)? {
            Value::Array(list) if !list.is_empty() && list.len() <= MAX_UPDATE => {
                list.into_iter().map(item).collect::<Result<_, _>>().map(ComponentList)
            }
            _ => Err(D::Error::custom(whole("`components` must be a list of component ids or names"))),
        }
    }
}

impl JsonSchema for ComponentList {
    fn schema_name() -> Cow<'static, str> {
        "ComponentList".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<ComponentRef>();
        json_schema!({
            "description": "Components, each by id or name.",
            "type": "array",
            "items": item,
            "minItems": 1,
            "maxItems": MAX_UPDATE
        })
    }
}

/// Read the linked designs again and replace the bodies of the components whose file changed,
/// as one undo step. Without `component` or `components`, every linked component is read. A
/// component that is not linked is refused.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct UpdateLinksKeys {
    /// One linked component, by id or name. Not with `components`.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// Linked components, by id or name. Not with `component`. With a list, an update goes on
    /// past a design it cannot read: that component keeps its bodies, is named in `failed` and
    /// gets a warning.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentList")]
    pub components: Option<ComponentList>,
}

/// `component.update_links`' parameters: [`UpdateLinksKeys`], with `component` and `components`
/// together refused first, as the handler does.
#[derive(Debug)]
pub struct UpdateLinksParams(pub UpdateLinksKeys);

impl<'de> Deserialize<'de> for UpdateLinksParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        if value.get("component").is_some() && value.get("components").is_some() {
            return Err(D::Error::custom(whole("give `component` or `components`, not both")));
        }
        serde_path_to_error::deserialize(&value).map(UpdateLinksParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for UpdateLinksParams {
    fn schema_name() -> Cow<'static, str> {
        "UpdateLinksParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = UpdateLinksKeys::json_schema(generator);
        schema.insert("not".into(), json!({"required": ["component", "components"]}));
        schema
    }
}

/// What `component.update_links` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateLinksResult {
    /// The components whose bodies changed, by name.
    pub updated: Vec<String>,
    /// The components read whose bodies did not change, by name.
    pub unchanged: Vec<String>,
    /// What the update noticed, also joints whose faces are gone or that no longer solve.
    pub warnings: Vec<String>,
    /// With a `components` list: the designs that could not be read.
    pub failed: Vec<LinkFailure>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A linked design `component.update_links` could not read: its component keeps its bodies.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinkFailure {
    /// The component, by id.
    pub component: u64,
    pub name: String,
    /// Why the design could not be read.
    pub error: String,
}

/// Replace the source of a linked component with another saved design, for all its occurrences,
/// keeping their identities and placements. The design is read and checked before anything
/// changes; one undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_and_path_required)]
pub struct RelinkParams {
    // Optional in the type and required in the schema (`component_and_path_required`).
    /// The linked component, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// The saved design to link instead, `.sc` or `.json`.
    #[serde(default, deserialize_with = "path_required", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
}

/// What `component.relink` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RelinkResult {
    /// The component, by id.
    pub component: u64,
    /// The Import feature that holds the linked bodies, by id.
    pub feature: u64,
    /// The new file, as the link stores it.
    pub path: String,
    /// The file the link had.
    pub previous_path: String,
    /// Whether the linked bodies are not the ones the link had.
    pub geometry_changed: bool,
    /// What the new design gave that did not come in, and joints to check.
    pub warnings: Vec<String>,
    /// Joints that no longer solve.
    pub joint_errors: Vec<JointDiagnostic>,
    /// Joints that conflict with others.
    pub joint_conflicts: Vec<JointDiagnostic>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A joint the solver reports after a relink.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JointDiagnostic {
    /// The joint, by id.
    pub joint: u64,
    pub name: String,
    pub why: String,
}

/// Forget a component's link to its design file and keep its bodies as they are. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct BreakLinkParams {
    // Optional in the type and required in the schema (`component_required`).
    /// The linked component, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
}

/// What `component.break_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BreakLinkResult {
    /// The component, by id.
    pub component: u64,
    /// The file it was linked to, as the link stored it.
    pub path: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `component.links` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct LinksParams {}

/// What `component.links` returns: every linked component and whether its file is there.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LinksResult {
    pub links: Vec<LinkInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One linked component. A file that is looked for has `found` and `changed`, and a `state` and
/// a `reason` when it can't be reached; a refused one is not looked for and has `found` null, a
/// `state` and a `reason`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LinkInfo {
    /// The file was looked for.
    Looked {
        /// The component, by id.
        component: u64,
        name: Option<String>,
        /// The link as stored.
        path: String,
        /// The file it resolves to.
        file: String,
        /// Whether the file is there; null when that can't be told.
        found: Option<bool>,
        /// Whether the file is no longer what this design last read (`component.update_links`
        /// reads it again); null when it is not found or the link is older than this record.
        changed: Option<bool>,
        /// Why the file can't be reached: `missing`, `unreadable` or `not_allowed`. Left out
        /// when it can be read.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state: Option<LinkState>,
        /// What to show about it. Left out with `state`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// The file is on a network location that is not allowed.
    Network {
        component: u64,
        name: Option<String>,
        path: String,
        file: String,
        /// Always null: it is not looked for.
        found: Option<bool>,
        /// The network location.
        host: String,
        /// Always `not_allowed`.
        state: LinkState,
        reason: String,
    },
    /// The path names a device.
    Device {
        component: u64,
        name: Option<String>,
        path: String,
        file: String,
        /// Always null: it is not looked for.
        found: Option<bool>,
        /// Always `unreadable`.
        state: LinkState,
        reason: String,
    },
}

/// Move a component's history, subcomponents and their joints into a design file of its own and
/// link the component to that file, as `component.insert_design` would have made it. The
/// component must not be linked already. Anything that crosses the component's boundary is
/// refused. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_and_path_required)]
pub struct ExtractParams {
    // Optional in the type and required in the schema (`component_and_path_required`).
    /// The component to extract, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// The design file to write, a `.sc`.
    #[serde(default, deserialize_with = "path_required", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
}

/// What `component.extract` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ExtractResult {
    /// The component, by id.
    pub component: u64,
    /// The Import feature that now holds its bodies, by id.
    pub feature: u64,
    /// The file as the link stores it.
    pub path: String,
    /// The file written.
    pub file: String,
    /// The component's bodies, by name.
    pub bodies: Vec<String>,
    /// What stays behind in this design.
    pub notices: Vec<String>,
    /// Joints to check.
    pub warnings: Vec<String>,
    /// Components that build the same bodies (see `component.replace_with_link`).
    pub copies: Vec<CopyInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A component, by id and name.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopyInfo {
    /// The component, by id.
    pub component: u64,
    pub name: Option<String>,
}

/// Find the components, not linked, whose own history builds the same bodies as a linked
/// component. The design is not changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = component_required)]
pub struct CopiesParams {
    // Optional in the type and required in the schema (`component_required`).
    /// The linked component, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
}

/// What `component.copies` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CopiesResult {
    /// The components that build the same bodies, not linked.
    pub copies: Vec<CopyInfo>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `component.replace_with_link`'s `components`: each by id or name; the handler reads the
/// first 1000 and ignores the rest.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceTargets(pub Vec<ComponentRef>);

impl<'de> Deserialize<'de> for ReplaceTargets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let item = |v: Value| match v {
            Value::Number(n) => Ok(ComponentRef(n.to_string())),
            Value::String(s) => Ok(ComponentRef(s)),
            _ => Err(D::Error::custom(whole("`component` must be a component id or name"))),
        };
        match Value::deserialize(d)? {
            Value::Array(list) => list.into_iter().take(MAX_REPLACE).map(item).collect::<Result<_, _>>().map(ReplaceTargets),
            _ => Err(D::Error::custom(whole("`components` must be a list of component ids or names"))),
        }
    }
}

impl JsonSchema for ReplaceTargets {
    fn schema_name() -> Cow<'static, str> {
        "ReplaceTargets".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<ComponentRef>();
        json_schema!({
            "description": "Components, each by id or name; only the first 1000 are used.",
            "type": "array",
            "items": item
        })
    }
}

/// Make components that are not linked, and build the same bodies as a linked component, more
/// placements of it: each one's history goes and its placements become placements of the linked
/// component. One is refused when its bodies are not the same as the link's. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = replace_required)]
pub struct ReplaceWithLinkKeys {
    // The schema asks for `component` or `components`, and `with` or `path` (`replace_required`).
    /// One component to replace, by id or name.
    #[serde(default, deserialize_with = "component", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub component: Option<ComponentRef>,
    /// Components to replace, by id or name; used with `component`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ReplaceTargets")]
    pub components: Option<ReplaceTargets>,
    /// The linked component to place instead, by id or name.
    #[serde(default, deserialize_with = "with", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ComponentRef")]
    pub with: Option<ComponentRef>,
    /// Without `with`: a design file (`.sc`). The component linked to it here, or else a new
    /// linked component.
    #[serde(default, deserialize_with = "with_or_path", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "path_schema")]
    pub path: Option<String>,
}

/// `component.replace_with_link`'s parameters: [`ReplaceWithLinkKeys`], with `path` left unread
/// when `with` is given, as the handler leaves it.
#[derive(Debug)]
pub struct ReplaceWithLinkParams(pub ReplaceWithLinkKeys);

impl<'de> Deserialize<'de> for ReplaceWithLinkParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = Value::deserialize(d)?;
        if let Some(keys) = value.as_object_mut().filter(|o| o.contains_key("with")) {
            keys.remove("path");
        }
        serde_path_to_error::deserialize(&value).map(ReplaceWithLinkParams).map_err(|e| D::Error::custom(located(&e)))
    }
}

impl JsonSchema for ReplaceWithLinkParams {
    fn schema_name() -> Cow<'static, str> {
        "ReplaceWithLinkParams".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ReplaceWithLinkKeys::json_schema(generator)
    }
}

fn replace_required(schema: &mut Schema) {
    let at_least_one = |keys: [&str; 2]| json!({"anyOf": keys.map(|k| json!({"required": [k]}))});
    schema.insert("allOf".into(), json!([at_least_one(["component", "components"]), at_least_one(["with", "path"])]));
}

/// What `component.replace_with_link` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReplaceWithLinkResult {
    /// The linked component that now has the placements, by id.
    pub component: u64,
    pub name: String,
    /// The components that went, by name.
    pub replaced: Vec<String>,
    /// Joints to check.
    pub warnings: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "component_link_tests.rs"]
mod tests;
