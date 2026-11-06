//! Contracts and result types for the commands in `cmd/browser.rs`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use solidcraft_doc::BrowserEntry;

use super::check::whole;
use super::types::{FeatureCommon, LengthExpr, feature_common, present};

/// Repair a part the modelling kernel refuses, or an open part that should be a solid: faces
/// SMLib refuses on their own are rebuilt on their own surfaces (geometry unchanged), free edges
/// that pair up within `gap` are sewn (each sewn edge carries its own gap as its tolerance; the
/// body's and the other edges' stay), and with `patch_holes` the loops of free edges left (missing
/// faces) are patched and sewn in. One undoable feature; refused, with why, when nothing applies.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RepairParams {
    /// The part (body name).
    pub body: String,
    /// The widest gap between two free edges that is sewn, more than 0 and at most 1 mm.
    /// Default: 0.2 mm.
    #[serde(default)]
    pub gap: Option<LengthExpr>,
    /// Patch each loop of free edges no pair explains (a missing face) and sew the patch in: the
    /// plane of the faces around it where they are all that plane, else a patch tangent to them
    /// (G1). Default: false (the reply offers the patches).
    #[serde(default)]
    pub patch_holes: Option<bool>,
    #[serde(flatten, deserialize_with = "feature_common")]
    pub common: FeatureCommon,
}

/// A face rebuilt on its own surface, or one SMLib still refuses.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairFace {
    /// The face's index in the part's face order before the repair (from 0).
    pub face: usize,
    /// Its surface: plane, cylinder, cone, sphere, torus or freeform.
    pub surface: String,
    /// What SMLib said about it.
    pub why: String,
}

/// Two free edges sewn into one.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairSewn {
    /// The faces on either side, by index before the repair.
    pub faces: [usize; 2],
    /// The widest distance between the two edges, mm.
    pub gap: f64,
    /// A point halfway along the gap, [x, y, z].
    pub at: [f64; 3],
}

/// A loop of free edges no pair explains: a missing face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairHole {
    /// The faces around it, by index before the repair.
    pub faces: Vec<usize>,
    /// g0 (a patch in the plane of the faces around it) or g1 (tangent to curved neighbours).
    pub continuity: String,
    /// The patch was made and sewn in.
    pub patched: bool,
    /// Why it was not, when `patch_holes` asked for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The Patch that fills it: `{command: "surface.patch", params}`.
    pub patch: Value,
}

/// What the repair did, face by face.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairDone {
    pub rebuilt: Vec<RepairFace>,
    /// Faces SMLib refuses that could not be rebuilt (the part may still be taken as a whole).
    pub refused: Vec<RepairFace>,
    pub sewn: Vec<RepairSewn>,
    pub holes: Vec<RepairHole>,
    /// Faces before and after.
    pub faces: [usize; 2],
    /// The largest edge tolerance before and after, mm.
    pub largest_edge_tolerance: [f64; 2],
    /// The part is a closed solid now.
    pub closed: bool,
    /// The modelling kernel takes the part now.
    pub modelling: bool,
    /// Its volume, mm³, when it is a closed solid.
    pub volume: Option<f64>,
}

/// What `body.repair` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RepairResult {
    /// The new feature's id.
    pub feature: u64,
    /// The new feature's name.
    pub name: String,
    /// Every body after the feature.
    pub bodies: Vec<String>,
    /// The bodies the feature made.
    pub created_bodies: Vec<String>,
    /// Bodies that were there before and now have other geometry.
    pub changed_bodies: Vec<String>,
    /// Bodies that are gone.
    pub removed_bodies: Vec<String>,
    /// A warning from the evaluation (a hole left open), or `null`.
    pub warning: Option<String>,
    /// How long the feature took to evaluate, in milliseconds.
    pub ms: Option<f64>,
    pub repair: RepairDone,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The units a design can show lengths in.
pub const DESIGN_UNITS: [&str; 5] = ["mm", "cm", "m", "in", "ft"];

/// One of [`DESIGN_UNITS`], with `document.units`' message for anything else.
#[derive(Clone, Debug, PartialEq)]
pub struct DesignUnits(pub String);

impl<'de> Deserialize<'de> for DesignUnits {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(u) if DESIGN_UNITS.contains(&u.as_str()) => Ok(DesignUnits(u)),
            _ => Err(D::Error::custom("must be mm, cm, m, in or ft")),
        }
    }
}

