//! Contracts and result types for the commands that browse the built-in render library, in
//! `cmd/render/library.rs` (#11443). `render.library.describe`, `.targets` and `.preview` read
//! these same parameter types, so the contract is exactly what they take. `render.library.list`
//! keeps the two readers it has: the physical profiles' takes `availability: null`, the other
//! catalogs' doesn't. Its type takes what either takes, and its schema says which `kind` takes
//! which (`availability_by_kind`). Like the handlers, a contract refuses unknown keys.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Every catalog `render.library.list` and `render.library.describe` read.
const KINDS: &[&str] = &[
    "capabilities",
    "environments",
    "hdr",
    "lighting",
    "materials",
    "native_scenes",
    "physical_profiles",
    "quality",
    "rigs",
    "scenes",
    "selene_appearances",
    "selene_shots",
    "surface_manifest",
    "surfaces",
];

/// List a page of one catalog of the built-in render library: each resource's id and name, its
/// family, its source's status, and what the app can do with it. Nothing is changed, read from a
/// file or rendered.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = availability_by_kind)]
pub struct LibraryListParams {
    /// The catalog. Default: surfaces.
    #[serde(default = "surfaces")]
    #[schemars(schema_with = "list_kind_schema")]
    pub kind: String,
    /// Text to look for, ignoring case: in a resource's id, name, family, description, domains,
    /// process, intended use and tags, or a physical profile's id, designation, family and
    /// material form. At most 256 bytes, without control characters. Default: every resource.
    #[serde(default)]
    pub query: String,
    /// Which resources: `all`, `usable` (the app can use them now), `specification` (described,
    /// not usable) or `blocked`. Physical profiles take only `all`, `usable` or `null`; the other
    /// catalogs don't take `null`. Default: all.
    #[serde(default)]
    #[schemars(schema_with = "availability_schema")]
    pub availability: Option<String>,
    /// How many matching resources to skip: at most 4096. Default: 0.
    #[serde(default)]
    #[schemars(range(max = 4096))]
    pub offset: usize,
    /// The most resources on the page: 1 to 128. Default: 48.
    #[serde(default = "page")]
    #[schemars(range(min = 1, max = 128))]
    pub limit: usize,
}

fn surfaces() -> String {
    "surfaces".into()
}

fn page() -> usize {
    48
}

fn list_kind_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": KINDS, "default": "surfaces"})
}

fn availability_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["string", "null"], "enum": ["all", "usable", "specification", "blocked", null]})
}

/// Which `availability` each of `render.library.list`'s readers takes, by `kind`: the physical
/// profiles' takes `all`, `usable` or `null`; every other catalog's (an absent `kind` is
/// `surfaces`) takes the four filters but not `null`. Only the schema says this. The type leaves
/// the refusal to the readers, which check the query and the page first, so a call with more
/// than one mistake keeps the handler's first message.
fn availability_by_kind(schema: &mut Schema) {
    let others: Vec<&str> = KINDS.iter().copied().filter(|kind| *kind != "physical_profiles").collect();
    schema.insert(
        "anyOf".into(),
        serde_json::json!([
            {"required": ["kind"], "properties": {"kind": {"enum": ["physical_profiles"]}, "availability": {"enum": ["all", "usable", null]}}},
            {"properties": {"kind": {"enum": others}, "availability": {"enum": ["all", "usable", "specification", "blocked"]}}}
        ]),
    );
}

/// Describe one resource of a catalog of the built-in render library: the resource as its
/// source holds it, and what the app can do with it. A described concept is not a preset the
/// app can run.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryDescribeParams {
    /// The catalog, as `render.library.list` takes it.
    #[schemars(schema_with = "kind_schema")]
    pub kind: String,
    /// The resource's exact id, as `render.library.list` gives it: 1 to 128 ASCII letters,
    /// digits, hyphens or underscores.
    pub id: String,
}

fn kind_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": KINDS})
}

