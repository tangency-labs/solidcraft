//! Shared rule-manager lifecycle; families retain loading, field and deletion policy.
// Hooks stay in the caller's scope, preserving widget IDs and completion/Enter ownership.
macro_rules! rule_editor {
    ($ui:expr, $id:literal, $rules:ident, $pick:ident, $name:ident, $active:ident, $loaded:ident;
        new_name: $new_name:expr, load: $load:block, fields: $fields:block
    ) => {{
        let mut names: Vec<String> = $rules.iter().map(|r| r.name.clone()).collect();
        names.push("New rule…".into());
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        $crate::ui_kit::choice_row($ui, "Rule", $id, $pick, &refs);
        if *$loaded != Some(*$pick) {
            $load
            if *$pick >= $rules.len() {
                *$name = $new_name;
            }
            *$loaded = Some(*$pick);
        }
        if *$pick >= $rules.len() {
            $crate::ui_kit::text_row($ui, "Name", $name, "Name");
        }
        $fields
        $crate::ui_kit::row($ui, "Active rule", |ui| $crate::ui_kit::checkbox(ui, "", $active));
    }};
}

pub(crate) use rule_editor;

/// Both managers omit blank fields and trim only names supplied for a new rule.
pub(crate) fn params<'a>(
    selected_name: Option<&str>,
    name: &str,
    active: bool,
    fields: impl IntoIterator<Item = (&'a str, &'a String)>,
) -> Result<serde_json::Value, String> {
    let name = selected_name.unwrap_or_else(|| name.trim());
    if name.is_empty() {
        return Err("give the rule a name".into());
    }
    let mut p = serde_json::json!({"name": name, "active": active});
    for (key, value) in fields {
        if !value.trim().is_empty() {
            p[key] = serde_json::json!(value);
        }
    }
    Ok(p)
}