impl JsonSchema for DesignUnits {
    fn schema_name() -> Cow<'static, str> {
        "DesignUnits".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "The units the design shows lengths in.", "type": "string", "enum": DESIGN_UNITS})
    }
}

/// Change the units the design shows lengths in, and reads a bare number in. Values keep their
/// meaning: a unit-less parameter holding a bare number that lengths read is written with the
/// old unit (`10` becomes `10 mm`) when that keeps those lengths where they were. One undo
/// step; the same units again changes nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct DocumentUnitsParams {
    /// The design's new units: mm, cm, m, in or ft.
    pub units: DesignUnits,
}

/// What `document.units` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DocumentUnitsResult {
    /// The design's units now.
    pub units: String,
    /// The unit-less parameters now written with the old unit, so the lengths that read them
    /// stay where they were. Empty when nothing needed it, or the units didn't change.
    pub kept: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

// ---- Browser folders and Rename Body (#12276) ----
//
// The handlers read their parameters from the raw JSON, so each key here reads as its handler
// reads it and fails with the handler's message. Whether a group, component, placement, joint,
// feature or body exists stays the handler's to check, and so do the keys that exclude one
// another. A key the handler reads only as text, a number or a flag, and otherwise takes as
// absent, is a plain optional field: the contract is the stricter of the two. A key the type
// has optional but the handler can't do without is required in the schema (`require`), so that
// the handler's own message, not "is required", is what a call that leaves it out gets.

/// The most keys, entries or paths a browser list may name, as the handlers cap them.
const MAX_LIST: usize = 10_000;
/// The longest item key, in bytes.
const MAX_KEY_BYTES: usize = 256;
/// The deepest placement path, in occurrence ids.
const MAX_PATH: usize = 64;
/// The longest group or body name, in bytes.
const MAX_NAME_BYTES: usize = 128;

/// The folders the browser lists items in, which a group or a move is in.
const FOLDERS: [&str; 3] = ["bodies", "sketches", "construction"];
/// The folders `browser.visibility` takes whole.
const VISIBILITY_FOLDERS: [&str; 6] = ["bodies", "sketches", "construction", "origin", "joints", "all"];

/// Adds each of `keys` to the schema's `required` list.
fn require(schema: &mut Schema, keys: &[&str]) {
    let mut required: Vec<Value> = schema.get("required").and_then(Value::as_array).cloned().unwrap_or_default();
    for key in keys {
        if !required.iter().any(|k| k == key) {
            required.push((*key).into());
        }
    }
    schema.insert("required".into(), Value::Array(required));
}

fn group_required(schema: &mut Schema) {
    require(schema, &["group"]);
}

fn group_and_name_required(schema: &mut Schema) {
    require(schema, &["group", "name"]);
}

fn body_and_name_required(schema: &mut Schema) {
    require(schema, &["body", "name"]);
}

fn folder_and_items_required(schema: &mut Schema) {
    require(schema, &["folder", "items"]);
}

fn folder_required(schema: &mut Schema) {
    require(schema, &["folder"]);
}

fn visible_required(schema: &mut Schema) {
    require(schema, &["visible"]);
}

/// A browser group's id, as `cmd/browser.rs` reads one: a non-negative whole number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupId(pub u64);

impl<'de> Deserialize<'de> for GroupId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(GroupId).ok_or_else(|| D::Error::custom("must be a group id"))
    }
}

impl JsonSchema for GroupId {
    fn schema_name() -> Cow<'static, str> {
        "GroupId".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A browser group's id (see `browser.group`).", "type": "integer", "minimum": 0})
    }
}

/// `group` as `group_id` reads it: given, `null` included, a group id.
fn some_group<'de, D: Deserializer<'de>>(d: D) -> Result<Option<GroupId>, D::Error> {
    GroupId::deserialize(d).map(Some)
}

/// One of the three browser folders a group or an item list is in.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserFolder(pub String);

impl<'de> Deserialize<'de> for BrowserFolder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(f) if FOLDERS.contains(&f.as_str()) => Ok(BrowserFolder(f)),
            _ => Err(D::Error::custom(whole("`folder` must be bodies, sketches or construction"))),
        }
    }
}

impl JsonSchema for BrowserFolder {
    fn schema_name() -> Cow<'static, str> {
        "BrowserFolder".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A browser folder: bodies, sketches or construction.", "type": "string", "enum": FOLDERS})
    }
}

