//! The contract and the result type for the command that reads the selection
//! (`cmd/selection.rs`): `selection.get`.
//!
//! Its one parameter, `detail`, is read the way the handler reads it — absent or `null` is the
//! default, anything else is the handler's own message — so a call refused in the strict
//! parameter-check mode is refused with the message it already gets. The
//! reply's items stay the maps the handler builds: [`SelectionGetResult`] holds them as they are,
//! so the JSON a call gets cannot change. The item schema names the keys the handler puts in an
//! item — the item's own fields, exactly as `select.set` takes them, with `kind` naming what it
//! is, and what the design says about it — and leaves the rest open, because the figures of a
//! body, face, edge or profile are what `inspect.measure` gives the same item (see its output
//! schema).

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value, json};

use super::check::whole;

/// Items resolved in full; the rest of a bigger selection is listed by identity only. Next to the
/// contract, so the cap the schema describes and the one the handler uses cannot drift apart.
pub(crate) const MAX_DETAIL: usize = 500;

/// The kinds an item can have: the selection item types, as `select.set` names them.
const KINDS: &[&str] = &[
    "occurrence",
    "body",
    "edge",
    "face",
    "feature",
    "sketch_curve",
    "sketch_point",
    "sketch_midpoint",
    "external_sketch_curve",
    "external_sketch_point",
    "sketch_constraint",
    "profile",
    "plane",
    "axis",
    "construction_point",
    "vertex",
];

/// `selection.get`: what is selected, resolved to what a script or an agent needs to act on it.
/// Reading the selection changes nothing — not the design, not the selection, not the undo
/// history.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SelectionGetParams {
    // The schema describes the key with `detail_schema`, so its text is there rather than here.
    #[serde(default, deserialize_with = "detail", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "detail_schema")]
    pub detail: Option<bool>,
}

/// `detail`: true or false; `null` is the default (true), and anything else is the handler's own
/// message for it.
fn detail<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    match Value::deserialize(d)? {
        Value::Bool(b) => Ok(Some(b)),
        Value::Null => Ok(None),
        _ => Err(D::Error::custom(whole("`detail` must be true or false"))),
    }
}

fn detail_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": format!("Resolve each item's geometry: a face's or edge's persistent name, a face's surface, \
            the figures of a body, face, edge or profile, the sketch a sketch item belongs to, a feature's name and \
            type, an occurrence's component and bodies. Default: true. At most {MAX_DETAIL} items are resolved in \
            full; past that the items are listed by identity only and the reply says so with `detail_limit`."),
        "type": "boolean"
    })
}

/// What `selection.get` returns: the selection, in the order it is held.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SelectionGetResult {
    /// How many items are selected.
    pub count: usize,
    /// Each item, as the schema of this command describes it.
    #[schemars(schema_with = "items_schema")]
    pub items: Vec<Map<String, Value>>,
    /// Only when the selection is bigger than the detail cap: how many items were resolved in
    /// full. The rest are listed by identity only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_limit: Option<usize>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A text property. The item schema is built from small pieces rather than one literal, which
/// the JSON macro cannot expand.
fn text(what: &str) -> Value {
    json!({"description": what, "type": "string"})
}

/// A flag property.
fn flag(what: &str) -> Value {
    json!({"description": what, "type": "boolean"})
}

/// A count or id property: a whole number 0 or more.
fn count(what: &str) -> Value {
    json!({"description": what, "type": "integer", "minimum": 0})
}

