//! The checks that need no geometry: header, layout, names, references, operations and their
//! arguments, units, imports and mass.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rowan::TextRange;

use crate::diagnostic::{Code, Diagnostic};
use crate::fmt::layout;
use crate::model::{Arg, Child, Document, Entry, Expr, ExprKind, Note, Spanned};
use crate::project::{Loaded, Project, import_path, imported_file, normalize, resolve};
use crate::units::{Quantity, UNITS, unit};
use crate::vocabulary::{self, AXES, FileKind, MATH, NOTE_KINDS, ORIGIN, Op, PLANES, Place, find, is_reserved, points_of};

/// What a value measures, as far as `check` can tell without geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Length,
    Angle,
    Mass,
    Density,
    Number,
    Direction,
    Text,
    /// Geometry, or a value whose kind is unknown because of an error reported elsewhere.
    Any,
}

impl Kind {
    fn described(self) -> &'static str {
        match self {
            Kind::Length => "a length",
            Kind::Angle => "an angle",
            Kind::Mass => "a mass",
            Kind::Density => "a density",
            Kind::Number => "a plain number",
            Kind::Direction => "a direction",
            Kind::Text => "text",
            Kind::Any => "a value",
        }
    }

    fn numeric(self) -> bool {
        matches!(self, Kind::Length | Kind::Angle | Kind::Mass | Kind::Density | Kind::Number)
    }
}

impl From<Quantity> for Kind {
    fn from(q: Quantity) -> Self {
        match q {
            Quantity::Length => Kind::Length,
            Quantity::Angle => Kind::Angle,
            Quantity::Mass => Kind::Mass,
            Quantity::Density => Kind::Density,
        }
    }
}

/// One file's result, kept so each file is checked once however often it is imported.
#[derive(Debug, Default)]
pub struct Checked {
    pub diagnostics: Vec<Diagnostic>,
    params: HashMap<String, Kind>,
}

/// Checks the file at `path` and, once each, the files it imports. `Err` when it can't be read.
pub fn check(project: &mut Project, path: &Path) -> Result<Rc<Checked>, String> {
    let path = normalize(path);
    if let Some(done) = project.checks.get(&path) {
        return Ok(done.clone());
    }
    // Reached again through a member of a file on an import cycle, which E0703 reports.
    if project.in_progress.contains(&path) {
        return Ok(Rc::new(Checked::default()));
    }
    let file = project.load(&path)?;
    project.in_progress.insert(path.clone());
    let checked = Rc::new(check_loaded(project, &file));
    project.in_progress.remove(&path);
    project.checks.insert(path, checked.clone());
    Ok(checked)
}

/// The files a file's `import` entries name.
fn imports_of(file: &Loaded) -> Vec<PathBuf> {
    let Some(doc) = &file.doc else { return Vec::new() };
    doc.entries.iter().filter_map(|e| imported_file(&file.path, e)).collect()
}

/// Does importing `from` lead back to `goal`?
fn reaches(project: &mut Project, from: &Path, goal: &Path) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![from.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path == goal {
            return true;
        }
        if seen.insert(path.clone())
            && let Ok(file) = project.load(&path)
        {
            pending.extend(imports_of(&file));
        }
    }
    false
}

fn check_loaded(project: &mut Project, file: &Rc<Loaded>) -> Checked {
    let Some(doc) = &file.doc else {
        let mut diagnostics = file.parse.errors.clone();
        diagnostics.sort_by_key(|d| (d.range.start(), d.code));
        return Checked { diagnostics, params: HashMap::new() };
    };
    let mut c = Checker { project, file: file.clone(), kind: None, names: HashMap::new(), params: HashMap::new(), diags: Vec::new() };
    c.diags.extend(layout(&file.text, doc));
    c.document(doc);
    let mut diagnostics = c.diags;
    diagnostics.sort_by_key(|d| (d.range.start(), d.code));
    Checked { diagnostics, params: c.params }
}

struct Checker<'p> {
    project: &'p mut Project,
    file: Rc<Loaded>,
    kind: Option<FileKind>,
    /// Each entry name and where it is first defined.
    names: HashMap<String, usize>,
    params: HashMap<String, Kind>,
    diags: Vec<Diagnostic>,
}

