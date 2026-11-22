//! Contracts and result types for the appearance commands in `cmd/inspect.rs`:
//! `appearance.assign`, `appearance.library` and `appearance.list`, and for the two gestures in
//! `cmd/render_ui.rs`: `appearance.paint` and `appearance.pick_up`.

use std::borrow::Cow;
use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use solidcraft_doc::appearance::Look;
use solidcraft_doc::pbr_appearance::PbrParameters;

use super::check::whole;
use super::types::{Point3, StringList};
use crate::params;

/// Give bodies, faces or components a look, or clear theirs: a library appearance or a starter
/// recipe by `appearance`, or a `color`. With neither (or `color: null`) the targets' own looks
/// are cleared, so a face follows its body, a body its component and a component its material.
/// One undo step; no dialogs.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceAssignParams {
    /// Bodies, by name (a placed copy's name, `KB (K:2)`, means the body it shows).
    #[serde(default)]
    pub bodies: Option<StringList>,
    /// Faces, by a point on each, at most 10000.
    #[serde(default)]
    pub faces: Option<FaceSpots>,
    /// The body the `faces` points are on, for points that don't name one. Default: the
    /// nearest body with a face there.
    #[serde(default)]
    pub body: Option<String>,
    /// Components, by name or id, or a placement's name (`K:2`).
    #[serde(default)]
    pub components: Option<StringList>,
    /// A library appearance's name or a starter recipe's name or stable `starter.*` id (see
    /// `appearance.library`). With `color` too, that colour under this name. A recipe that
    /// needs refraction (the glass ones) is refused.
    #[serde(default)]
    pub appearance: Option<String>,
    /// The colour, `"#rrggbb"` or `[r, g, b]` (0–255); with `appearance`, it replaces the
    /// recipe's colour. `null` (without `appearance`) clears the targets' looks.
    #[serde(default)]
    pub color: Option<ColorParam>,
    /// 0 (clear) to 1 (opaque). Default: 1, or the appearance's own. With `color` alone it
    /// must be within 0–1; with `appearance` it is clamped there.
    #[serde(default)]
    pub opacity: Option<f64>,
    /// A label for a copied look: needs `color`, not with `appearance`.
    #[serde(default)]
    pub name: Option<LookLabel>,
    /// The opaque PBR response (needs `color` or `appearance`, and opacity 1). Left out, a
    /// recipe keeps its own; `null` clears it.
    #[serde(default)]
    pub pbr: Option<PbrParam>,
}

/// `appearance.assign`'s `faces`: at most 10000 points, each `[x, y, z]` (or `{x, y, z}`) or
/// `{"point": [x, y, z], "body": name}`, read as the handler reads them.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceSpots(pub Vec<Value>);

impl<'de> Deserialize<'de> for FaceSpots {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let fail = || D::Error::custom("must list points [x, y, z]");
        let list = match Value::deserialize(d)? {
            Value::Array(list) if list.len() <= 10_000 => list,
            _ => return Err(fail()),
        };
        let point = |v: &Value| params::vec3(v).or_else(|| v.get("point").and_then(params::vec3));
        if list.iter().all(|v| point(v).is_some()) { Ok(FaceSpots(list)) } else { Err(fail()) }
    }
}

impl JsonSchema for FaceSpots {
    fn schema_name() -> Cow<'static, str> {
        "FaceSpots".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let point = generator.subschema_for::<Point3>();
        let on_body = json!({
            "type": "object",
            "properties": {"point": point, "body": {"type": "string"}},
            "required": ["point"]
        });
        json_schema!({
            "description": "Points in mm, one on each face: [x, y, z], or {\"point\": [x, y, z], \"body\": name} to say which body.",
            "type": "array",
            "items": {"anyOf": [point, on_body]},
            "maxItems": 10000
        })
    }
}

