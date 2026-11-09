//! Contracts and result types for the commands in `cmd/libraries.rs`: local material libraries
//! (#10938). The handlers read these same types, so the contract is exactly what they take;
//! like them, a contract refuses unknown keys.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcraft_io::catalog::{Catalog, FORMAT, Provenance, V1_FORMAT};

/// Read a local library file and check it, without changing the design.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryInspectParams {
    /// The library file (`solidcraft-library/1` or `/2` JSON, at most 4 MiB).
    pub path: String,
}

/// Write the looks assigned in this design and its own physical materials to a library file.
/// The design is never changed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryExportParams {
    /// Where to write it: a `.json` file.
    pub path: String,
    /// The library's id, which qualifies each entry's id: lowercase letters, digits, `.`, `_`
    /// and `-`, starting with a letter, at most 64.
    #[serde(default = "default_id")]
    pub id: String,
    /// The library's name: 1 to 128 bytes, without surrounding spaces or control characters.
    #[serde(default = "default_name")]
    pub name: String,
    /// Where the entries come from, saved with each entry.
    #[serde(default)]
    #[schemars(schema_with = "provenance_schema")]
    pub provenance: Provenance,
    /// Replace the file if it exists. Without it, an existing file is refused and left as it is.
    #[serde(default)]
    pub overwrite: bool,
    /// The file format. `solidcraft-library/1` refuses a design whose looks or materials have
    /// PBR or physical cards, before writing anything.
    #[serde(default = "default_format")]
    #[schemars(schema_with = "format_schema")]
    pub format: String,
}

fn default_id() -> String {
    "user.local".into()
}

fn default_name() -> String {
    "My library".into()
}

fn default_format() -> String {
    FORMAT.into()
}

/// Copy one library entry, at an exact revision, into this design in one undo step: an
/// appearance onto bodies, faces or components, or a physical material (assigned to bodies).
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryApplyParams {
    /// The library file.
    pub path: String,
    /// The entry's qualified id, as `library.inspect` lists it (at most 129 bytes).
    pub entry: String,
    /// The entry's revision, as `library.inspect` lists it: the SHA-256 of its content, 64 hex
    /// digits. An entry whose content has changed since is refused.
    pub revision: String,
    /// What to copy it onto. An appearance needs at least one target; a physical material takes
    /// bodies only, and with none it is only added to the design.
    #[serde(default)]
    pub targets: LibraryTargets,
    /// For a physical material: the name to give it in this design (1 to 128 bytes). Default:
    /// the entry's name. A material of that name already in the design is reused if it is the
    /// same, and refused if it differs. An appearance entry refuses it.
    pub name: Option<String>,
}

/// What a library entry is copied onto: at most 1024 targets in all.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryTargets {
    /// Bodies, by name.
    #[serde(default)]
    pub bodies: Vec<String>,
    /// Faces, each by its body and a point on it (each coordinate finite and under 1e9 in size).
    #[serde(default)]
    pub faces: Vec<LibraryFace>,
    /// Components, by name.
    #[serde(default)]
    pub components: Vec<String>,
}

/// A face, by its body and a point on it.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryFace {
    /// The body's name.
    pub body: String,
    /// A point on the face.
    pub point: [f64; 3],
}

