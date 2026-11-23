//! What every note worked out from a model feature shares (#7605 hole callouts, #13028 chamfer
//! notes): how its text is written ([`TextStyle`]) and how the feature id it names is read
//! ([`feature`]). One rule for both, so a sheet's hole callouts and chamfer notes read alike and
//! lose their features alike.

use crate::{DocError, Document, Feature, Result};

pub use super::dimension::MAX_DECIMALS;

/// How a note's text is written: the drawing's length unit and content language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextStyle {
    /// A length unit of the expression unit table (`mm`, `in`, `yd`, …), as a drawing fixes it.
    pub units: String,
    pub decimals: u8,
    /// The drawing's content language, a BCP 47 tag.
    pub language: String,
}

impl TextStyle {
    /// Refuses an unknown unit, too many decimals or a malformed language tag.
    pub fn validate(&self) -> Result<()> {
        if !super::is_length_unit(&self.units) {
            let unit: String = self.units.chars().take(32).collect();
            return Err(DocError::Invalid(format!("unknown length unit `{}` (such as mm or in)", unit.escape_debug())));
        }
        if self.decimals > MAX_DECIMALS {
            return Err(DocError::Invalid(format!("decimals are 0 to {MAX_DECIMALS}")));
        }
        if !super::is_language_tag(&self.language) {
            return Err(DocError::Invalid("the language is not a BCP 47 tag (such as `en` or `de-DE`)".into()));
        }
        Ok(())
    }

    /// An angle in radians as a note writes it: degrees, to the style's decimals.
    pub fn degrees(&self, radians: f64) -> String {
        format!("{:.*}°", usize::from(self.decimals.min(MAX_DECIMALS)), radians.to_degrees())
    }
}

/// The feature `id` names, for a note of a `kind` feature (`Hole`). `Ok(Err(reason))`: the note is
/// lost, since the id is one the design gave once and names nothing now. An id the design never
/// gave, or one of an item that is not a feature (a component, a folder, …), is an error.
pub fn feature<'d>(doc: &'d Document, id: u64, kind: &str) -> Result<std::result::Result<&'d Feature, String>> {
    if let Some(f) = doc.feature(id) {
        return Ok(Ok(f));
    }
    if let Some(other) = other_item(doc, id) {
        return Err(DocError::Invalid(format!("{id} is {other}, not a {kind} feature")));
    }
    // An id the design gave once belonged to something that has since been removed.
    if id >= 1 && id < doc.next_id {
        return Ok(Err(format!("feature {id} is gone")));
    }
    Err(DocError::Unknown(format!("feature {id}")))
}

/// What the id `id`, which names no feature, names instead, if anything.
fn other_item(doc: &Document, id: u64) -> Option<&'static str> {
    let a = &doc.assembly;
    [
        ("a component", doc.components.iter().any(|c| c.id == id)),
        ("an occurrence", doc.occurrences.iter().any(|o| o.id == id)),
        ("a browser folder", doc.browser_groups.iter().any(|g| g.id == id)),
        ("a timeline folder", doc.timeline_groups.iter().any(|g| g.id == id)),
        ("a canvas", doc.canvases.iter().any(|c| c.id == id)),
        ("a point cloud", doc.point_clouds.iter().any(|c| c.id == id)),
        ("a joint", a.joints.iter().any(|j| j.id == id)),
        ("a contact set", a.contact_sets.iter().any(|c| c.id == id)),
        ("a motion study", a.studies.iter().any(|s| s.id == id)),
        ("an exploded view", a.exploded.iter().any(|v| v.id == id)),
    ]
    .into_iter()
    .find_map(|(kind, is)| is.then_some(kind))
}
