//! Contracts and result types for the profile library commands in `cmd/frame_library.rs`
//! (`frame.library.list`, `.add`, `.edit`, `.remove`, `.replace`) and `frame.profile.points` (#4003).
//!
//! Unlike the older frame commands, these handlers read their parameters through the types here
//! ([`typed`]), so a call the contract accepts is exactly a call the handler reads. Whether a sketch
//! exists, an expression evaluates or a section can be built stays the handler's to check, as it
//! has the design.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;

use super::check::located;
use super::frame::{ProfileOut, ProfileParam};
use super::types::{ProfilesParam, SketchRef};
use crate::params::bad;

/// The call's parameters read as `T`, with the contract's message on the key that is wrong.
pub fn typed<T: DeserializeOwned>(cmd: &str, p: &Value) -> crate::Result<T> {
    let v = if p.is_null() { Value::Object(Default::default()) } else { p.clone() };
    serde_path_to_error::deserialize(&v).map_err(|e| bad(cmd, located(&e)))
}

/// A placement point an entry names: `{name, at: [x, y]}` in the profile's own mm, or
/// `{name, sketch_point}` for a point of the sketch the section is copied from.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct PointParam {
    /// The point's name: letters, digits, `_` and `-`, unique ignoring case, and not one of the
    /// names every profile has (`origin`, `center`, `top_left`...).
    pub name: String,
    /// Where it is, [x, y] mm in the profile's own plane (x across, y up).
    #[serde(default)]
    pub at: Option<[f64; 2]>,
    /// With `profile_sketch`: the id of the sketch point it is (`p3`).
    #[serde(default)]
    pub sketch_point: Option<String>,
}

/// Defines a parameter type with the keys `frame.library.add` and `frame.library.edit` share
/// (what an entry holds) after its own keys. Serde can't refuse unknown keys through `flatten`, so
/// each type spells them out.
macro_rules! entry_params {
    ($(#[$meta:meta])* $name:ident { $($own:tt)* }) => {
        $(#[$meta])*
        #[derive(Debug, Deserialize, JsonSchema)]
        #[schemars(deny_unknown_fields)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $($own)*
            /// The levels the profile is filed under in pickers, outermost first, at most 3:
            /// `["Steel", "SHS"]`.
            #[serde(default)]
            pub category: Option<Vec<String>>,
            /// The section as a built-in profile, its dimensions numbers in the design's units or
            /// length expressions over its parameters. Give it or `profile_sketch`, not both.
            #[serde(default)]
            pub profile: Option<ProfileParam>,
            /// The section copied from a sketch: its material (or the `profiles` picked), in the
            /// sketch's own x and y around its origin. The entry keeps the copy; later edits to the
            /// sketch don't change it. Give it or `profile`, not both.
            #[serde(default)]
            pub profile_sketch: Option<SketchRef>,
            /// With `profile_sketch`: which of its profiles are the section, as `solid.extrude`
            /// takes them.
            #[serde(default)]
            pub profiles: Option<ProfilesParam>,
            /// Placement points the entry names, beyond the ones every profile has and the ones its
            /// built-in kind generates (`frame.profile.points` lists them). Replaces the entry's list.
            #[serde(default)]
            pub points: Option<Vec<PointParam>>,
            /// The placement point a member made from it sits on unless it names another. Empty
            /// text clears it (the origin).
            #[serde(default)]
            pub default_point: Option<String>,
            /// The stock's material, by name. Empty text clears it.
            #[serde(default)]
            pub material: Option<String>,
            /// What the cut list calls it. Empty text clears it (the entry's name).
            #[serde(default)]
            pub description: Option<String>,
            /// The length the stock comes in, a length expression (`"6 m"`). Empty text clears it.
            #[serde(default)]
            pub stock_length: Option<String>,
        }
    };
}

entry_params! {
    /// Add a profile to the design's profile library. Members made from it (`frame.member` with
    /// `library`) change when it is edited.
    LibraryAddParams {
        /// Its name, unique in the library ignoring case: `SHS 40x40x3`.
        pub name: String,
    }
}

entry_params! {
    /// Change a profile of the library. Keys left out keep their values; every member made from
    /// it is rebuilt.
    LibraryEditParams {
        /// The profile to change, by id (`lp1`) or name.
        pub id: String,
        /// A new name, unique in the library ignoring case.
        #[serde(default)]
        pub name: Option<String>,
    }
}

/// Remove a profile from the library. Refused while members are made from it (`frame.library.replace`
/// moves them to another profile first).
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct LibraryRemoveParams {
    /// The profile, by id or name.
    pub id: String,
}

/// Move every member made from one library profile to another.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct LibraryReplaceParams {
    /// The profile the members are made from now, by id or name.
    pub from: String,
    /// The profile they are made from after, by id or name.
    pub to: String,
}

/// List the design's profile library.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct LibraryListParams {}

/// List a profile's placement points: a built-in `profile`, or a `library` profile. One of them.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
#[serde(deny_unknown_fields)]
pub struct ProfilePointsParams {
    /// A built-in profile.
    #[serde(default)]
    pub profile: Option<ProfileParam>,
    /// A profile of the design's library, by id or name.
    #[serde(default)]
    pub library: Option<String>,
}

/// A placement point: its name and where it is, mm in the profile's own plane (x across, y up).
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PointOut {
    pub name: String,
    pub at: [f64; 2],
}

/// An entry's section as it is saved: `{"source": "built_in", "profile": {...}}` or
/// `{"source": "drawn", "regions": [...]}`.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct SectionOut(pub Value);

impl JsonSchema for SectionOut {
    fn schema_name() -> Cow<'static, str> {
        "LibrarySection".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "The section: {\"source\": \"built_in\", \"profile\": {kind, …dimensions as given}} or {\"source\": \"drawn\", \"regions\": [the copied regions]}.",
            "type": "object",
            "required": ["source"]
        })
    }
}

/// One profile of the library, as the commands report it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct LibraryEntryOut {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub category: Vec<String>,
    pub section: SectionOut,
    /// With a built-in section: the profile in mm, every default filled in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileOut>,
    /// Every placement point: those every profile has, those its kind generates, then its own.
    pub points: Vec<PointOut>,
    /// The points the entry names itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_points: Vec<PointOut>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The stock length as given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_length: Option<String>,
    /// The stock length worked out, mm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock_length_mm: Option<f64>,
    /// The section's area, mm².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<f64>,
    /// The features whose members are made from it, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub used_by: Vec<String>,
    /// Why it can't be worked out in this design now (a parameter it uses is gone...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A feature that failed to rebuild after a library change.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct LibraryFeatureError {
    pub feature: String,
    pub error: String,
}

/// What `frame.library.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryListResult {
    pub profiles: Vec<LibraryEntryOut>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `frame.library.add` and `frame.library.edit` return: the entry, and the features that
/// failed to rebuild with it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryEntryResult {
    pub profile: LibraryEntryOut,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<LibraryFeatureError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `frame.library.remove` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryRemoveResult {
    /// The removed profile's id.
    pub removed: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `frame.library.replace` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LibraryReplaceResult {
    /// The features moved to the other profile, by name.
    pub features: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<LibraryFeatureError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `frame.profile.points` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfilePointsResult {
    pub points: Vec<PointOut>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "frame_library_tests.rs"]
mod tests;
