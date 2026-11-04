//! The one door the sketch commands go through to find and keep the sketch they work on (W38.0,
//! #5425). Today every sketch is a design feature; a drawing session (W38a/b) adds sheet and view
//! sketches behind these same calls, so the commands don't change again. A source scan
//! (`each_sketch_command_goes_through_the_store`) keeps every sketch read and write in the
//! commands here, apart from the design's own, each marked `// design-only:`.

use serde_json::Value;
use solidcraft_doc::{Document, FeatureKind};
use solidcraft_sketch::Sketch;

use crate::params::bad;
use crate::{EngineError, Result, Session};

/// The sketch a command acts on: its `sketch` param, or the sketch being edited (also when
/// `sketch` is null, as callers send an optional value they don't have). A number is an exact id;
/// text is an id written out or a name, so a sketch named "7" is reached only as text.
pub(super) fn target(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    if let Some(v) = p.get("sketch").filter(|v| !v.is_null()) {
        let (key, found) = match v {
            Value::Number(n) => {
                let id = n.as_u64().ok_or_else(|| bad(cmd, "`sketch` must be an id or a name"))?;
                (id.to_string(), s.doc.feature(id))
            }
            Value::String(x) => (x.clone(), s.doc.find_feature(x)),
            _ => return Err(bad(cmd, "`sketch` must be an id or a name")),
        };
        let f = found.ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
        if !matches!(f.kind, FeatureKind::Sketch { .. }) {
            return Err(bad(cmd, format!("`{key}` is not a sketch")));
        }
        return Ok(f.id);
    }
    s.active_sketch.ok_or_else(|| EngineError::Other("no sketch is being edited".into()))
}

/// The sketch being edited, if any; each command keeps its own refusal when there is none.
pub(super) fn active(s: &Session) -> Option<u64> {
    s.active_sketch
}

/// Sketch `id`, to read.
pub(super) fn read(doc: &Document, id: u64) -> Result<&Sketch> {
    Ok(doc.sketch(id)?)
}

/// Read the selected sketch for a command that needs an owned preflight before editing.
/// Sheet identities are resolved against the open Drawing, never the model feature ids;
/// resolving paper expressions and solving here does not publish either document.
pub(super) fn read_for_command(s: &Session, p: &Value, cmd: &str) -> Result<Sketch> {
    if s.open_drawing.is_some() {
        let entry = sheet_target(s, p, cmd)?;
        return Ok(s.drawing(cmd)?.solve_sheet_sketch(&s.doc, entry.sketch)?.sketch);
    }
    Ok(read(&s.doc, target(s, p, cmd)?)?.clone())
}

/// Keep `sk` as sketch `id`.
pub(super) fn write(doc: &mut Document, id: u64, sk: Sketch) -> Result<()> {
    *doc.sketch_mut(id)? = sk;
    Ok(())
}

/// Change sketch `id` on a copy of the document, then keep both: the link and text commands'
/// edit. As those commands always have, it neither solves the sketch nor runs [`checked`].
pub(super) fn update<T>(s: &mut Session, id: u64, f: impl FnOnce(&Session, &Document, &mut Sketch) -> Result<T>) -> Result<T> {
    let mut doc = (*s.doc).clone();
    let mut sk = read(&doc, id)?.clone();
    let out = f(s, &doc, &mut sk)?;
    write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok(out)
}

/// The checks an edited sketch passes before it is kept.
pub(super) fn checked(sk: &Sketch) -> Result<()> {
    sk.validate_periodic_splines()?;
    Ok(())
}

/// The exact active sheet target, checked before a shared tool reads or edits it.
/// A number/string can confirm that target; it never selects a model or another sheet.
pub(super) fn sheet_target(s: &Session, p: &Value, cmd: &str) -> Result<std::sync::Arc<crate::SheetSketchEdit>> {
    let entry = s.active_sheet_sketch.as_ref().ok_or_else(|| bad(cmd, "no sheet sketch is being edited"))?;
    if s.open_drawing != Some(entry.drawing) {
        return Err(bad(cmd, "the edited sketch does not belong to the open drawing"));
    }
    let mut owners = s.doc.drawings.iter().filter(|drawing| drawing.id == entry.drawing);
    let owner = owners.next().ok_or_else(|| bad(cmd, "the edited drawing no longer exists"))?;
    if owners.next().is_some() {
        return Err(bad(cmd, "the edited drawing id is ambiguous"));
    }
    // Target selection checks identity, not every payload. Staging checks the
    // candidate before solving; edit_drawing validates the whole publication.
    if owner.sketches.iter().filter(|record| record.id == entry.sketch).count() != 1
        || owner.sheets.iter().filter(|sheet| sheet.id == entry.sheet).count() != 1
    {
        return Err(bad(cmd, "the edited sketch or sheet id is missing or ambiguous"));
    }
    let record = owner.sheet_sketch(entry.sketch).ok_or_else(|| bad(cmd, "the edited sheet sketch no longer exists"))?;
    if record.sheet != entry.sheet {
        return Err(bad(cmd, "the edited sketch's owning sheet changed"));
    }
    if let Some(given) = p.get("sketch").filter(|value| !value.is_null()) {
        let target = match given {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => solidcraft_doc::ids::decode(text),
            _ => None,
        }
        .ok_or_else(|| bad(cmd, "a sheet sketch target must be its exact id"))?;
        if target != entry.sketch {
            return Err(bad(cmd, "finish or cancel before selecting a different sheet sketch"));
        }
    }
    Ok(entry.clone())
}

