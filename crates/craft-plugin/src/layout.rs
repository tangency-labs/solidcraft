//! Dialog layout annotations on an entry's input schema. They change how the dialog draws its
//! fields, never what the entry accepts: validation, tool schemas and parameters ignore them.

use crate::{Result, invalid};
use serde_json::Value;

/// On a property: a heading drawn above it (an empty string draws a divider line).
pub const SECTION: &str = "x-solidcraft-section";
/// On the root schema: `"collapsible"` lets each section's heading fold its fields away.
pub const SECTIONS: &str = "x-solidcraft-sections";
/// On a number property: the sibling choice whose current value is the field's unit suffix.
pub const UNIT_FROM: &str = "x-solidcraft-unit-from";
/// On a property: an `https` page the field's help button opens.
pub const HELP: &str = "x-solidcraft-help";
/// On a string `oneOf` choice, or on a list of them (`uniqueItems` with `items` a string `oneOf`):
/// `{"version": 1, "style": "buttons"}` draws the options as a row of icon buttons.
pub const CHOICE: &str = "x-solidcraft-choice";

const MAX_TITLE: usize = 80;
const MAX_URL: usize = 2048;

/// Refuse malformed layout annotations on `schema` (an entry's root input schema) at load.
pub(crate) fn validate(schema: &Value, field: &str) -> Result<()> {
    if let Some(style) = schema.get(SECTIONS)
        && style.as_str() != Some("collapsible")
    {
        return Err(invalid(format!("{field}.{SECTIONS}"), "must be \"collapsible\" when present"));
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else { return Ok(()) };
    for (key, property) in properties {
        let at = |name: &str| format!("{field}.properties.{key}.{name}");
        if let Some(title) = property.get(SECTION)
            && !title.as_str().is_some_and(|t| t.chars().count() <= MAX_TITLE && !t.chars().any(char::is_control))
        {
            return Err(invalid(at(SECTION), format!("must be a heading of at most {MAX_TITLE} characters, or \"\" for a divider")));
        }
        if let Some(source) = property.get(UNIT_FROM) {
            let choice = source
                .as_str()
                .and_then(|name| properties.get(name))
                .filter(|p| p.get("type").and_then(Value::as_str) == Some("string") && (p.get("enum").is_some() || p.get("oneOf").is_some()));
            let number = matches!(property.get("type").and_then(Value::as_str), Some("number" | "integer"));
            if choice.is_none() || !number || source.as_str() == Some(key) {
                return Err(invalid(at(UNIT_FROM), "must be on a number field and name another field of this schema that is a string choice"));
            }
        }
        if let Some(url) = property.get(HELP)
            && !url.as_str().is_some_and(help_url)
        {
            return Err(invalid(at(HELP), format!("must be an https:// link of at most {MAX_URL} characters")));
        }
        if let Some(choice) = property.get(CHOICE)
            && !(choice.as_object().is_some_and(|c| {
                c.len() == 2 && c.get("version").and_then(Value::as_u64) == Some(1) && c.get("style").and_then(Value::as_str) == Some("buttons")
            }) && icon_choice(property))
        {
            return Err(invalid(
                at(CHOICE),
                "must be {\"version\": 1, \"style\": \"buttons\"} on a string oneOf choice, or on an array with uniqueItems and items that are one",
            ));
        }
    }
    Ok(())
}

/// A string `oneOf` choice, or a list of distinct ones: the properties [`CHOICE`] may draw.
pub fn icon_choice(property: &Value) -> bool {
    let string_choice = |p: &Value| p.get("type").and_then(Value::as_str) == Some("string") && p.get("oneOf").is_some_and(Value::is_array);
    match property.get("type").and_then(Value::as_str) {
        Some("array") => property.get("uniqueItems") == Some(&Value::Bool(true)) && property.get("items").is_some_and(string_choice),
        _ => string_choice(property),
    }
}

/// A help link the dialog may open: an `https` URL whose host is a domain name (never an IP
/// address), with no spaces or control characters.
pub fn help_url(text: &str) -> bool {
    text.len() <= MAX_URL
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
        && url::Url::parse(text)
            .is_ok_and(|url| url.scheme() == "https" && matches!(url.host(), Some(url::Host::Domain(domain)) if !domain.is_empty()))
}
