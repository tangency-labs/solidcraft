//! Validation and deferred opening for the normal plugin command form.
use super::*;
use crate::schema::plugin::{FormFieldError, FormRequest};

/// Validate stored inputs without executing plugin code or changing the design. Errors use
/// property keys so every front end can put them beside the corresponding field.
pub fn form_errors(session: &Session, schema: &Value, params: &Value) -> BTreeMap<String, FormFieldError> {
    let mut errors = BTreeMap::new();
    let invalid = |reason: String| FormFieldError::InvalidValue { reason };
    let Some(values) = params.as_object() else {
        errors.insert("params".into(), invalid("must be an object".into()));
        return errors;
    };
    let rules = match conditions::InputConditions::parse(schema) {
        Ok(rules) => rules,
        Err(error) => {
            errors.insert("params".into(), invalid(error.to_string()));
            return errors;
        }
    };
    let controls = match rules.visible_values(values) {
        Ok(controls) => controls,
        Err(error) => {
            errors.insert("params".into(), invalid(error.to_string()));
            return errors;
        }
    };
    let properties = schema.get("properties").and_then(Value::as_object);
    if values.len() > 256 || properties.is_some_and(|p| p.len() > 256) {
        errors.insert("params".into(), invalid("at most 256 form fields".into()));
        return errors;
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str).take(256) {
            if !values.contains_key(key) && rules.fields.get(key).is_none_or(|r| r.visible(&controls)) {
                errors.insert(key.into(), invalid("is required".into()));
            }
        }
    }
    for (key, value) in values {
        if solidcraft_doc::progress::cancelled() {
            errors.insert("params".into(), invalid("validation cancelled".into()));
            return errors;
        }
        if rules.fields.get(key).is_some_and(|r| !r.visible(&controls)) {
            continue;
        }
        let Some(property) = properties.and_then(|p| p.get(key)) else {
            if schema.get("additionalProperties") != Some(&Value::Bool(true)) {
                errors.insert(key.clone(), invalid("is not an input of this entry".into()));
            }
            continue;
        };
        let annotation = property.get("x-solidcraft-selection");
        let names = matches!(key.as_str(), "bodies" | "body_ids");
        let selection = annotation.is_some() || names || matches!(key.as_str(), "selection" | "target" | "targets");
        // A generic object item in a selection field means the shared Sel object, not an
        // empty object. Keep explicit item constraints intact.
        let mut checked_property = property.clone();
        if selection
            && let Some(items) = checked_property.get_mut("items").and_then(Value::as_object_mut)
            && items.len() == 1
            && items.get("type").and_then(Value::as_str) == Some("object")
        {
            items.insert("additionalProperties".into(), Value::Bool(true));
        }
        if let Err(error) = check_input(&checked_property, value) {
            errors.insert(key.clone(), invalid(error.to_string()));
            continue;
        }
        if let (Some(rule), Some(options)) = (rules.fields.get(key), property.get("oneOf").and_then(Value::as_array))
            && options.iter().enumerate().all(|(i, option)| !rule.option_available(i, &controls) || check_input(option, value).is_err())
        {
            errors.insert(key.clone(), invalid("is not an available choice".into()));
            continue;
        }
        if !selection {
            continue;
        }
        if property.get("type").and_then(Value::as_str) != Some("array") {
            continue;
        }
        let Some(items) = value.as_array().filter(|items| items.len() <= 4096) else {
            errors.insert(key.clone(), invalid("at most 4096 picks".into()));
            continue;
        };
        for item in items {
            if solidcraft_doc::progress::cancelled() {
                errors.insert("params".into(), invalid("validation cancelled".into()));
                return errors;
            }
            let pick = if names {
                item.as_str().map(|name| crate::Sel::Body { name: name.into() })
            } else {
                serde_json::from_value::<crate::Sel>(item.clone()).ok()
            };
            let accepted = pick.as_ref().is_some_and(|pick| {
                let category = match pick {
                    crate::Sel::Body { .. } => "body",
                    crate::Sel::Face { .. } => "face",
                    crate::Sel::Edge { .. } => "edge",
                    crate::Sel::Profile { .. } => "profile",
                    crate::Sel::ExternalSketchPoint { .. } => "sketch_point",
                    crate::Sel::SketchCurve { sketch: Some(_), .. } | crate::Sel::ExternalSketchCurve { .. } => "sketch_curve",
                    _ => return false,
                };
                let takes = annotation.map_or_else(
                    || if names { category == "body" } else { matches!(category, "body" | "face" | "edge") },
                    |a| {
                        a.get("version").and_then(Value::as_u64) == Some(1)
                            && a.get("accept").and_then(Value::as_array).is_some_and(|a| a.iter().any(|v| v == category))
                    },
                );
                takes && pick_exists(session, pick)
            });
            if !accepted {
                errors.insert(key.clone(), FormFieldError::MissingPick);
                break;
            }
        }
    }
    errors
}