/// What `library.inspect` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryInspected {
    /// The library as read and checked.
    #[schemars(schema_with = "catalog_schema")]
    pub catalog: Catalog,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `library.export` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryExported {
    /// The file written.
    pub path: String,
    /// How many entries it holds.
    pub entries: usize,
    /// Its size in bytes.
    pub bytes: usize,
    /// The library as written.
    #[schemars(schema_with = "catalog_schema")]
    pub catalog: Catalog,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `library.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryApplied {
    /// The entry copied.
    pub entry: String,
    /// Its revision.
    pub revision: String,
    /// Always true: the entry's values were copied into the design, which keeps no link to the
    /// file.
    pub copied: bool,
    /// What was done with it.
    #[schemars(schema_with = "applied_schema")]
    pub result: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn provenance_schema(_: &mut SchemaGenerator) -> Schema {
    // `provenance()` is an object, which a schema always takes.
    Schema::try_from(provenance()).unwrap_or_else(|_| Schema::from(true))
}

fn provenance() -> Value {
    let text = |what: &str, max: u32| json!({"description": format!("{what}: 1 to {max} bytes, without surrounding spaces or control characters."), "type": "string"});
    json!({
        "description": "Where a library entry comes from. Information only: the source URL is never fetched.",
        "type": "object",
        "properties": {
            "license": text("Its licence, such as an SPDX expression", 128),
            "author": text("Who made it", 256),
            "source_url": text("Where it was published", 2048)
        },
        "required": ["license"],
        "additionalProperties": false
    })
}

fn format_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": [FORMAT, V1_FORMAT]})
}

/// A look as a library saves it.
fn look() -> Value {
    json!({
        "description": "A look: its name, colour and opacity, and a physically based response when it has one.",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "color": {"description": "Red, green and blue, 0 to 255.", "type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255}, "minItems": 3, "maxItems": 3},
            "opacity": {"type": "number", "minimum": 0, "maximum": 1},
            "pbr": {"description": "metallic (0 to 1), roughness (0.045 to 1) and dielectric_ior (1 to 3).", "type": "object"}
        },
        "required": ["name", "color", "opacity"]
    })
}

fn catalog_schema(_: &mut SchemaGenerator) -> Schema {
    let definition = |kind: &str, value: Value| {
        json!({
            "type": "object",
            "properties": {"kind": {"type": "string", "enum": [kind]}, "value": value},
            "required": ["kind", "value"]
        })
    };
    let material = json!({
        "description": "A physical material: its name and density, with its default look and a physical card when it has them.",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "density_g_cm3": {"type": "number", "exclusiveMinimum": 0},
            "appearance": look(),
            "physical_card": {"description": "Its validated engineering data, as material.inspect shows it.", "type": "object"}
        },
        "required": ["name", "density_g_cm3"]
    });
    json_schema!({
        "description": "A local library: no network, external resources or links to a design.",
        "type": "object",
        "properties": {
            "format": {"type": "string", "enum": [FORMAT, V1_FORMAT]},
            "id": {"description": "The library's id.", "type": "string"},
            "name": {"description": "The library's name.", "type": "string"},
            "entries": {
                "type": "array",
                "maxItems": 1024,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"description": "`<library id>/<kind>-<the revision's first 16 hex digits>`, as library.apply takes it.", "type": "string"},
                        "revision": {"description": "The SHA-256 of the entry's content, 64 hex digits, as library.apply takes it.", "type": "string"},
                        "provenance": provenance(),
                        "definition": {"anyOf": [definition("appearance", look()), definition("material", material)]}
                    },
                    "required": ["id", "revision", "provenance", "definition"]
                }
            }
        },
        "required": ["format", "id", "name", "entries"]
    })
}

fn applied_schema(_: &mut SchemaGenerator) -> Schema {
    let names = |what: &str| json!({"description": what, "type": "array", "items": {"type": "string"}});
    json_schema!({
        "anyOf": [
            {
                "description": "An appearance: what appearance.assign assigned it to, and the look.",
                "type": "object",
                "properties": {
                    "bodies": names("The bodies."),
                    "faces": {
                        "description": "The faces, each by its body and the point given.",
                        "type": "array",
                        "items": {"type": "object", "properties": {"body": {"type": "string"}, "point": {"type": "array", "items": {"type": "number"}}}}
                    },
                    "components": names("The components."),
                    "appearance": {"description": "The look assigned: name, colour (`#rrggbb`), opacity and pbr when it has one.", "type": ["object", "null"]}
                },
                "required": ["bodies", "faces", "components", "appearance"]
            },
            {
                "description": "A physical material: its name in this design, whether one already there was reused, and the bodies it was assigned to.",
                "type": "object",
                "properties": {
                    "material": {"type": "string"},
                    "reused": {"type": "boolean"},
                    "bodies": names("The bodies.")
                },
                "required": ["material", "reused", "bodies"]
            }
        ]
    })
}

#[cfg(test)]
#[path = "libraries_tests.rs"]
mod tests;
