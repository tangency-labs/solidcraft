//! The file name an export offers in the save dialog, from the Export file name preference: a
//! template where `{design}` stands for the design's name, `{body}` for the exported body's, and
//! `{project}`, `{part_number}`, `{revision}` and the other Design Properties for their values.
//! An empty template keeps the name each export offered before the preference existed.

use solidcraft_engine::doc::Metadata;
use solidcraft_engine::doc::metadata::{Owner, keys};

/// Placeholder name for the design's name.
const DESIGN: &str = "design";
/// Placeholder name for the exported body's name.
const BODY: &str = "body";

/// Longest name offered, in characters.
const MAX_CHARS: usize = 200;
/// Longest name offered, in UTF-8 bytes: common file systems (ext4, APFS, NTFS's UTF-16 aside)
/// allow 255 bytes for a file name, and this leaves room for the extension an export adds.
const MAX_BYTES: usize = 240;

/// Windows device names: a file named after one, with any extension (`CON.step`), can't be made.
const WINDOWS_DEVICES: &[&str] = &[
    "AUX", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "CON", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
    "LPT8", "LPT9", "NUL", "PRN",
];

/// What the placeholders of a template stand for in one export.
pub(crate) struct Names<'a> {
    pub design: &'a str,
    /// The exported body (the first one when an export holds several; `None` when the design has
    /// no body, and `{body}` then reads as the design's name).
    pub body: Option<&'a str>,
    /// The design's properties (File › Design Properties); one that is not set reads as empty.
    pub metadata: &'a Metadata,
}

impl Names<'_> {
    /// The value of placeholder `name`, or `None` when it is no placeholder.
    fn value(&self, name: &str) -> Option<&str> {
        match name {
            DESIGN => Some(self.design),
            BODY => Some(self.body.unwrap_or(self.design)),
            _ => keys(Owner::Design).find(|k| k.name == name).map(|k| self.metadata.text(k.name).unwrap_or_default()),
        }
    }
}

/// The file name, without extension, offered for an export. `legacy` is the name offered with an
/// empty template, or when the template leaves nothing usable as a file name.
pub(crate) fn stem(template: &str, names: &Names, legacy: &str) -> String {
    if template.trim().is_empty() {
        return legacy.to_string();
    }
    let named = fill(template, names);
    let mut safe = String::new();
    for c in named.chars().map(|c| if forbidden(c) { '_' } else { c }).take(MAX_CHARS) {
        if safe.len() + c.len_utf8() > MAX_BYTES {
            break;
        }
        safe.push(c);
    }
    // Windows drops trailing dots and spaces; leading ones hide the file or read as a mistake.
    let safe = safe.trim_matches(|c: char| c.is_whitespace() || c == '.');
    if safe.chars().all(|c| c == '_') {
        legacy.to_string()
    } else if is_windows_device(safe) {
        format!("_{safe}")
    } else {
        safe.to_string()
    }
}

/// Whether Windows reserves `name` for a device: its part before the first dot, without trailing
/// spaces, is one of [`WINDOWS_DEVICES`] in any case (`con`, `NUL.tar`, `Com1 .x`).
fn is_windows_device(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or_default().trim_end();
    WINDOWS_DEVICES.iter().any(|device| device.eq_ignore_ascii_case(base))
}

/// The file name, without extension, that an export of `body` from the open design offers, from
/// the Export file name preference. Every place that asks where to export a design or its bodies
/// takes its name from here; `legacy` is the name it offered before the preference existed.
pub(crate) fn offered(app: &crate::SolveApp, body: Option<&str>, legacy: &str) -> String {
    let names = Names { design: &app.session.doc.name, body, metadata: &app.session.doc.metadata };
    stem(&app.preferences.export_name, &names, legacy)
}