fn pick_exists(session: &Session, pick: &crate::Sel) -> bool {
    use crate::Sel;
    let Ok(state) = session.world_state() else { return false };
    match pick {
        Sel::Body { name } => state.body(name).is_some(),
        Sel::Face { body, index, point } => state.body(body).is_some_and(|b| {
            let Some(tolerance) = pick_coordinate_tolerance(&b.body) else { return false };
            b.body.point_lies_on_face(*index, *point, tolerance).is_ok_and(|p| p.is_some())
        }),
        Sel::Edge { body, index, point } => state.body(body).is_some_and(|b| {
            // Verify the stored index AND point on the exact trimmed curve. Never substitute
            // a nearest-looking edge or use the display mesh to establish identity.
            let Some(tolerance) = pick_coordinate_tolerance(&b.body) else { return false };
            solidcraft_kernel::services::exact_edge_distance(&b.body, *index, [point.x, point.y, point.z]).is_some_and(|d| d <= tolerance)
        }),
        Sel::Profile { sketch, index } => state.sketch(*sketch).is_some_and(|s| s.profiles.get(*index).is_some()),
        Sel::ExternalSketchPoint { sketch, id } => state.sketch(*sketch).is_some_and(|s| s.sketch.point_index(id).is_some()),
        Sel::ExternalSketchCurve { sketch, id } | Sel::SketchCurve { sketch: Some(sketch), id } => {
            state.sketch(*sketch).is_some_and(|s| s.sketch.curve_index(id).is_some())
        }
        _ => false,
    }
}

/// Shared selection coordinates can be approximate hits. Measure their allowance from exact
/// bounds, independently of tessellation or display settings, and verify only the supplied index.
fn pick_coordinate_tolerance(body: &crate::kernel::Body) -> Option<f64> {
    if body.is_mesh() {
        return None;
    }
    let tolerance = (body.exact_bounds().ok()?.diagonal() * 1e-3).max(1e-3).max(body.record().tolerance);
    tolerance.is_finite().then_some(tolerance)
}

impl SolidCraftHost {
    pub(super) fn request_form(&mut self, entry: &str, params: &Value) -> craft_plugin::Result<()> {
        if !self.session.plugin_forms_interactive {
            let entry = self.form_source.as_ref().map(|(plugin, _)| format!("{plugin}.{entry}")).unwrap_or_else(|| entry.into());
            return Err(HostError::InteractiveRequired { entry });
        }
        let Some((plugin, entries)) = &self.form_source else {
            return Err(HostError::InteractiveRequired { entry: entry.into() });
        };
        if !entries.iter().any(|e| e.name == entry && e.kind == EntryKind::Command) {
            return Err(HostError::InvalidInput { field: "entry".into(), reason: "unknown command entry in this plugin".into() });
        }
        if self.forms.len() >= 8 {
            return Err(HostError::LimitExceeded { limit: "at most 8 form requests".into() });
        }
        self.forms.push(FormRequest { plugin: plugin.clone(), entry: entry.into(), params: params.clone() });
        Ok(())
    }
}
