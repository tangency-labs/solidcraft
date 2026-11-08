//! Contracts and result types for the commands that browse the built-in render library, in
//! `cmd/render/library.rs` (#11443). `render.library.describe`, `.targets` and `.preview` read
//! these same parameter types, so the contract is exactly what they take. `render.library.list`
//! keeps the two readers it has: the physical profiles' takes `availability: null`, the other
//! catalogs' doesn't. Its type takes what either takes, and its schema says which `kind` takes
//! which (`availability_by_kind`). Like the handlers, a contract refuses unknown keys.
//!
//! The apply and prepare commands (`render.library.appearance.apply`, `.environment.apply`,
//! `.physical.prepare`, `.studio.apply`, `.studio.prepare` and `.surface.prepare`, #12552) keep
//! the readers they have in `cmd/render/library.rs` and `cmd/render/physical_library.rs`: each
//! reads the raw parameters into a private struct. Their parameters types here read each key
//! as that struct does and fail with its message, whole (see `reads!`), so strict refuses a
//! call exactly as the handler does. A key the handler refuses when it is left out is optional
//! in the type and required in the schema, so the handler keeps its message. The handlers read
//! every key in every call, whatever the other keys are, so the types do too.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use solidcraft_doc::physical_material::ValidatedCard;
use solidcraft_doc::render_resources::HdrId;
use solidcraft_doc::render_settings::RenderSetup;
use solidcraft_render::job::StudioLighting;
use solidcraft_render::job::textures::PlanarFrame;
use solidcraft_render::stage::Stage;

use super::appearance::AppearanceAssignResult;
use super::check::whole;

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

/// A key's value as its handler's struct reads it: `T` is the type that struct gives the key, and
/// anything it refuses gets serde's own message, whole (no key put in front), as the handler
/// words it.
fn read_value<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<T, D::Error> {
    serde_json::from_value(Value::deserialize(d)?).map_err(|e| D::Error::custom(whole(e)))
}

/// A deserializer for a key the handler reads as a `$ty`: a value `null` or of another type is
/// refused with the handler's message.
macro_rules! reads {
    ($name:ident -> $ty:ty) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<$ty>, D::Error> {
            read_value::<D, $ty>(d).map(Some)
        }
    };
}

/// A deserializer for a key the handler reads as an `Option<$ty>`, which takes `null`.
macro_rules! reads_option {
    ($name:ident -> $ty:ty) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<Option<$ty>, D::Error> {
            read_value::<D, Option<$ty>>(d)
        }
    };
}

reads!(read_string -> String);
reads!(read_bool -> bool);
reads!(read_f64 -> f64);
reads!(read_u32 -> u32);
reads!(read_usize -> usize);
reads!(read_strings -> Vec<String>);
reads!(read_point -> [f64; 3]);
reads!(read_plane -> Plane);
reads!(read_rig -> RigInput);
reads_option!(read_origin -> [f64; 3]);
reads_option!(read_name -> String);
reads_option!(read_revision -> u64);

/// The ids the handlers look up: 1 to 128 ASCII letters, digits, hyphens or underscores.
const ID_PATTERN: &str = "^[A-Za-z0-9_-]{1,128}$";

/// The fields of a render setup a `set` patch may name, as `render.settings.get` gives them.
/// `patched_settings` reads the patched setup as a `RenderSetup`, which refuses any other key.
const SETUP_FIELDS: &[&str] = &[
    "backend",
    "mode",
    "camera",
    "width",
    "height",
    "format",
    "exposure_ev",
    "environment_rotation",
    "environment_intensity",
    "background",
    "job",
    "surfaces",
    "optics",
    "coats",
    "stage",
];

/// `set`: changes to the setup, which `patched_settings` in `cmd/render.rs` refuses unless it is
/// an object of at most 15 fields, with its own message. The type takes any value, so the
/// handler keeps that message; the schema says what it takes.
fn set_schema(_: &mut SchemaGenerator) -> Schema {
    let properties: Map<String, Value> = SETUP_FIELDS.iter().map(|field| (field.to_string(), Value::Object(Map::new()))).collect();
    json_schema!({
        "description": "Changes to the render setup before the command works on it, as `render.settings.set` takes them: an object of at most 15 of the setup's fields. Default: none.",
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
        "maxProperties": 15,
        "default": {}
    })
}

fn id_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": ID_PATTERN})
}