/// Where a value is, for resolving its names.
#[derive(Clone, Copy)]
struct Scope<'d> {
    entry: usize,
    /// Inside a sketch: its entry and the index of the line being checked.
    sketch: Option<(&'d Entry, usize)>,
    /// The value is an `after:` argument, where a later step gets its own advice.
    after: bool,
    /// The value is a sketch element's point, which may be a seed.
    seed: bool,
}

impl Scope<'_> {
    fn inner(self) -> Self {
        Scope { seed: false, ..self }
    }
}

struct Kinds {
    positional: Vec<Kind>,
    named: Vec<Kind>,
}

/// What a dotted name has reached so far.
enum Target {
    Value(Kind),
    Entry {
        file: Rc<Loaded>,
        index: usize,
    },
    /// A sketch line, with the points it has when it is an element.
    Item(Option<&'static [&'static str]>),
    Point,
}

/// Why `name` can't name an entry or a sketch line, if it can't.
pub(crate) fn name_problem(name: &str) -> Option<(Code, String)> {
    let snake_case =
        name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if is_reserved(name) {
        Some((Code::ReservedName, format!("`{name}` is a built-in name")))
    } else if !snake_case {
        Some((Code::BadName, format!("`{name}` isn't snake_case, like `motor_mount`")))
    } else {
        None
    }
}

fn joined(names: impl IntoIterator<Item = &'static str>) -> String {
    names.into_iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

fn where_it_goes(op: &Op) -> &'static str {
    match op.place {
        Place::Entry([FileKind::Part]) => "is an entry in a part",
        Place::Entry([FileKind::Assembly]) => "is an entry in an assembly",
        Place::Entry(_) => "is an entry",
        Place::SketchLine => "is a line under a sketch",
        Place::StepOp => "is the operation of a `step`",
        Place::Value => "is a value",
    }
}

