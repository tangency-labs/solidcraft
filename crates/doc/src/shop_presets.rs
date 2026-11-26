//! Shop presets (#20113): a person's own shop rules profiles, kept with the other things a person
//! makes (their preferences, outside every design) and offered by `sheet.check_shop` and the Shop
//! Check window beside the built-in Generic profile. A preset is a profile
//! (`docs/formats/shop-rules.md`) kept under an id and a name; it is checked with the same parser as
//! any profile before it is kept, and again whenever the list is read back, so a damaged row is
//! left out and never breaks the others.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::shop_rules::{self, Profile, ShopRulesError, text};

/// The most presets a person keeps.
pub const MAX_PRESETS: usize = 32;
/// The most bytes one preset's profile takes, written as JSON.
pub const MAX_PRESET_BYTES: usize = 256 * 1024;
const MAX_NAME: usize = 64;

fn refuse<T>(why: impl Into<String>) -> Result<T, ShopRulesError> {
    Err(ShopRulesError(why.into()))
}

/// An id made from a name: lower case letters and digits, one `-` between words, at most 32
/// characters; none for a name with no letter or digit in it.
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.chars().take(32).collect();
    let out = out.trim_end_matches('-').to_string();
    (!out.is_empty()).then_some(out)
}

/// A saved shop profile: its id (what `sheet.check_shop`'s `shop` names), its name, and the profile
/// object, whose own `id` and `name` are these.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub profile: Value,
}

impl Preset {
    /// A preset from a profile object, under `name` (the profile's own name when none is given). Its
    /// id is the profile's `id`, else made from the name. Refused with the reason when the name or the
    /// profile is not valid, or the id is a built-in profile's.
    pub fn new(name: Option<&str>, mut profile: Value) -> Result<Preset, ShopRulesError> {
        let Some(object) = profile.as_object_mut() else { return refuse("a shop profile is an object") };
        let name = match name {
            Some(n) => n.to_string(),
            None => object.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        };
        let name = name.trim().to_string();
        text("name", &name, MAX_NAME)?;
        let id = match object.get("id") {
            None | Some(Value::Null) => slug(&name),
            Some(Value::String(id)) => Some(id.clone()),
            Some(_) => return refuse("the profile `id` is text"),
        };
        let Some(id) = id else {
            return refuse(format!("the name `{}` has no letter or digit to make an id of", name.chars().take(40).collect::<String>()));
        };
        let clash = shop_rules::profile_ids()
            .into_iter()
            .find(|b| b.eq_ignore_ascii_case(&id) || shop_rules::profile(b).is_ok_and(|p| p.name.eq_ignore_ascii_case(&name)));
        if let Some(builtin) = clash {
            return refuse(format!("`{builtin}` is the built-in profile: give a preset an id and a name of its own"));
        }
        object.insert("id".into(), Value::String(id.clone()));
        object.insert("name".into(), Value::String(name.clone()));
        let bytes = serde_json::to_vec(&profile).map_or(usize::MAX, |b| b.len());
        if bytes > MAX_PRESET_BYTES {
            return refuse(format!("a preset is at most {MAX_PRESET_BYTES} bytes, not {bytes}"));
        }
        Profile::from_value(&profile)?;
        Ok(Preset { id, name, profile })
    }

    /// The profile, to check a part against.
    pub fn parsed(&self) -> Result<Profile, ShopRulesError> {
        Profile::from_value(&self.profile)
    }
}

/// A person's presets, in the order they were first saved. Read back leniently: a row that fails
/// the checks of [`Preset::new`], repeats an id or a name, or is over [`MAX_PRESETS`] is left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Presets(Vec<Preset>);

impl<'de> Deserialize<'de> for Presets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let rows = match Value::deserialize(d)? {
            Value::Array(rows) => rows,
            _ => Vec::new(),
        };
        let mut presets = Presets::default();
        for row in rows {
            let Ok(saved) = serde_json::from_value::<Preset>(row) else { continue };
            // The row's id and name are its profile's: a row that disagrees is damaged.
            let Ok(preset) = Preset::new(Some(&saved.name), saved.profile.clone()) else { continue };
            if preset.id == saved.id {
                let _ = presets.save(preset);
            }
        }
        Ok(presets)
    }
}

impl Presets {
    pub fn list(&self) -> &[Preset] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The preset `id` (ignoring case).
    pub fn get(&self, id: &str) -> Option<&Preset> {
        let id = id.trim();
        self.0.iter().find(|p| p.id.eq_ignore_ascii_case(id))
    }

    /// Keep `preset`, replacing the one with its id; whether it replaced one. Refused when another
    /// preset has its name, or there are [`MAX_PRESETS`] already.
    pub fn save(&mut self, preset: Preset) -> Result<bool, ShopRulesError> {
        if let Some(other) = self.0.iter().find(|p| p.name.eq_ignore_ascii_case(&preset.name) && !p.id.eq_ignore_ascii_case(&preset.id)) {
            return refuse(format!("the preset `{}` already has the name `{}`", other.id, other.name));
        }
        match self.0.iter().position(|p| p.id.eq_ignore_ascii_case(&preset.id)) {
            Some(i) => {
                if let Some(slot) = self.0.get_mut(i) {
                    *slot = preset;
                }
                Ok(true)
            }
            None if self.0.len() >= MAX_PRESETS => refuse(format!("at most {MAX_PRESETS} shop presets: delete one first")),
            None => {
                self.0.push(preset);
                Ok(false)
            }
        }
    }

    /// Remove the preset `id`, and give it back.
    pub fn delete(&mut self, id: &str) -> Result<Preset, ShopRulesError> {
        let id = id.trim();
        match self.0.iter().position(|p| p.id.eq_ignore_ascii_case(id)) {
            Some(i) => Ok(self.0.remove(i)),
            None => refuse(format!("`{}` is not a shop preset: {}", id.chars().take(40).collect::<String>(), self.known())),
        }
    }

    /// What a message says of the presets there are.
    pub fn known(&self) -> String {
        if self.0.is_empty() {
            "there are no presets saved".to_string()
        } else {
            format!("the presets are {}", self.0.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", "))
        }
    }
}

#[cfg(test)]
#[path = "shop_presets_tests.rs"]
mod tests;