fn finite_vector_schema(maximum: f64) -> Value {
    serde_json::json!({
        "type": "array",
        "items": {"type": "number", "minimum": -maximum, "maximum": maximum},
        "minItems": 3,
        "maxItems": 3
    })
}

fn require_all(schema: &mut Schema, keys: &[&str]) {
    for key in keys {
        super::motion::require(schema, key);
    }
}

/// Apply a library surface's or a Selene appearance's original scalar look (colour, opacity and
/// PBR response) to bodies, through the ordinary appearance assignment, in one undo step. The
/// physical properties of the bodies are kept. Spatial maps and advanced target effects are
/// left out.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = appearance_required)]
pub struct AppearanceApplyParams {
    // `id`, `variant` and `bodies` are optional in the type and required in the schema
    // (`appearance_required`): left out, the handler refuses them with its own message.
    /// The appearance: a surface (`SURF-…`) or a Selene appearance (`SELENE-AP-…`), as
    /// `render.library.list` gives its id.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "id_schema")]
    pub id: Option<String>,
    /// Always `uniform_opaque`: the original scalar look as one opaque colour.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "uniform_opaque_schema")]
    pub variant: Option<String>,
    /// The bodies to give the look, by exact name: 1 to 256, each 1 to 1024 bytes without
    /// control characters.
    #[serde(default, deserialize_with = "read_strings", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "target_bodies_schema")]
    pub bodies: Option<Vec<String>>,
}

fn appearance_required(schema: &mut Schema) {
    require_all(schema, &["id", "variant", "bodies"]);
}

fn uniform_opaque_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["uniform_opaque"]})
}

fn target_bodies_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"type": "string", "minLength": 1, "maxLength": 1024}, "minItems": 1, "maxItems": 256})
}

/// What `render.library.appearance.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceApplied {
    /// The appearance's id.
    pub id: String,
    /// Always `uniform_opaque`.
    pub variant: String,
    /// What the library's description of the appearance leaves out of this look.
    #[schemars(schema_with = "omitted_schema")]
    pub omitted: Value,
    /// The look as it was assigned.
    pub appearance: LibraryLook,
    /// What `appearance.assign` reports for the bodies.
    #[schemars(with = "AppearanceAssignResult")]
    pub assignment: Value,
    /// Always true: the bodies' materials and physical properties are as they were.
    pub engineering_properties_unchanged: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A library appearance's look, as `render.library.appearance.apply` hands it to
/// `appearance.assign`. The library gives the values, so they are kept as it gives them.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryLook {
    /// The appearance's id and name, at most 128 characters.
    pub name: String,
    /// A surface's colour is `[r, g, b]` (0 to 255), a Selene appearance's `"#rrggbb"`.
    #[schemars(schema_with = "look_color_schema")]
    pub color: Value,
    /// Always 1: the look is opaque.
    #[schemars(schema_with = "look_number_schema")]
    pub opacity: Value,
    /// The PBR response.
    #[schemars(schema_with = "look_pbr_schema")]
    pub pbr: Value,
    /// The bodies it was given to.
    pub bodies: Vec<String>,
}

fn omitted_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"description": "What the library's description of the appearance leaves out of this look.", "type": "array", "items": {"type": "string"}})
}

fn look_number_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "number"})
}

fn look_color_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "anyOf": [
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255}, "minItems": 3, "maxItems": 3},
            {"type": "string"}
        ]
    })
}

fn look_pbr_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "metallic": {"description": "0 (a dielectric) to 1 (a metal).", "type": "number"},
            "roughness": {"description": "0 (smooth) to 1 (rough).", "type": "number"},
            "dielectric_ior": {"description": "The refractive index of its dielectric part.", "type": "number"}
        },
        "required": ["metallic", "roughness", "dielectric_ior"]
    })
}

