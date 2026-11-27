//! Edit intents: each GUI operation and each `edit` command, applied to the text. An edit
//! rewrites only the entries it changes, in canonical layout, and leaves every other byte alone.

use std::path::{Path, PathBuf};

use rowan::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

use crate::check::name_problem;
use crate::fmt;
use crate::model::{Arg, Child, Document, Entry, Expr, ExprKind, Item, Note, Spanned, lower, quote};
use crate::parse::parse;
use crate::project::{Project, imported_file, normalize};
use crate::vocabulary::points_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Why,
    Todo,
}

/// An argument by position (from 0) or by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgKey {
    Position(usize),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgEdit {
    pub arg: ArgKey,
    /// The new value as written in a file (`6 mm`), or `None` to remove a named argument.
    pub value: Option<String>,
}

/// Targets are an entry's name (`width`) or a sketch line's (`outline.base`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum Intent {
    SetParam {
        name: String,
        value: String,
    },
    EditArgs {
        target: String,
        args: Vec<ArgEdit>,
    },
    /// `text` is the whole entry as written, a sketch with its lines; first when `after` is `None`.
    InsertEntry {
        after: Option<String>,
        text: String,
    },
    DeleteEntry {
        name: String,
    },
    AddNote {
        target: String,
        kind: NoteKind,
        text: String,
    },
    /// `index` counts the target's notes from 0.
    RemoveNote {
        target: String,
        index: usize,
    },
    /// Moves a sketch point's rough position. `point` is `start`, `end` or `center`.
    SetSeed {
        target: String,
        point: String,
        x: String,
        y: String,
    },
    /// Renames an entry or a sketch line, and every reference to it in `files`.
    Rename {
        from: String,
        to: String,
    },
}

/// The new text of each file the intent changes. `files` are the project's other files, searched
/// for references by `rename` and `delete_entry`.
pub fn apply(project: &mut Project, files: &[PathBuf], path: &Path, intent: &Intent) -> Result<Vec<(PathBuf, String)>, String> {
    let path = normalize(path);
    let file = project.load(&path)?;
    let doc = file.doc.as_ref().ok_or_else(|| format!("{} has syntax errors; fix them first", path.display()))?;
    let text = &file.text;
    let changed = match intent {
        Intent::Rename { from, to } => return rename(project, files, &path, doc, from, to),
        Intent::DeleteEntry { name } => {
            let (i, e) = find_entry(doc, name)?;
            let refs = references(project, files, &path, &[name.as_str()])?;
            if let Some((file, _)) = refs.first() {
                let shown = path.parent().and_then(|dir| file.strip_prefix(dir).ok()).unwrap_or(file);
                return Err(format!("`{name}` is used in {}; remove those uses first", shown.display()));
            }
            let start = match i.checked_sub(1).and_then(|p| doc.entries.get(p)) {
                Some(prev) => prev.range.end(),
                None => line_end(text, doc.kind.range.end()),
            };
            splice(text, TextRange::new(start, e.range.end()), "")
        }
        Intent::InsertEntry { after, text: block } => {
            let new = parse_entry(block)?;
            if doc.entries.iter().any(|e| e.name.text == new.name.text) {
                return Err(format!("`{}` is already defined", new.name.text));
            }
            let block = fmt::entry(&new);
            match after {
                Some(name) => {
                    let (_, e) = find_entry(doc, name)?;
                    let at = e.range.end();
                    let lead = if text.get(..usize::from(at)).is_some_and(|t| t.ends_with('\n')) { "\n" } else { "\n\n" };
                    splice(text, TextRange::empty(at), &format!("{lead}{block}"))
                }
                None => match doc.entries.first() {
                    Some(first) => splice(text, TextRange::empty(first.range.start()), &format!("{block}\n")),
                    None => {
                        let lead = if text.ends_with('\n') { "\n" } else { "\n\n" };
                        format!("{text}{lead}{block}")
                    }
                },
            }
        }
        _ => {
            let (_, e) = find_entry(doc, target_entry(intent))?;
            let mut edited = e.clone();
            change(&mut edited, intent)?;
            splice(text, e.range, &fmt::entry(&edited))
        }
    };
    if let Some(problem) = parse(&changed).errors.first() {
        return Err(format!("the edit doesn't parse: {}", problem.message));
    }
    Ok(vec![(path, changed)])
}

