//! Notes: text anchored to a point in model space, drawn in the viewport as a small
//! screen-facing callout (#1631). Personal design documentation — reminders, decisions,
//! warnings — saved with the design, not collaboration.

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

/// Longest text a note may hold (characters).
pub const MAX_NOTE_CHARS: usize = 4000;

/// A note: an anchor point in model space (mm, design coordinates) and the text shown at
/// its projection. The callout always faces the screen because it is drawn in screen space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    /// The point the note is anchored to (design coordinates, mm).
    pub anchor: Vec3,
    /// The note's text (may span lines).
    pub text: String,
    /// Hidden notes are kept but not drawn (`browser.visibility` shows and hides them).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl Note {
    /// True when the text is usable: not empty and within [`MAX_NOTE_CHARS`].
    pub fn text_is_valid(text: &str) -> bool {
        !text.trim().is_empty() && text.chars().count() <= MAX_NOTE_CHARS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u64) -> Note {
        Note { id, anchor: Vec3::new(1.0, 2.0, 3.0), text: format!("Check clearance here.\nLine {id}"), hidden: false }
    }

    #[test]
    fn a_design_with_notes_survives_a_round_trip() {
        let mut doc = crate::Document::new("Noted");
        doc.notes.push(note(1));
        doc.notes.push(Note { id: 2, anchor: Vec3::new(-4.0, 0.0, 6.5), text: "Adjust after the test print".into(), hidden: true });
        let json = serde_json::to_string(&doc).unwrap();
        let back: crate::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.notes.len(), 2);
        assert!(back.notes[1].hidden);
    }

    #[test]
    fn a_design_saved_before_notes_loads_with_none() {
        // An older design file: no `notes` key at all.
        let json = serde_json::to_string(&crate::Document::new("Old")).unwrap();
        let back: crate::Document = serde_json::from_str(&json).unwrap();
        assert!(back.notes.is_empty());
    }

    #[test]
    fn hidden_notes_are_not_written_when_they_are_visible() {
        let mut doc = crate::Document::new("Noted");
        doc.notes.push(note(1));
        let json = serde_json::to_string(&doc).unwrap();
        assert!(!json.contains("hidden"));
        assert!(serde_json::from_str::<crate::Document>(&json).unwrap().notes[0].text.starts_with("Check"));
    }

    #[test]
    fn note_text_must_be_non_empty_and_bounded() {
        assert!(Note::text_is_valid("Extra thickness added for strength."));
        assert!(!Note::text_is_valid("   "));
        assert!(!Note::text_is_valid(&"x".repeat(MAX_NOTE_CHARS + 1)));
        assert!(Note::text_is_valid(&"x".repeat(MAX_NOTE_CHARS)));
    }
}