/// Save a built-in HDR environment (a library `HDR-…`) in a CPU raytrace or GPU path setup, in
/// one undo step. The image's original bytes are checked against their hash and kept in the
/// design, so it reopens without the library. The setup's existing environment is replaced only
/// with `replace_environment`. An HDR that carries the sun cannot be used beside an analytic sun.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = environment_required)]
pub struct EnvironmentApplyParams {
    // `id` and `name` are optional in the type and required in the schema
    // (`environment_required`), as above.
    /// The HDR, as `render.library.list` gives its id (`HDR-…`).
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "id_schema")]
    pub id: Option<String>,
    /// The render setup, saved or new: trimmed, 1 to 128 bytes, without control characters.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    /// Changes to the setup before the HDR is added, as `render.settings.set` takes them: an
    /// object of at most 15 of the setup's fields. The setup must render with the CPU raytracer
    /// or the GPU path tracer. Default: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "set_schema")]
    pub set: Option<Value>,
    /// The image's rotation as authored, in radians: finite, within ±1000000. The reply gives the
    /// rotation the renderers use, which has the image's origin offset by π. Default: 0.
    #[serde(default, deserialize_with = "read_f64", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = -1_000_000.0, max = 1_000_000.0), default = "zero")]
    pub rotation_rad: Option<f64>,
    /// The environment's brightness, linear: 0 to 64 for a CPU setup, 0 to 100 for a GPU one.
    /// Default: 1.
    #[serde(default, deserialize_with = "read_f64", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 0.0, max = 100.0), default = "one")]
    pub intensity: Option<f64>,
    /// A CPU setup's samples per lighting estimate: 1, 4, 16 or 64. A GPU setup reads it and
    /// ignores it. Default: 16.
    #[serde(default, deserialize_with = "read_u32", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "u32", default = "sixteen")]
    pub samples: Option<u32>,
    /// Replace an environment the setup already has: an HDR, a sky or a constant one. Without
    /// it, such a setup is refused. Default: false.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", default = "no")]
    pub replace_environment: Option<bool>,
}

// The schemas' defaults, as the field types give them (the types are optional so that the
// handler, not the check, fills in a key left out).
fn zero() -> Option<f64> {
    Some(0.0)
}

fn one() -> Option<f64> {
    Some(1.0)
}

fn sixteen() -> Option<u32> {
    Some(16)
}

fn no() -> Option<bool> {
    Some(false)
}

fn xy() -> Option<Plane> {
    Some(Plane::Xy)
}

fn environment_required(schema: &mut Schema) {
    require_all(schema, &["id", "name"]);
}

/// What `render.library.environment.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct EnvironmentApplied {
    /// The HDR's id.
    pub id: String,
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it.
    #[schemars(schema_with = "setup_reply_schema")]
    pub setup: RenderSetup,
    /// The image's SHA-256 in hex, the id the design keeps its bytes under.
    #[schemars(with = "String")]
    pub resource: HdrId,
    /// The rotation as given, in radians.
    pub author_rotation_rad: f64,
    /// The rotation the setup keeps: the authored one plus `orientation_offset_rad`, within 0 to
    /// 2π.
    pub native_rotation_rad: f64,
    /// Always π: where the image's origin sits in the renderers.
    pub orientation_offset_rad: f64,
    /// Always true: the design keeps the image's bytes.
    pub self_contained: bool,
    /// The SHA-256 the library records for the image, in hex: the same as `resource`.
    #[schemars(schema_with = "hash_schema")]
    pub source_sha256: Value,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn hash_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string"})
}

/// A render setup as the render settings commands give it.
fn setup_reply_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A render setup, as the render settings commands give it.",
        "type": "object",
        "properties": {
            "backend": {"type": "string"},
            "mode": {"type": "string"},
            "camera": {"type": "object"},
            "width": {"type": "integer", "minimum": 0},
            "height": {"type": "integer", "minimum": 0},
            "format": {"type": "string"},
            "exposure_ev": {"type": "number"},
            "environment_rotation": {"type": "number"},
            "environment_intensity": {"type": "number"},
            "background": {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255}, "minItems": 3, "maxItems": 3},
            "job": {"type": "object"},
            "surfaces": {},
            "optics": {},
            "coats": {},
            "stage": {"type": "array"}
        },
        "required": ["backend", "mode", "camera", "width", "height", "format", "exposure_ev", "environment_rotation", "environment_intensity", "background"]
    })
}

/// Plan a material from a built-in physical reference profile, without making it: a
/// `material.create` call that carries the profile's density and its native card, the receipts
/// of the sources it came from, and what the card leaves out. Nothing is created or assigned.
/// A profile with no single supported density is refused: nothing is guessed.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = physical_required)]
pub struct PhysicalPrepareParams {
    // All three are optional in the type and required in the schema (`physical_required`).
    /// The physical profile, as `render.library.list` gives it with `kind` `physical_profiles`.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "id_schema")]
    pub id: Option<String>,
    /// The name for the material: trimmed, 1 to 128 bytes, without control characters.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "material_name_schema")]
    pub name: Option<String>,
    /// Must be true: the profile is a reference to review, not a grade to rely on.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "acknowledge_schema")]
    pub acknowledge_reference: Option<bool>,
}