/// `browser.order`'s folder: one of the three, or `components`, the placed sub-components.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderFolder(pub String);

impl<'de> Deserialize<'de> for OrderFolder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(f) if f == "components" || FOLDERS.contains(&f.as_str()) => Ok(OrderFolder(f)),
            _ => Err(D::Error::custom(whole("`folder` must be bodies, sketches or construction"))),
        }
    }
}

impl JsonSchema for OrderFolder {
    fn schema_name() -> Cow<'static, str> {
        "OrderFolder".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A browser folder: bodies, sketches, construction or components.",
            "type": "string",
            "enum": ["bodies", "sketches", "construction", "components"]
        })
    }
}

/// A group's name as `browser.group` reads it: surrounding spaces are cut; nothing left means
/// the default name.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupName(pub String);

impl<'de> Deserialize<'de> for GroupName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(n) if n.trim().len() > MAX_NAME_BYTES => Err(D::Error::custom(whole("`name` is too long"))),
            Value::String(n) => Ok(GroupName(n)),
            _ => Err(D::Error::custom("must be text")),
        }
    }
}

impl JsonSchema for GroupName {
    fn schema_name() -> Cow<'static, str> {
        "GroupName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A name of at most 128 bytes once surrounding spaces are cut.", "type": "string"})
    }
}

/// A name of 1 to 128 bytes once surrounding spaces are cut, as `browser.rename_group` and
/// `body.rename` read `name`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewName(pub String);

impl<'de> Deserialize<'de> for NewName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(n) if !n.trim().is_empty() && n.trim().len() <= MAX_NAME_BYTES => Ok(NewName(n)),
            _ => Err(D::Error::custom(whole("`name` must be 1…128 characters"))),
        }
    }
}

impl JsonSchema for NewName {
    fn schema_name() -> Cow<'static, str> {
        "NewName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "Text of 1 to 128 bytes once surrounding spaces are cut.", "type": "string"})
    }
}

/// `name` for the commands that can't do without it: given, `null` included, a [`NewName`].
fn some_new_name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<NewName>, D::Error> {
    NewName::deserialize(d).map(Some)
}

/// The name of a body, as `body.rename` reads `body`.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyName(pub String);

impl<'de> Deserialize<'de> for BodyName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(n) => Ok(BodyName(n)),
            _ => Err(D::Error::custom(whole("`body` must be a body name"))),
        }
    }
}

impl JsonSchema for BodyName {
    fn schema_name() -> Cow<'static, str> {
        "BodyName".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A body's name. A placed copy's name (`KB (K:2)`) means the body it shows.", "type": "string"})
    }
}

fn some_body<'de, D: Deserializer<'de>>(d: D) -> Result<Option<BodyName>, D::Error> {
    BodyName::deserialize(d).map(Some)
}

/// What `entries` in `cmd/browser.rs` reads of a list of groups (numbers) and item keys
/// (strings): at most 10000, no key empty or over 256 bytes. `key` names the parameter in the
/// messages.
fn browser_entries(value: Value, key: &str) -> Result<Vec<BrowserEntry>, String> {
    let list = match value {
        Value::Array(a) => a
            .into_iter()
            .map(|x| match x {
                Value::String(s) => Some(BrowserEntry::Item(s)),
                _ => x.as_u64().map(BrowserEntry::Group),
            })
            .collect::<Option<Vec<_>>>(),
        _ => None,
    };
    let list = list.ok_or_else(|| whole(format!("`{key}` must be a list of item keys (strings) and group ids (numbers)")))?;
    if list.len() > MAX_LIST || list.iter().any(|e| e.as_item().is_some_and(|k| k.is_empty() || k.len() > MAX_KEY_BYTES)) {
        return Err(whole(format!("`{key}` has bad keys")));
    }
    Ok(list)
}

/// `browser.move`'s `items`: item keys and group ids, at least one.
#[derive(Clone, Debug, PartialEq)]
pub struct MovedEntries(pub Vec<BrowserEntry>);

impl<'de> Deserialize<'de> for MovedEntries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = browser_entries(Value::deserialize(d)?, "items").map_err(D::Error::custom)?;
        if list.is_empty() { Err(D::Error::custom(whole("`items` must list item keys or group ids"))) } else { Ok(MovedEntries(list)) }
    }
}