impl Checker<'_> {
    fn error(&mut self, code: Code, range: TextRange, message: impl Into<String>) {
        self.diags.push(Diagnostic::new(code, range, message));
    }

    fn document(&mut self, doc: &Document) {
        match doc.version.text.parse::<u32>() {
            Ok(1) => {}
            Ok(n) if n > 1 => self.error(Code::NewerVersion, doc.version.range, format!("this file is esc {n}, newer than this app's esc 1")),
            _ => self.error(Code::MissingVersion, doc.version.range, "the language version is `esc 1`"),
        }
        self.kind = match doc.kind.text.as_str() {
            "part" => Some(FileKind::Part),
            "assembly" => Some(FileKind::Assembly),
            other => {
                self.error(Code::MissingKind, doc.kind.range, format!("the file kind is `part` or `assembly`, not `{other}`"));
                None
            }
        };
        for (i, e) in doc.entries.iter().enumerate() {
            self.names.entry(e.name.text.clone()).or_insert(i);
        }
        for (i, e) in doc.entries.iter().enumerate() {
            self.entry(doc, i, e);
        }
        self.mass(doc);
    }

    fn name(&mut self, name: &Spanned) {
        if let Some((code, message)) = name_problem(&name.text) {
            self.error(code, name.range, message);
        }
    }

    fn notes<'n>(&mut self, notes: impl IntoIterator<Item = &'n Note>) {
        for n in notes {
            if !NOTE_KINDS.contains(&n.kind.text.as_str()) {
                self.error(
                    Code::UnknownNoteKind,
                    n.kind.range,
                    format!("a note is one of {}, not `{}`", joined(NOTE_KINDS.iter().copied()), n.kind.text),
                );
            }
        }
    }

    fn entry(&mut self, doc: &Document, i: usize, e: &Entry) {
        self.name(&e.name);
        if self.names.get(&e.name.text) != Some(&i) {
            self.error(Code::DuplicateName, e.name.range, format!("`{}` is already defined above", e.name.text));
        }
        let scope = Scope { entry: i, sketch: None, after: false, seed: false };
        let op = self.operation(&e.value, Place::Entry(&[]), scope);
        let is_sketch = op.is_some_and(|o| o.name == "sketch");
        for child in &e.children {
            match child {
                Child::Note(n) => self.notes([n]),
                Child::Item(item) if !is_sketch => {
                    self.error(Code::NotAllowedHere, item.name.range, "only a sketch has indented `name = ...` lines");
                }
                Child::Item(_) => {}
            }
        }
        if is_sketch {
            self.sketch(doc, i, e);
        }
        let Some(op) = op else { return };
        let kinds = self.args(op, &e.value, scope);
        match op.name {
            "param" => {
                let kind = kinds.positional.first().copied().unwrap_or(Kind::Any);
                self.params.entry(e.name.text.clone()).or_insert(kind);
            }
            "import" => self.import(e, &kinds.named),
            "import_step" => self.step_file(e),
            _ => {}
        }
    }

    fn sketch(&mut self, doc: &Document, i: usize, e: &Entry) {
        let items: Vec<_> = e.items().collect();
        for (n, item) in items.iter().enumerate() {
            self.name(&item.name);
            if doc.entries.iter().any(|other| other.name.text == item.name.text) {
                self.error(Code::DuplicateName, item.name.range, format!("`{}` is already an entry's name", item.name.text));
            } else if items.iter().position(|other| other.name.text == item.name.text) != Some(n) {
                self.error(Code::DuplicateName, item.name.range, format!("`{}` is already defined in this sketch", item.name.text));
            }
            self.notes(&item.notes);
            let scope = Scope { entry: i, sketch: Some((e, n)), after: false, seed: false };
            if let Some(op) = self.operation(&item.value, Place::SketchLine, scope) {
                self.args(op, &item.value, scope);
            }
        }
    }

    /// The operation a call names, if it may stand at `place`. An entry's place takes any
    /// operation this file kind allows.
    fn operation(&mut self, e: &Expr, place: Place, scope: Scope) -> Option<&'static Op> {
        let ExprKind::Call { name, .. } = &e.kind else {
            let message = match place {
                Place::Entry(_) => "an entry's value is an operation, like `param(60 mm)`",
                Place::SketchLine => "a sketch line is an element or a constraint, like `line(a.end, b.start)`",
                _ => "expected an operation",
            };
            self.error(Code::NotAllowedHere, e.range, message);
            self.value(e, scope.inner());
            return None;
        };
        let range = name.first().map_or(e.range, |n| n.range);
        let Some(text) = e.call_name() else {
            self.error(Code::UnknownOperation, e.range, "operations have one-word names");
            return None;
        };
        let Some(op) = find(text) else {
            if MATH.contains(&text) {
                self.error(Code::NotAllowedHere, range, format!("`{text}` is a formula function, used inside a value"));
            } else {
                self.error(Code::UnknownOperation, range, format!("there is no operation `{text}`"));
            }
            return None;
        };
        let fits = match (place, op.place) {
            (Place::Entry(_), Place::Entry(kinds)) => self.kind.is_none_or(|k| kinds.contains(&k)),
            (Place::Entry(_), Place::Value) => true,
            (a, b) => a == b,
        };
        if !fits {
            self.error(Code::NotAllowedHere, range, format!("`{text}` can't go here; it {}", where_it_goes(op)));
            return None;
        }
        Some(op)
    }

    /// Checks a call's arguments against its operation, and returns their kinds.
    fn args(&mut self, op: &'static Op, call: &Expr, scope: Scope) -> Kinds {
        let args = call.args();
        let mut kinds = Vec::new();
        let mut named: Vec<(&str, Kind)> = Vec::new();
        for a in args {
            match &a.name {
                None => {
                    if !named.is_empty() {
                        self.error(Code::PositionalAfterNamed, a.value.range, "positional arguments come before named ones");
                    }
                    let index = kinds.len();
                    let spec = op.positional.get(index).copied().unwrap_or(vocabulary::Arg::Any);
                    let kind = self.positional(op, index, spec, &a.value, scope);
                    kinds.push(kind);
                }
                Some(name) => {
                    if named.iter().any(|(n, _)| *n == name.text) {
                        self.error(Code::DuplicateArgument, name.range, format!("`{}` is given twice", name.text));
                    }
                    let spec = op.named.iter().find(|(n, _)| *n == name.text).map(|(_, s)| *s);
                    let spec = match spec {
                        Some(spec) => spec,
                        None if op.open => vocabulary::Arg::Any,
                        None => {
                            let takes = if op.named.is_empty() { "none".to_owned() } else { joined(op.named.iter().map(|(n, _)| *n)) };
                            self.error(Code::UnknownArgument, name.range, format!("`{}` has no argument `{}`; it takes {takes}", op.name, name.text));
                            vocabulary::Arg::Any
                        }
                    };
                    let inner = Scope { after: op.name == "step" && name.text == "after", ..scope.inner() };
                    let kind = self.expect(spec, &a.value, inner);
                    named.push((&name.text, kind));
                }
            }
        }
        let (n, required, max) = (kinds.len(), op.required, op.positional.len());
        if n < required || n > max {
            let count = match (required, max) {
                (0, 0) => "no positional arguments".to_owned(),
                (r, m) if r == m => format!("{m} positional argument{}", if m == 1 { "" } else { "s" }),
                (r, m) => format!("{r} to {m} positional arguments"),
            };
            self.error(Code::PositionalCount, call.range, format!("`{}` takes {count}, not {n}", op.name));
        }
        Kinds { positional: kinds, named: named.into_iter().map(|(_, k)| k).collect() }
    }

    fn positional(&mut self, op: &'static Op, index: usize, spec: vocabulary::Arg, e: &Expr, scope: Scope) -> Kind {
        match (op.name, index) {
            ("step", 0) => {
                if let Some(step_op) = self.operation(e, Place::StepOp, scope.inner()) {
                    self.args(step_op, e, scope.inner());
                }
                Kind::Any
            }
            ("instance", 0) => {
                let imported = match self.path_target(e, scope.inner()) {
                    Some(Some(Target::Entry { file, index })) => {
                        file.doc.as_ref().and_then(|d| d.entries.get(index)).and_then(Entry::call_name) == Some("import")
                    }
                    Some(_) => false,
                    None => return self.value(e, scope.inner()),
                };
                if !imported {
                    self.error(Code::WrongKind, e.range, "`instance` takes the name of an import");
                }
                Kind::Any
            }
            _ => {
                let seed = points_of(op.name).is_some();
                self.expect(spec, e, Scope { seed, ..scope.inner() })
            }
        }
    }

    /// `Some` when `e` is a dotted name, with what it reaches (`None` after an error).
    fn path_target(&mut self, e: &Expr, scope: Scope) -> Option<Option<Target>> {
        let ExprKind::Path(segs) = &e.kind else { return None };
        Some(self.resolve(segs, scope))
    }

    fn expect(&mut self, spec: vocabulary::Arg, e: &Expr, scope: Scope) -> Kind {
        use vocabulary::Arg as A;
        if let A::Choice(options) = spec {
            if !e.single_name().is_some_and(|n| options.contains(&n)) {
                self.error(Code::UnknownChoice, e.range, format!("expected one of {}", joined(options.iter().copied())));
            }
            return Kind::Any;
        }
        let kind = self.value(e, scope);
        let wanted = match spec {
            A::Length => Kind::Length,
            A::Angle => Kind::Angle,
            A::Number => Kind::Number,
            A::Text => Kind::Text,
            A::Direction => Kind::Direction,
            A::Quantity if kind.numeric() => return kind,
            A::Quantity => Kind::Number,
            A::Any | A::Choice(_) => return kind,
        };
        if kind != wanted && kind != Kind::Any {
            if kind == Kind::Number && wanted.numeric() {
                self.error(Code::MissingUnit, e.range, format!("expected {}; give the number a unit, like `3 mm` or `90 deg`", wanted.described()));
            } else {
                self.error(Code::WrongKind, e.range, format!("expected {}, found {}", wanted.described(), kind.described()));
            }
        }
        kind
    }

    fn value(&mut self, e: &Expr, scope: Scope) -> Kind {
        match &e.kind {
            ExprKind::Number { unit: None, .. } => Kind::Number,
            ExprKind::Number { unit: Some(u), .. } => match unit(u) {
                Some(q) => q.into(),
                None => {
                    self.error(Code::UnknownUnit, e.range, format!("`{u}` isn't a unit; units are {}", joined(UNITS.iter().map(|(u, _)| *u))));
                    Kind::Any
                }
            },
            ExprKind::Str(_) => Kind::Text,
            ExprKind::Path(segs) => match self.resolve(segs, scope) {
                Some(Target::Value(k)) => k,
                Some(Target::Entry { file, index }) => self.entry_kind(&file, index),
                Some(Target::Item(_) | Target::Point) | None => Kind::Any,
            },
            ExprKind::Call { .. } => self.call(e, scope.inner()),
            ExprKind::List(items) => {
                for item in items {
                    self.value(item, scope.inner());
                }
                Kind::Any
            }
            ExprKind::Seed(args) => {
                if !scope.seed {
                    self.error(Code::NotAllowedHere, e.range, "a seed `~(x, y)` only stands for a point of a `line`, `arc` or `circle`");
                }
                self.seed(e, args, scope.inner());
                Kind::Any
            }
            ExprKind::Paren(inner) => self.value(inner, scope.inner()),
            ExprKind::Unary { expr, .. } => {
                let kind = self.value(expr, scope.inner());
                if matches!(kind, Kind::Text) {
                    self.error(Code::WrongKind, e.range, "a sign goes before a number, a quantity or a direction, not text");
                    return Kind::Any;
                }
                kind
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.value(lhs, scope.inner());
                let r = self.value(rhs, scope.inner());
                self.arithmetic(*op, (l, lhs), (r, rhs), e.range)
            }
            ExprKind::Missing => Kind::Any,
        }
    }

    fn seed(&mut self, e: &Expr, args: &[Arg], scope: Scope) {
        for a in args {
            if let Some(name) = &a.name {
                self.error(Code::UnknownArgument, name.range, "a seed takes two lengths, `~(x, y)`, without names");
            }
            self.expect(vocabulary::Arg::Length, &a.value, scope);
        }
        if args.len() != 2 {
            self.error(Code::PositionalCount, e.range, format!("a seed takes 2 lengths, `~(x, y)`, not {}", args.len()));
        }
    }

    fn arithmetic(&mut self, op: char, (l, lhs): (Kind, &Expr), (r, rhs): (Kind, &Expr), range: TextRange) -> Kind {
        if l == Kind::Any || r == Kind::Any {
            return Kind::Any;
        }
        if !l.numeric() || !r.numeric() {
            let bad = if l.numeric() { r } else { l };
            self.error(Code::WrongKind, range, format!("`{op}` takes numbers and quantities, not {}", bad.described()));
            return Kind::Any;
        }
        match op {
            '+' | '-' if l == r => l,
            '+' | '-' if l == Kind::Number || r == Kind::Number => {
                let (bare, other) = if l == Kind::Number { (lhs, r) } else { (rhs, l) };
                self.error(Code::MissingUnit, bare.range, format!("this plain number meets {}; give it a unit", other.described()));
                Kind::Any
            }
            '+' | '-' => {
                self.error(Code::MixedKinds, range, format!("`{op}` mixes {} and {}", l.described(), r.described()));
                Kind::Any
            }
            '*' if l == Kind::Number => r,
            '*' if r == Kind::Number => l,
            '*' if l == Kind::Length && r == Kind::Length => {
                self.error(Code::LengthProduct, range, "a length times a length is an area, which esc 1 doesn't have");
                Kind::Any
            }
            '/' if r == Kind::Number => l,
            '/' if l == r => Kind::Number,
            _ => {
                self.error(Code::MixedKinds, range, format!("`{op}` of {} and {} has no unit in esc 1", l.described(), r.described()));
                Kind::Any
            }
        }
    }

    /// A call inside a value: a formula function or a value operation like `point(...)`.
    fn call(&mut self, e: &Expr, scope: Scope) -> Kind {
        if let Some(name) = e.call_name().filter(|n| MATH.contains(n)) {
            return self.math(name, e, scope);
        }
        match self.operation(e, Place::Value, scope) {
            Some(op) => {
                self.args(op, e, scope);
                if op.name == "dir" { Kind::Direction } else { Kind::Any }
            }
            None => {
                for a in e.args() {
                    self.value(&a.value, scope);
                }
                Kind::Any
            }
        }
    }

    fn math(&mut self, name: &str, e: &Expr, scope: Scope) -> Kind {
        let mut kinds = Vec::new();
        for a in e.args() {
            if let Some(n) = &a.name {
                self.error(Code::UnknownArgument, n.range, format!("`{name}` takes no named arguments"));
            }
            kinds.push((self.value(&a.value, scope), a.value.range));
        }
        let count = if matches!(name, "min" | "max" | "atan2") { 2 } else { 1 };
        if kinds.len() != count {
            self.error(
                Code::PositionalCount,
                e.range,
                format!("`{name}` takes {count} argument{}, not {}", if count == 1 { "" } else { "s" }, kinds.len()),
            );
            return Kind::Any;
        }
        if kinds.iter().any(|(k, _)| *k == Kind::Any) {
            return Kind::Any;
        }
        let first = kinds.first().map_or(Kind::Any, |(k, _)| *k);
        let expected = match name {
            "sin" | "cos" | "tan" => Kind::Angle,
            "sqrt" => Kind::Number,
            _ => first,
        };
        for (kind, range) in &kinds {
            if *kind != expected || !kind.numeric() {
                self.error(Code::WrongKind, *range, format!("`{name}` takes {}, found {}", expected.described(), kind.described()));
                return Kind::Any;
            }
        }
        match name {
            "sin" | "cos" | "tan" => Kind::Number,
            "atan2" => Kind::Angle,
            _ => first,
        }
    }

    fn resolve(&mut self, segs: &[Spanned], scope: Scope) -> Option<Target> {
        let (head, rest) = segs.split_first()?;
        let mut target = self.head(head, scope)?;
        for seg in rest {
            target = self.member(target, seg)?;
        }
        Some(target)
    }

    fn head(&mut self, name: &Spanned, scope: Scope) -> Option<Target> {
        let text = name.text.as_str();
        if let Some((sketch, at)) = scope.sketch {
            let items: Vec<_> = sketch.items().collect();
            if let Some(item) = items.iter().take(at).find(|i| i.name.text == text) {
                return Some(Target::Item(item.value.call_name().and_then(points_of)));
            }
            if items.iter().skip(at).any(|i| i.name.text == text) {
                self.error(
                    Code::DefinedLater,
                    name.range,
                    format!("`{text}` is defined later in the sketch; a line can only use the lines above it"),
                );
                return None;
            }
        }
        if text == ORIGIN || PLANES.contains(&text) {
            return Some(Target::Value(Kind::Any));
        }
        if AXES.contains(&text) {
            return Some(Target::Value(Kind::Direction));
        }
        match self.names.get(text).copied() {
            Some(index) if index < scope.entry => Some(Target::Entry { file: self.file.clone(), index }),
            Some(index) if index == scope.entry => {
                self.error(Code::DefinedLater, name.range, format!("`{text}` can't use itself"));
                None
            }
            Some(_) if scope.after => {
                self.error(
                    Code::DefinedLater,
                    name.range,
                    format!("step `{text}` is written later; move this step below it, since the file order is the guide order"),
                );
                None
            }
            Some(_) => {
                self.error(Code::DefinedLater, name.range, format!("`{text}` is defined later; an entry can only use the entries above it"));
                None
            }
            None => {
                self.error(Code::UnknownName, name.range, format!("nothing is named `{text}`"));
                None
            }
        }
    }

    fn member(&mut self, target: Target, seg: &Spanned) -> Option<Target> {
        let name = seg.text.as_str();
        match target {
            Target::Entry { file, index } => {
                let entry = file.doc.as_ref().and_then(|d| d.entries.get(index))?;
                let owner = entry.name.text.clone();
                match entry.call_name() {
                    Some("sketch") => match entry.items().find(|i| i.name.text == name) {
                        Some(item) => Some(Target::Item(item.value.call_name().and_then(points_of))),
                        None => {
                            self.error(Code::NoMember, seg.range, format!("sketch `{owner}` has no line `{name}`"));
                            None
                        }
                    },
                    Some("import") => {
                        let imported = self.imported(&file, entry)?;
                        self.entry_in(imported, &owner, seg)
                    }
                    Some("instance") => {
                        let import = entry.value.args().first().and_then(|a| a.value.single_name())?;
                        let doc = file.doc.as_ref()?;
                        let import = doc.entries.iter().take(index).find(|e| e.name.text == import)?;
                        let imported = self.imported(&file, import)?;
                        self.entry_in(imported, &owner, seg)
                    }
                    _ => {
                        self.error(Code::NoMember, seg.range, format!("`{owner}` has nothing to reach into with `.`"));
                        None
                    }
                }
            }
            Target::Item(Some(points)) => {
                if points.contains(&name) {
                    Some(Target::Point)
                } else {
                    self.error(Code::NoMember, seg.range, format!("this element's points are {}", joined(points.iter().copied())));
                    None
                }
            }
            Target::Item(None) | Target::Point | Target::Value(_) => {
                self.error(Code::NoMember, seg.range, "this has nothing to reach into with `.`");
                None
            }
        }
    }

    fn entry_in(&mut self, file: Rc<Loaded>, owner: &str, seg: &Spanned) -> Option<Target> {
        let index = file.doc.as_ref()?.entries.iter().position(|e| e.name.text == seg.text);
        match index {
            Some(index) => Some(Target::Entry { file, index }),
            None => {
                self.error(Code::NoMember, seg.range, format!("`{owner}` has no entry `{}`", seg.text));
                None
            }
        }
    }

    /// The file an `import` entry of `file` loads, when it can be read and parsed.
    fn imported(&mut self, file: &Loaded, entry: &Entry) -> Option<Rc<Loaded>> {
        let path = imported_file(&file.path, entry)?;
        self.project.load(&path).ok().filter(|l| l.doc.is_some())
    }

    fn entry_kind(&mut self, file: &Rc<Loaded>, index: usize) -> Kind {
        let Some(entry) = file.doc.as_ref().and_then(|d| d.entries.get(index)) else { return Kind::Any };
        if entry.call_name() != Some("param") {
            return Kind::Any;
        }
        if Rc::ptr_eq(file, &self.file) {
            return self.params.get(&entry.name.text).copied().unwrap_or(Kind::Any);
        }
        let checked = check(self.project, &file.path).ok();
        checked.and_then(|c| c.params.get(&entry.name.text).copied()).unwrap_or(Kind::Any)
    }

    fn import(&mut self, e: &Entry, given: &[Kind]) {
        let Some((written, range)) = e.written_path() else { return };
        let Some(path) = import_path(&self.file.path, &written) else {
            self.error(Code::AbsolutePath, range, "imports are relative to this file, so the folder can move");
            return;
        };
        if !path.is_file() {
            self.error(Code::FileNotFound, range, format!("there is no file `{written}.esc` here"));
            return;
        }
        let own = self.file.path.clone();
        if reaches(self.project, &path, &own) {
            self.error(Code::ImportCycle, range, format!("importing `{written}` makes a cycle: it imports this file, directly or through others"));
            return;
        }
        let checked = match check(self.project, &path) {
            Ok(checked) => checked,
            Err(message) => {
                self.error(Code::FileNotFound, range, message);
                return;
            }
        };
        if !checked.diagnostics.is_empty() {
            let n = checked.diagnostics.len();
            self.error(Code::ImportHasErrors, range, format!("`{written}` has {n} problem{}; check it", if n == 1 { "" } else { "s" }));
        }
        let named = e.value.args().iter().filter_map(|a| a.name.as_ref().map(|n| (n, &a.value)));
        for ((name, value), &kind) in named.zip(given) {
            match checked.params.get(&name.text) {
                None => self.error(Code::NotAParam, name.range, format!("`{written}` has no param `{}`; only params can be set", name.text)),
                Some(&wanted) => {
                    if kind != Kind::Any && wanted != Kind::Any && kind != wanted {
                        self.error(Code::WrongKind, value.range, format!("`{}` is {}, found {}", name.text, wanted.described(), kind.described()));
                    }
                }
            }
        }
    }

    fn step_file(&mut self, e: &Entry) {
        let Some((written, range)) = e.written_path() else { return };
        match resolve(&self.file.path, &written) {
            None => self.error(Code::AbsolutePath, range, "STEP paths are relative to this file, so the folder can move"),
            Some(path) if !path.is_file() => self.error(Code::FileNotFound, range, format!("there is no file `{written}` here")),
            Some(_) => {}
        }
    }

    fn mass(&mut self, doc: &Document) {
        let mut found = Vec::new();
        for e in &doc.entries {
            let wanted = match e.name.text.as_str() {
                "mass" => Kind::Mass,
                "density" => Kind::Density,
                _ => continue,
            };
            if e.call_name() != Some("param") {
                continue;
            }
            if self.kind == Some(FileKind::Assembly) {
                self.error(
                    Code::NotAllowedHere,
                    e.name.range,
                    format!("`{}` belongs to a part; an assembly's mass comes from its parts", e.name.text),
                );
            }
            let kind = self.params.get(&e.name.text).copied().unwrap_or(Kind::Any);
            if kind != wanted && kind != Kind::Any {
                self.error(Code::WrongKind, e.value.range, format!("`{}` is {}, found {}", e.name.text, wanted.described(), kind.described()));
            }
            found.push(e);
        }
        if let [_, second, ..] = found.as_slice()
            && found.iter().any(|e| e.name.text == "mass")
            && found.iter().any(|e| e.name.text == "density")
        {
            self.error(Code::MassAndDensity, second.name.range, "a part gives `mass` or `density`, not both; the volume gives the other");
        }
    }
}