/// `template` with each `{placeholder}` replaced, in one pass: a value that holds braces itself
/// (a body named `{revision}`) is kept as it is. Other text in braces is kept as typed.
fn fill(template: &str, names: &Names) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('{') {
        out.push_str(before);
        let Some((name, tail)) = after.split_once('}') else {
            out.push('{');
            rest = after;
            continue;
        };
        match names.value(name) {
            Some(value) => out.push_str(value),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Every placeholder with what it stands for, one per line, for the setting's help.
pub(crate) fn placeholder_list() -> String {
    let own = [(DESIGN, "prefs.export_name.design"), (BODY, "prefs.export_name.body")];
    own.into_iter()
        .chain(keys(Owner::Design).map(|k| (k.name, k.label)))
        .map(|(name, label)| format!("{{{name}}}  {}", solidcraft_ui_text::tr(label)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body `{body}` names in an export of the whole design: its first body, in model order.
pub(crate) fn first_body(app: &crate::SolveApp) -> Option<String> {
    crate::open_meshing::session_world(&app.session).bodies.first().map(|b| b.name.clone())
}

/// A character no common file system takes in a file name.
fn forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn metadata(values: serde_json::Value) -> Metadata {
        serde_json::from_value(values).unwrap()
    }

    fn name(template: &str, design: &str, body: Option<&str>, legacy: &str) -> String {
        stem(template, &Names { design, body, metadata: &Metadata::default() }, legacy)
    }

    #[test]
    fn an_empty_template_offers_the_old_name() {
        assert_eq!(name("", "Bracket", Some("Base"), "Base"), "Base");
        assert_eq!(name("   ", "Bracket", None, "Bracket"), "Bracket");
    }

    #[test]
    fn placeholders_take_the_design_and_body_names() {
        assert_eq!(name("{design}-{body}", "Bracket", Some("Base"), "x"), "Bracket-Base");
        assert_eq!(name("{body} ({design}) {body}", "Bracket", Some("Base"), "x"), "Base (Bracket) Base");
        assert_eq!(name("print_{design}", "Bracket", Some("Base"), "x"), "print_Bracket");
        assert_eq!(name("{design}-{unknown}", "Bracket", Some("Base"), "x"), "Bracket-{unknown}", "other text is kept as typed");
        assert_eq!(name("{design}-{body", "Bracket", Some("Base"), "x"), "Bracket-{body", "an unclosed brace too");
    }

    #[test]
    fn design_properties_are_placeholders_and_an_unset_one_is_empty() {
        let m = metadata(json!({"project": "Rover", "part_number": "RV-102", "revision": "C", "author": "Sam",
            "organization": "Acme", "description": "Motor mount", "license": "CC-BY-4.0"}));
        let names = Names { design: "Bracket", body: Some("Base"), metadata: &m };
        let all = "{project} {part_number} {revision} {author} {organization} {description} {license}";
        assert_eq!(stem(all, &names, "x"), "Rover RV-102 C Sam Acme Motor mount CC-BY-4.0");
        assert_eq!(stem("{part_number}_rev{revision}", &names, "x"), "RV-102_revC");

        let names = Names { design: "Bracket", body: None, metadata: &metadata(json!({"revision": 3})) };
        assert_eq!(stem("{design}_rev{revision}", &names, "x"), "Bracket_rev", "unset, or not text, reads as empty");
        assert_eq!(stem("{part_number}", &names, "Old"), "Old", "nothing is left");
    }

    #[test]
    fn a_value_with_braces_is_not_filled_in_again() {
        let m = metadata(json!({"revision": "C"}));
        let names = Names { design: "{body}", body: Some("{revision}"), metadata: &m };
        assert_eq!(stem("{design}-{body}-{revision}", &names, "x"), "{body}-{revision}-C");
    }

    #[test]
    fn a_design_with_no_body_names_the_body_after_the_design() {
        assert_eq!(name("{body}", "Bracket", None, "x"), "Bracket");
    }

    #[test]
    fn the_name_is_safe_as_a_file_name() {
        assert_eq!(name("{design}/{body}", "A:B", Some("C?"), "x"), "A_B_C_");
        assert_eq!(name(" .{design}. ", "Bracket", None, "x"), "Bracket");
        assert_eq!(name("{body}", "Bracket", Some("a\nb\t"), "x"), "a_b_");
        assert_eq!(name("{body}", "Bracket", Some(".."), "Old"), "Old", "nothing usable is left");
        assert_eq!(name("///", "Bracket", None, "Old"), "Old");
        assert_eq!(name(&"{design}".repeat(100), "Bracketé", None, "x").chars().count(), MAX_CHARS);
    }

    #[test]
    fn a_long_name_of_wide_characters_fits_the_file_name_byte_limit() {
        // 70 four-byte emoji are under the character cap, but 280 bytes.
        let long = name(&"🔧".repeat(70), "Bracket", None, "x");
        assert!(long.len() <= MAX_BYTES && long.len() + ".step".len() <= 255, "{} bytes", long.len());
        assert_eq!(long, "🔧".repeat(MAX_BYTES / 4), "cut between characters, never inside one");
        // Three-byte characters: cut at the last whole one within the limit.
        let cjk = name(&"設".repeat(100), "Bracket", None, "x");
        assert_eq!(cjk.chars().count(), MAX_BYTES / 3);
    }

    #[test]
    fn a_windows_device_name_is_prefixed() {
        for device in ["CON", "con", "Nul", "COM1", "lpt9", "AUX", "PRN"] {
            assert_eq!(name("{design}", device, None, "x"), format!("_{device}"));
        }
        assert_eq!(name("{design}.tar", "CON", None, "x"), "_CON.tar", "with any extension");
        assert_eq!(name("{design} .v2", "nul", None, "x"), "_nul .v2", "and trailing spaces");
        for fine in ["CONSOLE", "COM10", "Bracket-CON", "LPT", "COM0"] {
            assert_eq!(name("{design}", fine, None, "x"), fine, "only the exact device names");
        }
        assert_eq!(name("", "CON", None, "CON"), "CON", "an empty template keeps the old name as it was");
    }

    #[test]
    fn the_help_lists_every_placeholder_once() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("en");
        let list = placeholder_list();
        let names: Vec<&str> = list.lines().filter_map(|l| l.split_once("  ").map(|(n, _)| n)).collect();
        assert_eq!(
            names,
            ["{design}", "{body}", "{project}", "{part_number}", "{revision}", "{author}", "{organization}", "{description}", "{license}"]
        );
        assert!(list.contains("{part_number}  Part number"), "each with its label: {list}");
    }
}
