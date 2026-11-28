//! Edit intents: each GUI operation and each `edit` command, applied to the text. An edit
//! rewrites only the entries it changes, in canonical layout, and leaves every other byte alone.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rowan::{TextRange, TextSize};
use serde::{Deserialize, Serialize};

use crate::check::name_problem;
use crate::fmt;
use crate::model::{Arg, Child, Document, Entry, Expr, ExprKind, Item, Note, Spanned, lower, quote};
use crate::parse::parse;
use crate::project::{Loaded, Project, identity, imported_file, instance_file, normalize};
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
    let doc = editable_document(&file)?;
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
fn paths(doc: &Document) -> Vec<(&[Spanned], Option<&Entry>)> {
    let mut out = Vec::new();
    for e in &doc.entries {
        let values = std::iter::once((&e.value, None)).chain(e.items().map(|i| (&i.value, Some(e))));
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
    let mut seen = HashSet::new();
    let target_path = identity(path);
    for other in std::iter::once(path).chain(files.iter().map(PathBuf::as_path)) {
        let other = normalize(other);
        if !seen.insert(identity(&other)) {
            continue;
        }
        let file = project.load(&other)?;
        let doc = editable_document(&file)?;
        for e in &doc.entries {
            if let [entry] = target
                && imported_file(&other, e).is_some_and(|p| identity(&p) == target_path)
            {
                let overrides = e.value.args().iter().filter_map(|a| a.name.as_ref()).filter(|n| n.text == *entry);
                out.extend(overrides.map(|n| (other.clone(), n.range)));
            }
        }
        for (segs, sketch) in paths(doc) {
            out.extend(reference_ranges(project, &file, segs, sketch, &target_path, target)?.into_iter().map(|range| (other.clone(), range)));
        }
    }
    Ok(out)
}

fn editable_document(file: &Loaded) -> Result<&Document, String> {
    file.doc.as_ref().ok_or_else(|| format!("{} has syntax errors; fix them first", file.path.display()))
}

fn reference_ranges(
    project: &mut Project,
    file: &Rc<Loaded>,
    segs: &[Spanned],
    sketch: Option<&Entry>,
    path: &Path,
    target: &[&str],
) -> Result<Vec<TextRange>, String> {
    let mut out = Vec::new();
    if let Some(head) = segs.first()
        && let Some(sketch) = sketch.filter(|e| e.items().any(|i| i.name.text == head.text))
    {
        if identity(&file.path) == path && target == [sketch.name.text.as_str(), head.text.as_str()] {
            out.push(head.range);
        }
        return Ok(out);
    }
    let mut file = file.clone();
    let mut file_path = identity(&file.path);
    for (index, seg) in segs.iter().enumerate() {
        let doc = editable_document(&file)?;
        let Some((at, entry)) = doc.entries.iter().enumerate().find(|(_, e)| e.name.text == seg.text) else { break };
        if file_path == path {
            match target {
                [name] if *name == entry.name.text => out.push(seg.range),
                [name, line] if *name == entry.name.text && entry.call_name() == Some("sketch") => {
                    if let Some(next) = segs.get(index + 1).filter(|s| s.text == *line && entry.items().any(|i| i.name.text == s.text)) {
                        out.push(next.range);
                    }
                }
                _ => {}
            }
        }
        if index + 1 == segs.len() {
            break;
        }
        let imported = match entry.call_name() {
            Some("import") => imported_file(&file.path, entry).map(|p| project.load(&p)).transpose()?,
            Some("instance") => instance_file(project, &file, at)?,
            _ => None,
        };
        let Some(imported) = imported else { break };
        file = imported;
        file_path = identity(&file.path);
    }
    Ok(out)
}

fn rename(project: &mut Project, files: &[PathBuf], path: &Path, doc: &Document, from: &str, to: &str) -> Result<Vec<(PathBuf, String)>, String> {
    if let Some((_, problem)) = name_problem(to) {
        return Err(problem);
    }
    let target: Vec<&str> = from.split('.').collect();
    let definition = match target.as_slice() {
        [entry] => {
            if doc.entries.iter().any(|e| e.name.text == to) || doc.entries.iter().flat_map(|e| e.items()).any(|i| i.name.text == to) {
                return Err(format!("`{to}` is already used in this file"));
            }
            find_entry(doc, entry)?.1.name.range
        }
        [sketch, line] => {
            let (_, e) = find_entry(doc, sketch)?;
            if e.items().any(|i| i.name.text == to) {
                return Err(format!("`{to}` is already used in sketch `{sketch}`"));
            }
            e.items().find(|i| i.name.text == *line).map(|i| i.name.range).ok_or_else(|| format!("`{sketch}` has no line `{line}`"))?
        }
        _ => return Err(format!("`{from}` isn't an entry or a sketch line")),
    };
    let mut edits = BTreeMap::<_, Vec<_>>::new();
    edits.entry(path.to_path_buf()).or_default().push(definition);
    for (p, range) in references(project, files, path, &target)? {
        edits.entry(p).or_default().push(range);
    }
    let mut out = Vec::new();
    for (p, mut ranges) in edits {
        ranges.sort_by_key(|range| range.start());
        ranges.dedup();
        let file = project.load(&p)?;
        let mut text = String::with_capacity(file.text.len());
        let mut end = 0;
        for range in ranges {
            text.push_str(file.text.get(end..usize::from(range.start())).ok_or("overlapping rename ranges")?);
            text.push_str(to);
            end = usize::from(range.end());
        }
        text.push_str(file.text.get(end..).ok_or("invalid rename range")?);
        out.push((p, text));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PART: &str = "esc 1\npart\n\nwidth = param(10 mm)\n\noutline = sketch(on: plane(XY))\n  base = line(~(0 mm, 0 mm), ~(10 mm, 0 mm))\n  side = line(base.end, ~(10 mm, 10 mm))\n  size = length(base, width)\n";
    const MIDDLE: &str = "esc 1\nassembly\n\npart = import(\"part\", width: 20 mm)\n\ncopy = instance(part)\n";
    const TOP: &str = "esc 1\nassembly\n\ngroup = import(\"middle\")\n\nplaced = instance(group)\n\nspan = param(group.part.width + placed.copy.width)\n\nanchor = connector(origin: placed.copy.outline.base.start, z: +Z, x: +X)\n";
    const DOTTED_TOP: &str = "esc 1\nassembly\n\ngroup = import(\"middle\")\n\ncatalogue = instance(group)\n\nplaced = instance(catalogue.part)\n\ndirect = instance(group.part)\n\nspan = param(placed.width + direct.width)\n\nanchor = connector(origin: placed.outline.base.start, z: +Z, x: +X)\n";

    fn project() -> Project {
        let mut project = Project::default();
        for (path, text) in [("part.esc", PART), ("middle.esc", MIDDLE), ("top.esc", TOP)] {
            project.replace(Path::new(path), text.to_owned());
        }
        project
    }

    fn rename_intent(from: &str, to: &str) -> Intent {
        Intent::Rename { from: from.to_owned(), to: to.to_owned() }
    }

    #[test]
    fn supplied_files_must_be_readable_and_parseable_before_rename_or_delete() {
        let mut project = project();
        let missing = std::env::temp_dir().join(format!("solidcraft-esc-missing-{}.esc", std::process::id()));
        let malformed = PathBuf::from("malformed.esc");
        project.replace(&malformed, "esc 1\npart\n\nbad = param(\n".to_owned());
        for file in [missing, malformed] {
            for intent in [rename_intent("width", "breadth"), Intent::DeleteEntry { name: "width".to_owned() }] {
                let error = apply(&mut project, std::slice::from_ref(&file), Path::new("part.esc"), &intent).unwrap_err();
                assert!(error.contains(&file.display().to_string()), "{error}");
            }
        }
    }

    #[test]
    fn rename_follows_multiple_imports_and_instances_and_updates_overrides() {
        let mut project = project();
        let files = ["top.esc", "middle.esc", "part.esc"].map(PathBuf::from);
        let changed = apply(&mut project, &files, Path::new("part.esc"), &rename_intent("width", "breadth")).unwrap();
        assert_eq!(
            changed,
            vec![
                (PathBuf::from("middle.esc"), MIDDLE.replace("width:", "breadth:")),
                (PathBuf::from("part.esc"), PART.replace("width", "breadth")),
                (PathBuf::from("top.esc"), TOP.replace(".width", ".breadth")),
            ]
        );
    }

    #[test]
    fn deletion_finds_references_through_files_not_in_the_scan_list() {
        let mut project = project();
        let error =
            apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &Intent::DeleteEntry { name: "outline".to_owned() }).unwrap_err();
        assert!(error.contains("top.esc"), "{error}");
        project.replace(Path::new("part.esc"), "esc 1\npart\n\nwidth = param(10 mm)\n".to_owned());
        let error =
            apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &Intent::DeleteEntry { name: "width".to_owned() }).unwrap_err();
        assert!(error.contains("top.esc"), "{error}");
    }

    #[test]
    fn rename_follows_dotted_instance_imports_and_instance_namespaces() {
        let mut project = project();
        project.replace(Path::new("top.esc"), DOTTED_TOP.to_owned());
        for (from, to, part, top) in [
            ("width", "breadth", PART.replace("width", "breadth"), DOTTED_TOP.replace(".width", ".breadth")),
            ("outline.base", "bottom", PART.replace("base", "bottom"), DOTTED_TOP.replace("outline.base", "outline.bottom")),
        ] {
            let changed = apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &rename_intent(from, to)).unwrap();
            assert_eq!(changed, vec![(PathBuf::from("part.esc"), part), (PathBuf::from("top.esc"), top)]);
        }
    }

    #[test]
    fn deletion_finds_references_through_dotted_instances() {
        let mut project = project();
        project.replace(Path::new("top.esc"), DOTTED_TOP.to_owned());
        let error =
            apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &Intent::DeleteEntry { name: "outline".to_owned() }).unwrap_err();
        assert!(error.contains("top.esc"), "{error}");
    }

    #[test]
    fn dotted_instance_cycles_report_an_error_during_reference_scanning() {
        let mut project = project();
        project.replace(
            Path::new("top.esc"),
            "esc 1\nassembly\n\ngroup = import(\"cycle\")\n\nplaced = instance(group.copy.part)\n\nspan = param(placed.width)\n".to_owned(),
        );
        project.replace(Path::new("cycle.esc"), "esc 1\nassembly\n\ngroup = import(\"top\")\n\ncopy = instance(group.placed.part)\n".to_owned());
        for intent in [rename_intent("width", "breadth"), Intent::DeleteEntry { name: "outline".to_owned() }] {
            let error = apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &intent).unwrap_err();
            assert!(error.contains("cycle"), "{error}");
        }
    }

    #[test]
    fn sketch_line_rename_uses_only_the_owning_sketch_namespace() {
        let mut project = project();
        let text = format!(
            "{PART}\nbottom = param(2 mm)\n\nother = sketch(on: plane(XY))\n  bottom = line(~(0 mm, 0 mm), ~(1 mm, 0 mm))\n  base = line(bottom.end, ~(1 mm, 1 mm))\n"
        );
        project.replace(Path::new("part.esc"), text.clone());
        let changed = apply(&mut project, &[PathBuf::from("top.esc")], Path::new("part.esc"), &rename_intent("outline.base", "bottom")).unwrap();
        assert_eq!(
            changed,
            vec![
                (PathBuf::from("part.esc"), text.replacen("base", "bottom", 3)),
                (PathBuf::from("top.esc"), TOP.replace("outline.base", "outline.bottom")),
            ]
        );
        assert!(apply(&mut project, &[], Path::new("part.esc"), &rename_intent("outline.base", "side")).is_err());
    }

    #[test]
    fn entry_rename_does_not_change_references_to_sketch_local_names() {
        let mut project = project();
        let text = "esc 1\npart\n\nbase = param(1 mm)\n\noutline = sketch(on: plane(XY))\n  base = line(~(0 mm, 0 mm), ~(1 mm, 0 mm))\n  side = line(base.end, ~(1 mm, 1 mm))\n\nsize = param(base)\n";
        project.replace(Path::new("part.esc"), text.to_owned());
        let changed = apply(&mut project, &[], Path::new("part.esc"), &rename_intent("base", "width")).unwrap();
        assert_eq!(
            changed,
            vec![(PathBuf::from("part.esc"), text.replacen("base = param", "width = param", 1).replace("param(base)", "param(width)"))]
        );
    }

    #[test]
    fn rename_preserves_untouched_bytes_with_longer_names() {
        let mut project = Project::default();
        let text = "esc 1\npart\n\nw = param(10 mm)\n\nspan = param(w + w + w)\n  why: \"w is a label, not a reference\"\n";
        project.replace(Path::new("part.esc"), text.to_owned());
        let changed = apply(&mut project, &[], Path::new("part.esc"), &rename_intent("w", "width")).unwrap();
        assert_eq!(changed, vec![(PathBuf::from("part.esc"), text.replace("w =", "width =").replace("w + w + w", "width + width + width"))]);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aliases_match_references_without_duplicate_file_edits() {
        let dir = std::env::temp_dir().join(format!("solidcraft-esc-edit-symlinks-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let part = dir.join("part.esc");
        let alias = dir.join("alias.esc");
        let scene = dir.join("scene.esc");
        let scene_alias = dir.join("scene-alias.esc");
        let scene_text = "esc 1\nassembly\n\nreal = import(\"part\", width: 20 mm)\n\nlinked = import(\"alias\", width: 30 mm)\n\nspan = param(real.width + linked.width)\n\nanchor = connector(origin: linked.outline.base.start, z: +Z, x: +X)\n";
        std::fs::write(&part, PART).unwrap();
        std::fs::write(&scene, scene_text).unwrap();
        std::os::unix::fs::symlink("part.esc", &alias).unwrap();
        std::os::unix::fs::symlink("scene.esc", &scene_alias).unwrap();
        for path in [&part, &alias] {
            let mut project = Project::default();
            let files = [part.clone(), alias.clone(), scene.clone(), scene_alias.clone()];
            let changed = apply(&mut project, &files, path, &rename_intent("width", "breadth")).unwrap();
            assert_eq!(changed.len(), 2);
            assert!(changed.contains(&(path.clone(), PART.replace("width", "breadth"))));
            assert!(changed.contains(&(scene.clone(), scene_text.replace("width", "breadth"))));
            let error = apply(&mut project, &files, path, &Intent::DeleteEntry { name: "outline".to_owned() }).unwrap_err();
            assert!(error.contains("scene.esc"), "{error}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