fn physical_required(schema: &mut Schema) {
    require_all(schema, &["id", "name", "acknowledge_reference"]);
}

fn material_name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "minLength": 1, "maxLength": 128})
}

fn acknowledge_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "boolean", "const": true})
}

/// What `render.library.physical.prepare` returns: a plan, not a change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PhysicalPlan {
    /// Always `plan_only_not_executed`.
    pub status: String,
    /// The profile's id.
    pub profile_id: String,
    /// The SHA-256 digests of the profiles and cards read.
    #[schemars(with = "ProfileReceipts")]
    pub receipts: Value,
    /// The native card's check.
    pub native_validation: Value,
    /// The profile's properties the card leaves out, each with why.
    pub not_projected: Value,
    /// The profile as its catalog holds it, unchanged.
    #[schemars(with = "Map<String, Value>")]
    pub extended_profile: Value,
    /// Always false.
    pub document_changed: bool,
    /// Always empty: no body is assigned.
    pub assigned_bodies: Vec<String>,
    /// The `material.create` call that would make the material.
    pub command: PlannedCommand,
    /// What to do before running it.
    pub requirements: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A command to run, not run yet.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlannedCommand {
    /// Always `material.create`.
    pub command: String,
    pub params: PlannedMaterial,
}

/// `material.create`'s parameters, as planned.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlannedMaterial {
    /// The name asked for.
    pub name: String,
    /// The profile's density, in g/cm³.
    pub density_g_cm3: f64,
    /// The profile's native physical material card.
    #[schemars(schema_with = "physical_card_schema")]
    pub physical_card: ValidatedCard,
}

fn physical_card_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "A physical material card: the designation, family, sourced properties and sources.",
        "type": "object",
        "properties": {
            "schema_version": {"type": "integer"},
            "designation": {"type": "string"},
            "family": {"type": "string"},
            "properties": {"type": "array", "items": {"type": "object"}},
            "sources": {"type": "array", "items": {"type": "object"}}
        },
        "required": ["schema_version", "designation", "family", "properties", "sources"]
    })
}

/// Which plane a surface's maps lie in.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Plane {
    /// The XY plane, repeating along X and Y.
    #[default]
    Xy,
    /// The YZ plane.
    Yz,
    /// The ZX plane.
    Zx,
}

/// Prepare a library surface's original maps as a temporary planar binding on a body, for the
/// GPU path tracer, and as the source `render.library.surface.bind` saves. Nothing in the design
/// changes. Height and displacement maps and true anisotropy are not included. The body must be
/// opaque.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = surface_required)]
pub struct SurfacePrepareParams {
    // `id`, `body_index` and `body_name` are optional in the type and required in the schema
    // (`surface_required`).
    /// The surface, as `render.library.list` gives its id (`SURF-…`).
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "id_schema")]
    pub id: Option<String>,
    /// The body's place in the render scene, as `render.library.targets` gives it, from 0.
    #[serde(default, deserialize_with = "read_usize", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "usize")]
    pub body_index: Option<usize>,
    /// The body's exact name, which must be the one at `body_index`.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub body_name: Option<String>,
    /// The point the maps start from, in mm. Default: the centre of the body's bounds.
    #[serde(default, deserialize_with = "read_origin", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "origin_schema")]
    pub origin: Option<[f64; 3]>,
    /// The repeat scale: 0.001 to 1000. Default: 1.
    #[serde(default, deserialize_with = "read_f64", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 0.001, max = 1000.0), default = "one")]
    pub scale: Option<f64>,
    /// The plane the maps lie in. Default: xy.
    #[serde(default, deserialize_with = "read_plane", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Plane", default = "xy")]
    pub plane: Option<Plane>,
    /// The grain's rotation about the plane's normal, in radians, positive around the normal:
    /// finite, within ±1000000. Default: 0.
    #[serde(default, deserialize_with = "read_f64", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = -1_000_000.0, max = 1_000_000.0), default = "zero")]
    pub rotation_rad: Option<f64>,
}