/// Apply an existing geometry tool through R2. The legacy callback may read its
/// Document, but any attempted source mutation is refused before publication.
/// This is the sheet receiver of this store, not a second store or solver.
pub(super) fn edit_sheet<T>(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    change: impl FnOnce(&mut Sketch, &mut Document) -> Result<T>,
) -> Result<(T, Value)> {
    let entry = sheet_target(s, p, cmd)?;
    let design = s.doc.clone();
    let mut candidate = s.drawing(cmd)?.solve_sheet_sketch(&design, entry.sketch)?.sketch;
    let mut source = (*design).clone();
    let out = change(&mut candidate, &mut source)?;
    if source != *design {
        return Err(bad(cmd, "a sheet sketch cannot change the source design or its parameters"));
    }
    let solved = s.edit_drawing(cmd, |drawing| {
        let (_, solved) = drawing.edit_sheet_sketch(&design, entry.sketch, |sketch| {
            *sketch = candidate;
            Ok(())
        })?;
        if solved.sheet != entry.sheet {
            return Err(bad(cmd, "the edited sketch's owning sheet changed"));
        }
        Ok(solved)
    })?;
    Ok((out, super::sketch::report_json(&solved.report)))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    /// No `sketch`, or a null one, is the sketch being edited; anything else that isn't an id or
    /// a name is refused (#6336: sketch.inspect's callers send null for "none given").
    #[test]
    fn target_reads_a_null_sketch_as_none_given() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
        let active = s.active_sketch.unwrap();
        for p in [json!({}), json!({"sketch": null})] {
            assert_eq!(super::target(&s, &p, "test").unwrap(), active, "{p}");
        }
        assert!(super::target(&s, &json!({"sketch": true}), "test").is_err());
    }

    /// A number is an exact id, never a name (0xlillith on #6336): a sketch named "999999" isn't
    /// sketch 999999, so a command given that number refuses instead of picking it, and the name
    /// as text still finds it. Through the commands that read only ids before W38.0.
    #[test]
    fn target_reads_a_number_as_an_id_and_never_a_name() {
        let mut s = Session::default();
        s.execute("sketch.create", &json!({"plane": "XY", "name": "999999"})).unwrap();
        let named = s.active_sketch.unwrap();
        assert_ne!(named, 999999);
        let c = s.execute("sketch.circle.center", &json!({"center": [0, 0], "radius": 5})).unwrap()["curves"][0].clone();
        s.execute("sketch.finish", &json!({})).unwrap();
        // Glyphs and the curvature inspects need a sketch being edited; it's another one.
        s.execute("sketch.create", &json!({"plane": "XZ"})).unwrap();
        for (cmd, p) in [
            ("sketch.export_dxf", json!({})),
            ("sketch.glyphs", json!({})),
            ("inspect.curvature_comb", json!({"curves": [c]})),
            ("inspect.minimum_radius", json!({"curves": [c]})),
        ] {
            let with = |sketch: Value| {
                let mut p = p.clone();
                p["sketch"] = sketch;
                p
            };
            let e = s.execute(cmd, &with(json!(999999))).unwrap_err().to_string();
            assert!(e.contains("no sketch `999999`"), "{cmd}: {e}");
            s.execute(cmd, &with(json!("999999"))).unwrap_or_else(|e| panic!("{cmd} by name: {e}"));
            s.execute(cmd, &with(json!(named))).unwrap_or_else(|e| panic!("{cmd} by id: {e}"));
            for bad in [json!(-1), json!(1.5)] {
                let e = s.execute(cmd, &with(bad.clone())).unwrap_err().to_string();
                assert!(e.contains("must be an id or a name"), "{cmd} {bad}: {e}");
            }
        }
        assert_eq!(super::target(&s, &json!({"sketch": "999999"}), "test").unwrap(), named);
        assert_eq!(super::target(&s, &json!({"sketch": named.to_string()}), "test").unwrap(), named);
    }

    /// Each sketch command finds and keeps its sketch through this store (W38.0). Outside it, the
    /// command sources (non-test code) reach a sketch directly only on a line marked
    /// `// design-only:` in the files that work on the design's own sketches.
    #[test]
    fn each_sketch_command_goes_through_the_store() {
        // Sketches read or written, and the sketch being edited taken as a value (let-else, if let,
        // a match arm, `and_then`, `map`, `ok_or`); its state checks, assignments and snapshot copies
        // stay allowed.
        const PATTERNS: [&str; 8] = [
            ".sketch_mut(",
            "doc.sketch(",
            " d.sketch(",
            "active_sketch.ok_or",
            ") = s.active_sketch",
            "=> s.active_sketch",
            "s.active_sketch.and_then(",
            "s.active_sketch.map(",
        ];
        // A motion trace's wire, a joint's curve and Redefine Sketch Plane: the design's sketches.
        const DESIGN_ONLY: [&str; 3] = ["browser.rs", "joints_inspect.rs", "motion_trace.rs"];
        let mut stack = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cmd")];
        let (mut offenders, mut scanned) = (Vec::new(), 0);
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !name.ends_with(".rs") || name.ends_with("_tests.rs") || name == "sketch_store.rs" {
                    continue;
                }
                scanned += 1;
                // A Windows checkout's CRLF lines, read as written, so the split below finds the tests.
                let text = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
                let lines: Vec<&str> = text.split("\n#[cfg(test)]\nmod ").next().unwrap_or_default().lines().collect();
                for (n, line) in lines.iter().enumerate() {
                    let marked = n > 0 && lines[n - 1].trim_start().starts_with("// design-only:") && DESIGN_ONLY.contains(&name.as_str());
                    if !line.trim_start().starts_with("//") && !marked && PATTERNS.iter().any(|p| line.contains(p)) {
                        offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(scanned > 50, "the command sources were scanned ({scanned} files)");
        assert!(offenders.is_empty(), "a sketch read or written outside cmd::sketch_store; go through it:\n{}", offenders.join("\n"));
    }
}
