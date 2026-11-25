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
    }
    Ok(())
}

/// A help link the dialog may open: `https`, with a host, no spaces or control characters.
pub fn help_url(url: &str) -> bool {
    url.len() <= MAX_URL && !url.chars().any(|c| c.is_whitespace() || c.is_control()) && url_host(url).is_some_and(|host| !host.is_empty())
}

/// The host of an `https` URL: its authority without user info or port (an IPv6 host keeps its
/// brackets). `None` when the URL is not `https`.
fn url_host(url: &str) -> Option<&str> {
    let authority = url.strip_prefix("https://")?.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    Some(match host_port.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map(|(ip, _)| ip)?,
        None => host_port.split(':').next()?,
    })
}