fn surface_required(schema: &mut Schema) {
    require_all(schema, &["id", "body_index", "body_name"]);
}

fn origin_schema(_: &mut SchemaGenerator) -> Schema {
    let mut schema = json_schema!({"description": "A point [x, y, z], in mm."});
    schema.insert("anyOf".into(), serde_json::json!([finite_vector_schema(f64::MAX), {"type": "null"}]));
    schema
}

/// What `render.library.surface.prepare` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SurfacePrepared {
    /// The surface's id.
    pub id: String,
    /// The design's revision the body was read at.
    pub revision: u64,
    /// Always `gpu-path`.
    pub executor: String,
    /// Always `gpu_planar_maps`.
    pub variant: String,
    /// The binding, as `render.library.surface.bind` and the GPU render take it.
    pub texture: PreparedTexture,
    /// The surface's source maps by role (`base_color`, `normal_open_gl`, `roughness`,
    /// `metallic`, `height`): the file, size, encoding and hashes the library records.
    #[schemars(schema_with = "source_maps_schema")]
    pub source_maps: Value,
    /// What the binding leaves out.
    pub omitted: Vec<String>,
    /// Always `captured_body_value`: the body's own refractive index is used.
    pub dielectric_ior: String,
    /// Always says the binding is a request's, not saved.
    pub persistence: String,
    /// The setups the maps can be saved in: both in a build with the bundled render library, none
    /// without.
    pub compatible_saved_methods: Vec<String>,
    /// What a saved CPU setup does with the maps.
    pub cpu_maps: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

fn source_maps_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The surface's source maps by role, each with its file, size, encoding and hashes.",
        "type": "object",
        "additionalProperties": {"type": "object"}
    })
}

/// A planar binding of a surface's maps to a body.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PreparedTexture {
    /// The body's place in the render scene.
    pub body_index: usize,
    /// The body's name.
    pub body_name: String,
    /// The maps' origin and the two repeat vectors, in world mm.
    #[schemars(schema_with = "frame_schema")]
    pub frame: PlanarFrame,
    /// Always 1.
    pub normal_strength: f64,
    /// Always true: image v increases downward.
    pub flip_v: bool,
    /// Always `replace`: the map is the base colour.
    pub base_color_mode: String,
    /// The base-colour map, by its built-in address.
    pub base_color_srgb: String,
    /// The normal map, OpenGL convention.
    pub normal_opengl: String,
    /// The roughness map, linear.
    pub roughness_linear: String,
    /// The metallic map, linear.
    pub metallic_linear: String,
}

fn frame_schema(_: &mut SchemaGenerator) -> Schema {
    let vector = finite_vector_schema(f64::MAX);
    json_schema!({
        "description": "The maps' origin and the repeat vectors u and v, in world mm.",
        "type": "object",
        "properties": {"origin": vector, "u": vector, "v": vector},
        "required": ["origin", "u", "v"],
        "additionalProperties": false
    })
}

/// A rig as the studio commands take it: which authored rig, how much of it, and where the
/// subject is.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RigInput {
    /// The rig, as `render.library.list` gives its id with `kind` `rigs` (`RIG-…`).
    #[schemars(schema_with = "id_schema")]
    pub id: String,
    /// `complete` for every rig, `emitters_only` for a rig with passive cards, which it leaves
    /// out.
    #[schemars(schema_with = "variant_schema")]
    pub variant: String,
    /// The subject's centre, in mm: each within ±1e12.
    #[schemars(schema_with = "centre_schema")]
    pub center_mm: [f64; 3],
    /// The subject's largest size, in mm: 0.001 to 1000000. The rig scales to it.
    #[schemars(range(min = 0.001, max = 1_000_000.0))]
    pub subject_extent_mm: f64,
}

fn variant_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["complete", "emitters_only"]})
}

fn centre_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "array",
        "items": {"type": "number", "minimum": -1e12, "maximum": 1e12},
        "minItems": 3,
        "maxItems": 3
    })
}

