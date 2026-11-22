//! `solidcraft-cli diff A B [--json]` prints what changed from design A to design B. It wraps the
//! engine command `file.diff` (#626), which reads both designs and upgrades them without
//! evaluating either. The text is for people and may change, so scripts should read `--json`.
//! `main` turns the answer into the exit code, 0 for the same design and 1 when they differ.

use std::fmt::Write as _;
use std::io::Write as _;

use serde_json::json;
use solidcraft_engine::Session;
use solidcraft_engine::schema::file::{DiffFeature, DiffMarker, DiffParameter, DiffPayload, DiffResult};

/// Compares the two designs the arguments name and prints what changed. True when they are the same.
pub fn run(args: &[String]) -> Result<bool, String> {
    crate::known_args("diff", args, &[], &["--json"], true)?;
    let paths: Vec<&String> = args.iter().filter(|a| a.as_str() != "--json").collect();
    let [a, b] = paths.as_slice() else { return Err(crate::command_usage("diff", &[])) };
    let reply = Session::default().execute("file.diff", &json!({"a": a, "b": b})).map_err(|e| e.to_string())?;
    let result: DiffResult = serde_json::from_value(reply.clone()).map_err(|e| format!("file.diff: {e}"))?;
    for warning in &result.warnings {
        solidcraft_engine::note!("warning: {warning}");
    }
    let text = if args.iter().any(|a| a == "--json") { format!("{}\n", crate::pretty(&reply)) } else { describe(&result) };
    // A reader may stop early (`diff a b | head`), which is no error.
    match std::io::stdout().lock().write_all(text.as_bytes()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(format!("stdout: {e}")),
        _ => Ok(result.same),
    }
}

fn feature(f: &DiffFeature) -> String {
    format!("{} ({}, id {})", f.name, f.kind, f.id)
}

fn after(f: Option<&DiffFeature>) -> String {
    f.map_or_else(|| "at the start".to_owned(), |f| format!("after {}", f.name))
}

fn marker(m: &DiffMarker) -> String {
    match m {
        DiffMarker::End => "at the end".to_owned(),
        DiffMarker::Start => "at the start".to_owned(),
        DiffMarker::After(f) => format!("after {}", f.name),
    }
}

/// A parameter row by name, with the feature input it names, or its kind when it is no user
/// parameter, and its row when the name has more than one.
fn parameter(p: &DiffParameter) -> String {
    let mut about = Vec::new();
    match (&p.feature, &p.input) {
        (Some(id), Some(input)) => about.push(format!("feature id {id}, {input}")),
        _ if p.source != "user" => about.push(p.source.clone()),
        _ => {}
    }
    about.extend(p.row.map(|row| format!("row {row}")));
    if about.is_empty() { p.name.clone() } else { format!("{} ({})", p.name, about.join(", ")) }
}

/// A parameter row only one design has. A feature input is listed for its comment alone.
fn one_side(sign: char, p: &DiffParameter) -> String {
    if p.source == "feature" {
        format!("  {sign} {} comment {:?}", parameter(p), p.comment)
    } else {
        format!("  {sign} {} = {}", parameter(p), p.expression)
    }
}

fn stored(parts: &[DiffPayload]) -> String {
    let parts: Vec<String> = parts.iter().map(|p| format!("{} {} ({} bytes)", p.kind, p.sha256.get(..12).unwrap_or(&p.sha256), p.bytes)).collect();
    if parts.is_empty() { "nothing".to_owned() } else { parts.join(", ") }
}

/// The reply in words, a heading for each part that changed. Nothing when the designs are the same.
fn describe(r: &DiffResult) -> String {
    let mut out = String::new();
    if r.same {
        return out;
    }
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(if r.format.a == r.format.b { format!("format {}", r.format.a) } else { format!("format {} -> {}", r.format.a, r.format.b) });
    let f = &r.features;
    if !(f.added.is_empty() && f.removed.is_empty() && f.moved.is_empty() && f.changed.is_empty() && f.marker.is_none()) {
        line("features".into());
        for x in &f.added {
            line(format!("  + {}, {}", feature(&x.feature), after(x.after.as_ref())));
        }
        for x in &f.removed {
            line(format!("  - {}", feature(x)));
        }
        for x in &f.moved {
            line(format!("  > {} now {} (was {})", feature(&x.feature), after(x.after_b.as_ref()), after(x.after_a.as_ref())));
        }
        for x in &f.changed {
            let mut what = Vec::new();
            what.extend(x.renamed_from.as_ref().map(|old| format!("renamed from {old}")));
            what.extend(x.suppressed.map(|s| if s { "suppressed".to_owned() } else { "unsuppressed".to_owned() }));
            if !x.keys.is_empty() {
                what.push(format!("changed {}", x.keys.join(", ")));
            }
            line(format!("  ~ {} {}", feature(&x.feature), what.join(", ")));
        }
        for id in &f.reused_ids {
            line(format!("  ! id {id} is a different feature on each side"));
        }
        if let Some(m) = &f.marker {
            line(format!("  rollback marker {} -> {}", marker(&m.a), marker(&m.b)));
        }
    }
    let p = &r.parameters;
    if !(p.added.is_empty() && p.removed.is_empty() && p.changed.is_empty()) {
        line("parameters".into());
        for x in &p.added {
            line(one_side('+', x));
        }
        for x in &p.removed {
            line(one_side('-', x));
        }
        for x in &p.changed {
            let mut text = format!("  ~ {}", parameter(&x.b));
            if x.b.source != "feature" {
                let _ = write!(text, " = {}", x.a.expression);
            }
            if x.a.expression != x.b.expression {
                let _ = write!(text, " -> {}", x.b.expression);
            }
            if x.a.unit != x.b.unit {
                let _ = write!(text, ", unit {} -> {}", x.a.unit, x.b.unit);
            }
            if x.a.source != x.b.source {
                let _ = write!(text, ", {} -> {}", x.a.source, x.b.source);
            }
            if x.a.comment != x.b.comment {
                let _ = write!(text, ", comment {:?} -> {:?}", x.a.comment, x.b.comment);
            }
            line(text);
        }
    }
    if !r.sketches.is_empty() {
        line("sketches".into());
        for s in &r.sketches {
            let counts: Vec<String> = [("points", &s.points), ("curves", &s.curves), ("constraints", &s.constraints)]
                .iter()
                .map(|(what, n)| if n.a == n.b { format!("{} {what}", n.a) } else { format!("{} -> {} {what}", n.a, n.b) })
                .collect();
            line(format!("  ~ {} changed {} ({})", feature(&s.feature), s.keys.join(", "), counts.join(", ")));
        }
    }
    if !r.imports.is_empty() {
        line("imported files".into());
        for i in &r.imports {
            let (sign, what) = match (i.a.is_empty(), i.b.is_empty()) {
                (true, _) => ("+", stored(&i.b)),
                (_, true) => ("-", stored(&i.a)),
                _ => ("~", format!("{} -> {}", stored(&i.a), stored(&i.b))),
            };
            line(format!("  {sign} {} {}, {what}", feature(&i.feature), i.file));
        }
    }
    for (heading, keys) in [("other changes", &r.other), ("view settings", &r.view)] {
        if !keys.is_empty() {
            line(heading.into());
            line(format!("  {}", keys.join(", ")));
        }
    }
    out
}