/// The text form `inspect::color_param` reads: three bytes after any number of `#`, with
/// spaces at either end as Rust's `trim` drops them. A byte is two hex digits, or a `+` and one
/// hex digit, because `u8::from_str_radix` takes a leading `+` (`"+f"` is 15).
fn color_pattern() -> String {
    format!("^[{ws}]*#*(?:[0-9A-Fa-f]{{2}}|\\+[0-9A-Fa-f]){{3}}[{ws}]*$", ws = super::types::whitespace_class())
}

/// A colour, `"#rrggbb"` (the `#` optional) or `[r, g, b]` of numbers 0–255, as
/// `inspect::color_param` reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorParam(pub [u8; 3]);

impl<'de> Deserialize<'de> for ColorParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        crate::cmd::inspect::color_param(&Value::deserialize(d)?)
            .map(ColorParam)
            .ok_or_else(|| D::Error::custom("must be \"#rrggbb\" or [r, g, b] with values 0–255"))
    }
}

impl JsonSchema for ColorParam {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let channel = json!({"type": "number", "minimum": 0, "maximum": 255});
        json_schema!({
            "description": "A colour: \"#rrggbb\" or [r, g, b] with values 0–255.",
            "anyOf": [
                {"type": "string", "pattern": color_pattern()},
                {"type": "array", "items": channel, "minItems": 3, "maxItems": 3}
            ]
        })
    }
}

/// A copied look's label: 1 to 128 characters, none of them a control character.
#[derive(Clone, Debug, PartialEq)]
pub struct LookLabel(pub String);

impl<'de> Deserialize<'de> for LookLabel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::String(s) if !s.is_empty() && s.chars().count() <= 128 && !s.chars().any(char::is_control) => Ok(LookLabel(s)),
            _ => Err(D::Error::custom("must be a nonempty label of at most 128 characters without controls")),
        }
    }
}

impl JsonSchema for LookLabel {
    fn schema_name() -> Cow<'static, str> {
        "LookLabel".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A label for a copied look, without control characters.",
            "type": "string",
            "minLength": 1,
            "maxLength": 128
        })
    }
}

/// An opaque PBR response, exactly `metallic`, `roughness` and `dielectric_ior`, checked as
/// the handler checks it (`attach_pbr`), with its messages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PbrParam(pub PbrParameters);

impl<'de> Deserialize<'de> for PbrParam {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let object = value
            .as_object()
            .filter(|o| o.len() == 3 && o.keys().all(|k| matches!(k.as_str(), "metallic" | "roughness" | "dielectric_ior")))
            .ok_or_else(|| D::Error::custom(whole("`pbr` requires exactly metallic, roughness and dielectric_ior")))?;
        let number = |key: &str| {
            object.get(key).and_then(Value::as_f64).ok_or_else(|| D::Error::custom(whole(format!("`pbr.{key}` must be a finite number"))))
        };
        PbrParameters::new(number("metallic")?, number("roughness")?, number("dielectric_ior")?).map(PbrParam).map_err(|e| D::Error::custom(whole(e)))
    }
}

impl JsonSchema for PbrParam {
    fn schema_name() -> Cow<'static, str> {
        "PbrParameters".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An opaque PBR response.",
            "type": "object",
            "properties": {
                "metallic": {"type": "number", "minimum": 0, "maximum": 1},
                "roughness": {"type": "number", "minimum": 0.045, "maximum": 1},
                "dielectric_ior": {"type": "number", "minimum": 1, "maximum": 3}
            },
            "required": ["metallic", "roughness", "dielectric_ior"],
            "additionalProperties": false
        })
    }
}

/// A look's PBR response, as the appearance commands report it.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub struct PbrReply {
    pub metallic: f64,
    pub roughness: f64,
    pub dielectric_ior: f64,
}

impl From<PbrParameters> for PbrReply {
    fn from(p: PbrParameters) -> Self {
        PbrReply { metallic: p.metallic(), roughness: p.roughness(), dielectric_ior: p.dielectric_ior() }
    }
}

