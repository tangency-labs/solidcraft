//! Contracts and result types for the commands in `cmd/libraries.rs`: local material libraries
//! (#10938). The handlers read these same types, so the contract is exactly what they take;
//! like them, a contract refuses unknown keys.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcraft_io::catalog::{Catalog, Definition, FORMAT, Provenance, V1_FORMAT};

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

/// What a library entry holds: an appearance or a physical material.
fn definition() -> Value {
    let kind = |kind: &str, value: Value| {
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
    json!({"anyOf": [kind("appearance", look()), kind("material", material)]})
}

fn definition_schema(_: &mut SchemaGenerator) -> Schema {
    // `definition()` is an object, which a schema always takes.
    Schema::try_from(definition()).unwrap_or_else(|_| Schema::from(true))
}

fn optional_provenance_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"anyOf": [provenance(), {"type": "null"}]})
}

fn catalog_schema(_: &mut SchemaGenerator) -> Schema {
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
                        "definition": definition()
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

/// List the library places: the built-in library, My Library and the user's places, in the
/// order their libraries are used. Reads no files: it answers from what the session holds.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlacesListParams {}

/// What `library.places.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryPlacesListed {
    /// Every place, in the order libraries are used: the built-in library first.
    pub places: Vec<ListedPlace>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One place as the session holds it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedPlace {
    /// `built_in`, `library` (one library folder) or `folder` (a folder of library folders).
    pub kind: String,
    /// Its folder; none for the built-in library.
    pub path: Option<String>,
    /// Whether SolidCraft may not change the libraries there.
    pub read_only: bool,
    /// Whether it is My Library, where new libraries are saved.
    pub my_library: bool,
    /// How far its scan has got: `scanned`, `scanning` or `not_scanned`.
    pub scan: String,
    /// Why the place can't be read, or why its scan couldn't start.
    pub why: Option<String>,
    /// Its libraries as the scan found them, each with its status among all places.
    pub libraries: Vec<ListedLibrary>,
}

/// One library of a place.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedLibrary {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Its own folder; none for the built-in library.
    pub folder: Option<String>,
    /// Whether its entries are used.
    pub available: bool,
    /// Why it isn't used, or why it can't be changed.
    pub why: Option<String>,
    /// Entries listed, refused ones included.
    pub entries: usize,
    /// Entries listed but refused, each for its own reason.
    pub refused: usize,
}

/// Add a library place. The app keeps the list: the engine checks the change and echoes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlacesAddParams {
    /// The place's folder: an absolute path, at most 4096 bytes.
    pub path: String,
    /// `library`: the folder is one library folder. `folder`: its subfolders are library folders.
    #[schemars(schema_with = "place_kind_schema")]
    pub kind: String,
    /// Whether SolidCraft may not change the libraries there. Default: a network folder is
    /// read-only, any other is not.
    pub read_only: Option<bool>,
    /// Where in the list it goes, from 0. Default: the end.
    pub at: Option<usize>,
}

/// Remove a library place from the list. Its files are left as they are.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlacesRemoveParams {
    /// The place's folder, as listed.
    pub path: String,
}

/// Change a library place: whether it is read-only, and where it is in the list.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlacesSetParams {
    /// The place's folder, as listed.
    pub path: String,
    pub read_only: Option<bool>,
    /// Where in the list it moves to, from 0.
    pub at: Option<usize>,
}

/// What `library.places.add`, `.remove` and `.set` return: the change as checked, for the app
/// to apply to its list.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryPlaceChange {
    /// The place's folder.
    pub path: String,
    /// `library` or `folder`, for an added place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Whether the place is read-only after the change, when it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// Where in the list the place is after the change, when it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<usize>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// Scan library places again: one place, or every place.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPlacesRefreshParams {
    /// The place's folder, as listed. Default: every place.
    pub path: Option<String>,
    /// Scan only the places whose files changed since their last scan, judged by each file's
    /// length and time (opening the window, and its focus). Default: scan in full (the Refresh
    /// button), which also reads an edit that kept a file's length and time.
    #[serde(default)]
    pub changed_only: bool,
}

/// What `library.places.refresh` returns: which places the app scans again.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryPlacesRefreshed {
    /// The place's folder; none for every place.
    pub path: Option<String>,
    /// Only the places whose files changed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub changed_only: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn place_kind_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["library", "folder"]})
}

/// Make a new library in a listed place.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryCreateParams {
    /// A listed place's folder, as `library.places.list` gives it. In a folder place the library
    /// gets a new subfolder named after it; a library place's own folder must be empty.
    pub place: String,
    /// The library's name: 1 to 128 bytes, without surrounding spaces or control characters.
    pub name: String,
    /// Where its entries come from, saved in the library's manifest. Default: user-provided.
    #[serde(default)]
    #[schemars(schema_with = "provenance_schema")]
    pub provenance: Provenance,
}

/// List one library's entries, refused ones included. Reads no files: it answers from what the
/// session holds.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntryListParams {
    /// The library's id, as `library.places.list` gives it.
    pub library: String,
}

/// Add an entry to a library, under a new id made from its name.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntryAddParams {
    /// The library's id.
    pub library: String,
    /// What the entry holds.
    #[schemars(schema_with = "definition_schema")]
    pub definition: Definition,
    /// Where it comes from. Default: user-provided.
    #[serde(default)]
    #[schemars(schema_with = "provenance_schema")]
    pub provenance: Provenance,
}

/// Change an entry, when its file still holds the revision the caller read.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntryEditParams {
    /// The entry's id, as `library.entry.list` gives it.
    pub entry: String,
    /// The revision read with it: 64 hex digits. A file that holds another is refused.
    pub revision: String,
    /// What the entry holds now; its kind can't change.
    #[schemars(schema_with = "definition_schema")]
    pub definition: Definition,
    /// Where it comes from now. Default: as it was.
    #[serde(default)]
    #[schemars(schema_with = "optional_provenance_schema")]
    pub provenance: Option<Provenance>,
}

/// Move an entry's file to its library's `.removed` folder, when it still holds the revision
/// read. Nothing is deleted.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntryRemoveParams {
    /// The entry's id, as `library.entry.list` gives it.
    pub entry: String,
    /// The revision read with it: 64 hex digits.
    pub revision: String,
}

/// Copy an entry into a library, under a new id there.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntryCopyParams {
    /// The entry's id, in any library, the built-in one too.
    pub entry: String,
    /// The id of the library it is copied to.
    pub to: String,
}

/// What `library.create` and the `library.entry.*` writes return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryWritten {
    /// The library's id.
    pub library: String,
    /// The entry's id, for an entry's write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// The entry's revision after the write (for a removal: the one removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The file written, from the library's folder, with `/` between names (for a removal: where
    /// it moved).
    pub file: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `library.entry.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryEntriesListed {
    /// The library's id.
    pub library: String,
    /// Whether its entries are used.
    pub available: bool,
    /// Why it isn't used.
    pub why: Option<String>,
    /// Every entry file, refused ones included.
    pub entries: Vec<ListedEntry>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One entry as its library lists it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListedEntry {
    /// Its id; none when the file can't be used.
    pub id: Option<String>,
    /// Its revision, as `library.entry.edit` and `.remove` take it.
    pub revision: Option<String>,
    /// `material` or `appearance`: the folder it is in.
    pub kind: String,
    pub name: Option<String>,
    /// Its file, from the library's folder.
    pub file: String,
    /// Why it can't be used.
    pub why: Option<String>,
}

#[cfg(test)]
#[path = "libraries_tests.rs"]
mod tests;