/// `browser.order`'s `order`: item keys and top-level group ids, in display order; none clears
/// the folder's order.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderEntries(pub Vec<BrowserEntry>);

impl<'de> Deserialize<'de> for OrderEntries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        browser_entries(Value::deserialize(d)?, "order").map(OrderEntries).map_err(D::Error::custom)
    }
}

/// `browser.group`'s `items`: item keys, no group ids.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupedItems(pub Vec<BrowserEntry>);

impl<'de> Deserialize<'de> for GroupedItems {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = browser_entries(Value::deserialize(d)?, "items").map_err(D::Error::custom)?;
        if list.iter().any(|e| e.as_item().is_none()) {
            Err(D::Error::custom(whole("`items` are item keys: move groups with browser.move")))
        } else {
            Ok(GroupedItems(list))
        }
    }
}

fn entries_schema(description: &str, group_ids: bool, min_items: u32) -> Schema {
    let item = json!({"type": "string", "minLength": 1, "maxLength": MAX_KEY_BYTES});
    let entry = if group_ids { json!({"anyOf": [item, {"type": "integer", "minimum": 0}]}) } else { item };
    json_schema!({"description": description, "type": "array", "items": entry, "minItems": min_items, "maxItems": MAX_LIST})
}

impl JsonSchema for MovedEntries {
    fn schema_name() -> Cow<'static, str> {
        "MovedEntries".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        entries_schema("Item keys (strings) and group ids (numbers); a number is always a group.", true, 1)
    }
}

impl JsonSchema for OrderEntries {
    fn schema_name() -> Cow<'static, str> {
        "OrderEntries".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        entries_schema("Item keys (strings) and top-level group ids (numbers); a number is always a group.", true, 0)
    }
}

impl JsonSchema for GroupedItems {
    fn schema_name() -> Cow<'static, str> {
        "GroupedItems".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        entries_schema("Item keys (strings).", false, 0)
    }
}

/// Where `browser.move` puts items: before an item key or a group id.
#[derive(Clone, Debug, PartialEq)]
pub struct BeforeEntry(pub BrowserEntry);

impl<'de> Deserialize<'de> for BeforeEntry {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(k) => Ok(BeforeEntry(BrowserEntry::Item(k))),
            v => v.as_u64().map(|id| BeforeEntry(BrowserEntry::Group(id))).ok_or_else(|| D::Error::custom("must be an item key or a group id")),
        }
    }
}

impl JsonSchema for BeforeEntry {
    fn schema_name() -> Cow<'static, str> {
        "BeforeEntry".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "An item key (text) or a group id (a number).", "anyOf": [{"type": "string"}, {"type": "integer", "minimum": 0}]})
    }
}

/// An item key or a group id in a reply.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EntryKey {
    /// A group, by id.
    Group(u64),
    /// An item, by key.
    Item(String),
}

/// Make a group in a Browser folder, of the items given or empty. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BrowserGroupParams {
    /// The folder: bodies, sketches or construction. Needed unless `parent` gives it.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BrowserFolder")]
    pub folder: Option<BrowserFolder>,
    /// The component whose folder it is. Default: 0, the design.
    #[serde(default)]
    pub component: Option<u64>,
    /// The group the new one nests in, which gives it its component and folder (at most 8
    /// levels).
    #[serde(default)]
    pub parent: Option<GroupId>,
    /// The group's name, at most 128 bytes. Default: "Group" and the next number in the folder.
    #[serde(default)]
    pub name: Option<GroupName>,
    /// The items to put in it, by key; they leave the other groups they were in.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "GroupedItems")]
    pub items: Option<GroupedItems>,
}

/// What `browser.group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserGroupMade {
    /// The new group's id.
    pub group: u64,
    pub name: String,
    /// The group it is in, or `null` at the folder's top level.
    pub parent: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Remove a group: its items and child groups go up to the group that holds it, or the folder,
/// in the place the group had. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = group_required)]
pub struct BrowserUngroupParams {
    // Required in the schema (`group_required`), optional in the type: left out, the handler
    // refuses it with its own message.
    #[serde(default, deserialize_with = "some_group", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "GroupId")]
    pub group: Option<GroupId>,
}

/// What `browser.ungroup` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserUngrouped {
    /// The group that is gone.
    pub group: u64,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Rename a Browser group. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = group_and_name_required)]