fn target_entry(intent: &Intent) -> &str {
    let target = match intent {
        Intent::SetParam { name, .. } => name,
        Intent::EditArgs { target, .. } | Intent::AddNote { target, .. } | Intent::RemoveNote { target, .. } | Intent::SetSeed { target, .. } => {
            target
        }
        Intent::InsertEntry { .. } | Intent::DeleteEntry { .. } | Intent::Rename { .. } => "",
    };
    target.split('.').next().unwrap_or(target)
}

/// The sketch line a dotted target names, or `None` for the entry itself.
fn target_item(target: &str) -> Option<&str> {
    target.split_once('.').map(|(_, item)| item)
}

fn change(e: &mut Entry, intent: &Intent) -> Result<(), String> {
    match intent {
        Intent::SetParam { name, value } => {
            if e.call_name() != Some("param") {
                return Err(format!("`{name}` isn't a param"));
            }
            set_arg(&mut e.value, &ArgKey::Position(0), Some(value))
        }
        Intent::EditArgs { target, args } => {
            let value = match target_item(target) {
                Some(item) => &mut find_item(e, item)?.value,
                None => &mut e.value,
            };
            args.iter().try_for_each(|a| set_arg(value, &a.arg, a.value.as_deref()))
        }
        Intent::AddNote { target, kind, text } => {
            let kind = match kind {
                NoteKind::Why => "why",
                NoteKind::Todo => "todo",
            };
            let note = Note { kind: spanned(kind), text: spanned(&quote(text)) };
            match target_item(target) {
                Some(item) => find_item(e, item)?.notes.push(note),
                None => {
                    let at = e.children.iter().take_while(|c| matches!(c, Child::Note(_))).count();
                    e.children.insert(at, Child::Note(note));
                }
            }
            Ok(())
        }
        Intent::RemoveNote { target, index } => {
            let missing = || format!("`{target}` has no note {index}");
            match target_item(target) {
                Some(item) => {
                    let notes = &mut find_item(e, item)?.notes;
                    if *index >= notes.len() {
                        return Err(missing());
                    }
                    notes.remove(*index);
                }
                None => {
                    let at =
                        e.children.iter().enumerate().filter(|(_, c)| matches!(c, Child::Note(_))).nth(*index).map(|(i, _)| i).ok_or_else(missing)?;
                    e.children.remove(at);
                }
            }
            Ok(())
        }
        Intent::SetSeed { target, point, x, y } => {
            let item = target_item(target).ok_or_else(|| format!("`{target}` isn't a sketch line; name it as `sketch.line`"))?;
            set_seed(e, item, point, x, y)
        }
        Intent::InsertEntry { .. } | Intent::DeleteEntry { .. } | Intent::Rename { .. } => Ok(()),
    }
}

/// Shared points are written once, as a seed, and referred to as `line.end` elsewhere; the seed
/// goes where the point is written.
fn set_seed(e: &mut Entry, item: &str, point: &str, x: &str, y: &str) -> Result<(), String> {
    let seed = Expr {
        kind: ExprKind::Seed(vec![Arg { name: None, value: parse_value(x)? }, Arg { name: None, value: parse_value(y)? }]),
        range: TextRange::default(),
    };
    let (mut item, mut point) = (item.to_owned(), point.to_owned());
    for _ in 0..e.items().count() + 1 {
        let line = find_item(e, &item)?;
        let element = line.value.call_name().unwrap_or_default().to_owned();
        let points = points_of(&element).ok_or_else(|| format!("`{item}` is a constraint, not an element with points"))?;
        let index = points.iter().position(|p| *p == point).ok_or_else(|| format!("a {element} has the points {}", points.join(", ")))?;
        let ExprKind::Call { args, .. } = &mut line.value.kind else { return Err(format!("`{item}` isn't an element")) };
        let arg = args.iter_mut().filter(|a| a.name.is_none()).nth(index).ok_or_else(|| format!("`{item}` has no {point} written"))?;
        match &arg.value.kind {
            ExprKind::Seed(_) => {
                arg.value = seed;
                return Ok(());
            }
            ExprKind::Path(segs) if let [line, at] = segs.as_slice() => (item, point) = (line.text.clone(), at.text.clone()),
            _ => return Err(format!("`{item}.{point}` is fixed to another point and has no seed")),
        }
    }
    Err(format!("`{item}.{point}` refers around in a loop"))
}

