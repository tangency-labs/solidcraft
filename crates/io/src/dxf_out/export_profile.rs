//! A DXF profile: which layer, colour and linetype each role of a flat pattern is written with,
//! and whether it is written at all. A shop keeps one as a file for its cutter
//! (`docs/formats/dxf-profiles.md`).
//!
//! A profile never changes what the flat-pattern producer may build: the file is made and checked
//! under its contract (`export_compatibility`) with the standard layer names, and the profile is
//! applied as the file is written. The neutral profile holds the standard names and styles, so
//! writing with it gives the bytes of writing without one.

use std::borrow::Cow;

use serde_json::{Map, Value, json};

use super::DxfLinetype;
use crate::{IoError, Result};

/// The `format` of a profile file.
pub const DXF_PROFILE_FORMAT: &str = "solidcraft-dxf-profile";
/// The version this code reads and writes.
pub const DXF_PROFILE_VERSION: u64 = 1;
/// A profile file is at most this long.
pub const MAX_DXF_PROFILE_BYTES: usize = 1 << 20;
/// A layer name of a profile is at most this long, which leaves room for a pocket layer's side
/// and depth within DXF's 255 characters.
const MAX_LAYER: usize = 200;
const MAX_NAME: usize = 128;
const MAX_NOTE: usize = 2000;

/// What a line of a flat pattern is for. Each role has a layer of its own in the standard file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DxfRole {
    /// The outer contour of each piece, and the cuts with no width along split bends.
    Outline,
    /// What is cut out inside the outline, and the loose pieces a cut leaves.
    Cut,
    /// Letters cut through the sheet.
    CutText,
    /// Letters marked on the sheet, and text machined to a depth.
    Etch,
    BendUp,
    BendDown,
    /// The note beside each bend line.
    BendNotes,
    /// Pocket boundaries, on one layer per side and depth.
    Pockets,
    /// The holes of a perforation.
    Perforation,
    /// Where a roll starts and ends, and its middle.
    Roll,
}

impl DxfRole {
    pub const ALL: [DxfRole; 10] = [
        DxfRole::Outline,
        DxfRole::Cut,
        DxfRole::CutText,
        DxfRole::Etch,
        DxfRole::BendUp,
        DxfRole::BendDown,
        DxfRole::BendNotes,
        DxfRole::Pockets,
        DxfRole::Perforation,
        DxfRole::Roll,
    ];

    /// The role's name in a profile file.
    pub fn key(self) -> &'static str {
        match self {
            DxfRole::Outline => "outline",
            DxfRole::Cut => "cut",
            DxfRole::CutText => "cut_text",
            DxfRole::Etch => "etch",
            DxfRole::BendUp => "bend_up",
            DxfRole::BendDown => "bend_down",
            DxfRole::BendNotes => "bend_notes",
            DxfRole::Pockets => "pockets",
            DxfRole::Perforation => "perforation",
            DxfRole::Roll => "roll",
        }
    }

    /// The standard layer (for pockets, the word their layer names start with), colour and
    /// linetype: what the flat-pattern producer's contract admits.
    fn standard(self) -> (&'static str, i16, DxfLinetype) {
        match self {
            DxfRole::Outline => ("OUTLINE", 7, DxfLinetype::Continuous),
            DxfRole::Cut => ("CUT", 4, DxfLinetype::Continuous),
            DxfRole::CutText => ("TEXT", 2, DxfLinetype::Continuous),
            DxfRole::Etch => ("ETCH", 6, DxfLinetype::Continuous),
            DxfRole::BendUp => ("BEND_UP", 1, DxfLinetype::Dashed),
            DxfRole::BendDown => ("BEND_DOWN", 5, DxfLinetype::Dashed),
            DxfRole::BendNotes => ("BEND_NOTES", 8, DxfLinetype::Continuous),
            DxfRole::Pockets => (POCKET, 3, DxfLinetype::Continuous),
            DxfRole::Perforation => ("PERFORATION", 3, DxfLinetype::Continuous),
            DxfRole::Roll => ("ROLL", 3, DxfLinetype::Continuous),
        }
    }

    /// The role a standard layer of a flat pattern holds, and what follows the role's own name in
    /// it (a pocket layer's side and depth). Layer 0 and a layer this version has no role for
    /// have none, and are written as they are.
    fn of_layer(name: &str) -> Option<(DxfRole, &str)> {
        if let Some(rest) = name.strip_prefix(POCKET) {
            return rest.starts_with('_').then_some((DxfRole::Pockets, rest));
        }
        DxfRole::ALL.into_iter().find(|role| role.standard().0 == name).map(|role| (role, ""))
    }
}

const POCKET: &str = "POCKET";

/// How one role is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxfRoleStyle {
    /// False leaves the role's entities and its layer out of the file.
    pub on: bool,
    pub layer: String,
    /// An AutoCAD colour index, 1 to 255.
    pub color: i16,
    pub linetype: DxfLinetype,
}