pub struct BrowserRenameGroupParams {
    #[serde(default, deserialize_with = "some_group", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "GroupId")]
    pub group: Option<GroupId>,
    /// The new name, 1 to 128 characters once surrounding spaces are cut.
    #[serde(default, deserialize_with = "some_new_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NewName")]
    pub name: Option<NewName>,
}

/// What `browser.rename_group` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserGroupRenamed {
    pub group: u64,
    /// The name now, without surrounding spaces.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Move items and groups in a Browser folder: into a group or to the folder's top level, before
/// an item or group or at the end. A group can't go into itself or past 8 levels. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = folder_and_items_required)]
pub struct BrowserMoveParams {
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BrowserFolder")]
    pub folder: Option<BrowserFolder>,
    /// The component whose folder it is. Default: 0, the design.
    #[serde(default)]
    pub component: Option<u64>,
    /// What to move: item keys (strings) and group ids (numbers), at least one.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "MovedEntries")]
    pub items: Option<MovedEntries>,
    /// The group to move them into. Default: the folder's top level.
    #[serde(default)]
    pub group: Option<GroupId>,
    /// Put them before this item key or group id, in the list they go into. Default: the end.
    #[serde(default)]
    pub before: Option<BeforeEntry>,
}

/// What `browser.move` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserMoved {
    /// What was moved, as given.
    #[schemars(with = "Vec<EntryKey>")]
    pub items: Vec<BrowserEntry>,
    /// The group they went into, or `null` for the folder's top level.
    pub group: Option<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Set the display order of a Browser folder. An item that sits in a group belongs to the
/// group's list only. One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = folder_required)]
pub struct BrowserOrderParams {
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OrderFolder")]
    pub folder: Option<OrderFolder>,
    /// The component whose folder it is. Default: 0, the design.
    #[serde(default)]
    pub component: Option<u64>,
    /// The display order: item keys (strings) and top-level group ids (numbers); for
    /// `components`, the ids of the components placed in it. Empty or left out: the folder goes
    /// back to its default order.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "OrderEntries")]
    pub order: Option<OrderEntries>,
}

/// What `browser.order` returns: nothing but the warnings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserOrdered {
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A list of item keys (strings, or numbers read as text), as `keys` in `cmd/browser.rs` reads
/// one: at most 10000, no key empty or over 256 bytes.
fn item_keys(value: Value, key: &str) -> Result<Vec<Value>, String> {
    let list = match value {
        Value::Array(a) if a.iter().all(|x| x.is_string() || x.as_u64().is_some()) => a,
        _ => return Err(whole(format!("`{key}` must be a list of item keys"))),
    };
    let bad_key = |x: &Value| x.as_str().is_some_and(|k| k.is_empty() || k.len() > MAX_KEY_BYTES);
    if list.len() > MAX_LIST || list.iter().any(bad_key) { Err(whole(format!("`{key}` has bad keys"))) } else { Ok(list) }
}

/// `browser.visibility`'s `items`: sketches, bodies, construction items, joints or origin items.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownItems(pub Vec<Value>);

impl<'de> Deserialize<'de> for ShownItems {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        item_keys(Value::deserialize(d)?, "items").map(ShownItems).map_err(D::Error::custom)
    }
}

/// `browser.visibility`'s `features`: feature ids or names.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownFeatures(pub Vec<Value>);

impl<'de> Deserialize<'de> for ShownFeatures {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        item_keys(Value::deserialize(d)?, "features").map(ShownFeatures).map_err(D::Error::custom)
    }
}

fn keys_schema(description: &str) -> Schema {
    json_schema!({
        "description": description,
        "type": "array",
        "items": {"anyOf": [{"type": "string", "minLength": 1, "maxLength": MAX_KEY_BYTES}, {"type": "integer", "minimum": 0}]},
        "maxItems": MAX_LIST
    })
}

impl JsonSchema for ShownItems {
    fn schema_name() -> Cow<'static, str> {
        "ShownItems".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        keys_schema("Sketches (id or name), bodies, construction items, joints or origin items (O, X, Y, Z, XY, XZ, YZ), by name.")
    }
}

impl JsonSchema for ShownFeatures {
    fn schema_name() -> Cow<'static, str> {
        "ShownFeatures".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        keys_schema("Feature ids or names.")
    }
}

/// `browser.visibility`'s `joints`: one to 10000 joint ids or names.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownJoints(pub Vec<Value>);