/// A look, as the appearance commands report it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct LookReply {
    /// The library appearance's or recipe's name, or the look's label (`Custom` for a plain
    /// colour).
    pub name: String,
    /// `"#rrggbb"`.
    pub color: String,
    /// 0 (clear) to 1 (opaque).
    pub opacity: f64,
    /// Its opaque PBR response, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbr: Option<PbrReply>,
}

impl From<&Look> for LookReply {
    fn from(l: &Look) -> Self {
        LookReply { name: l.name.clone(), color: l.hex(), opacity: l.opacity, pbr: l.pbr.map(PbrReply::from) }
    }
}

/// A face with a look of its own: its body and the point on it the look was given at.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FaceSpot {
    pub body: String,
    /// `[x, y, z]` in mm.
    pub point: [f64; 3],
}

/// What `appearance.assign` returns: the targets, and the look they got (`null`: cleared).
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceAssignResult {
    pub bodies: Vec<String>,
    pub faces: Vec<FaceSpot>,
    /// The components, by id.
    pub components: Vec<u64>,
    pub appearance: Option<LookReply>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `appearance.library` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceLibraryParams {}

/// What `appearance.library` returns: every appearance `appearance.assign` takes by name.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceLibraryResult {
    pub appearances: Vec<LibraryAppearance>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One library entry: a plain appearance or finish, or a starter recipe.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum LibraryAppearance {
    Plain(PlainAppearance),
    Starter(StarterAppearance),
}

/// A legacy scalar appearance, or an opaque PBR finish (which has `pbr`).
#[derive(Debug, Serialize, JsonSchema)]
pub struct PlainAppearance {
    #[serde(flatten)]
    pub look: LookReply,
    /// The part of the name before ` - `, such as `Aluminum`.
    pub category: String,
}

/// A starter recipe: its look, where it comes from and what showing it needs.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StarterAppearance {
    /// Its stable id, `starter.*`, which `appearance.assign` takes as `appearance`.
    pub id: String,
    pub name: String,
    /// wood, metal, plastic, glass, solar or regolith.
    pub category: Value,
    pub revision: u32,
    /// Its preview colour, `"#rrggbb"`.
    pub color: String,
    pub opacity: f64,
    /// Its opaque PBR response; `null` for a recipe that has none (glass).
    pub pbr: Option<PbrReply>,
    /// False for a recipe this build can't show (the glass ones need refraction).
    pub available: bool,
    /// What showing it needs: opaque_pbr, refraction, certified_thin_or_closed.
    pub requirements: Value,
    /// Who made it and under which licence: {author, license, source, kind}.
    pub provenance: Value,
    /// Its optical recipe: {kind: opaque, color, metallic, roughness, dielectric_ior} or
    /// {kind: glass, dielectric_ior, transmission_linear_rgb, reference_path_mm, boundary}.
    pub recipe: Value,
    pub preview_note: String,
    /// The texture it is meant to get later ({id, repeat_mm, grain_axis}), or `null`.
    pub future_texture_recipe: Value,
}

/// `appearance.list` takes one parameter, `body`, optional.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceListParams {
    /// A body (or a placed copy's name, `KB (K:2)`, for the body it shows): the reply adds
    /// `body`, its resolved look and its faces' looks.
    #[serde(default)]
    pub body: Option<String>,
}

/// What `appearance.list` returns: the looks the design assigns, by look and by target.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceListResult {
    /// Each look the design uses, with where it is used.
    pub in_design: Vec<LookInDesign>,
    /// The bodies with a look of their own, by name.
    pub bodies: BTreeMap<String, LookReply>,
    /// The faces with a look of their own.
    pub faces: Vec<FaceLookReply>,
    /// The components with a look of their own.
    pub components: Vec<ComponentLookReply>,
    /// With `body`: that body's resolved look and its faces'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyLooks>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A look the design uses, and the bodies, faces and components it is given to.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LookInDesign {
    #[serde(flatten)]
    pub look: LookReply,
    pub bodies: Vec<String>,
    pub faces: Vec<FaceSpot>,
    /// Components, by id.
    pub components: Vec<u64>,
}

