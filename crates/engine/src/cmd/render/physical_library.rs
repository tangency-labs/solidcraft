//! Read-only bridge between optical concepts and sourced physical references.
//! No appearance identifies a grade, assigns density, or supplies missing constants.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    physical_material::{PropertyData, ValidatedCard},
    render_resources::HdrId,
};
use std::{collections::BTreeSet, sync::OnceLock};
const PROFILES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/render/physical/profiles.json"));
const BINDINGS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/render/physical/bindings.json"));
const CARDS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/render/physical/exports/native-cards.json"));
const QUANTITIES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/render/physical/quantities.json"));
const SOURCES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/render/physical/sources.json"));
struct Data {
    profiles: Value,
    bindings: Value,
    cards: Value,
    sources: Value,
}
static DATA: OnceLock<std::result::Result<Data, String>> = OnceLock::new();
fn data(command: &str) -> Result<&'static Data> {
    let parsed = DATA.get_or_init(|| {
        let parse = |source: &str| -> std::result::Result<Value, String> {
            if source.len() > 4 * 1024 * 1024 {
                return Err("built-in physical source exceeds admission".into());
            }
            serde_json::from_str(source).map_err(|e| e.to_string())
        };
        let data = Data { profiles: parse(PROFILES)?, bindings: parse(BINDINGS)?, cards: parse(CARDS)?, sources: parse(SOURCES)? };
        for (value, key, maximum) in [
            (&data.profiles, "profiles", 256),
            (&data.bindings, "catalog", 1024),
            (&data.bindings, "surfaces", 256),
            (&data.cards, "items", 256),
            (&data.sources, "sources", 256),
        ] {
            let items =
                value[key].as_array().filter(|items| items.len() <= maximum).ok_or_else(|| format!("invalid built-in physical collection {key}"))?;
            if items.is_empty() {
                return Err(format!("empty built-in physical collection {key}"));
            }
        }
        for (name, source) in [("profiles.json", PROFILES), ("sources.json", SOURCES), ("bindings.json", BINDINGS), ("quantities.json", QUANTITIES)] {
            if data.cards["source_sha256"][name].as_str() != Some(HdrId::of(source.as_bytes()).to_string().as_str()) {
                return Err(format!("physical native projection source hash mismatch: {name}"));
            }
        }
        Ok(data)
    });
    parsed.as_ref().map_err(|e| bad(command, e.clone()))
}
fn items<'a>(value: &'a Value, key: &str, command: &str) -> Result<&'a [Value]> {
    value[key].as_array().map(Vec::as_slice).ok_or_else(|| bad(command, "physical reference array missing"))
}
fn by<'a>(values: &'a [Value], key: &str, id: &str, command: &str) -> Result<&'a Value> {
    if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')) {
        return Err(bad(command, "invalid physical reference identity"));
    }
    values.iter().find(|value| value[key].as_str() == Some(id)).ok_or_else(|| bad(command, format!("unknown physical reference {id}")))
}
fn summary(profile: &Value, card: &Value) -> Value {
    json!({"id":profile["id"],"name":profile["designation"],"family":profile["family"],"source_status":"reference_selection_required",
        "projection":{"status":"reference_only","native_projection_status":card["native_projection_status"],"physical_assignment":false,"render_optics":false},
        "property_count":profile["properties"].as_array().map_or(0,Vec::len)})
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Query {
    kind: String,
    query: String,
    offset: usize,
    limit: usize,
    #[serde(default)]
    availability: Option<String>,
}
impl Default for Query {
    fn default() -> Self {
        Self { kind: "physical_profiles".into(), query: String::new(), offset: 0, limit: 48, availability: None }
    }
}
pub(super) fn list(params: &Value) -> Result<Value> {
    let command = "render.library.list";
    let input: Query = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.kind != "physical_profiles"
        || input.query.len() > 256
        || input.query.chars().any(char::is_control)
        || input.offset > 4096
        || !(1..=128).contains(&input.limit)
    {
        return Err(bad(command, "bounded physical profile query required"));
    }
    if input.availability.as_deref().is_some_and(|v| !matches!(v, "all" | "usable")) {
        return Err(bad(command, "physical profiles are reference-only; use all or usable for read-only inspection"));
    }
    let data = data(command)?;
    let profiles = items(&data.profiles, "profiles", command)?;
    let cards = items(&data.cards, "items", command)?;
    let query = input.query.to_lowercase();
    let matches = |p: &&Value| {
        ["id", "designation", "family", "material_form"].iter().any(|key| p[*key].as_str().is_some_and(|text| text.to_lowercase().contains(&query)))
    };
    let total = profiles.iter().filter(matches).count();
    let records = profiles
        .iter()
        .filter(matches)
        .skip(input.offset)
        .take(input.limit)
        .map(|profile| {
            let id = profile["id"].as_str().ok_or_else(|| bad(command, "missing physical profile id"))?;
            Ok(summary(profile, by(cards, "profile_id", id, command)?))
        })
        .collect::<Result<Vec<_>>>()?;
    let end = input.offset.saturating_add(records.len());
    Ok(json!({"kind":"physical_profiles","total":total,"offset":input.offset,"next_offset":(end<total).then_some(end),"items":records}))
}
pub(super) fn describe(id: &str) -> Result<Value> {
    let command = "render.library.describe";
    let data = data(command)?;
    let profile = by(items(&data.profiles, "profiles", command)?, "id", id, command)?;
    let projection = by(items(&data.cards, "items", command)?, "profile_id", id, command)?;
    let card = ValidatedCard::from_value(&projection["card"]).map_err(|e| bad(command, e.to_string()))?;
    let validation = card.card().validate().map_err(|e| bad(command, e.to_string()))?;
    let mut ids = BTreeSet::new();
    for property in items(profile, "properties", command)? {
        if let Some(source) = property["source_id"].as_str() {
            ids.insert(source);
        }
    }
    let sources =
        items(&data.sources, "sources", command)?.iter().filter(|s| s["id"].as_str().is_some_and(|id| ids.contains(id))).collect::<Vec<_>>();
    Ok(json!({"kind":"physical_profiles","resource":profile,"projection":summary(profile,projection)["projection"],
        "native_projection":projection,"native_validation":validation,"sources":sources,
        "receipts":{"profiles_sha256":HdrId::of(PROFILES.as_bytes()),"native_cards_sha256":HdrId::of(CARDS.as_bytes())},
        "document_changed":false,"physical_assignment":false,"source_manifest_unchanged":true}))
}
/// Return the exact role/relation associations; never pick the first candidate.
pub(super) fn associations(id: &str) -> Result<Value> {
    let command = "render.library.describe";
    let data = data(command)?;
    let concepts = items(&data.bindings, "catalog", command)?;
    let selected = if id.starts_with("SURF-") {
        let surface = by(items(&data.bindings, "surfaces", command)?, "surface_id", id, command)?;
        let ids = surface["physical_binding_ids"].as_array().ok_or_else(|| bad(command, "surface physical references missing"))?;
        ids.iter().map(|id| by(concepts, "catalog_id", id.as_str().unwrap_or(""), command)).collect::<Result<Vec<_>>>()?
    } else {
        vec![by(concepts, "catalog_id", id, command)?]
    };
    let mut ids = BTreeSet::new();
    for concept in &selected {
        for reference in items(concept, "references", command)? {
            if let Some(id) = reference["profile_id"].as_str() {
                ids.insert(id);
            }
        }
    }
    let profiles = items(&data.profiles, "profiles", command)?;
    let cards = items(&data.cards, "items", command)?;
    let references =
        ids.into_iter().map(|id| Ok(summary(by(profiles, "id", id, command)?, by(cards, "profile_id", id, command)?))).collect::<Result<Vec<_>>>()?;
    Ok(json!({"specifications":selected,"candidate_profiles":references,"auto_assign":false,"unknown_is_zero":false}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepare {
    id: String,
    name: String,
    acknowledge_reference: bool,
}
pub(super) fn prepare(_s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.physical.prepare";
    let input: Prepare = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if !input.acknowledge_reference
        || input.name.trim() != input.name
        || input.name.is_empty()
        || input.name.len() > 128
        || input.name.chars().any(char::is_control)
    {
        return Err(bad(command, "explicit reference acknowledgement and a bounded trimmed material name are required"));
    }
    let description = describe(&input.id)?;
    let card = ValidatedCard::from_value(&description["native_projection"]["card"]).map_err(|e| bad(command, e.to_string()))?;
    let mut density = None;
    for property in &card.card().properties {
        if let PropertyData::Density { kg_m3 } = &property.data
            && density.replace(*kg_m3).is_some()
        {
            return Err(bad(command, "multiple density conditions require an explicit selection; no default is inferred"));
        }
    }
    let density =
        density.ok_or_else(|| bad(command, "profile lacks one supported point density; ranges and unknowns cannot be replaced by a midpoint"))?;
    Ok(json!({"status":"plan_only_not_executed","profile_id":input.id,"receipts":description["receipts"],
        "native_validation":description["native_validation"],"not_projected":description["native_projection"]["not_projected"],
        "extended_profile":description["resource"],"document_changed":false,"assigned_bodies":[],
        "command":{"command":"material.create","params":{"name":input.name,"density_g_cm3":density/1000.0,"physical_card":card}},
        "requirements":["Review grade, formulation, source and conditions before execution.","This partial card is not a complete constitutive model or a certification.","Create does not assign a body; never use a constituent as an effective laminate, foam or populated assembly.","Retain this profile and omission receipt; the native card cannot encode all extended properties."]}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_profiles_and_bindings_are_accessible_without_changing_a_design() {
        let mut s = Session::default();
        let before = s.doc.clone();
        let history = (s.undo.len(), s.revision);
        let listing = list(&json!({"limit":128})).unwrap();
        assert_eq!(listing["total"], 59);
        for item in listing["items"].as_array().unwrap() {
            let id = item["id"].as_str().unwrap();
            let detail = describe(id).unwrap();
            assert_eq!(detail["physical_assignment"], false);
            assert!(!detail["native_validation"]["analysis_solver_available"].as_bool().unwrap());
        }
        // The integrated package now supplies references for all 48 authored
        // surfaces. A genuinely unbound catalog grade remains unknown, not zero.
        let paint = associations("SURF-013").unwrap();
        assert!(!paint["candidate_profiles"].as_array().unwrap().is_empty());
        assert_eq!(paint["auto_assign"], false);
        let missing = associations("MAT-009").unwrap();
        assert!(missing["candidate_profiles"].as_array().unwrap().is_empty());
        let steel = associations("SURF-001").unwrap();
        assert_eq!(steel["auto_assign"], false);
        assert!(!steel["candidate_profiles"].as_array().unwrap().is_empty());
        let plan = prepare(&mut s, &json!({"id":"steel-304-plate","name":"Reviewed reference","acknowledge_reference":true})).unwrap();
        assert_eq!(plan["command"]["params"]["density_g_cm3"], 7.9);
        assert_eq!(plan["status"], "plan_only_not_executed");
        assert_eq!(s.doc, before);
        assert_eq!((s.undo.len(), s.revision), history);
    }
    #[test]
    fn reference_plans_require_acknowledgement_and_do_not_guess_missing_density() {
        let mut s = Session::default();
        assert!(prepare(&mut s, &json!({"id":"steel-304-plate","name":"Review","acknowledge_reference":false})).is_err());
        assert!(list(&json!({"limit":129})).is_err());
        assert!(describe("../profiles").is_err());
        let values = list(&json!({"limit":128})).unwrap();
        let mut accepted = 0;
        let mut refused = 0;
        for value in values["items"].as_array().unwrap() {
            match prepare(&mut s, &json!({"id":value["id"],"name":"Reference","acknowledge_reference":true})) {
                Ok(_) => accepted += 1,
                Err(_) => refused += 1,
            }
        }
        assert_eq!((accepted, refused), (46, 13));
        assert!(s.doc.user_materials.is_empty());
    }
}