impl<'de> Deserialize<'de> for ShownJoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = match Value::deserialize(d)? {
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_LIST => a,
            _ => return Err(D::Error::custom(whole("`joints` must list 1..10000 joint ids or names"))),
        };
        if list.iter().all(|j| j.is_number() || j.is_string()) {
            Ok(ShownJoints(list))
        } else {
            Err(D::Error::custom(whole("`joint` must be an id or a name")))
        }
    }
}

impl JsonSchema for ShownJoints {
    fn schema_name() -> Cow<'static, str> {
        "ShownJoints".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Joint ids (numbers) or names.",
            "type": "array",
            "items": {"type": ["number", "string"]},
            "minItems": 1,
            "maxItems": MAX_LIST
        })
    }
}

/// A placement's path, occurrence ids from the outermost down, as the handlers read one: `min`
/// to 64 whole numbers.
fn occurrence_ids(value: &Value, min: usize) -> Option<Vec<u64>> {
    value.as_array().filter(|a| a.len() >= min && a.len() <= MAX_PATH).and_then(|a| a.iter().map(Value::as_u64).collect::<Option<Vec<_>>>())
}

fn path_schema(description: &str, min_items: u32) -> Schema {
    json_schema!({"description": description, "type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": min_items, "maxItems": MAX_PATH})
}

/// The path of one placement for `browser.visibility`'s `path`: at most 64 occurrence ids.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementPath(pub Vec<u64>);

impl<'de> Deserialize<'de> for PlacementPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        occurrence_ids(&Value::deserialize(d)?, 0)
            .map(PlacementPath)
            .ok_or_else(|| D::Error::custom(whole("`path` must list at most 64 occurrence ids")))
    }
}

impl JsonSchema for PlacementPath {
    fn schema_name() -> Cow<'static, str> {
        "PlacementPath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        path_schema("A placement, by the occurrence ids from the design down.", 0)
    }
}

/// `browser.expand_all` and `browser.collapse_all`'s `path`: 1 to 64 occurrence ids.
#[derive(Clone, Debug, PartialEq)]
pub struct FoldPath(pub Vec<u64>);

impl<'de> Deserialize<'de> for FoldPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        occurrence_ids(&Value::deserialize(d)?, 1).map(FoldPath).ok_or_else(|| D::Error::custom(whole("`path` must list 1..64 occurrence ids")))
    }
}

impl JsonSchema for FoldPath {
    fn schema_name() -> Cow<'static, str> {
        "FoldPath".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        path_schema("A placement, by the occurrence ids from the design down.", 1)
    }
}

/// `browser.visibility`'s `paths`: one to 10000 placements, each 1 to 64 occurrence ids.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacementPaths(pub Vec<Vec<u64>>);

impl<'de> Deserialize<'de> for PlacementPaths {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = match Value::deserialize(d)? {
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_LIST => a,
            _ => return Err(D::Error::custom(whole("paths must list 1..10000 occurrence paths"))),
        };
        list.iter()
            .map(|p| occurrence_ids(p, 1))
            .collect::<Option<Vec<_>>>()
            .map(PlacementPaths)
            .ok_or_else(|| D::Error::custom(whole("each path must list 1..64 occurrence ids")))
    }
}

impl JsonSchema for PlacementPaths {
    fn schema_name() -> Cow<'static, str> {
        "PlacementPaths".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Placements, each by the occurrence ids from the design down; each with its descendants.",
            "type": "array",
            "items": generator.subschema_for::<FoldPath>(),
            "minItems": 1,
            "maxItems": MAX_LIST
        })
    }
}

/// A browser folder or group key, as `tree_folder_key` reads one.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeFolderKeys(pub Vec<String>);

impl<'de> Deserialize<'de> for TreeFolderKeys {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let list = match Value::deserialize(d)? {
            Value::Array(a) if !a.is_empty() && a.len() <= MAX_LIST => a,
            _ => return Err(D::Error::custom(whole("`tree_folders` must list 1..10000 browser folder or group keys"))),
        };
        list.into_iter()
            .map(|k| match k {
                Value::String(k) if crate::cmd::browser::tree_folder_key(&k) => Some(k),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(TreeFolderKeys)
            .ok_or_else(|| {
                D::Error::custom(whole("a browser folder or group key is `<c0 | o:ids>/<folder | g<group id>>`, such as `c0/bodies` or `o:4/g7`"))
            })
    }
}

impl JsonSchema for TreeFolderKeys {
    fn schema_name() -> Cow<'static, str> {
        "TreeFolderKeys".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Browser folder or group keys: `c0` (the design) or `o:` and the placement's occurrence ids joined by `/`, then `/bodies`, `/sketches`, `/construction`, `/joints`, `/canvases`, `/point_clouds` or `/g` and a group id.",
            "type": "array",
            "items": {"type": "string", "maxLength": 2048},
            "minItems": 1,
            "maxItems": MAX_LIST
        })
    }
}

