//! `cargo xtask book`: generate the book's command reference and screenshots, then build it.
//!
//! The command reference (`docs/book/src/reference/commands.md`) comes from
//! `solidcraft-cli commands`, so it can't drift from the registry. The screenshots in
//! `docs/book/src/generated/` are GPU renders by `solidcraft-cli snapshot` of `examples/*.json`
//! and `docs/book/scripts/*.json`, plus the app screenshot from `docs/screenshots/`. Both are
//! gitignored and attributed in ATTRIBUTION.md. The book itself is built by `mdbook` (a separate
//! tool, not a dependency) into `target/book`.

use std::path::Path;

use serde_json::Value;

use crate::cargo;

/// Toolbar tabs in toolbar order; commands not on the toolbar come last, grouped by id prefix.
const TABS: &[&str] = &["SOLID", "SKETCH", "SURFACE", "SHEET METAL", "PLASTIC"];
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
    let md = reference(cmds.as_array().map(Vec::as_slice).unwrap_or_default());
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
fn reference(cmds: &[Value]) -> String {
    let mut md = format!(
        "# Command reference\n\nBrowse commands by their names and default keyboard shortcuts. \
         Entries are organized by command group and panel; ungrouped commands follow by area.\n\n\
         For step-by-step instructions, start with the [window tour](../guide/ui-tour.md), \
         [sketching guide](../guide/sketching.md) or [solid features guide](../guide/features.md). \
         Each entry links to a related guide when one is available.\n\n\
         Expand **Automation details** for the command ID and parameters used by scripts, the CLI and MCP. \
         In that notation, `?` marks an optional parameter and `expr` means a number or an expression with units.\n\n\
         This catalog is generated from `solidcraft-cli commands` by `cargo xtask book` ({} commands).\n",
        cmds.len()
    );
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
            if let Some((label, path)) = guide(c) {
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
}