/// List a page of the bodies a render would show, with their place in the render scene and
/// their looks, to pick the targets for the library's surfaces. The model is brought up to date
/// first, as for any command; no mesh is returned.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryTargetsParams {
    /// Text to look for in a body's name, ignoring case: at most 256 bytes, without control
    /// characters. Default: every body.
    pub query: String,
    /// How many matching bodies to skip: at most 262 144. Default: 0.
    #[schemars(range(max = 262_144))]
    pub offset: usize,
    /// The most bodies on the page: 1 to 128. Default: 128.
    #[schemars(range(min = 1, max = 128))]
    pub limit: usize,
}

impl Default for LibraryTargetsParams {
    fn default() -> Self {
        Self { query: String::new(), offset: 0, limit: 128 }
    }
}

/// Read a library surface's original base-colour map, as PNG bytes checked against their
/// recorded hash. This is the source picture, not a render of the material. It needs a build
/// with the bundled render library.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryPreviewParams {
    /// The surface's id: `SURF-` and its number, as `render.library.list` gives it.
    pub id: String,
}

/// What `render.library.list` returns: a page of one catalog.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryPage {
    /// The catalog listed.
    pub kind: String,
    /// How many resources match, on all pages together.
    pub total: usize,
    /// How many matching resources come before this page.
    pub offset: usize,
    /// The `offset` of the next page; `null` on the last page.
    pub next_offset: Option<usize>,
    /// The page's resources. A physical profile's has its own fields.
    #[schemars(with = "Vec<LibraryItem>")]
    pub items: Vec<ResourceSummary>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A resource on a page: of a catalog, or a physical profile.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LibraryItem {
    Resource(ResourceSummary),
    Profile(ProfileSummary),
}

/// A resource of a catalog, as a page lists it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ResourceSummary {
    pub id: String,
    pub name: String,
    /// The kinds of product it suits; `null` when its catalog gives none.
    pub domains: Option<Vec<String>>,
    /// Its family; `null` when its catalog gives none.
    pub family: Option<String>,
    /// The status its source gives it; `null` when the source gives none.
    pub source_status: Option<String>,
    #[schemars(schema_with = "projection_schema")]
    pub projection: Value,
}

/// A physical profile, as a page lists it: a reference material's measured properties to choose
/// from. It is never assigned to a body by itself.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileSummary {
    pub id: String,
    /// Its designation.
    pub name: String,
    pub family: String,
    /// Always `reference_selection_required`: someone has to choose it.
    pub source_status: String,
    #[schemars(schema_with = "projection_schema")]
    pub projection: Value,
    /// How many properties it gives.
    pub property_count: usize,
}

/// What the app can do with a resource: its `status`, and the forms it can take or why it can't.
fn projection_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "What the app can do with the resource: its `status`, and the forms it can take or why it can't.",
        "type": "object",
        "properties": {
            "status": {
                "description": "`implemented_pending_batch_validation`: the app can use it. `native_composition`: a studio scene the app builds. `blocked`: not in this build, or not usable as authored. `specification_only`: described, not usable. `reference_only`: a physical profile, read but never assigned.",
                "type": "string",
                "enum": ["implemented_pending_batch_validation", "native_composition", "blocked", "specification_only", "reference_only"]
            },
            "variants": {"description": "The forms it can be used in.", "type": "array", "items": {"type": "string"}},
            "reason": {"description": "Why it is blocked or only a specification.", "type": "string"},
            "omitted": {"description": "What the app leaves out of it.", "type": "array", "items": {"type": "string"}}
        },
        "required": ["status"]
    })
}

/// What `render.library.describe` returns: a resource of a catalog, or a physical profile.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LibraryDescription {
    Resource(LibraryResource),
    Profile(ProfileDescription),
}

/// A resource of a catalog, described.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryResource {
    /// The catalog.
    pub kind: String,
    /// The resource as its catalog holds it, unchanged.
    pub resource: Map<String, Value>,
    #[schemars(schema_with = "projection_schema")]
    pub projection: Value,
    /// Always true: describing a resource leaves its catalog as it is.
    pub source_manifest_unchanged: bool,
    /// For a material or a surface: the physical profiles that may suit it, which nothing
    /// assigns by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<PhysicalReferences>")]
    pub physical_references: Option<Value>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The physical profiles that may suit a material or a surface.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PhysicalReferences {
    /// What the material or surface asks of a physical material, from the bindings catalog.
    pub specifications: Vec<Map<String, Value>>,
    /// The profiles that meet it, as a page lists them.
    pub candidate_profiles: Vec<ProfileSummary>,
    /// Always false: no profile is assigned by itself.
    pub auto_assign: bool,
    /// Always false: a property a profile doesn't give is unknown, not zero.
    pub unknown_is_zero: bool,
}

