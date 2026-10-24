//! Written help for the ribbon tooltips (#2758): two short sentences per command, shown under the
//! tool name when the pointer rests on its button. The names stay as they are; the help is a
//! separate message `help.<command id>` in the catalogs, so a tool without help keeps its plain
//! tooltip. Text only for now; pictures and animations are a later step.

use crate::i18n;

/// The commands that have help text. Each one needs a `help.<id>` message in the catalogs.
pub const DOCUMENTED: &[&str] = &[
    "solid.chamfer",
    "solid.combine",
    "solid.extrude",
    "solid.fillet",
    "solid.hole",
    "solid.loft",
    "solid.mirror",
    "solid.pattern.circular",
    "solid.pattern.rectangular",
    "solid.revolve",
    "solid.shell",
    "solid.sweep",
];

/// The help for `command` in the current language, when it has any.
pub fn help(command: &str) -> Option<String> {
    if !DOCUMENTED.contains(&command) {
        return None;
    }
    let key = format!("help.{command}");
    let text = i18n::tr(&key);
    (text != key).then(|| text.to_owned())
}

/// Show `tip` (name, shortcut or the reason the tool is disabled) as the tooltip of `response`,
/// with the help of `command` below it when there is one.
pub fn tooltip(response: egui::Response, command: &str, tip: String) -> egui::Response {
    let help = help(command);
    response.on_hover_ui(move |ui| {
        ui.set_max_width(300.0);
        ui.label(tip);
        if let Some(help) = help {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(help).weak());
        }
    })
}

/// The tooltip text of a panel drop-down row: the reason when the command is disabled (as before),
/// else its name and shortcut when it has help to show below, else nothing (no tooltip).
pub fn row_tip(command: &str, label: &str, shortcut: Option<&str>, disabled_reason: Option<&str>) -> Option<String> {
    match disabled_reason {
        Some(why) => Some(why.to_owned()),
        None => help(command).map(|_| format!("{label}{}", shortcut.map(|k| format!("  ({k})")).unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn catalog(source: &str) -> std::collections::BTreeMap<String, String> {
        serde_json::from_str(source).unwrap()
    }

    /// The `help.<command id>` messages; other `help.*` messages (About dialog texts) are not tool help.
    fn help_keys(source: &str) -> BTreeSet<String> {
        catalog(source)
            .into_keys()
            .filter(|k| k.starts_with("help.") && (k.starts_with("help.solid.") || DOCUMENTED.contains(&&k["help.".len()..])))
            .collect()
    }

    #[test]
    fn every_documented_id_is_a_real_command() {
        for id in DOCUMENTED {
            assert!(solidcraft_engine::find_command(id).is_some(), "{id} is not a command");
        }
        assert!(DOCUMENTED.is_sorted());
    }

    #[test]
    fn every_help_message_is_for_a_documented_command() {
        let english = help_keys(include_str!("../../../assets/locales/en.json"));
        let wanted: BTreeSet<String> = DOCUMENTED.iter().map(|id| format!("help.{id}")).collect();
        assert_eq!(english, wanted);
        for key in &english {
            let id = key.trim_start_matches("help.");
            assert!(solidcraft_engine::find_command(id).is_some(), "{key} names no command");
        }
    }

    #[test]
    fn the_help_texts_are_written_in_every_complete_language() {
        let english = catalog(include_str!("../../../assets/locales/en.json"));
        for source in [include_str!("../../../assets/locales/ja.json"), include_str!("../../../assets/locales/zh.json")] {
            let other = catalog(source);
            for (key, text) in english.iter().filter(|(k, _)| k.starts_with("help.")) {
                let translated = other.get(key).unwrap_or_else(|| panic!("{key} missing"));
                assert!(!translated.trim().is_empty());
                // The messages carry no placeholders; if one is added, both sides must agree.
                assert_eq!(text.matches('{').count(), translated.matches('{').count(), "{key}");
            }
        }
    }

    #[test]
    fn a_documented_command_has_help_and_others_do_not() {
        for id in DOCUMENTED {
            assert!(help(id).is_some_and(|text| !text.is_empty()), "{id}");
        }
        assert!(help("solid.draft").is_none());
    }

    #[test]
    fn a_drop_down_row_has_a_tip_only_for_a_reason_or_help() {
        assert_eq!(row_tip("solid.shell", "Shell", Some("S"), None).as_deref(), Some("Shell  (S)"));
        assert_eq!(row_tip("solid.hole", "Hole", None, None).as_deref(), Some("Hole"));
        assert_eq!(row_tip("solid.hole", "Hole", None, Some("Select a face")).as_deref(), Some("Select a face"));
        assert_eq!(row_tip("solid.draft", "Draft", None, None), None);
        assert_eq!(row_tip("solid.draft", "Draft", None, Some("No body")).as_deref(), Some("No body"));
    }
}