/// Convert an authored lighting rig to the numbers the renderers use, for a subject: its
/// rectangular lights, a sun, and its passive cards as a stage. With `name`, also the setup a
/// CPU raytrace would save, and what adopting a sun's indirect lighting needs. Nothing is
/// changed. Rigs whose calibration is unresolved or that use an external sun are refused, and a
/// light brighter than the CPU takes is refused, not scaled.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = studio_prepare_required)]
pub struct StudioPrepareParams {
    // `id`, `variant`, `center_mm` and `subject_extent_mm` are optional in the type and required
    // in the schema (`studio_prepare_required`).
    /// The rig (`RIG-…`).
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "id_schema")]
    pub id: Option<String>,
    /// `complete`, or `emitters_only` for a rig with passive cards, which it leaves out.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "variant_schema")]
    pub variant: Option<String>,
    /// The subject's centre, in mm: each within ±1e12.
    #[serde(default, deserialize_with = "read_point", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "centre_schema")]
    pub center_mm: Option<[f64; 3]>,
    /// The subject's largest size, in mm: 0.001 to 1000000.
    #[serde(default, deserialize_with = "read_f64", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "f64", range(min = 0.001, max = 1_000_000.0))]
    pub subject_extent_mm: Option<f64>,
    /// The render setup to work out the CPU projection in, saved or new. Without it, only the
    /// rig's numbers are returned, and `set` and `enable_indirect` are refused. Default: none.
    #[serde(default, deserialize_with = "read_name", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "optional_name_schema")]
    pub name: Option<String>,
    /// Changes to the setup before the rig is projected, as `render.settings.set` takes them: an
    /// object of at most 15 of the setup's fields. Only with `name`; the setup must render with
    /// the CPU raytracer. Default: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "set_schema")]
    pub set: Option<Value>,
    /// Has no effect when `name` is given: the reply's `cpu.indirect_adoption` already shows what
    /// `render.library.studio.apply` would adopt. Without `name`, `true` is refused (as is a
    /// non-empty `set`). Default: false.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", default = "no")]
    pub enable_indirect: Option<bool>,
}

fn studio_prepare_required(schema: &mut Schema) {
    require_all(schema, &["id", "variant", "center_mm", "subject_extent_mm"]);
}

fn optional_name_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["string", "null"]})
}

/// What `render.library.studio.prepare` returns. With `name`, also the CPU projection.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudioPrepared {
    /// The rig's lights as the renderers take them.
    #[schemars(schema_with = "studio_lighting_schema")]
    pub lighting: StudioLighting,
    /// The rig's passive cards as stage panels; empty for `emitters_only` and for a rig without
    /// cards.
    #[schemars(schema_with = "stage_schema")]
    pub stage: Stage,
    /// What the rig was scaled to.
    pub projection: RigProjection,
    /// Always true: the lights' distance from the subject needs a look.
    pub requires_geometry_clearance_review: bool,
    /// Always false.
    pub document_changed: bool,
    /// With `name`: the CPU projection.
    #[serde(flatten)]
    pub cpu: Option<CpuProjection>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What a rig becomes in a CPU raytrace setup, given with `name`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CpuProjection {
    /// The setup the rig would give, not saved.
    #[schemars(schema_with = "setup_reply_schema")]
    pub setup: RenderSetup,
    /// Always `cpu-raytrace`.
    pub executor: String,
    /// The design's revision it was read at: pass it as `expected_revision` to apply.
    pub revision: u64,
    /// Whether a sun needs indirect lighting the setup lacks.
    pub requires_indirect: bool,
    /// What `enable_indirect` adopts; `null` when no sun needs it.
    pub indirect_adoption: Option<IndirectAdoption>,
}

/// The indirect lighting a CPU sun rig adopts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct IndirectAdoption {
    /// Always 8.
    pub path_depth: u32,
    /// The progressive settings given when the setup has none.
    pub progressive_if_absent: ProgressiveDefaults,
    /// Always 1.
    pub primary_samples: u32,
    /// Always true.
    pub shadows: bool,
}

/// The progressive sampling a CPU setup gets when it has none.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProgressiveDefaults {
    /// Always 16.
    pub samples_per_pixel: u32,
    /// Always 0.
    pub seed: u32,
}

/// What a rig was scaled to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RigProjection {
    /// The rig.
    pub id: String,
    /// The variant asked for.
    pub variant: String,
    /// The passive cards left out: all of them for `emitters_only`.
    pub passive_cards_omitted: usize,
    /// The passive cards in the stage: all of them for `complete`.
    pub passive_cards_included: usize,
    /// The rig's size in mm divided by 1000.
    pub scale: f64,
    /// Always says the lengths are world millimetres and the radiance is relative linear sRGB.
    pub physical_units: String,
    /// Always true: the rig's calibration is as authored.
    pub reference_calibration_not_remeasured: bool,
}

