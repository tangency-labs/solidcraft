//! Weld graphics: saved and undoable, with no timeline feature or body.
use super::CommandSpec;
use crate::params::{bad, req_expr};
use crate::schema::weld::{WeldCreateParams, WeldCreated, WeldDeleteParams, WeldDeleted};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};
use serde_json::Value;
use solidcraft_doc::expr::Kind;
use solidcraft_doc::weld::{MAX_WELDS, WeldRecord, WeldSettings};
const CREATE: TypedContract<WeldCreateParams> = TypedContract::new();
const DELETE: TypedContract<WeldDeleteParams> = TypedContract::new();
pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("weld.create", "Weld", crate::checked!("weld.create", create)).begins_edit()
        .at("WELDMENTS", "CREATE").icon("weld").contract(&CREATE).output(output_schema_for::<WeldCreated>)
        .params("weld_type: panel type id; picks?: grouped point or body/index edge/face references; size?: {a,z,z1,h,s,d,fill,collar,t,w: length expr}; face?, extent/sides, flags, look; symbol?: manual weld symbol → id, number"),
    CommandSpec::new("weld.delete", "Delete Weld", crate::checked!("weld.delete", delete))
        .icon("delete").contract(&DELETE).output(output_schema_for::<WeldDeleted>).params("id: weld graphic identity → deleted"),
];
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "weld.create";
    // Deserialize the contract even in schema-off mode: wrong types must never turn into defaults.
    let _: WeldCreateParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    let mut raw = p.as_object().cloned().ok_or_else(|| bad(CMD, "parameters must be an object"))?;
    let symbol = raw
        .remove("symbol")
        .filter(|v| !v.is_null())
        .map(serde_json::from_value::<solidcraft_geom::weld::Weld>)
        .transpose()
        .map_err(|e| bad(CMD, e.to_string()))?;
    if let Some(symbol) = &symbol {
        symbol.validate().map_err(|e| bad(CMD, e.to_string()))?;
    }
    // Optional null has the same meaning as omission in the contract.
    raw.retain(|_, v| !v.is_null());
    if let Some(picks) = raw.get_mut("picks") {
        let groups = picks.as_object_mut().ok_or_else(|| bad(CMD, "picks must be grouped lists"))?;
        let mut total = 0usize;
        for group in groups.values_mut() {
            let items = group.as_array_mut().ok_or_else(|| bad(CMD, "a pick group must be a list"))?;
            total = total.saturating_add(items.len());
            if total > solidcraft_doc::weld::MAX_PICKS {
                return Err(bad(CMD, "too many weld picks"));
            }
            for item in items {
                for key in ["point", "edge", "face"] {
                    if let Some(value) = item.get_mut(key).filter(|v| !v.is_null()) {
                        let p = crate::params::vec3(value).ok_or_else(|| bad(CMD, "invalid pick coordinates"))?;
                        *value = serde_json::json!([p.x, p.y, p.z]);
                    }
                }
            }
        }
    }
    if let Some(size) = raw.get("size") {
        let values = size.as_object().ok_or_else(|| bad(CMD, "size must be an object"))?;
        if values.len() > solidcraft_doc::weld::SIZE_KEYS.len() {
            return Err(bad(CMD, "too many size fields"));
        }
        let mut evaluated = serde_json::Map::new();
        for key in values.keys() {
            let expr = req_expr(CMD, size, key)?;
            let value = s.doc.eval(&expr, Kind::Length).map_err(|e| bad(CMD, e.to_string()))?;
            evaluated.insert(key.clone(), serde_json::json!(value));
        }
        raw.insert("size".into(), Value::Object(evaluated));
    }
    if raw.contains_key("pattern_count") {
        let expr = req_expr(CMD, p, "pattern_count")?;
        let value = s.doc.eval(&expr, Kind::Unitless).map_err(|e| bad(CMD, e.to_string()))?;
        raw.insert("pattern_count".into(), serde_json::json!(value));
    }
    if raw.contains_key("pattern_pitch") {
        let expr = req_expr(CMD, p, "pattern_pitch")?;
        let value = s.doc.eval(&expr, Kind::Length).map_err(|e| bad(CMD, e.to_string()))?;
        raw.insert("pattern_pitch".into(), serde_json::json!(value));
    }
    if raw.contains_key("edge_distance") {
        let expr = req_expr(CMD, p, "edge_distance")?;
        let value = s.doc.eval(&expr, Kind::Length).map_err(|e| bad(CMD, e.to_string()))?;
        raw.insert("edge_distance".into(), serde_json::json!(value));
    }
    if raw.contains_key("density") {
        let expr = req_expr(CMD, p, "density")?;
        let value = s.doc.eval(&expr, Kind::Unitless).map_err(|e| bad(CMD, e.to_string()))?;
        raw.insert("density".into(), serde_json::json!(value));
    }
    if raw.contains_key("overlap") {
        let expr = req_expr(CMD, p, "overlap")?;
        let value = s.doc.eval(&expr, Kind::Length).map_err(|e| bad(CMD, e.to_string()))?;
        raw.insert("overlap".into(), serde_json::json!(value));
    }
    let settings: WeldSettings = serde_json::from_value(Value::Object(raw)).map_err(|e| bad(CMD, e.to_string()))?;
    settings.validate().map_err(|e| bad(CMD, e))?;
    // Validate saved references against exact topology, never the display mesh.
    if settings.picks.values().flatten().any(|pick| pick.body.is_some()) {
        let state = s.world_state().map_err(|e| bad(CMD, e.to_string()))?;
        for (group, picks) in &settings.picks {
            for pick in picks {
                if let Some(body) = &pick.body {
                    let body = state.body(body).ok_or_else(|| bad(CMD, "picked body does not exist"))?;
                    if body.body.is_mesh() {
                        return Err(bad(CMD, "a weld topology pick requires an exact body"));
                    }
                    let exact = &body.body;
                    let face = pick.face.is_some() || (pick.edge.is_none() && ["groove", "area"].contains(&group.as_str()));
                    let count = if face { exact.face_count() } else { exact.edge_count() };
                    if pick.index.is_none_or(|index| index >= count) {
                        return Err(bad(CMD, "picked topology index does not exist"));
                    }
                }
            }
        }
    }
    if s.doc.welds.len() >= MAX_WELDS {
        return Err(bad(CMD, "too many weld graphics"));
    }
    let number = s
        .doc
        .welds
        .iter()
        .map(|w| w.number)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|n| *n <= 1_000_000)
        .ok_or_else(|| bad(CMD, "too many weld numbers"))?;
    if s.doc.next_item_id()? > solidcraft_doc::MAX_FEATURE_ID {
        return Err(bad(CMD, "too many document identities"));
    }
    let id = s.doc_mut().allocate_id()?;
    s.doc_mut().welds.push(WeldRecord { id, number, settings, symbol });
    serde_json::to_value(WeldCreated { id, number: format!("W{number}"), param_warnings: Vec::new() }).map_err(|e| bad(CMD, e.to_string()))
}
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "weld.delete";
    let params: WeldDeleteParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    let index = s.doc.welds.iter().position(|w| w.id == params.id).ok_or_else(|| bad(CMD, "weld graphic does not exist"))?;
    let record = s.doc_mut().welds.remove(index);
    serde_json::to_value(WeldDeleted { deleted: record.id, param_warnings: Vec::new() }).map_err(|e| bad(CMD, e.to_string()))
}
