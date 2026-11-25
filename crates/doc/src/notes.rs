//! Notes: text anchored to a point in model space, drawn in the viewport as a small
//! screen-facing callout (#1631). Personal design documentation — reminders, decisions,
//! warnings — saved with the design, not collaboration.

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

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
    /// Done notes are kept as a record that the comment was worked off (an agent, or the
    /// user, ticked it in the Notes panel).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
    /// What was selected when the note was made: the body and the face, edge or vertex it
    /// refers to, so a reader (an agent) knows which element the comment means. Informative
    /// only: face and edge indices can change when the design rebuilds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<NoteTarget>,
}

/// Longest body name a note target keeps (characters).
pub const MAX_TARGET_CHARS: usize = 256;

/// The element a note refers to: a body, or one face, edge or vertex of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteTarget {
    /// `body`, `face`, `edge` or `vertex`.
    pub kind: String,
    /// The body's name.
    pub body: String,
    /// The face or edge index within the body (none for a whole body or a vertex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
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
        Note { id, anchor: Vec3::new(1.0, 2.0, 3.0), text: format!("Check clearance here.\nLine {id}"), hidden: false, done: false, target: None }
    }

    #[test]
    fn a_design_with_notes_survives_a_round_trip() {
        let mut doc = crate::Document::new("Noted");
        doc.notes.push(note(1));
        doc.notes.push(Note {
            id: 2,
            anchor: Vec3::new(-4.0, 0.0, 6.5),
            text: "Adjust after the test print".into(),
            hidden: true,
            done: true,
            target: Some(NoteTarget { kind: "face".into(), body: "Plate".into(), index: Some(15) }),
        });
        let json = serde_json::to_string(&doc).unwrap();
        let back: crate::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back, doc);
        assert_eq!(back.notes.len(), 2);
        assert!(back.notes[1].hidden);
        assert!(back.notes[1].done);
        assert_eq!(back.notes[1].target.as_ref().map(|t| t.body.as_str()), Some("Plate"));
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
        // Only the note itself: other parts of a document may have their own `done` or `target`.
        let note_json = serde_json::to_string(&doc.notes[0]).unwrap();
        assert!(!note_json.contains("done"));
        assert!(!note_json.contains("target"));
        assert!(serde_json::from_str::<crate::Document>(&json).unwrap().notes[0].text.starts_with("Check"));
    }

    #[test]
    fn a_note_target_follows_its_body_when_the_body_is_renamed() {
        let mut doc = crate::Document::new("Noted");
        let target = |body: &str| Some(NoteTarget { kind: "face".into(), body: body.into(), index: Some(3) });
        doc.notes.push(Note { target: target("Plate"), ..note(1) });
        doc.notes.push(Note { target: target("Bracket"), ..note(2) });
        doc.notes.push(note(3));
        doc.rename_body_keys("Plate", "Base plate");
        assert_eq!(doc.notes[0].target, target("Base plate"));
        assert_eq!(doc.notes[1].target, target("Bracket"), "a note on another body keeps its target");
        assert_eq!(doc.notes[2].target, None);
    }

    #[test]
    fn note_text_must_be_non_empty_and_bounded() {
        assert!(Note::text_is_valid("Extra thickness added for strength."));
        assert!(!Note::text_is_valid("   "));
        assert!(!Note::text_is_valid(&"x".repeat(MAX_NOTE_CHARS + 1)));
        assert!(Note::text_is_valid(&"x".repeat(MAX_NOTE_CHARS)));
    }
}