/// A folder `browser.visibility` shows or hides whole.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityFolder(pub String);

impl<'de> Deserialize<'de> for VisibilityFolder {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(f) if VISIBILITY_FOLDERS.contains(&f.as_str()) => Ok(VisibilityFolder(f)),
            Value::String(f) => Err(D::Error::custom(whole(format!("unknown folder `{f}` (bodies, sketches, construction, origin, joints, all)")))),
            _ => Err(D::Error::custom("must be bodies, sketches, construction, origin, joints or all")),
        }
    }
}

impl JsonSchema for VisibilityFolder {
    fn schema_name() -> Cow<'static, str> {
        "VisibilityFolder".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A folder, whole: bodies, sketches, construction, origin, joints or all.", "type": "string", "enum": VISIBILITY_FOLDERS})
    }
}

/// `visible`: true or false.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visible(pub bool);

impl<'de> Deserialize<'de> for Visible {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Bool(b) => Ok(Visible(b)),
            _ => Err(D::Error::custom(whole("`visible` must be true or false"))),
        }
    }
}

impl JsonSchema for Visible {
    fn schema_name() -> Cow<'static, str> {
        "Visible".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "true to show, false to hide.", "type": "boolean"})
    }
}

fn some_visible<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Visible>, D::Error> {
    Visible::deserialize(d).map(Some)
}

/// Show or hide what is in the Browser: name one kind of target (`items`, `joints`, `features`,
/// `folder`, `tree_folders` or `paths`). Saved with the design and undone like any other
/// change; the camera is not. With `isolate`, everything else is hidden.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = visible_required)]
pub struct BrowserVisibilityParams {
    // Required in the schema (`visible_required`), optional in the type, as `group` above.
    #[serde(default, deserialize_with = "some_visible", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Visible")]
    pub visible: Option<Visible>,
    /// Sketches, bodies, construction items, joints or origin items, by key. With `isolate` and
    /// `visible: true`: the bodies to keep.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ShownItems")]
    pub items: Option<ShownItems>,
    /// Joints, by id or name: their glyphs (they still constrain and move parts).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ShownJoints")]
    pub joints: Option<ShownJoints>,
    /// Features, by id or name: the bodies they made. Sketches go in `items`.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "ShownFeatures")]
    pub features: Option<ShownFeatures>,
    /// With `features`: only the bodies of this placement, which must be one of the features'
    /// component.
    #[serde(default)]
    pub path: Option<PlacementPath>,
    /// All of a kind: bodies, sketches, construction, origin, joints or all. Showing also shows
    /// the browser folders and groups that hold it, and `all` takes in the occurrence hides.
    #[serde(default)]
    pub folder: Option<VisibilityFolder>,
    /// The folders or groups themselves, by tree key; their items keep their own state.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "TreeFolderKeys")]
    pub tree_folders: Option<TreeFolderKeys>,
    /// Exact placements and their descendants, each by its occurrence ids.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PlacementPaths")]
    pub paths: Option<PlacementPaths>,
    /// With `paths`, or `items` of bodies, and `visible: true`: show only those and hide
    /// everything else, other placements as placements. Default: false.
    #[serde(default)]
    pub isolate: Option<bool>,
}