/// A position in world mm.
fn point(what: &str) -> Value {
    json!({"description": what, "type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3})
}

/// A path of occurrence ids, from the root component.
fn path(what: &str) -> Value {
    json!({"description": what, "type": "array", "items": {"type": "integer", "minimum": 0}})
}

/// A `{id, name}` pair: the feature that made a body, or the component a body belongs to.
fn owner(what: &str, id: &str) -> Value {
    json!({
        "description": what,
        "type": "object",
        "properties": {
            "id": count(id),
            "name": {"description": "The name, null when there is none.", "type": ["string", "null"]}
        },
        "required": ["id"]
    })
}

/// The items: each one's own fields as `select.set` takes them, with `kind` naming what it is,
/// and with `detail` what the design says about it. The figures a body, face, edge or profile
/// carries are what `inspect.measure` gives the same item, so they are not listed here.
fn items_schema(_: &mut SchemaGenerator) -> Schema {
    let kind = json!({"description": "What the item is, the same for `kind` and `type`.", "type": "string", "enum": KINDS});
    let mut props = Map::new();
    let mut put = |key: &str, schema: Value| {
        props.insert(key.into(), schema);
    };
    for key in ["kind", "type"] {
        put(key, kind.clone());
    }
    put(
        "name",
        text(
            "The body's, plane's, axis's or construction point's name; a feature's name, an occurrence's \
        component name, or a face's or edge's persistent name, with `detail`.",
        ),
    );
    put(
        "id",
        json!({
            "description": "A feature's id, or the id of a sketch curve, point, line midpoint or constraint.",
            "type": ["string", "integer"],
            "minimum": 0
        }),
    );
    put("sketch", count("The sketch a curve, point or profile belongs to, as `select.set` takes it."));
    put("sketch_name", text("The name of that sketch, with `detail`."));
    put("body", text("The body a face, edge or vertex belongs to."));
    put("index", count("A face's or edge's index in its body, or a profile's index in its sketch."));
    put("point", point("A point on the face or edge picked, or the vertex's position, in world mm."));
    put("path", path("An occurrence's path of occurrence ids, from the root component."));
    put("position", point("A sketch point's or line midpoint's position, in world mm, with `detail`."));
    put(
        "entity",
        text(
            "What kind of sketch entity an item is: the curve's or constraint's kind, or `point`, \
        `midpoint` or `other`, with `detail`.",
        ),
    );
    put("construction", flag("A sketch curve that is a construction curve, with `detail`."));
    put("driven", flag("A constraint that only follows the sketch rather than holding it, with `detail`."));
    put("parameter", text("The model parameter a driven dimension drives, with `detail`."));
    put("feature", owner("The feature that made a body, with `detail`.", "The feature's id."));
    put("feature_type", text("A timeline feature's kind, with `detail`."));
    put("suppressed", flag("A timeline feature that is switched off, with `detail`."));
    put("visible", flag("A body that is shown, with `detail`."));
    put(
        "component",
        owner("The component a body or occurrence belongs to, in an assembly, with `detail`.", "The component's id; 0 is the root component."),
    );
    put("source_body", text("The body another placement of it comes from, in an assembly, with `detail`."));
    put("occurrence_path", path("The path of the occurrence a body belongs to, in an assembly, with `detail`."));
    put("grounded", flag("An occurrence that cannot be moved, with `detail`."));
    put(
        "bodies",
        json!({
            "description": "The world bodies an occurrence holds, with `detail`.",
            "type": "array",
            "items": {"type": "string"}
        }),
    );
    put("name_exact", flag("A face whose persistent name is the one the design holds now, with `detail`."));
    put(
        "surface",
        json!({
            "description": "A face's surface, with `detail`: `plane` with `normal` and a `point` on it (only the normal \
                when the face is flat rather than analytic), `cylinder` with `origin`, `axis` and `radius`, `cone` with \
                `apex`, `axis` and `half_angle_deg`, `sphere` with `center` and `radius`, `torus` with `center`, `axis`, \
                `major_radius` and `minor_radius`, or `other`.",
            "type": "object",
            "properties": {"type": {"type": "string", "enum": ["plane", "cylinder", "cone", "sphere", "torus", "other"]}},
            "required": ["type"]
        }),
    );
    put("start", point("An edge's first end, in world mm, with `detail`."));
    put("end", point("An edge's last end, in world mm, with `detail`."));
    put("direction", point("An edge's direction at the point it was picked, with `detail`."));
    let item = json!({"type": "object", "properties": Value::Object(props), "required": ["kind", "type"]});
    json_schema!({
        "description": "Each item of the selection, in the order it is held: its own fields, exactly as \
            `select.set` takes them, with `kind` naming what it is. With `detail`, what the design says about \
            it. A body, face, edge or profile also carries the figures `inspect.measure` gives the same item \
            (`area_mm2`, `length_mm`, `volume_mm3`, `closed`, a round one's `radius` or `diameter`, …), which \
            are described there.",
        "type": "array",
        "items": item
    })
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