fn set_arg(call: &mut Expr, key: &ArgKey, value: Option<&str>) -> Result<(), String> {
    let ExprKind::Call { args, .. } = &mut call.kind else { return Err("the target isn't an operation".to_owned()) };
    let value = value.map(parse_value).transpose()?;
    match key {
        ArgKey::Position(index) => {
            let value = value.ok_or("only named arguments can be removed")?;
            let arg = args.iter_mut().filter(|a| a.name.is_none()).nth(*index).ok_or_else(|| format!("there is no positional argument {index}"))?;
            arg.value = value;
        }
        ArgKey::Name(name) => {
            let at = args.iter().position(|a| a.name.as_ref().is_some_and(|n| n.text == *name));
            match (at, value) {
                (Some(at), Some(value)) => {
                    if let Some(arg) = args.get_mut(at) {
                        arg.value = value;
                    }
                }
                (Some(at), None) => {
                    args.remove(at);
                }
                (None, Some(value)) => args.push(Arg { name: Some(spanned(name)), value }),
                (None, None) => return Err(format!("there is no argument `{name}` to remove")),
            }
        }
    }
    Ok(())
}

fn spanned(text: &str) -> Spanned {
    Spanned { text: text.to_owned(), range: TextRange::default() }
}

fn find_entry<'d>(doc: &'d Document, name: &str) -> Result<(usize, &'d Entry), String> {
    doc.entries.iter().enumerate().find(|(_, e)| e.name.text == name).ok_or_else(|| format!("nothing is named `{name}`"))
}

fn find_item<'e>(e: &'e mut Entry, name: &str) -> Result<&'e mut Item, String> {
    let owner = e.name.text.clone();
    e.children
        .iter_mut()
        .find_map(|c| match c {
            Child::Item(i) if i.name.text == name => Some(i),
            _ => None,
        })
        .ok_or_else(|| format!("`{owner}` has no line `{name}`"))
}

/// Parses one value written as in a file, like `width / 2 - 6 mm`.
fn parse_value(text: &str) -> Result<Expr, String> {
    if text.contains('\n') {
        return Err("a value stays on one line".to_owned());
    }
    let e = parse_entry(&format!("value = {text}"))?;
    Ok(e.value)
}

/// Parses one entry, with any lines under it.
fn parse_entry(text: &str) -> Result<Entry, String> {
    let parsed = parse(&format!("esc 1\npart\n\n{}\n", text.trim_end_matches('\n')));
    if let Some(problem) = parsed.errors.first() {
        return Err(format!("`{text}` doesn't parse: {}", problem.message));
    }
    let mut doc = lower(&parsed.syntax());
    match (doc.entries.pop(), doc.entries.is_empty()) {
        (Some(e), true) => Ok(e),
        _ => Err("expected exactly one entry".to_owned()),
    }
}

fn line_end(text: &str, at: TextSize) -> TextSize {
    match text.get(usize::from(at)..) {
        Some(rest) if rest.starts_with('\n') => at + TextSize::from(1),
        _ => at,
    }
}

fn splice(text: &str, range: TextRange, with: &str) -> String {
    let (start, end) = (usize::from(range.start()), usize::from(range.end()));
    format!("{}{with}{}", text.get(..start).unwrap_or(text), text.get(end..).unwrap_or(""))
}

fn walk<'e>(e: &'e Expr, f: &mut impl FnMut(&'e Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Call { args, .. } | ExprKind::Seed(args) => args.iter().for_each(|a| walk(&a.value, f)),
        ExprKind::List(items) => items.iter().for_each(|i| walk(i, f)),
        ExprKind::Paren(inner) | ExprKind::Unary { expr: inner, .. } => walk(inner, f),
        ExprKind::Binary { lhs, rhs, .. } => {
            walk(lhs, f);
            walk(rhs, f);
        }
        ExprKind::Number { .. } | ExprKind::Str(_) | ExprKind::Path(_) | ExprKind::Missing => {}
    }
}

