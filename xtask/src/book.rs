//! `cargo xtask book`: generate the book's command reference and screenshots, then build it.
//!
//! The command reference (`docs/book/src/reference/commands.md`) comes from
//! `solidcraft-cli commands`, so it can't drift from the registry. The screenshots in
//! `docs/book/src/generated/` are GPU renders by `solidcraft-cli snapshot` of `examples/*.json`
//! and `docs/book/scripts/*.json`, plus the app screenshot from `docs/screenshots/`. Both are
//! gitignored and attributed in ATTRIBUTION.md. The book itself is built by `mdbook` (a separate
//! tool, not a dependency) into `target/book`.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::cargo;

/// Toolbar tabs in toolbar order; commands not on the toolbar come last, grouped by id prefix.
const TABS: &[&str] = &["SOLID", "SKETCH", "SURFACE", "SHEET METAL", "WELDMENTS", "PLASTIC"];
const SNAPSHOT_SIZE: (&str, &str) = ("960", "600");

pub fn run(root: &Path, build: bool) -> Result<(), String> {
    let book = root.join("docs/book");
    std::fs::copy(root.join("assets/branding/web/source/og.png"), book.join("src/og.png")).map_err(|e| format!("book preview card: {e}"))?;
    let mut c = cargo();
    c.args(["build", "-q", "--release", "-p", "solidcraft-cli"]);
    crate::run(c, "cargo build -p solidcraft-cli")?;

    let out = cli().arg("commands").output().map_err(|e| format!("solidcraft-cli: {e}"))?;
    if !out.status.success() {
        return Err(format!("solidcraft-cli commands failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    let cmds: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("commands JSON: {e}"))?;
    let cmds = cmds.as_array().ok_or("commands JSON must be an array")?;
    let guides = load_guides(&book.join("command-guides"), cmds)?;
    let md = reference(cmds, &guides);
    write(&book.join("src/reference/commands.md"), &md)?;

    let gen_dir = book.join("src/generated");
    std::fs::create_dir_all(&gen_dir).map_err(|e| format!("{}: {e}", gen_dir.display()))?;
    let mut scripts = vec![root.join("examples/bracket.json")];
    scripts.extend(json_files(&book.join("scripts"))?);
    for s in &scripts {
        let stem = s.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let png = gen_dir.join(format!("{stem}.png"));
        let mut c = cli();
        c.arg("snapshot").arg(s).arg("--out").arg(&png).args(["--width", SNAPSHOT_SIZE.0, "--height", SNAPSHOT_SIZE.1]);
        let o = c.output().map_err(|e| format!("solidcraft-cli snapshot: {e}"))?;
        if !o.status.success() {
            return Err(format!("snapshot of {} failed:\n{}", s.display(), String::from_utf8_lossy(&o.stderr)));
        }
        println!("book: {}", png.strip_prefix(root).unwrap_or(&png).display());
    }
    let shot = "sample-plate.png";
    std::fs::copy(root.join("docs/screenshots").join(shot), gen_dir.join(shot)).map_err(|e| format!("{shot}: {e}"))?;

    if build {
        let mut c = std::process::Command::new("mdbook");
        c.arg("build").arg(&book);
        crate::run(c, "mdbook build docs/book").map_err(|e| format!("{e}\n(install mdbook with `cargo install mdbook`)"))?;
        println!("book: built into target/book");
    }
    Ok(())
}

/// The release solidcraft-cli just built.
fn cli() -> std::process::Command {
    let mut c = cargo();
    c.args(["run", "-q", "--release", "-p", "solidcraft-cli", "--"]);
    c
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn json_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    v.sort();
    Ok(v)
}

/// The command reference page: one section per toolbar tab and panel, in registry order.
fn reference(cmds: &[Value], guides: &BTreeMap<String, String>) -> String {
    let mut md = format!(
        "# Command reference\n\nBrowse commands by their names and default keyboard shortcuts. \
         Entries are organized by command group and panel; ungrouped commands follow by area.\n\n\
         For step-by-step instructions, start with the [window tour](../guide/ui-tour.md), \
         [sketching guide](../guide/sketching.md) or [solid features guide](../guide/features.md). \
         Practical command guides below explain prerequisites, options and examples in place. \
         Entries without a practical guide link to an overview instead. \
         [Contribute a guide, tip, demo or model link](../dev/command-guides.md).\n\n\
         Expand **Automation details** for the command ID and parameters used by scripts, the CLI and MCP. \
         In that notation, `?` marks an optional parameter and `expr` means a number or an expression with units.\n\n\
         This catalog is generated from `solidcraft-cli commands` by `cargo xtask book` ({} commands).\n",
        cmds.len()
    );
    let practical: Vec<_> = cmds.iter().filter(|c| guides.contains_key(command_text(c, "id"))).collect();
    if !practical.is_empty() {
        md.push_str("\n**Practical guides:**\n\n");
        for command in practical {
            md.push_str(&format!("- {}\n", command_link(command)));
        }
        md.push('\n');
    }
    let mut sections: Vec<(String, Vec<&Value>)> = Vec::new();
    for tab in TABS {
        for c in cmds.iter().filter(|c| command_text(c, "tab") == *tab) {
            let panel = command_text(c, "panel").trim();
            let title = if panel.is_empty() { tab.to_string() } else { format!("{tab} › {panel}") };
            add(&mut sections, title, c);
        }
    }
    for c in cmds.iter().filter(|c| !TABS.contains(&command_text(c, "tab"))) {
        let prefix = command_text(c, "id").split('.').next().filter(|s| !s.is_empty());
        let title = prefix.map_or_else(|| "Other commands".into(), |area| format!("Other commands: {area}"));
        add(&mut sections, title, c);
    }
    for (title, list) in sections {
        md.push_str(&format!("\n## {}\n\n", prose(&title)));
        for c in list {
            md.push_str(&format!("<a id=\"{}\"></a>\n\n", command_anchor(command_text(c, "id"))));
            let label = command_text(c, "label").trim();
            md.push_str(&format!("### {}\n\n", prose(if label.is_empty() { "Unnamed command" } else { label })));
            let tab = command_text(c, "tab").trim();
            let panel = command_text(c, "panel").trim();
            if !tab.is_empty() {
                let location = if panel.is_empty() { tab.to_string() } else { format!("{tab} › {panel}") };
                md.push_str(&format!("**Command group:** {}\n\n", prose(&location)));
            }
            let shortcut = command_text(c, "shortcut").trim();
            if !shortcut.is_empty() {
                md.push_str(&format!("**Default keyboard shortcut:** {}\n\n", prose(shortcut)));
            }
            if let Some(text) = guides.get(command_text(c, "id")) {
                md.push_str(text);
                md.push_str("\n\n");
            } else if let Some((label, path)) = guide(c) {
                md.push_str(&format!("Read the [{label}]({path}).\n\n"));
            }
            md.push_str("<details>\n<summary>Automation details</summary>\n\n");
            md.push_str(&format!("<p><strong>Command ID:</strong> <code>{}</code></p>\n\n", html(command_text(c, "id"))));
            let p = command_text(c, "params");
            if !p.is_empty() {
                // HTML code blocks preserve the complete source text, including newlines and
                // arbitrary runs of backticks, without letting it close the details element.
                md.push_str(&format!(
                    "<p><strong>Parameters:</strong></p>\n<pre><code style=\"white-space: pre-wrap; overflow-wrap: anywhere;\">{}</code></pre>\n",
                    html(p)
                ));
            } else {
                md.push_str("<p>No parameter description is provided by the command registry.</p>\n");
            }
            md.push_str("\n</details>\n\n");
        }
    }
    md
}

/// Hand-authored Markdown is reviewed documentation, separate from registry metadata.
/// Unknown filenames fail the build rather than silently dropping a renamed command's guide.
fn load_guides(dir: &Path, cmds: &[Value]) -> Result<BTreeMap<String, String>, String> {
    let mut guides = BTreeMap::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("command guides {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("command guides: {e}"))?.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let id = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| format!("invalid guide filename: {}", path.display()))?;
        if !cmds.iter().any(|c| command_text(c, "id") == id) {
            return Err(format!("command guide {}: unknown command `{id}`", path.display()));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("command guide {}: {e}", path.display()))?;
        if text.trim().is_empty() {
            return Err(format!("command guide {} is empty", path.display()));
        }
        guides.insert(id.to_string(), command_links(&text, cmds).map_err(|e| format!("command guide {}: {e}", path.display()))?);
    }
    Ok(guides)
}

/// A small explicit book directive; ordinary Markdown links, tables and images stay untouched.
fn command_links(text: &str, cmds: &[Value]) -> Result<String, String> {
    let mut output = String::new();
    let mut rest = text;
    while let Some((before, link)) = rest.split_once("{{#command") {
        output.push_str(before);
        // Other directive names (for example {{#commands ...}}) are not ours.
        if link.starts_with(|c: char| !c.is_whitespace() && c != '}') {
            output.push_str("{{#command");
            rest = link;
            continue;
        }
        let (id, after) = link.split_once("}}").ok_or("unterminated {{#command ...}} link")?;
        let id = id.trim();
        if id.is_empty() {
            return Err("missing command ID in {{#command ...}} link".into());
        }
        let command = cmds.iter().find(|c| command_text(c, "id") == id).ok_or_else(|| format!("unknown related command `{id}`"))?;
        output.push_str(&command_link(command));
        rest = after;
    }
    output.push_str(rest);
    Ok(output)
}

fn command_link(command: &Value) -> String {
    let id = command_text(command, "id");
    let label = command_text(command, "label").trim();
    format!("[{}](#{})", prose(if label.is_empty() { id } else { label }), command_anchor(id))
}

/// Keep dotted command IDs readable, with an unambiguous escape for any other input byte.
fn command_anchor(id: &str) -> String {
    let mut anchor = String::from("command-");
    for byte in id.bytes() {
        if byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_') {
            anchor.push(char::from(byte));
        } else {
            anchor.push_str(&format!("-{byte:02x}"));
        }
    }
    anchor
}

fn command_text<'a>(command: &'a Value, key: &str) -> &'a str {
    command.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Text in generated Markdown is literal text, never contributor-provided Markdown or HTML.
fn prose(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\r' | '\n' => escaped.push(' '),
            '\\' | '`' | '*' | '_' | '[' | ']' | '{' | '}' | '(' | ')' | '#' | '+' | '-' | '!' | '|' | '~' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    html(&escaped)
}

/// Escape once, including ampersands that already look like HTML entities.
fn html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Only link to guide pages that exist; parameter descriptions are deliberately not parsed.
fn guide(command: &Value) -> Option<(&'static str, &'static str)> {
    let area = command_text(command, "id").split('.').next().unwrap_or_default();
    match area {
        "parameters" | "expr" | "config" => Some(("parameters and expressions guide", "../guide/parameters.md")),
        "component" | "occurrence" | "joint" | "motion" => Some(("components and joints guide", "../guide/assemblies.md")),
        "file" | "doc" => Some(("import and export guide", "../guide/files.md")),
        "view" | "browser" | "select" => Some(("window tour", "../guide/ui-tour.md")),
        _ => match command_text(command, "tab") {
            "SKETCH" => Some(("sketching and constraints guide", "../guide/sketching.md")),
            "SOLID" | "SURFACE" => Some(("solid features guide", "../guide/features.md")),
            "SHEET METAL" => Some(("sheet metal guide", "../guide/sheet-metal.md")),
            "PLASTIC" => Some(("plastic parts guide", "../guide/plastic.md")),
            _ => None,
        },
    }
}

fn add<'a>(sections: &mut Vec<(String, Vec<&'a Value>)>, title: String, c: &'a Value) {
    match sections.iter_mut().find(|(t, _)| *t == title) {
        Some((_, v)) => v.push(c),
        None => sections.push((title, vec![c])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reference(cmds: &[Value]) -> String {
        super::reference(cmds, &BTreeMap::new())
    }

    fn decoded_code(text: &str) -> String {
        text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
    }

    #[test]
    fn every_command_has_one_entry_with_its_complete_automation_details() {
        let cmds = [
            json!({"id":"file.export","label":"Export","params":"path: text","tab":"","panel":""}),
            json!({"id":"solid.extrude","label":"Extrude","params":"distance: expr","tab":"SOLID","panel":"CREATE","shortcut":"E"}),
            json!({"id":"sketch.line","label":"Line","params":"points: [x,y]","tab":"SKETCH","panel":"CREATE"}),
            json!({"id":"future.action","label":"Future action","params":"option?: text","tab":"NEW WORKSPACE","panel":"TOOLS"}),
        ];
        let md = reference(&cmds);
        let ids: Vec<&str> = md.split("<strong>Command ID:</strong> <code>").skip(1).map(|tail| tail.split("</code>").next().unwrap()).collect();
        assert_eq!(ids, ["solid.extrude", "sketch.line", "file.export", "future.action"]);
        assert_eq!(md.matches("<details>").count(), cmds.len());
        assert_eq!(md.matches("### ").count(), cmds.len());
        assert!(md.contains("### Extrude\n\n**Command group:** SOLID › CREATE\n\n**Default keyboard shortcut:** E"));
        assert!(md.contains("## Other commands: future"));
        assert!(md.contains("**Command group:** NEW WORKSPACE › TOOLS"));
        for cmd in &cmds {
            assert!(md.contains(&format!(
                "<pre><code style=\"white-space: pre-wrap; overflow-wrap: anywhere;\">{}</code></pre>",
                command_text(cmd, "params")
            )));
        }
    }

    #[test]
    fn source_markup_cannot_add_links_headings_or_close_automation_details() {
        let params = "path: id|name\n```html\n</code></pre></details><script>alert('x')</script>\n``````\n&amper; &lt;  \n";
        let md = reference(&[json!({
            "id":"odd.<tag>&id",
            "label":"[Follow](javascript:alert(1)) <img src=x>\n### Injected",
            "tab":"UNKNOWN <script>",
            "panel":"[Panel](https://invalid.example)",
            "shortcut":"Ctrl+<b>&`",
            "params":params
        })]);
        assert_eq!(md.matches("<details>").count(), 1);
        assert_eq!(md.matches("</details>").count(), 1);
        assert_eq!(md.lines().filter(|line| line.starts_with("### ")).count(), 1);
        assert!(!md.contains("<script>"));
        assert!(!md.contains("<img"));
        assert!(!md.contains("[Follow]("));
        assert!(!md.contains("[Panel]("));
        let stored =
            md.split("<pre><code style=\"white-space: pre-wrap; overflow-wrap: anywhere;\">").nth(1).unwrap().split("</code></pre>").next().unwrap();
        assert_eq!(decoded_code(stored), params, "every parameter byte, including newlines and backticks, stays readable");
        assert!(md.contains("<code>odd.&lt;tag&gt;&amp;id</code>"));
    }

    #[test]
    fn missing_optional_metadata_does_not_invent_a_toolbar_shortcut_or_guide() {
        for cmd in [
            json!({"id":"future.unlisted","label":"Unlisted"}),
            json!({"id":"future.unlisted","label":"Unlisted","tab":false,"panel":42,"shortcut":false,"params":null}),
            json!({"id":"future.unlisted","label":"Unlisted","tab":" ","panel":" ","shortcut":" \n"}),
        ] {
            let md = reference(&[cmd]);
            assert!(md.contains("### Unlisted"));
            assert!(md.contains("<code>future.unlisted</code>"));
            assert!(!md.contains("**Command group:**"));
            assert!(!md.contains("**Default keyboard shortcut:**"));
            assert!(!md.contains("Read the ["));
            assert!(md.contains("No parameter description is provided"));
        }
    }

    #[test]
    fn related_guides_are_existing_pages_and_cover_commands_without_a_toolbar() {
        let samples = [
            json!({"id":"parameters.change"}),
            json!({"id":"joint.drive"}),
            json!({"id":"file.export"}),
            json!({"id":"view.home"}),
            json!({"id":"sketch.line","tab":"SKETCH"}),
            json!({"id":"solid.extrude","tab":"SOLID"}),
            json!({"id":"surface.patch","tab":"SURFACE"}),
            json!({"id":"sheet.flange","tab":"SHEET METAL"}),
            json!({"id":"plastic.boss","tab":"PLASTIC"}),
        ];
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/book/src/reference");
        for cmd in samples {
            let (label, path) = guide(&cmd).expect("a related user guide");
            assert!(base.join(path).is_file(), "missing guide {path}");
            assert!(reference(&[cmd]).contains(&format!("Read the [{label}]({path}).")));
        }
        assert!(guide(&json!({"id":"future.unlisted","tab":"NEW WORKSPACE"})).is_none());
    }

    #[test]
    fn practical_content_is_inline_and_automation_stays_complete() {
        let cmds = [
            json!({"id":"solid.extrude","label":"Extrude","tab":"SOLID","params":"distance: expr"}),
            json!({"id":"sheet.hem","label":"Hem","tab":"SHEET METAL","params":"edges; length"}),
        ];
        let text = "Choose a closed region.\n\n#### Options\n\n| Choice | Effect |\n|---|---|\n| Distance | Size of the block |\n\nSee {{#command sheet.hem}}.\n\n![Example](../generated/bracket.png)";
        let guides = BTreeMap::from([("solid.extrude".into(), command_links(text, &cmds).unwrap())]);
        let md = super::reference(&cmds, &guides);
        assert!(md.contains("<a id=\"command-solid.extrude\"></a>"));
        assert!(md.contains("| Distance | Size of the block |"));
        assert!(md.contains("See [Hem](#command-sheet.hem)."));
        assert!(md.contains("![Example](../generated/bracket.png)"));
        assert!(!md.contains("Read the [solid features guide]"), "an inline guide replaces the generic overview link");
        assert!(md.contains("Read the [sheet metal guide]"), "unwritten entries keep their overview");
        assert!(md.find("Choose a closed region.").unwrap() < md.find("<details>").unwrap());
        assert!(md.contains("<code>solid.extrude</code>"));
        assert!(md.contains("distance: expr</code></pre>"));
        assert_eq!(md.matches("<details>").count(), 2);
    }

    #[test]
    fn related_command_links_use_current_labels_and_refuse_stale_ids() {
        let cmds = [json!({"id":"sheet.hem","label":"Hem [updated]"})];
        assert_eq!(
            command_links("Use {{#command sheet.hem}} twice: {{#command sheet.hem}}.", &cmds).unwrap(),
            "Use [Hem \\[updated\\]](#command-sheet.hem) twice: [Hem \\[updated\\]](#command-sheet.hem)."
        );
        assert!(command_links("{{#command missing.tool}}", &cmds).unwrap_err().contains("unknown related command"));
        assert!(command_links("{{#command sheet.hem", &cmds).unwrap_err().contains("unterminated"));
        for unfinished in ["{{#command", "{{#command\t", "{{#command sheet.hem}"] {
            assert!(command_links(unfinished, &cmds).unwrap_err().contains("unterminated"));
        }
        for empty in ["{{#command}}", "{{#command }}", "{{#command\n}}"] {
            assert!(command_links(empty, &cmds).unwrap_err().contains("missing command ID"));
        }
        assert_eq!(command_links("{{#command\t sheet.hem\n}}", &cmds).unwrap(), "[Hem \\[updated\\]](#command-sheet.hem)");
        let other = "{{#commands sheet.hem}} then {{#command sheet.hem}}";
        assert_eq!(command_links(other, &cmds).unwrap(), "{{#commands sheet.hem}} then [Hem \\[updated\\]](#command-sheet.hem)");
        let ordinary = "[Demo](https://example.com/demo) and `distance: 20`";
        assert_eq!(command_links(ordinary, &cmds).unwrap(), ordinary);
    }

    #[test]
    fn practical_index_lists_only_available_guides_with_current_safe_labels() {
        let cmds = [
            json!({"id":"solid.extrude","label":"Extrude [updated]"}),
            json!({"id":"sheet.flange","label":"Flange"}),
            json!({"id":"sheet.hem","label":" "}),
        ];
        let guides = BTreeMap::from([("sheet.hem".into(), "Hem guide".into()), ("solid.extrude".into(), "Extrude guide".into())]);
        let md = super::reference(&cmds, &guides);
        let index = md.split("**Practical guides:**\n\n").nth(1).unwrap().split("\n## ").next().unwrap();
        assert_eq!(index, "- [Extrude \\[updated\\]](#command-solid.extrude)\n- [sheet.hem](#command-sheet.hem)\n\n");
        assert!(!reference(&cmds).contains("**Practical guides:**"));
    }

    #[test]
    fn stable_command_anchors_cannot_inject_markup_or_collide_with_escaped_ids() {
        assert_eq!(command_anchor("solid.extrude"), "command-solid.extrude");
        assert_eq!(command_anchor("odd.<tag>&id"), "command-odd.-3ctag-3e-26id");
        assert_ne!(command_anchor("odd.<tag>"), command_anchor("odd.-3ctag-3e"));
    }

    #[test]
    fn guide_files_load_without_a_hardcoded_list_and_refuse_orphaned_or_empty_entries() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-book-guides-{}-{stamp}", std::process::id()));
        // Atomic creation establishes ownership of this fresh directory before cleanup.
        std::fs::create_dir(&path).unwrap();
        let dir = Fixture(path);
        let cmds = [json!({"id":"solid.extrude","label":"Extrude"}), json!({"id":"sheet.hem","label":"Hem"})];
        let source = "A readable guide.\n\nSee {{#command sheet.hem}}.";
        std::fs::write(dir.0.join("solid.extrude.md"), source).unwrap();
        std::fs::write(dir.0.join("notes.txt"), "not a guide").unwrap();
        let guides = load_guides(&dir.0, &cmds).unwrap();
        assert_eq!(guides.len(), 1);
        assert_eq!(guides.get("solid.extrude").unwrap(), "A readable guide.\n\nSee [Hem](#command-sheet.hem).");
        assert!(load_guides(&dir.0, &[]).unwrap_err().contains("unknown command"));
        assert!(load_guides(&dir.0.join("missing-directory"), &cmds).is_err());
        std::fs::write(dir.0.join("solid.extrude.md"), " \n").unwrap();
        assert!(load_guides(&dir.0, &cmds).unwrap_err().contains("is empty"));
    }
}