/// A face's own look.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceLookReply {
    pub body: String,
    /// `[x, y, z]` in mm: the point the look was given at.
    pub point: [f64; 3],
    pub appearance: LookReply,
}

/// A component's own look.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ComponentLookReply {
    /// The component's id.
    pub component: u64,
    pub appearance: LookReply,
}

/// A body's resolved look and its faces' looks.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BodyLooks {
    pub name: String,
    /// The look it shows (its own, else its component's, else its material's), or `null`.
    pub appearance: Option<LookReply>,
    /// The colour it is drawn in, `[r, g, b]` from 0 to 1, or `null` for the default.
    pub color: Option<[f32; 3]>,
    /// Its faces with a look (their own or imported), by face index.
    pub faces: Vec<FaceIndexLook>,
}

/// A face's look, by the face's index in `model.faces`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct FaceIndexLook {
    pub face: usize,
    pub appearance: LookReply,
}

/// `appearance.paint` takes a look exactly as `appearance.assign` does — the same five keys, read
/// by the same parser (`cmd::appearance_look`) — and `done: true` to end the UI's paint tool.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearancePaintParams {
    /// A library appearance's name or a starter recipe's name or stable `starter.*` id (see
    /// `appearance.library`). With `color` too, that colour under this name.
    #[serde(default)]
    pub appearance: Option<String>,
    /// The colour, `"#rrggbb"` or `[r, g, b]` (0–255); with `appearance`, it replaces the
    /// recipe's colour.
    #[serde(default)]
    pub color: Option<ColorParam>,
    /// A label for the look: needs `color`, not with `appearance`.
    #[serde(default)]
    pub name: Option<LookLabel>,
    /// 0 (clear) to 1 (opaque). Default: 1, or the appearance's own.
    #[serde(default)]
    pub opacity: Option<f64>,
    /// The opaque PBR response (needs `color` or `appearance`, and opacity 1).
    #[serde(default)]
    pub pbr: Option<PbrParam>,
    /// `true` ends the UI's paint tool; no look is read.
    #[serde(default)]
    pub done: Option<bool>,
}

/// What a paint request answers: one of the two objects the handler returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearancePaintResult {
    /// `true` when `done: true` ended the tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    /// The look to paint with, when the call gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<LookReply>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `appearance.pick_up` takes one parameter, `target`, optional.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearancePickUpParams {
    /// The body, face or component whose look to read. Left out, the reply asks the UI to
    /// sample one.
    #[serde(default)]
    pub target: Option<ChainTarget>,
}

/// What a pick-up request answers: one of the two objects the handler returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearancePickUpResult {
    /// `"pick_up"` when no target was given, which asks the UI to sample a look.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<&'static str>,
    /// The look the target resolves to, when a target was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<LookReply>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// An `appearance.chain` target, which is what `appearance.pick_up` takes: `{body}`,
/// `{body, face}` (a zero-based `model.faces` index) or `{component}` (a name or id, or a
/// placement's name such as `K:2`). Read as `cmd::appearance_chain::chain` reads it, with its
/// messages. The handler counts 4096 *bytes* of name; `maxLength` counts characters, so a name
/// of multi-byte characters is refused later than the schema says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainTarget {
    Body(String),
    Face { body: String, face: u64 },
    Component(String),
}

/// The chain's name check: text of 1..4096 bytes, as `cmd::appearance_chain::name` reads it.
fn chain_text(value: &str) -> Option<&str> {
    (!value.is_empty() && value.len() <= 4096).then_some(value)
}

/// The same check for a JSON value: a name, and nothing else.
fn chain_name(value: &Value) -> Option<&str> {
    value.as_str().and_then(chain_text)
}

