//! Saved Section Analyses: named sets of cutting planes a design keeps, so one can be shown in
//! place of another without filling in the planes again (#20649). The section on show is view
//! state; what is kept here is only what a name stands for.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

/// Longest name a saved section may have (characters).
pub const MAX_SAVED_SECTION_NAME_CHARS: usize = 128;
/// Most sections a design keeps.
pub const MAX_SAVED_SECTIONS: usize = 100;

fn yes() -> bool {
    true
}

/// One cutting plane of a saved section: what lies on the side its normal points to is hidden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSectionPlane {
    /// The plane's id in the section (1 to 6); it decides the hatch angle.
    pub id: u64,
    /// A point on the plane (design coordinates, mm).
    pub origin: Vec3,
    /// The plane's unit normal.
    pub normal: Vec3,
    /// The plane was hidden with its eye: kept, cutting nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// A named Section Analysis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedSection {
    pub name: String,
    /// The planes, shown and hidden, by id.
    #[serde(default)]
    pub planes: Vec<SavedSectionPlane>,
    /// Whether the cut faces are hatched.
    #[serde(default = "yes")]
    pub hatch: bool,
}

impl SavedSection {
    /// Whether `name` can name a saved section: 1 to [`MAX_SAVED_SECTION_NAME_CHARS`] characters.
    pub fn name_is_valid(name: &str) -> bool {
        !name.is_empty() && name.chars().count() <= MAX_SAVED_SECTION_NAME_CHARS
    }
}
