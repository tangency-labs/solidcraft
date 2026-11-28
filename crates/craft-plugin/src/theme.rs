//! Capability `themes@0.1`: a plugin adds colour themes to SolidCraft's theme picker. Each theme is
//! a `solidcraft-theme/1` file in the plugin's folder, the format an imported theme uses, so the
//! host reads it with its own reader and lists it after the built-in themes.
//!
//! ```json
//! "capabilities": ["themes@0.1"],
//! "themes": [{"id": "solid-glass", "source": "solid-glass.json"}]
//! ```

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{HostError, Result, invalid, valid_id};

/// The most themes one plugin declares.
pub const MAX_THEMES: usize = 8;
/// The largest theme file, in bytes.
pub const MAX_THEME_BYTES: usize = 64 * 1024;
/// The theme file format a plugin's theme must declare.
pub const FORMAT: &str = "solidcraft-theme/1";

/// One theme in a plugin's manifest (`"themes": [...]`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeDecl {
    /// Unique within the plugin; the app names it `plugin:<plugin-id>/<id>`.
    pub id: String,
    /// A `.json` file name in the plugin's folder.
    pub source: String,
}

/// Checks a manifest's `themes` list: at most [`MAX_THEMES`], distinct ids, plain `.json` file names.
pub fn validate(themes: &[ThemeDecl]) -> Result<()> {
    if themes.len() > MAX_THEMES {
        return Err(invalid("themes", format!("at most {MAX_THEMES} themes")));
    }
    let mut ids = BTreeSet::new();
    for theme in themes {
        if !valid_id(&theme.id) || !ids.insert(&theme.id) {
            return Err(invalid("themes", "each theme needs a distinct lowercase id"));
        }
        if !plain_json_name(&theme.source) {
            return Err(invalid(format!("themes.{}.source", theme.id), "must be one lowercase .json file name in the plugin's folder"));
        }
    }
    Ok(())
}

/// Checks one theme file the package holds: at most [`MAX_THEME_BYTES`], a JSON object in the
/// theme format with a name. The host's reader checks the rest when it lists the theme.
pub fn check_file(id: &str, bytes: &[u8]) -> Result<()> {
    let field = format!("themes.{id}");
    if bytes.len() > MAX_THEME_BYTES {
        return Err(HostError::LimitExceeded { limit: format!("{field} is larger than 64 KiB") });
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|error| invalid(&field, format!("not JSON: {error}")))?;
    if !value.is_object() || value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(invalid(&field, format!("not a theme file: `format` must be \"{FORMAT}\"")));
    }
    if value.get("name").and_then(Value::as_str).is_none_or(|name| name.trim().is_empty()) {
        return Err(invalid(&field, "a theme file needs a `name`"));
    }
    Ok(())
}

fn plain_json_name(name: &str) -> bool {
    let Some((stem, "json")) = name.rsplit_once('.') else { return false };
    (1..=64).contains(&stem.len())
        && stem.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
        && !stem.starts_with(['-', '_'])
}
