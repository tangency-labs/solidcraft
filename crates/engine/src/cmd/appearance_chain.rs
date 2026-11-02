//! Read the shared document appearance chain and replace exact saved overrides.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcraft_doc::appearance::{AppearanceScope, AppearanceTarget, Look};

use crate::params::bad;
use crate::{Result, Session};

const MAX_TARGETS: usize = 10_000;

fn name<'a>(cmd: &str, value: &'a Value) -> Result<&'a str> {
    value.as_str().filter(|s| !s.is_empty() && s.len() <= 4096).ok_or_else(|| bad(cmd, "target name must have 1..4096 bytes"))
}

fn component(s: &Session, cmd: &str, value: &Value) -> Result<u64> {
    let key = match value {
        Value::String(_) => name(cmd, value)?.to_string(),
        Value::Number(_) => value.as_u64().ok_or_else(|| bad(cmd, "component must be a name or unsigned id"))?.to_string(),
        _ => return Err(bad(cmd, "component must be a name or unsigned id")),
    };
    // A placement's name (`K:2`) means its component, as appearance.assign takes it (#9740).
    s.component_or_placement(&key).ok_or_else(|| bad(cmd, "unknown or root component"))
}

pub(super) fn chain(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "appearance.chain";
    let target = p.get("target").and_then(Value::as_object).ok_or_else(|| bad(cmd, "give one target object"))?;
    // A placed copy's name (`KB (K:2)`) means the body it shows, as appearance.assign takes it
    // (#9740); a name that is neither is left for the chain to refuse.
    let body = target.get("body").map(|b| name(cmd, b).map(|n| s.own_body_name(n).unwrap_or_else(|| n.to_string()))).transpose()?;
    let target = match (body.as_deref(), target.get("face"), target.get("component")) {
        (Some(body), None, None) if target.len() == 1 => AppearanceTarget::Body(body),
        (Some(body), Some(face), None) if target.len() == 2 => AppearanceTarget::Face {
            body,
            face: face.as_u64().and_then(|f| usize::try_from(f).ok()).ok_or_else(|| bad(cmd, "face must be a zero-based unsigned index"))?,
        },
        (None, None, Some(id)) if target.len() == 1 => AppearanceTarget::Component(component(s, cmd, id)?),
        _ => return Err(bad(cmd, "target must be {body}, {body, face}, or {component}")),
    };
    let chain = s.doc.appearance_chain(&s.model.state(), target).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(json!({
        "steps": chain.steps().into_iter().map(|(step, look)| json!({"step":step,"look":look.map(super::look_json)})).collect::<Vec<_>>(),
        "active": chain.active(),
    }))
}

/// A saved in-design look's label is inert; a string explicitly selects a library
/// recipe. Reuse assignment's optical parsing rather than a second preset resolver.
fn look(cmd: &str, value: &Value) -> Result<Look> {
    let params = match value {
        Value::String(_) => Cow::Owned(json!({"appearance":name(cmd, value)?})),
        Value::Object(_) => Cow::Borrowed(value),
        _ => return Err(bad(cmd, "from and to must be library names or look objects")),
    };
    if let Some(opacity) = params.get("opacity")
        && opacity.as_f64().filter(|o| o.is_finite() && (0.0..=1.0).contains(o)).is_none()
    {
        return Err(bad(cmd, "opacity must be a finite number in 0..1"));
    }
    if let Some(appearance) = params.get("appearance") {
        name(cmd, appearance)?;
    }
    super::appearance_look(cmd, &params)?.ok_or_else(|| bad(cmd, "from and to require a look; use appearance.assign to clear"))
}

fn scope(s: &Session, cmd: &str, value: &Value) -> Result<AppearanceScope> {
    let object = value
        .as_object()
        .filter(|o| !o.is_empty() && o.keys().all(|k| matches!(k.as_str(), "bodies" | "components")))
        .ok_or_else(|| bad(cmd, "scope must contain bodies and/or components lists"))?;
    let list =
        |key: &str| -> Result<&Vec<Value>> { object.get(key).and_then(Value::as_array).ok_or_else(|| bad(cmd, "scope targets must be lists")) };
    let mut count = 0usize;
    for key in object.keys() {
        count = count.saturating_add(list(key)?.len());
        if count > MAX_TARGETS {
            return Err(bad(cmd, "scope has more than 10000 targets"));
        }
    }
    let mut scope = AppearanceScope::default();
    if object.contains_key("bodies") {
        let state = s.model.state();
        let bodies: BTreeSet<_> = state.bodies.iter().map(|b| b.name.as_str()).collect();
        // A placed copy's name means the body it shows (#9740). Built once, and only when a
        // name is not the model's own.
        let mut placed: Option<BTreeMap<String, String>> = None;
        for body in list("bodies")? {
            let body = name(cmd, body)?;
            let own = if bodies.contains(body) {
                body.to_string()
            } else {
                let placed = placed.get_or_insert_with(|| s.body_frames().iter().map(|(world, own, _)| (world.clone(), own.clone())).collect());
                placed.get(body).cloned().ok_or_else(|| bad(cmd, "unknown body in scope"))?
            };
            scope.bodies.insert(own);
        }
    }
    if object.contains_key("components") {
        // Build indexes once: 10000 repeated component targets must not scan a
        // 10000-component design once per target.
        let ids: BTreeSet<_> = s.doc.components.iter().map(|c| c.id).collect();
        let names: BTreeMap<_, _> = s.doc.components.iter().rev().map(|c| (c.name.as_str(), c.id)).collect();
        // A placement's name (`K:2`) means its component (#9740).
        let placements: BTreeMap<_, _> = s.doc.occurrences.iter().rev().map(|o| (o.name.as_str(), o.component)).collect();
        for target in list("components")? {
            let id = match target {
                Value::String(_) => {
                    let key = name(cmd, target)?;
                    key.parse::<u64>().ok().filter(|id| ids.contains(id)).or_else(|| names.get(key).copied()).or_else(|| placements.get(key).copied())
                }
                Value::Number(_) => target.as_u64().filter(|id| ids.contains(id)),
                _ => None,
            }
            .filter(|id| *id != 0)
            .ok_or_else(|| bad(cmd, "unknown component in scope"))?;
            scope.components.insert(id);
        }
    }
    Ok(scope)
}

pub(super) fn replace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "appearance.replace";
    s.doc.appearances.check_budget().map_err(|e| bad(cmd, e.to_string()))?;
    let from = look(cmd, p.get("from").ok_or_else(|| bad(cmd, "from is required"))?)?;
    let to = look(cmd, p.get("to").ok_or_else(|| bad(cmd, "to is required"))?)?;
    let scope = p.get("scope").map(|v| scope(s, cmd, v)).transpose()?;
    // Every validation and allocation completes before obtaining a mutable document.
    let replacement = s.doc.appearances.replacing(&from, &to, scope.as_ref()).map_err(|e| bad(cmd, e.to_string()))?;
    let count = if let Some((appearances, count)) = replacement {
        s.doc_mut().appearances = appearances;
        count
    } else {
        0
    };
    Ok(json!({"count": count}))
}
