//! The file name an export offers in the save dialog, from the Export file name preference: a
//! template where `{design}` stands for the design's name, `{body}` for the exported body's,
//! `{component}` for the explicitly exported component's name (else the usual export name),
//! `{source}` for the name of the file it was imported from, and `{project}`, `{part_number}`, `{revision}` and the other Design Properties for their values.
//! An empty template uses the export's default name; `{body}` chooses the body explicitly.
//! The Send to Slicer file name preference names the file a slicer is sent the same way ([`sent`]).

use solidcraft_engine::doc::metadata::{Owner, keys};
use solidcraft_engine::doc::{FeatureKind, Metadata};

/// Placeholder name for the design's name.
const DESIGN: &str = "design";
/// Placeholder name for the exported body's name.
const BODY: &str = "body";
/// Placeholder name for the explicitly exported component.
const COMPONENT: &str = "component";
/// Placeholder name for the name, without extension, of the file the export was imported from.
const SOURCE: &str = "source";

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
    /// The explicitly exported component, or the usual export name.
    pub component: Option<&'a str>,
    /// The name, without extension, of the file the exported body was imported from (else the
    /// design's first import); `None` when nothing was imported, and `{source}` then reads as the
    /// design's name.
    pub source: Option<&'a str>,
    /// The design's properties (File › Design Properties); one that is not set reads as empty.
    pub metadata: &'a Metadata,
}

