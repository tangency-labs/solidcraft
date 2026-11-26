//! The finish field (#20114): free text for a part's finish, with a short list of common finishes
//! to pick from beside it. Picking only fills the field in; what is typed is kept as it is, and
//! nothing checks it against the list. The Design Properties window and the Bill of Materials
//! panel both edit a finish through [`editor`].

use crate::ui_kit;
use solidcraft_ui_text::tr;

/// The message ids of the common finishes, in the order the list shows them. Each message is the
/// finish as it is written into the design (a colour or grade stays in the text).
const COMMON: [&str; 8] = [
    "ui.finish.common.powder_coat_matt",
    "ui.finish.common.powder_coat_gloss",
    "ui.finish.common.zinc_plate",
    "ui.finish.common.anodized_clear",
    "ui.finish.common.anodized_black",
    "ui.finish.common.passivated",
    "ui.finish.common.brushed",
    "ui.finish.common.mill",
];

/// The common finishes in the current language, in the order the list shows them.
pub fn common() -> impl Iterator<Item = &'static str> {
    COMMON.iter().map(|id| tr(id))
}

/// The common finishes as they are written into a design (what a pick stores), in the list's order:
/// in reading order, as the language's catalog holds them. [`common`] is what the list draws: a
/// right-to-left line comes back in display order, as egui has no bidirectional layout (see
/// `solidcraft_ui_text::bidi`), and stored that way it would read backwards everywhere else. A
/// message the language lacks reads in English, as it does on screen.
pub fn common_values() -> Vec<String> {
    let catalog = solidcraft_ui_text::locale().catalog();
    let english = solidcraft_ui_text::Locale::for_language("en").catalog();
    COMMON.iter().map(|id| catalog.messages().get(*id).or_else(|| english.messages().get(*id)).map_or(id.to_string(), String::clone)).collect()
}

/// The finish's text field with its pick list at the right. Returns whether the text changed.
/// `id` keeps the list's state apart from other finish fields.
pub fn editor(ui: &mut egui::Ui, id: &str, value: &mut String) -> bool {
    let was = value.clone();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui_kit::compact_dropdown_menu(ui, ("sc_finish_common", id), tr("ui.finish.pick"), |ui| {
            for (i, (finish, stored)) in common().zip(common_values()).enumerate() {
                let option = ui_kit::list_picker(ui, &format!("finish_common_{i}"), finish, *value == stored);
                crate::scenario::publish_handle(&format!("dialog:Finish:{finish}"), option.rect.center());
                if option.clicked() {
                    *value = stored;
                    ui.close();
                }
            }
        });
        ui_kit::text_field(ui, value, "");
    });
    *value != was
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list offers distinct, single-line, ordinary text, so a pick is always a finish the design
    /// can hold (`metadata::check_text`) and the same one never shows twice.
    #[test]
    fn the_common_finishes_are_distinct_text_a_design_can_hold() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("en");
        let all: Vec<&str> = common().collect();
        assert!(all.len() >= 6, "{all:?}");
        let mut seen = std::collections::BTreeSet::new();
        for finish in &all {
            assert!(!finish.is_empty() && !finish.starts_with("ui.finish."), "a message is missing: {finish}");
            assert!(finish.trim() == *finish && !finish.contains('\n'), "{finish}");
            assert!(solidcraft_engine::doc::metadata::check_text("finish", finish).is_ok(), "{finish}");
            assert!(seen.insert(*finish), "{finish} twice");
        }
        assert!(all.contains(&"Powder coat, RAL 9005, matt"), "a black powder coat is on the list");
    }

    /// A pick stores the finish in reading order, as a catalog holds it, not as the window draws it
    /// (right-to-left lines are drawn in display order, as egui has no bidirectional layout): a
    /// design opened in another language, or printed, must read right. Hebrew and Arabic have the
    /// finishes in their own script.
    #[test]
    fn a_picked_finish_is_stored_in_reading_order_in_a_right_to_left_language() {
        crate::i18n::install();
        for language in ["he", "ar"] {
            let _language = solidcraft_ui_text::scope(language);
            let catalog = solidcraft_ui_text::locale().catalog();
            let english = solidcraft_ui_text::Locale::for_language("en").catalog();
            let stored = common_values();
            assert_eq!(stored.len(), COMMON.len());
            let mut shown_differs = false;
            for (id, value) in COMMON.iter().zip(&stored) {
                let reading = catalog.messages().get(*id).or_else(|| english.messages().get(*id)).unwrap();
                assert_eq!(value, reading, "{language} {id}");
                shown_differs |= tr(id) != reading;
            }
            assert!(shown_differs, "{language}: some finish is drawn in display order, or this test proves nothing");
        }
    }
}