fn studio_lighting_schema(_: &mut SchemaGenerator) -> Schema {
    let vector = finite_vector_schema(f64::MAX);
    json_schema!({
        "description": "Lights as the renderers take them, in world mm: rectangular area lights (a centre, half-edge vectors u and v, and an RGB radiance) and an optional sun.",
        "type": "object",
        "properties": {
            "sun": {
                "description": "A distant disk; `null` when the rig has none.",
                "anyOf": [
                    {
                        "type": "object",
                        "properties": {
                            "direction": {"description": "Unit vector toward the sun.", "type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3},
                            "angular_radius_rad": {"type": "number"},
                            "radiance": vector
                        },
                        "required": ["direction", "angular_radius_rad", "radiance"]
                    },
                    {"type": "null"}
                ]
            },
            "environment": vector,
            "area_lights": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {"center": vector, "u": vector, "v": vector, "radiance": vector},
                    "required": ["center", "u", "v", "radiance"]
                }
            }
        },
        "required": ["sun", "environment", "area_lights"]
    })
}

fn stage_schema(_: &mut SchemaGenerator) -> Schema {
    let vector = finite_vector_schema(f64::MAX);
    json_schema!({
        "description": "Render-only panels: each a rectangle by its centre and half-edge vectors u and v, in world mm, with a linear reflectance.",
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "center_mm": vector,
                "half_u_mm": vector,
                "half_v_mm": vector,
                "reflectance_linear": vector
            },
            "required": ["id", "center_mm", "half_u_mm", "half_v_mm", "reflectance_linear"]
        }
    })
}

/// Save an authored lighting rig in a CPU raytrace or GPU path setup for a subject, in one undo
/// step: its lights, and its passive cards as the setup's stage. The setup's existing lighting
/// and environment go, and with them an existing stage only with `replace_stage`. The subject's
/// clearance from the lights is checked against bounding boxes, not exact geometry.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields, transform = studio_apply_required)]
pub struct StudioApplyParams {
    // `rig`, `name` and `replace_lighting_and_environment` are optional in the type and required
    // in the schema (`studio_apply_required`).
    /// The rig and the subject it is scaled to.
    #[serde(default, deserialize_with = "read_rig", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "RigInput")]
    pub rig: Option<RigInput>,
    /// The render setup, saved or new.
    #[serde(default, deserialize_with = "read_string", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub name: Option<String>,
    /// Changes to the setup before the rig is added, as `render.settings.set` takes them: an
    /// object of at most 15 of the setup's fields. The setup must render with the CPU raytracer
    /// or the GPU path tracer. Default: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "set_schema")]
    pub set: Option<Value>,
    /// Must be true: the rig replaces the setup's lighting and environment, so that one thing
    /// owns the energy.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "confirm_schema")]
    pub replace_lighting_and_environment: Option<bool>,
    /// Replace a stage the setup already has. Without it, such a setup is refused. Default: false.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", default = "no")]
    pub replace_stage: Option<bool>,
    /// Adopt the indirect lighting a CPU sun rig needs, as `render.library.studio.prepare`
    /// reports it. Without it, such a rig is refused. Default: false.
    #[serde(default, deserialize_with = "read_bool", skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool", default = "no")]
    pub enable_indirect: Option<bool>,
    /// The design's revision when the caller last read it, as `render.library.studio.prepare`
    /// gives it. If the design changed since, the call is refused. Default: not checked.
    #[serde(default, deserialize_with = "read_revision", skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "revision_schema")]
    pub expected_revision: Option<u64>,
}

fn studio_apply_required(schema: &mut Schema) {
    require_all(schema, &["rig", "name", "replace_lighting_and_environment"]);
}

fn confirm_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "boolean", "const": true})
}

fn revision_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": ["integer", "null"], "minimum": 0})
}

/// What `render.library.studio.apply` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StudioApplied {
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it.
    #[schemars(schema_with = "setup_reply_schema")]
    pub setup: RenderSetup,
    /// What the rig was scaled to.
    pub projection: RigProjection,
    /// Says the clearance was checked against bounding boxes, not exact geometry.
    pub clearance: String,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_library_tests.rs"]
mod tests;
