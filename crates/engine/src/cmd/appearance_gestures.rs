//! Assignment tools share the existing look parser, chain and atomic assignment command.
use crate::{Result, Session, params::bad};
use serde_json::{Value, json};
use solidcraft_doc::appearance::Look;

pub(in crate::cmd) fn paint(_s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("done").and_then(Value::as_bool) == Some(true) {
        return Ok(json!({"done":true}));
    }
    let look = super::appearance_look("appearance.paint", p)?.ok_or_else(|| bad("appearance.paint", "choose a look first"))?;
    Ok(json!({"look": super::look_json(&look)}))
}

fn resolved(s: &mut Session, p: &Value) -> Result<Look> {
    let chain = super::appearance_chain::chain(s, p)?;
    let value = chain["steps"].as_array().and_then(|steps| steps.iter().find(|step| !step["look"].is_null())).map(|step| step["look"].clone());
    if let Some(value) = value {
        return super::appearance_look("appearance.pick_up", &value)?.ok_or_else(|| bad("appearance.pick_up", "target has no look"));
    }
    // Imported paint is outside the four-step chain, but remains the display fallback.
    let state = s.model.state();
    let body = p
        .pointer("/target/body")
        .and_then(Value::as_str)
        .and_then(|name| state.body(name))
        .ok_or_else(|| bad("appearance.pick_up", "target has no resolved look"))?;
    let displayed = p
        .pointer("/target/face")
        .and_then(Value::as_u64)
        .and_then(|face| usize::try_from(face).ok())
        .map_or_else(|| s.doc.model_display_look(body), |face| s.doc.model_face_look(body, face));
    if let Some(look) = displayed {
        return Ok(look);
    }
    let fallback = crate::view::colors::BODY;
    let color =
        s.doc.body_color(body).map(|rgb| rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)).unwrap_or([fallback.0, fallback.1, fallback.2]);
    Ok(Look { name: "Default".into(), color, opacity: 1.0, pbr: None })
}

pub(in crate::cmd) fn pick_up(s: &mut Session, p: &Value) -> Result<Value> {
    if p.as_object().is_some_and(|p| p.is_empty()) {
        return Ok(json!({"tool":"pick_up"}));
    }
    Ok(json!({"look":super::look_json(&resolved(s, p)?)}))
}

pub(in crate::cmd) fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let look = resolved(s, p)?;
    let reply = json!({"look":super::look_json(&look)});
    s.appearance_clipboard = Some(look);
    Ok(reply)
}

pub(in crate::cmd) fn favorite(_s: &mut Session, p: &Value) -> Result<Value> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        look: Value,
        favorite: bool,
    }
    let request: Request = serde_json::from_value(p.clone()).map_err(|e| bad("appearance.favorite", e.to_string()))?;
    let look = super::appearance_look("appearance.favorite", &request.look)?.ok_or_else(|| bad("appearance.favorite", "give a look"))?;
    Ok(json!({"look":super::look_json(&look), "favorite":request.favorite}))
}

pub(in crate::cmd) fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    let look = s.appearance_clipboard.as_ref().ok_or_else(|| bad("appearance.paste", "copy a look first"))?;
    let mut params = p.as_object().cloned().ok_or_else(|| bad("appearance.paste", "give assignment targets"))?;
    for key in ["appearance", "name", "color", "opacity", "pbr"] {
        params.remove(key);
    }
    for (key, value) in super::look_json(look).as_object().into_iter().flatten() {
        params.insert(key.clone(), value.clone());
    }
    super::appearance(s, &Value::Object(params))
}
