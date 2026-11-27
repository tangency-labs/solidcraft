//! Keyboard profiles (#15302): the keys of another CAD tool, mapped onto SolidCraft's own commands,
//! for people who already know that tool's keys. Each profile is a JSON file in
//! `crates/ui-egui/keyboard-profiles/`, embedded at build time (`build.rs`), so a profile is
//! added or fixed without touching any code:
//!
//! ```json
//! {
//!     "label": "NX",
//!     "provenance": "where the keys come from",
//!     "keys": { "view.fit": "Ctrl+F" }
//! }
//! ```
//!
//! A profile is a layer between a command's registered keys and the user's own keys
//! (`keymap::keys`). It puts its key first on each command it names, and takes that key off any
//! other command that could not share it; every other key stays SolidCraft's. A key the app keeps
//! for itself (`keymap::RESERVED`) is never applied, and is listed instead (`LeftOut`).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde_json::Value;

use crate::keymap;

// `BUILTIN`, `(id, JSON text)` sorted by id.
include!(concat!(env!("OUT_DIR"), "/keyboard_profiles.rs"));

/// The `keyboard_profile` preference for SolidCraft's own keys, with no profile applied.
pub const DEFAULT: &str = "solidcraft";

/// A profile file's largest key list for one command, as for the user's own keys.
const MAX_KEYS: usize = 8;

pub struct Profile {
    pub id: &'static str,
    /// The other tool's name, as on the Navigation page's mouse presets.
    pub label: String,
    /// Command id → its keys in this profile: the profile's own first, then the registered keys
    /// it did not give to another command. Only the commands the profile changes.
    pub base: BTreeMap<&'static str, Vec<String>>,
    /// Command id → the keys the profile file gives it (canonical, applicable ones).
    pub mapped: BTreeMap<&'static str, Vec<String>>,
    /// Keys in the file that are never applied.
    pub reserved: Vec<LeftOut>,
}

/// A profile key that is not applied: the app keeps it, or one of the user's own keys uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftOut {
    pub command: &'static str,
    pub key: String,
    /// The command whose key, set by the user, holds the key; `None`: the app keeps it.
    pub taken_by: Option<&'static str>,
}

/// The built-in profiles, by label.
pub fn all() -> &'static [Profile] {
    static PROFILES: LazyLock<Vec<Profile>> = LazyLock::new(|| {
        let mut profiles: Vec<Profile> = BUILTIN
            .iter()
            .filter_map(|(id, text)| match serde_json::from_str::<Value>(text).map_err(|e| e.to_string()).and_then(|v| parse(id, &v)) {
                Ok(profile) => Some(profile),
                Err(e) => {
                    log::warn!("keyboard profile `{id}` ignored: {e}");
                    None
                }
            })
            .collect();
        profiles.sort_by_key(|p| p.label.to_lowercase());
        profiles
    });
    &PROFILES
}

/// The profile with this id; `None` for SolidCraft's own keys and an id this version doesn't know
/// (a newer version's profile), which keep the standard keys.
pub fn find(id: &str) -> Option<&'static Profile> {
    all().iter().find(|p| p.id == id)
}

/// The profile chosen in Preferences, if any.
pub fn chosen(app: &crate::SolveApp) -> Option<&'static Profile> {
    find(&app.preferences.keyboard_profile)
}

/// One profile file. A command this version doesn't have, or a key name it can't read, is skipped
/// with a warning, so a profile still loads when a command is renamed.
pub(crate) fn parse(id: &'static str, v: &Value) -> Result<Profile, String> {
    let label = v.get("label").and_then(Value::as_str).filter(|l| !l.trim().is_empty()).ok_or("no label")?;
    let keys = v.get("keys").and_then(Value::as_object).ok_or("no keys")?;
    let mut mapped: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut reserved = Vec::new();
    for (command, value) in keys {
        let Some(spec) = solidcraft_engine::find_command(command) else {
            log::warn!("keyboard profile `{id}`: no command `{command}`");
            continue;
        };
        let names: Vec<&str> = match value {
            Value::String(key) => vec![key.as_str()],
            Value::Array(list) => list.iter().filter_map(Value::as_str).take(MAX_KEYS).collect(),
            _ => return Err(format!("`{command}`: keys must be a key or a list of keys")),
        };
        let mut list: Vec<String> = Vec::new();
        for name in names {
            let Some(key) = keymap::canonical_key(name) else {
                log::warn!("keyboard profile `{id}`: unknown key for `{command}`");
                continue;
            };
            if keymap::reserved_for(spec.id, &key) {
                reserved.push(LeftOut { command: spec.id, key, taken_by: None });
            } else if !list.contains(&key) {
                list.push(key);
            }
        }
        mapped.insert(spec.id, list);
    }
    // Each command keeps the registered keys no profile key needs: its own after the profile's,
    // and another command's only where the two can share a key (a sketch tool and a model command).
    let takes = |id: &str, key: &str| mapped.iter().any(|(other, keys)| *other != id && keymap::clash(other, id) && keys.iter().any(|k| k == key));
    let mut base = BTreeMap::new();
    for spec in solidcraft_engine::command_specs() {
        let registered = keymap::default_keys(spec.id, spec.shortcut);
        let own = mapped.get(spec.id);
        if own.is_none_or(Vec::is_empty) && !registered.iter().any(|k| takes(spec.id, k)) {
            continue;
        }
        let mut keys: Vec<String> = own.cloned().unwrap_or_default();
        for key in registered {
            if !takes(spec.id, key) && !keys.iter().any(|k| k == key) {
                keys.push(key.to_string());
            }
        }
        base.insert(spec.id, keys);
    }
    Ok(Profile { id, label: label.trim().to_string(), base, mapped, reserved })
}

impl Profile {
    /// The keys of `id` in this profile before the user's own: `None` when the profile leaves the
    /// command's registered keys as they are. A profile key one of the user's own keys already
    /// holds for a command that can't share it is not applied (`left_out`).
    pub fn keys_of<'a>(&'a self, app: &crate::SolveApp, id: &str) -> Option<Vec<&'a str>> {
        let keys = self.base.get(id)?;
        let mapped = self.mapped.get(id);
        Some(
            keys.iter()
                .filter(|key| !mapped.is_some_and(|m| m.contains(key)) || keymap::user_key_holder(app, id, key).is_none())
                .map(String::as_str)
                .collect(),
        )
    }

    /// Every key of this profile that is not applied now: the reserved ones, then the ones the
    /// user's own keys hold.
    pub fn left_out(&self, app: &crate::SolveApp) -> Vec<LeftOut> {
        let mut out = self.reserved.clone();
        for (command, keys) in &self.mapped {
            for key in keys {
                if let Some(holder) = keymap::user_key_holder(app, command, key) {
                    out.push(LeftOut { command, key: key.clone(), taken_by: Some(holder) });
                }
            }
        }
        out
    }
}
