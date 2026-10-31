//! Section Analyses: view cuts through the model, saved with the design (#3977).
//!
//! A design keeps any number of them, each with its own plane, distance, flip and hatch; at most
//! one is shown at a time. They change no geometry: the session shows the one that is on by
//! cutting the view (`Session::section`), and the browser lists them in its Analysis folder.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

/// Most Section Analyses one design keeps (a guard against hostile files and scripts).
pub const MAX_SECTIONS: usize = 1000;

/// The design's Section Analyses, and which one is shown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionAnalyses {
    /// In the order they were made (the browser lists them so).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SectionAnalysis>,
    /// The one whose eye is on (at most one: showing another turns this one off).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<u64>,
    /// The Analysis folder's own eye is off: no section is shown, and `shown` comes back when it
    /// is on again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub folder_hidden: bool,
}

impl SectionAnalyses {
    /// No Section Analysis (what older designs have).
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.shown.is_none() && !self.folder_hidden
    }

    /// The section that cuts the view now: the shown one, unless the folder is hidden.
    pub fn visible(&self) -> Option<&SectionAnalysis> {
        if self.folder_hidden {
            return None;
        }
        self.shown.and_then(|id| self.get(id))
    }

    pub fn get(&self, id: u64) -> Option<&SectionAnalysis> {
        self.items.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut SectionAnalysis> {
        self.items.iter_mut().find(|s| s.id == id)
    }

    /// An id no section has.
    pub fn next_id(&self) -> u64 {
        self.items.iter().map(|s| s.id).max().unwrap_or(0).saturating_add(1)
    }

    /// The first `name(n)` no section has, counting from one more than there are sections, as
    /// named views are numbered: `Section 1`, `Section 2`, …
    pub fn free_name(&self, name: impl Fn(usize) -> String) -> String {
        (self.items.len() + 1..=self.items.len() + MAX_SECTIONS + 1)
            .map(&name)
            .find(|n| !self.items.iter().any(|s| s.name == *n))
            .unwrap_or_else(|| name(self.items.len() + 1))
    }

    /// Body `old` is now called `new`: the faces sections are on follow it.
    pub fn rename_body(&mut self, old: &str, new: &str) {
        for s in &mut self.items {
            if let SectionPlane::Face { body, .. } = &mut s.plane
                && body == old
            {
                *body = new.to_string();
            }
        }
    }
}

/// One Section Analysis: a plane, moved along its normal by `offset`, cutting away what lies on
/// its normal's side (the other side with `flip`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionAnalysis {
    pub id: u64,
    pub name: String,
    /// The plane it cuts along (one plane today; a second one for a quarter cut could join it).
    pub plane: SectionPlane,
    /// The distance along the plane's own normal: a length expression (empty: zero).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub offset: String,
    /// Keep the plane's normal side instead of the other.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Hatch the cut faces (in each body's colour).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub hatch: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// What a Section Analysis cuts along. A construction plane and a planar face are followed:
/// the cut moves with them when the model changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SectionPlane {
    /// `XY`, `XZ` or `YZ`.
    Origin { name: String },
    /// A construction plane (by feature id).
    Construction { feature: u64 },
    /// A planar body face, by its body and its persistent name; `at` is where it was picked, to
    /// choose the nearest piece when the face is split.
    Face { body: String, face: String, at: Vec3 },
    /// A fixed plane in world coordinates (unit normal).
    Fixed { origin: Vec3, normal: Vec3 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_designs_and_defaults_round_trip() {
        let none: SectionAnalyses = serde_json::from_str("{}").unwrap();
        assert!(none.is_empty());
        let s = SectionAnalysis {
            id: 1,
            name: "Section 1".into(),
            plane: SectionPlane::Origin { name: "XY".into() },
            offset: String::new(),
            flip: false,
            hatch: true,
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({"id": 1, "name": "Section 1", "plane": {"type": "origin", "name": "XY"}}));
        let back: SectionAnalysis = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn free_names_count_on_from_the_sections() {
        let mut all = SectionAnalyses::default();
        let name = |n: usize| format!("Section {n}");
        assert_eq!(all.free_name(name), "Section 1");
        for (id, n) in [(1, "Section 1"), (2, "Section 3")] {
            all.items.push(SectionAnalysis {
                id,
                name: n.into(),
                plane: SectionPlane::Origin { name: "XY".into() },
                offset: String::new(),
                flip: false,
                hatch: true,
            });
        }
        assert_eq!(all.free_name(name), "Section 4");
        assert_eq!(all.next_id(), 3);
    }
}