impl<'de> Deserialize<'de> for ChainTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let object = value.as_object().ok_or_else(|| D::Error::custom(whole("give one target object")))?;
        // The handler reads the name first, so a name it can't use is what it says, whatever
        // else the object holds.
        let bad_name = || D::Error::custom(whole("target name must have 1..4096 bytes"));
        match (object.get("body"), object.get("face"), object.get("component")) {
            (Some(body), None, None) if object.len() == 1 => Ok(ChainTarget::Body(chain_name(body).ok_or_else(bad_name)?.to_string())),
            (Some(body), Some(face), None) if object.len() == 2 => {
                let body = chain_name(body).ok_or_else(bad_name)?.to_string();
                let face = face.as_u64().ok_or_else(|| D::Error::custom(whole("face must be a zero-based unsigned index")))?;
                Ok(ChainTarget::Face { body, face })
            }
            (None, None, Some(component)) if object.len() == 1 => match component {
                Value::String(_) => Ok(ChainTarget::Component(chain_name(component).ok_or_else(bad_name)?.to_string())),
                Value::Number(_) => component
                    .as_u64()
                    .map(|id| ChainTarget::Component(id.to_string()))
                    .ok_or_else(|| D::Error::custom(whole("component must be a name or unsigned id"))),
                _ => Err(D::Error::custom(whole("component must be a name or unsigned id"))),
            },
            _ => Err(D::Error::custom(whole("target must be {body}, {body, face}, or {component}"))),
        }
    }
}

impl JsonSchema for ChainTarget {
    fn schema_name() -> Cow<'static, str> {
        "ChainTarget".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let name = json!({"type": "string", "minLength": 1, "maxLength": 4096});
        json_schema!({
            "description": "A body, one of its faces, or a component: {body}, {body, face} or {component}.",
            "anyOf": [
                {"type": "object", "properties": {"body": name}, "required": ["body"], "additionalProperties": false},
                {"type": "object", "properties": {"body": name, "face": {"type": "integer", "minimum": 0}},
                 "required": ["body", "face"], "additionalProperties": false},
                {"type": "object", "properties": {"component": {"anyOf": [name, {"type": "integer", "minimum": 0}]}},
                 "required": ["component"], "additionalProperties": false}
            ]
        })
    }
}

/// `appearance.copy` takes one parameter, the `appearance.chain` target whose look goes into the
/// session's look clipboard. Unlike `appearance.pick_up`, which asks the UI to sample one, this call
/// has no form without a target.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceCopyParams {
    /// The body, face or component whose look to copy.
    pub target: ChainTarget,
}

/// What a copy request answers: the look that went into the clipboard, which is what
/// `appearance.paste` then applies. The clipboard is transient session state: nothing is saved with
/// the design and no dialog opens.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceCopyResult {
    /// The look that was copied.
    pub look: LookReply,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// The look `appearance.favorite` asks about: the same five keys `appearance.assign` and
/// `appearance.paint` take, read by the same parser (`cmd::appearance_look`), so a call either
/// names a library appearance or a starter recipe, or describes a look by `color`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct FavoriteLook {
    /// A library appearance's name or a starter recipe's name or stable `starter.*` id (see
    /// `appearance.library`). With `color` too, that colour under this name.
    #[serde(default)]
    pub appearance: Option<String>,
    /// The colour, `"#rrggbb"` or `[r, g, b]` (0–255); with `appearance`, it replaces the
    /// recipe's colour.
    #[serde(default)]
    pub color: Option<ColorParam>,
    /// A label for the look: needs `color`, not with `appearance`.
    #[serde(default)]
    pub name: Option<LookLabel>,
    /// 0 (clear) to 1 (opaque). Default: 1, or the appearance's own.
    #[serde(default)]
    pub opacity: Option<f64>,
    /// The opaque PBR response (needs `color` or `appearance`, and opacity 1).
    #[serde(default)]
    pub pbr: Option<PbrParam>,
}