/// A checked profile: every role has a style, and roles that share a layer agree on its colour
/// and linetype.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DxfExportProfile {
    name: String,
    note: String,
    roles: Vec<(DxfRole, DxfRoleStyle)>,
}

fn refused(message: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("DXF profile: {message}"))
}

fn role_keys() -> String {
    DxfRole::ALL.map(DxfRole::key).join(", ")
}

fn linetype_key(linetype: DxfLinetype) -> &'static str {
    match linetype {
        DxfLinetype::Continuous => "continuous",
        DxfLinetype::Dashed => "dashed",
        DxfLinetype::Hidden => "hidden",
    }
}

/// Text of at most `max` characters on one line, trimmed.
fn text(top: &Map<String, Value>, key: &str, max: usize, lines: bool) -> Result<String> {
    match top.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) if s.chars().count() <= max && !s.chars().any(|c| c.is_control() && !(lines && c == '\n')) => Ok(s.trim().to_string()),
        Some(_) => Err(refused(format!("`{key}` must be text of at most {max} characters{}", if lines { "" } else { " on one line" }))),
    }
}

/// A layer name a DXF table can hold as it is: printable ASCII (the writer has no other layer
/// names yet), none of the characters DXF keeps for itself, and not layer 0, which is not a
/// role's to restyle.
fn layer_name(role: DxfRole, value: &Value) -> Result<String> {
    let key = role.key();
    let Value::String(name) = value else { return Err(refused(format!("role `{key}`: `layer` must be text"))) };
    if name.is_empty() || name.len() > MAX_LAYER {
        return Err(refused(format!("role `{key}`: `layer` must be 1 to {MAX_LAYER} characters")));
    }
    if name == "0" {
        return Err(refused(format!("role `{key}`: `layer` cannot be 0; `layers: single` writes everything on layer 0")));
    }
    let forbidden = |c: char| {
        !c.is_ascii() || c.is_ascii_control() || matches!(c, '<' | '>' | '/' | '\\' | '"' | '\'' | ':' | ';' | '?' | '*' | '|' | '=' | ',' | '`')
    };
    if name.chars().any(forbidden) || name.starts_with(' ') || name.ends_with(' ') {
        return Err(refused(format!(
            "role `{key}`: layer {name:?} cannot be a DXF layer name; use letters, digits, spaces inside the name, and - _ . $ ( ) # + and the like, not < > / \\ \" ' : ; ? * | = , `"
        )));
    }
    Ok(name.clone())
}

fn color(role: DxfRole, value: &Value) -> Result<i16> {
    value
        .as_i64()
        .filter(|index| (1..=255).contains(index))
        .and_then(|index| i16::try_from(index).ok())
        .ok_or_else(|| refused(format!("role `{}`: `color` must be an AutoCAD colour index, a whole number from 1 to 255, not {value}", role.key())))
}

fn linetype(role: DxfRole, value: &Value) -> Result<DxfLinetype> {
    let named = value.as_str().map(str::to_ascii_lowercase);
    match named.as_deref() {
        Some("continuous") => Ok(DxfLinetype::Continuous),
        Some("dashed") => Ok(DxfLinetype::Dashed),
        Some("hidden") => Ok(DxfLinetype::Hidden),
        _ => Err(refused(format!("role `{}`: `linetype` must be continuous, dashed or hidden, not {value}", role.key()))),
    }
}

impl DxfExportProfile {
    /// The built-in profile: every role on, with the standard layer, colour and linetype.
    pub fn neutral() -> DxfExportProfile {
        let roles = DxfRole::ALL
            .into_iter()
            .map(|role| {
                let (layer, color, linetype) = role.standard();
                (role, DxfRoleStyle { on: true, layer: layer.to_string(), color, linetype })
            })
            .collect();
        DxfExportProfile { name: "Neutral".to_string(), note: String::new(), roles }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn note(&self) -> &str {
        &self.note
    }

    /// How `role` is written.
    pub fn role(&self, role: DxfRole) -> Option<&DxfRoleStyle> {
        self.roles.iter().find(|(r, _)| *r == role).map(|(_, style)| style)
    }

    /// Read a profile file. Refused, with a message naming the value, when it is too long, is not
    /// UTF-8 or JSON, or says something this version cannot do as asked.
    pub fn read(bytes: &[u8]) -> Result<DxfExportProfile> {
        if bytes.len() > MAX_DXF_PROFILE_BYTES {
            return Err(refused(format!("the file is {} bytes; a profile is at most {MAX_DXF_PROFILE_BYTES} bytes", bytes.len())));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| refused("the file is not UTF-8 text"))?;
        let value: Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| refused(format!("the file is not valid JSON: {e}")))?;
        DxfExportProfile::from_json(&value)
    }