/// A physical profile, described.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileDescription {
    /// Always `physical_profiles`.
    pub kind: String,
    /// The profile as its catalog holds it, unchanged.
    pub resource: Map<String, Value>,
    #[schemars(schema_with = "projection_schema")]
    pub projection: Value,
    /// The physical material card the profile becomes in the app.
    pub native_projection: Map<String, Value>,
    /// That card's check.
    pub native_validation: Value,
    /// The sources of the profile's properties.
    pub sources: Vec<Map<String, Value>>,
    /// The SHA-256 digests of the profiles and cards read.
    pub receipts: ProfileReceipts,
    /// Always false.
    pub document_changed: bool,
    /// Always false: describing assigns nothing.
    pub physical_assignment: bool,
    /// Always true.
    pub source_manifest_unchanged: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The SHA-256 digests of the physical profile data read.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileReceipts {
    pub profiles_sha256: String,
    pub native_cards_sha256: String,
}

/// What `render.library.targets` returns: a page of the bodies a render would show.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryTargets {
    pub items: Vec<RenderTarget>,
    /// How many bodies match, on all pages together.
    pub total: usize,
    /// How many matching bodies come before this page.
    pub offset: usize,
    /// The `offset` of the next page; `null` on the last page.
    pub next_offset: Option<usize>,
    /// The design's revision the bodies were read at.
    pub revision: u64,
    /// The box around every body shown (mm); `null` when none is.
    pub bounds: Option<SceneBounds>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A body a render would show.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RenderTarget {
    /// Its place in the render scene, from 0, among all bodies shown.
    pub ordinal: usize,
    /// Its name, with its component's path when it is in one.
    pub name: String,
    /// Its saved definition, feature and occurrence identity; `null` when no stable target exists.
    pub stable_target: Option<StableRenderTarget>,
    /// Its colour: red, green and blue, 0 to 255.
    pub color: [u8; 3],
    /// How opaque it is: 1 is solid.
    pub opacity: f64,
    /// Its PBR look; `null` when it has none.
    pub pbr: Option<Pbr>,
}

/// A stable body identity accepted by saved surface, optical and clear-coat bindings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StableRenderTarget {
    /// The saved body definition's name.
    pub definition: String,
    /// The feature that produced the body.
    pub feature: u64,
    /// The owning component, or 0 for the root definition.
    pub component: u64,
    /// The ordered occurrence ids from the root; empty for a root definition.
    pub occurrence_path: Vec<u64>,
}

impl From<&solidcraft_doc::render_surface::RenderBodyTarget> for StableRenderTarget {
    fn from(target: &solidcraft_doc::render_surface::RenderBodyTarget) -> Self {
        Self {
            definition: target.definition.clone(),
            feature: target.feature,
            component: target.component,
            occurrence_path: target.occurrence_path.clone(),
        }
    }
}

/// A body's PBR look.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Pbr {
    pub metallic: f64,
    pub roughness: f64,
    pub dielectric_ior: f64,
}

/// A box, by its corners (mm).
#[derive(Debug, Serialize, JsonSchema)]
pub struct SceneBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// What `render.library.preview` returns: a surface's original base-colour map.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryPreview {
    /// The surface's id.
    pub id: String,
    /// Always `image/png`.
    pub mime: String,
    /// The picture's SHA-256, in hex.
    pub sha256: String,
    /// The PNG file's bytes, each 0 to 255.
    pub bytes: Vec<u8>,
    /// Always `authored_base_color_source_map_not_a_render`: the source picture, not a render.
    pub fidelity: String,
    /// Always false.
    pub document_changed: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_library_tests.rs"]
mod tests;