/// `appearance.favorite` takes the look to mark and the preference: `look` is read as
/// `appearance.assign` reads one, `favorite` says whether to mark it. It writes nothing to the
/// design (its `doc_only`), so the reply is what the call described.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceFavoriteParams {
    /// The look to mark or unmark.
    pub look: FavoriteLook,
    /// `true` to mark it as a favourite, `false` to clear the mark.
    pub favorite: bool,
}

/// What a favourite request answers: the look the call described, resolved as `appearance.assign`
/// resolves one, and the preference back. Nothing was written to the design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceFavoriteResult {
    /// The look that was resolved, in the shape `appearance.copy` reports it.
    pub look: LookReply,
    /// The preference the call asked for.
    pub favorite: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `appearance.replace` takes as `from` and `to`: either a library appearance's name or a
/// starter recipe as plain text, or a look object with the same five keys as `appearance.assign`
/// (the same [`FavoriteLook`] `appearance.favorite` takes). Plain text means `{"appearance": …}`:
/// a saved look's own label is inert, only an explicit `appearance` selects a recipe. Read as
/// `cmd::appearance_chain::look` reads it, with its messages.
#[derive(Debug)]
pub enum ReplaceLook {
    /// A library appearance's name, or a starter recipe's name or `starter.*` id.
    Recipe(String),
    /// A look described by the assignment's own keys.
    Look(FavoriteLook),
}

impl<'de> Deserialize<'de> for ReplaceLook {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        match value {
            // The handler checks the name before it resolves anything, so a name it can't use is
            // what it says, whatever the library would have made of it.
            Value::String(name) => {
                Ok(ReplaceLook::Recipe(chain_text(&name).ok_or_else(|| D::Error::custom(whole("target name must have 1..4096 bytes")))?.to_string()))
            }
            // The same five keys `appearance.favorite` takes, read by the same type, so a look
            // object that is not one of those five is refused as the favourite's is.
            Value::Object(_) => FavoriteLook::deserialize(value).map(ReplaceLook::Look).map_err(|e| D::Error::custom(e.to_string())),
            _ => Err(D::Error::custom(whole("from and to must be library names or look objects"))),
        }
    }
}

impl JsonSchema for ReplaceLook {
    fn schema_name() -> Cow<'static, str> {
        "ReplaceLook".into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let name = json!({"type": "string", "minLength": 1, "maxLength": 4096});
        json_schema!({
            "description": "A library appearance or starter recipe by name, or a look object with the assignment's look keys.",
            "anyOf": [name, generator.subschema_for::<FavoriteLook>()]
        })
    }
}

/// The targets `appearance.replace`'s `scope` may name, at most 10000 of them between the two
/// lists, as `cmd::appearance_chain::scope` counts them.
const MAX_SCOPE_TARGETS: usize = 10_000;

/// `appearance.replace`'s optional `scope`: which saved overrides to search, `{bodies: [names],
/// components: [names or ids]}` with at least one of the two lists and at most 10000 targets
/// between them. A placed copy's name (`KB (K:2)`, `K:2`) means the body or component it shows, as
/// `appearance.assign` takes it (#9740). A body includes its face overrides; a component names only
/// its own override. Read as `cmd::appearance_chain::scope` reads it, with its messages; left out,
/// every override in the design is searched.
#[derive(Debug, Default)]
pub struct ReplaceScope {
    /// The bodies to search, by name.
    pub bodies: Vec<String>,
    /// The components to search, by name or id, kept as they were given.
    pub components: Vec<Value>,
}