    /// A profile from its JSON. Nothing is skipped: a role, field or version this code does not
    /// know is refused, since a cutting file must not silently ignore a line of its profile.
    pub fn from_json(value: &Value) -> Result<DxfExportProfile> {
        let Value::Object(top) = value else { return Err(refused("a profile is a JSON object")) };
        // Another kind of file, then a later version, are named before a field they would hold.
        if top.get("format").and_then(Value::as_str) != Some(DXF_PROFILE_FORMAT) {
            return Err(refused(format!("`format` must be \"{DXF_PROFILE_FORMAT}\"")));
        }
        match top.get("version") {
            None | Some(Value::Null) => {}
            Some(v) if v.as_u64() == Some(DXF_PROFILE_VERSION) => {}
            Some(v) if v.as_u64().is_some_and(|n| n > DXF_PROFILE_VERSION) => {
                return Err(refused(format!("the profile is version {v}; this SolidCraft reads version {DXF_PROFILE_VERSION}")));
            }
            Some(v) => return Err(refused(format!("`version` must be {DXF_PROFILE_VERSION}, not {v}"))),
        }
        if let Some(key) = top.keys().find(|key| !matches!(key.as_str(), "format" | "version" | "name" | "note" | "roles")) {
            return Err(refused(format!("`{key}` is not known; a profile has format, version, name, note and roles")));
        }
        let mut profile = DxfExportProfile::neutral();
        profile.name = text(top, "name", MAX_NAME, false)?;
        profile.note = text(top, "note", MAX_NOTE, true)?;
        let none = Map::new();
        let roles = match top.get("roles") {
            None | Some(Value::Null) => &none,
            Some(Value::Object(roles)) => roles,
            Some(_) => return Err(refused("`roles` must be an object of roles")),
        };
        for (key, given) in roles {
            let Some((role, style)) = profile.roles.iter_mut().find(|(role, _)| role.key() == key) else {
                return Err(refused(format!("role `{key}` is not known; the roles are {}", role_keys())));
            };
            let role = *role;
            let Value::Object(given) = given else { return Err(refused(format!("role `{key}` must be an object"))) };
            for (field, value) in given {
                match field.as_str() {
                    "on" => style.on = value.as_bool().ok_or_else(|| refused(format!("role `{key}`: `on` must be true or false")))?,
                    "layer" => style.layer = layer_name(role, value)?,
                    "color" => style.color = color(role, value)?,
                    "linetype" => style.linetype = linetype(role, value)?,
                    _ => return Err(refused(format!("role `{key}`: `{field}` is not known; a role has on, layer, color and linetype"))),
                }
            }
        }
        profile.shared_layers_agree()?;
        Ok(profile)
    }

    /// Roles may share a layer (both bend directions on one, say), but a layer has one colour and
    /// one linetype. DXF layer names ignore case.
    fn shared_layers_agree(&self) -> Result<()> {
        let on = || self.roles.iter().filter(|(role, style)| style.on && *role != DxfRole::Pockets);
        for (i, (a, first)) in on().enumerate() {
            for (b, second) in on().skip(i + 1) {
                if first.layer.eq_ignore_ascii_case(&second.layer) && (first.color, first.linetype) != (second.color, second.linetype) {
                    return Err(refused(format!(
                        "roles `{}` and `{}` share layer {:?} but give it different colours or linetypes",
                        a.key(),
                        b.key(),
                        first.layer
                    )));
                }
            }
        }
        Ok(())
    }

    /// The profile as a file's JSON, every role in full.
    pub fn to_json(&self) -> Value {
        let mut roles = Map::new();
        for (role, style) in &self.roles {
            roles.insert(
                role.key().to_string(),
                json!({"on": style.on, "layer": style.layer, "color": style.color, "linetype": linetype_key(style.linetype)}),
            );
        }
        json!({"format": DXF_PROFILE_FORMAT, "version": DXF_PROFILE_VERSION, "name": self.name, "note": self.note, "roles": roles})
    }

    /// What a standard layer of a flat pattern is written as: its name, colour and linetype, or
    /// nothing when its role is off. A layer with no role keeps what it has.
    pub(super) fn written<'a>(&'a self, name: &'a str, color: i16, linetype: DxfLinetype) -> Option<(Cow<'a, str>, i16, DxfLinetype)> {
        let Some((style, rest)) = DxfRole::of_layer(name).and_then(|(role, rest)| Some((self.role(role)?, rest))) else {
            return Some((Cow::Borrowed(name), color, linetype));
        };
        if !style.on {
            return None;
        }
        let name = if rest.is_empty() { Cow::Borrowed(style.layer.as_str()) } else { Cow::Owned(format!("{}{rest}", style.layer)) };
        Some((name, style.color, style.linetype))
    }
}