/// Every dotted name in the document, with the sketch it sits in.
fn paths(doc: &Document) -> Vec<(&[Spanned], Option<&str>)> {
    let mut out = Vec::new();
    for e in &doc.entries {
        let values = std::iter::once((&e.value, None)).chain(e.items().map(|i| (&i.value, Some(e.name.text.as_str()))));
        for (value, sketch) in values {
            walk(value, &mut |x| {
                if let ExprKind::Path(segs) = &x.kind {
                    out.push((segs.as_slice(), sketch));
                }
            });
        }
    }
    out
}

/// Where `target` (`[entry]` or `[sketch, line]`) of the file at `path` is used: in the file
/// itself, and through `import` and `instance` entries in `files`. Each range is the name's own
/// segment, ready to be renamed.
fn references(project: &mut Project, files: &[PathBuf], path: &Path, target: &[&str]) -> Result<Vec<(PathBuf, TextRange)>, String> {
    let mut out = Vec::new();
    let file = project.load(path)?;
    if let Some(doc) = &file.doc {
        for (segs, sketch) in paths(doc) {
            let names: Vec<&str> = segs.iter().map(|s| s.text.as_str()).collect();
            // The segment that names the target: a sketch's own lines are written without the sketch's name.
            let seg = match target {
                [entry] => names.starts_with(&[entry]).then_some(0),
                [sketch_name, line] if sketch == Some(*sketch_name) && names.starts_with(&[line]) => Some(0),
                [sketch_name, line] => names.starts_with(&[sketch_name, line]).then_some(1),
                _ => None,
            };
            out.extend(seg.and_then(|i| segs.get(i)).map(|s| (path.to_path_buf(), s.range)));
        }
    }
    for other in files {
        let other = normalize(other);
        if other == path {
            continue;
        }
        let Ok(importer) = project.load(&other) else { continue };
        let Some(doc) = &importer.doc else { continue };
        let mut aliases: Vec<&str> = Vec::new();
        for e in &doc.entries {
            match e.call_name() {
                Some("import") => {
                    if imported_file(&other, e).as_deref() != Some(path) {
                        continue;
                    }
                    aliases.push(&e.name.text);
                    if let [entry] = target {
                        let overrides = e.value.args().iter().filter_map(|a| a.name.as_ref()).filter(|n| n.text == *entry);
                        out.extend(overrides.map(|n| (other.clone(), n.range)));
                    }
                }
                Some("instance") if e.value.args().first().and_then(|a| a.value.single_name()).is_some_and(|n| aliases.contains(&n)) => {
                    aliases.push(&e.name.text);
                }
                _ => {}
            }
        }
        for (segs, _) in paths(doc) {
            let names: Vec<&str> = segs.iter().map(|s| s.text.as_str()).collect();
            if names.first().is_some_and(|n| aliases.contains(n)) && names.get(1..).is_some_and(|rest| rest.starts_with(target)) {
                out.extend(segs.get(target.len()).map(|s| (other.clone(), s.range)));
            }
        }
    }
    Ok(out)
}

fn rename(project: &mut Project, files: &[PathBuf], path: &Path, doc: &Document, from: &str, to: &str) -> Result<Vec<(PathBuf, String)>, String> {
    if let Some((_, problem)) = name_problem(to) {
        return Err(problem);
    }
    let target: Vec<&str> = from.split('.').collect();
    let taken = doc.entries.iter().any(|e| e.name.text == to) || doc.entries.iter().flat_map(|e| e.items()).any(|i| i.name.text == to);
    if taken {
        return Err(format!("`{to}` is already used in this file"));
    }
    let definition = match target.as_slice() {
        [entry] => find_entry(doc, entry)?.1.name.range,
        [sketch, line] => {
            let (_, e) = find_entry(doc, sketch)?;
            e.items().find(|i| i.name.text == *line).map(|i| i.name.range).ok_or_else(|| format!("`{sketch}` has no line `{line}`"))?
        }
        _ => return Err(format!("`{from}` isn't an entry or a sketch line")),
    };
    let mut edits = references(project, files, path, &target)?;
    edits.push((path.to_path_buf(), definition));
    edits.sort_by_key(|(p, r)| (p.clone(), std::cmp::Reverse(r.start())));
    edits.dedup();
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for (p, range) in edits {
        if out.last().is_none_or(|(last, _)| *last != p) {
            out.push((p.clone(), project.load(&p)?.text.clone()));
        }
        if let Some((_, text)) = out.last_mut() {
            *text = splice(text, range, to);
        }
    }
    Ok(out)
}