/// What the browser hides and shows, as the design saves it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct VisibilityState {
    /// Bodies hidden, by name.
    pub hidden_bodies: Vec<String>,
    /// Placements hidden with all their descendants, by occurrence path.
    pub hidden_occurrences: Vec<Vec<u64>>,
    /// Origin items and construction geometry hidden, by name.
    pub hidden_origin: Vec<String>,
    /// Sketches hidden, by feature id.
    pub hidden_sketches: Vec<u64>,
    /// Finished sketches shown although a feature uses them, by feature id.
    pub shown_sketches: Vec<u64>,
    /// Joints whose glyphs are hidden, by id. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_joints: Vec<u64>,
    /// Whether the Origin folder is hidden. Present when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_hidden: Option<bool>,
    /// Component Origin folders explicitly shown, by occurrence path. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown_component_origins: Vec<Vec<u64>>,
    /// Sketches whose closed profiles are not shaded, by feature id. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_profiles: Vec<u64>,
    /// Sketches whose dimensions are shown, by feature id. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shown_dimensions: Vec<u64>,
    /// Bodies locked in the browser, by name. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked_bodies: Vec<String>,
    /// Browser folders and groups hidden themselves, by tree key. Present when there are any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_folders: Vec<String>,
}

impl From<&solidcraft_doc::Visibility> for VisibilityState {
    fn from(v: &solidcraft_doc::Visibility) -> Self {
        VisibilityState {
            hidden_bodies: v.hidden_bodies.clone(),
            hidden_occurrences: v.hidden_occurrences.clone(),
            hidden_origin: v.hidden_origin.clone(),
            hidden_sketches: v.hidden_sketches.clone(),
            shown_sketches: v.shown_sketches.clone(),
            hidden_joints: v.hidden_joints.clone(),
            origin_hidden: v.origin_hidden,
            shown_component_origins: v.shown_component_origins.clone(),
            hidden_profiles: v.hidden_profiles.clone(),
            shown_dimensions: v.shown_dimensions.clone(),
            locked_bodies: v.locked_bodies.clone(),
            hidden_folders: v.hidden_folders.clone(),
        }
    }
}

/// What an isolate kept.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Isolated {
    /// The bodies kept, by name (`items`).
    Bodies(Vec<String>),
    /// The placements kept, by occurrence path (`paths`).
    Paths(Vec<Vec<u64>>),
}

/// What `browser.visibility` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserVisibilityResult {
    /// What was asked for.
    pub visible: bool,
    /// How many targets were changed. Not given by an isolate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<usize>,
    /// What an isolate kept. Only given by an isolate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolated: Option<Isolated>,
    /// What is hidden and shown now.
    pub visibility: VisibilityState,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The component id `browser.expand_all` and `browser.collapse_all` read: a whole number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FoldComponent(pub u64);

impl<'de> Deserialize<'de> for FoldComponent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Value::deserialize(d)?.as_u64().map(FoldComponent).ok_or_else(|| D::Error::custom(whole("`component` must be a component id")))
    }
}

impl JsonSchema for FoldComponent {
    fn schema_name() -> Cow<'static, str> {
        "FoldComponent".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"description": "A component's id; 0 is the design.", "type": "integer", "minimum": 0})
    }
}

/// Open every row of the Browser below a row (`browser.expand_all`) or close them, the row
/// itself staying open (`browser.collapse_all`). Display only: not saved with the design and not
/// an undo step. Give `path` or `component`, not both; neither takes the whole tree.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct BrowserFoldParams {
    /// A placement and its descendants, by occurrence ids (1 to 64).
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FoldPath")]
    pub path: Option<FoldPath>,
    /// A component's own folders; 0 is the whole tree.
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "FoldComponent")]
    pub component: Option<FoldComponent>,
}

/// What `browser.expand_all` and `browser.collapse_all` return: which Browser rows start open
/// and which closed.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BrowserFoldResult {
    /// True after `browser.expand_all`, false after `browser.collapse_all`.
    pub open: bool,
    /// Tree keys of the rows that start open, sorted.
    pub open_by_default: Vec<String>,
    /// Tree keys of the rows that start closed (Origin, Named Views, Features…), sorted.
    pub closed_by_default: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Rename a body; the features that use it follow. A placed copy's name means the body it shows.
/// One undo step.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = body_and_name_required)]
pub struct BodyRenameParams {
    // Required in the schema (`body_and_name_required`), optional in the type, as `group` above.
    #[serde(default, deserialize_with = "some_body", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "BodyName")]
    pub body: Option<BodyName>,
    /// The new name, 1 to 128 characters once surrounding spaces are cut; no other body may have it.
    #[serde(default, deserialize_with = "some_new_name", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "NewName")]
    pub name: Option<NewName>,
}

/// What `body.rename` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyRenamed {
    /// The body's name before (the body a placed copy's name shows).
    pub body: String,
    /// Its name now, without surrounding spaces.
    pub name: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod tests;