impl Names<'_> {
    /// The value of placeholder `name`, or `None` when it is no placeholder.
    fn value(&self, name: &str) -> Option<&str> {
        match name {
            DESIGN => Some(self.design),
            BODY => Some(self.body.unwrap_or(self.design)),
            COMPONENT => Some(self.component.unwrap_or(self.design)),
            SOURCE => Some(self.source.unwrap_or(self.design)),
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
        // The `_` in front counts too: the name stays within the byte limit, cut between characters.
        let mut cut = safe.len().min(MAX_BYTES - 1);
        while !safe.is_char_boundary(cut) {
            cut -= 1;
        }
        let kept = safe.get(..cut).unwrap_or_default().trim_end_matches(|c: char| c.is_whitespace() || c == '.');
        format!("_{kept}")
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

/// The default for an export of `bodies`: use the design name when the selection holds
/// the entire design (including a single-body part). A subset of a multi-body design keeps
/// the first exported body's name. The `{body}` template restores that earlier choice.
pub(crate) fn default_for_bodies(app: &crate::SolveApp, bodies: &[String]) -> String {
    let all = crate::open_meshing::session_world(&app.session);
    if !bodies.is_empty() && all.bodies.len() == bodies.len() && all.bodies.iter().all(|body| bodies.contains(&body.name)) {
        return safe_design_name(app);
    }
    bodies.first().cloned().unwrap_or_else(|| safe_design_name(app))
}

/// Sanitize the design name before using it as a filename fallback. Explicit templates
/// already go through `stem` and retain their existing behaviour.
fn safe_design_name(app: &crate::SolveApp) -> String {
    let names = Names { design: &app.session.doc.name, body: None, component: None, source: None, metadata: &app.session.doc.metadata };
    stem("{design}", &names, "Untitled")
}

/// The file name, without extension, that an export of `body` from the open design offers, from
/// the Export file name preference. Every place that asks where to export a design or its bodies
/// takes its name from here; `legacy` is the export's suggested name with no template.
pub(crate) fn offered(app: &crate::SolveApp, body: Option<&str>, legacy: &str) -> String {
    offered_for_component(app, body, None, legacy)
}

/// The existing filename template may explicitly request a component's name.
pub(crate) fn offered_for_component(app: &crate::SolveApp, body: Option<&str>, component: Option<u64>, legacy: &str) -> String {
    let template = &app.preferences.export_name;
    let component = component.filter(|id| *id != 0).and_then(|id| app.session.doc.components.iter().find(|c| c.id == id));
    let component = component.map_or(legacy, |c| c.name.as_str());
    // Only a template that asks for it evaluates the model to find the body's import.
    let source = if template.contains("{source}") { imported_file(&app.session, body) } else { None };
    let names =
        Names { design: &app.session.doc.name, body, component: Some(component), source: source.as_deref(), metadata: &app.session.doc.metadata };
    stem(template, &names, legacy)
}

/// The name, without extension, of the file `body` was imported from: the file of the import that
/// built it, else of the design's first import (a body made by a later feature, or none named).
/// `None` when the design imports nothing.
pub(crate) fn imported_file(session: &solidcraft_engine::Session, body: Option<&str>) -> Option<String> {
    let doc = &session.doc;
    let file = |id: u64| match doc.feature(id).map(|f| &f.kind) {
        Some(FeatureKind::Import { file, .. } | FeatureKind::MeshImport { file, .. }) => Some(file.as_str()),
        _ => None,
    };
    let built = body.and_then(|name| {
        let world = crate::open_meshing::session_world(session);
        world.bodies.iter().find(|b| b.name == name).and_then(|b| file(b.feature))
    });
    let file = built.or_else(|| doc.features.iter().find_map(|f| file(f.id)))?;
    Some(file_stem(file).to_string())
}

/// `file` without its folders and its last extension (`parts/Bracket.v2.step` → `Bracket.v2`),
/// for either separator whatever the platform; a name that is only an extension (`.step`) is kept.
fn file_stem(file: &str) -> &str {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The name, without extension, of the file Send to Slicer writes for a send whose first body is
/// `body`, from the Send to Slicer file name preference. `None` when the preference is empty or
/// leaves nothing usable as a file name: the file then keeps the name `file.send_to_slicer` gives
/// it (the design's name followed by a code).
pub(crate) fn sent(app: &crate::SolveApp, body: Option<&str>) -> Option<String> {
    let template = &app.preferences.slicer_file_name;
    // As for exports: only a template that asks for it evaluates the model to find the body's import.
    let source = if template.contains("{source}") { imported_file(&app.session, body) } else { None };
    let names = Names { design: &app.session.doc.name, body, component: None, source: source.as_deref(), metadata: &app.session.doc.metadata };
    let name = stem(template, &names, "");
    (!name.is_empty()).then_some(name)
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
    let own = [
        (DESIGN, "prefs.export_name.design"),
        (BODY, "prefs.export_name.body"),
        (COMPONENT, "prefs.export_name.component"),
        (SOURCE, "prefs.export_name.source"),
    ];
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
        stem(template, &Names { design, body, component: None, source: None, metadata: &Metadata::default() }, legacy)
    }

    #[test]
    fn a_design_name_fallback_is_safe_for_the_export_file_picker() {
        let mut h = crate::scenario::Harness::new();
        h.app.session.doc_mut().name = "Bracket/Rev:2?".into();
        assert_eq!(default_for_bodies(&h.app, &[]), "Bracket_Rev_2_");
        h.app.run("solid.box", json!({"length": 2, "width": 3, "height": 4, "body_name": "Base"})).unwrap();
        assert_eq!(default_for_bodies(&h.app, &["Base".into()]), "Bracket_Rev_2_");
    }

    #[test]
    fn source_is_the_name_of_the_file_the_body_was_imported_from() {
        let mut h = crate::scenario::Harness::new();
        h.app.session.doc_mut().name = "Bracket".into();
        h.app.preferences.export_name = "{source}-{body}".into();
        assert_eq!(offered(&h.app, None, "x"), "Bracket-Bracket", "nothing imported: the design's name");

        let dir = std::env::temp_dir().join(format!("solidcraft-export-source-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Clamp plate.v2.stl");
        let facet = |a: &str, b: &str, c: &str| format!("facet normal 0 0 0\nouter loop\nvertex {a}\nvertex {b}\nvertex {c}\nendloop\nendfacet\n");
        let (o, x, y, z) = ("0 0 0", "10 0 0", "0 10 0", "0 0 10");
        let stl = format!("solid t\n{}{}{}{}endsolid t\n", facet(o, y, x), facet(o, x, z), facet(o, z, y), facet(x, y, z));
        std::fs::write(&path, stl).unwrap();
        let import = h.app.run("file.insert_mesh", json!({"path": path.to_string_lossy()})).unwrap()["feature"].as_u64().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        h.app.run("solid.box", json!({"length": 2, "width": 3, "height": 4, "body_name": "Base"})).unwrap();

        let world = crate::open_meshing::session_world(&h.app.session);
        let mesh = world.bodies.iter().find(|b| b.feature == import).map(|b| b.name.clone()).unwrap();
        assert_eq!(offered(&h.app, Some(&mesh), "x"), format!("Clamp plate.v2-{mesh}"), "the file's name without its folder or extension");
        assert_eq!(offered(&h.app, Some("Base"), "x"), "Clamp plate.v2-Base", "a body made later: the design's import");
        h.app.preferences.export_name = String::new();
        assert_eq!(offered(&h.app, Some("Base"), "Base"), "Base", "the default is unchanged");

        assert_eq!(file_stem(r"C:\parts\Plate.step"), "Plate");
        assert_eq!(file_stem("parts/Plate"), "Plate");
        assert_eq!(file_stem(".step"), ".step");
        assert_eq!(file_stem("Base independent base"), "Base independent base");
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
    fn component_placeholder_uses_the_component_and_keeps_other_names() {
        let names =
            Names { design: "Assembly", body: Some("PartBody"), component: Some("Bracket/Rev"), source: None, metadata: &Metadata::default() };
        assert_eq!(stem("", &names, "PartBody"), "PartBody");
        assert_eq!(stem("{component}-{body}-{design}", &names, "PartBody"), "Bracket_Rev-PartBody-Assembly");
        assert_eq!(stem("{body}", &names, "PartBody"), "PartBody");
    }

    #[test]
    fn design_properties_are_placeholders_and_an_unset_one_is_empty() {
        let m = metadata(json!({"project": "Rover", "part_number": "RV-102", "revision": "C", "author": "Sam",
            "organization": "Acme", "description": "Motor mount", "license": "CC-BY-4.0"}));
        let names = Names { design: "Bracket", body: Some("Base"), component: None, source: None, metadata: &m };
        let all = "{project} {part_number} {revision} {author} {organization} {description} {license}";
        assert_eq!(stem(all, &names, "x"), "Rover RV-102 C Sam Acme Motor mount CC-BY-4.0");
        assert_eq!(stem("{part_number}_rev{revision}", &names, "x"), "RV-102_revC");

        // The material and finish of a design that is one part (#20114) are placeholders too; a
        // template that named them before kept the braces as typed, and now fills them in.
        let m = metadata(json!({"material": "5052-H32, 0.090 in", "finish": "Powder coat, RAL 9005, matt"}));
        let names = Names { design: "Bracket", body: None, component: None, source: None, metadata: &m };
        assert_eq!(stem("{design} {material} {finish}", &names, "x"), "Bracket 5052-H32, 0.090 in Powder coat, RAL 9005, matt");
        let bare = Names { design: "Bracket", body: None, component: None, source: None, metadata: &metadata(json!({})) };
        assert_eq!(stem("{design}-{material}", &bare, "x"), "Bracket-", "unset: empty, as every property");

        let names = Names { design: "Bracket", body: None, component: None, source: None, metadata: &metadata(json!({"revision": 3})) };
        assert_eq!(stem("{design}_rev{revision}", &names, "x"), "Bracket_rev", "unset, or not text, reads as empty");
        assert_eq!(stem("{part_number}", &names, "Old"), "Old", "nothing is left");
    }

    #[test]
    fn a_value_with_braces_is_not_filled_in_again() {
        let m = metadata(json!({"revision": "C"}));
        let names = Names { design: "{body}", body: Some("{revision}"), component: None, source: None, metadata: &m };
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
        // The `_` in front does not take a long name over the byte limit: 240 bytes before it.
        let long = name(&format!("CON.{}", "🔧".repeat(59)), "Bracket", None, "x");
        assert_eq!(long, format!("_CON.{}", "🔧".repeat(58)), "{} bytes", long.len());
        assert!(long.len() <= MAX_BYTES);
    }

    #[test]
    fn the_help_lists_every_placeholder_once() {
        crate::i18n::install();
        let _language = solidcraft_ui_text::scope("en");
        let list = placeholder_list();
        let names: Vec<&str> = list.lines().filter_map(|l| l.split_once("  ").map(|(n, _)| n)).collect();
        assert_eq!(
            names,
            [
                "{design}",
                "{body}",
                "{component}",
                "{source}",
                "{project}",
                "{part_number}",
                "{revision}",
                "{author}",
                "{organization}",
                "{description}",
                "{material}",
                "{finish}",
                "{license}"
            ]
        );
        assert!(list.contains("{part_number}  Part number"), "each with its label: {list}");
    }
}