impl<'de> Deserialize<'de> for ReplaceScope {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let object = value
            .as_object()
            .filter(|o| !o.is_empty() && o.keys().all(|key| matches!(key.as_str(), "bodies" | "components")))
            .ok_or_else(|| D::Error::custom(whole("scope must contain bodies and/or components lists")))?;
        // Counted as the handler counts: each key's list in turn, so a value that isn't one is
        // what it says, and a list that goes past the limit is refused before the next key is read.
        let list = |key: &str| -> Result<Option<&Vec<Value>>, D::Error> {
            match object.get(key) {
                None => Ok(None),
                Some(value) => Ok(Some(value.as_array().ok_or_else(|| D::Error::custom(whole("scope targets must be lists")))?)),
            }
        };
        let mut count = 0usize;
        for key in object.keys() {
            count = count.saturating_add(list(key)?.map_or(0, Vec::len));
            if count > MAX_SCOPE_TARGETS {
                return Err(D::Error::custom(whole("scope has more than 10000 targets")));
            }
        }
        let bodies = list("bodies")?;
        let components = list("components")?;
        let mut scope = ReplaceScope::default();
        for body in bodies.into_iter().flatten() {
            let name = chain_name(body).ok_or_else(|| D::Error::custom(whole("target name must have 1..4096 bytes")))?.to_string();
            scope.bodies.push(name);
        }
        // A component that is neither a name nor an id is the handler's to refuse as unknown: it
        // can only ever name one, and an id it can't read names none.
        for component in components.into_iter().flatten() {
            match component {
                Value::String(name) => {
                    let name = chain_text(name).ok_or_else(|| D::Error::custom(whole("target name must have 1..4096 bytes")))?.to_string();
                    scope.components.push(Value::String(name));
                }
                Value::Number(_) => {
                    scope.components.push(json!(component.as_u64().ok_or_else(|| D::Error::custom(whole("unknown component in scope")))?))
                }
                _ => return Err(D::Error::custom(whole("unknown component in scope"))),
            }
        }
        Ok(scope)
    }
}

impl JsonSchema for ReplaceScope {
    fn schema_name() -> Cow<'static, str> {
        "ReplaceScope".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let name = json!({"type": "string", "minLength": 1, "maxLength": 4096});
        json_schema!({
            "description": "The saved overrides to search: {bodies: [names]} and/or {components: [names or ids]}, at most 10000 targets in all.",
            "type": "object",
            "properties": {
                "bodies": {"type": "array", "items": name, "maxItems": MAX_SCOPE_TARGETS},
                "components": {"type": "array", "items": {"anyOf": [name, {"type": "integer", "minimum": 0}]}, "maxItems": MAX_SCOPE_TARGETS}
            },
            "minProperties": 1,
            "additionalProperties": false
        })
    }
}

/// `appearance.replace` rewrites the saved overrides that match `from` exactly — label, colour,
/// opacity and PBR alike — with `to`, in `scope` or in the whole design. One undo step; a call
/// that matches nothing changes neither the design nor its history. Physical-material defaults are
/// outside its scope.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct AppearanceReplaceParams {
    /// The look to replace: a library appearance or starter recipe by name, or a look object.
    pub from: ReplaceLook,
    /// The look to write in its place, in the same two forms.
    pub to: ReplaceLook,
    /// Which saved overrides to search: `{bodies}` and/or `{components}`. Left out, every override
    /// in the design, including hidden and suppressed ones.
    #[serde(default, deserialize_with = "scope")]
    #[schemars(schema_with = "scope_schema")]
    pub scope: Option<ReplaceScope>,
}

/// The handler reads a `scope` that isn't one of the two lists, `null` included, so the contract
/// refuses it the same way rather than passing it on as no scope.
fn scope<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ReplaceScope>, D::Error> {
    ReplaceScope::deserialize(d).map(Some)
}

fn scope_schema(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<ReplaceScope>()
}

/// What a replacement answers: how many saved overrides it rewrote. `0` means nothing matched —
/// `from` and `to` the same look, or no override in scope — which leaves the design and its
/// history unchanged.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AppearanceReplaceResult {
    /// The overrides that were rewritten: bodies, faces and components that matched `from`.
    pub count: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "appearance_tests.rs"]
mod tests;
