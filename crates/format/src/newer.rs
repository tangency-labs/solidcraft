//! A file a newer build saved: it opens, with a warning, and saving it loses nothing.
//!
//! A newer format number, or a change the file names that this build doesn't have, says a newer
//! build saved the file ([`crate::Family::newer`]). It may use nothing this build lacks, so the app
//! reads it as usual. When part of it can't be read, that part stays out of the way and the rest
//! opens: the app reads the elements of its lists it can ([`NewerFile::keep_element`]), then any
//! other top-level section it can't read is left out whole ([`leave_out_unreadable`],
//! [`read_around`]), and a field it doesn't know, which typed reading would drop, is noticed by
//! comparing the file with what was read ([`NewerFile::compare`]).
//!
//! All of these are named in [`NewerFile::not_understood`] and go back into the file when it is
//! saved ([`NewerFile::restore_into`]), as they came. What can't be put back because the file was
//! changed around it is what `restore_into` returns.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::Newer;

/// The most sections left out one at a time to find the one a file can't be read with.
const MAX_SECTION_TRIES: usize = 64;

/// The most bytes of the lines that name the fields a comparison keeps, and of those that name what
/// couldn't go back on saving. A line names a field by its path (`components[Part 1].colour`), with
/// every name and key on the way whole. A real design's lines take far less. Most are under 100
/// bytes, and a line through a face whose name has 1 MiB (the longest face name a design reads
/// back) fits 15 times. Past it the rest are counted in one more line (`12 more fields`), so a
/// file of huge names or of millions of fields can't make naming them ask for more memory than
/// this.
const NAMED_BYTES: usize = 16 << 20;

/// What the file says and holds that this build doesn't read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewerFile {
    /// The app, as the notice names it.
    pub app: &'static str,
    /// The number the file says.
    pub format: u32,
    /// The number this build reads up to.
    pub reads_up_to: u32,
    /// The changes the file names that this build doesn't have.
    pub unknown_changes: Vec<String>,
    /// What this build couldn't read, by name.
    pub not_understood: Vec<String>,
    /// Fields known records lost in typed reading.
    kept: Option<Kept>,
    /// Top-level sections left out whole.
    sections: Vec<Section>,
    /// Elements left out of a top-level list: its key and the element.
    elements: Vec<(String, Value)>,
    /// The parts of a container this build has no use for: name and bytes. A blob only one of the
    /// kept pieces refers to is one of them.
    pub parts: Vec<(String, Arc<[u8]>)>,
    /// The file's `container.json`, when this build doesn't know what it says.
    pub container: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
struct Section {
    key: String,
    value: Value,
    /// This build's record of the section, without the file's: while it is unchanged, the file's
    /// goes back.
    empty: Option<Value>,
}

/// The fields of a record that typed reading dropped.
#[derive(Clone, Debug, PartialEq)]
enum Kept {
    Field(Value),
    Fields(BTreeMap<String, Kept>),
    Items(Vec<(Anchor, Kept)>),
}

/// Which element of a list some kept fields belong to.
#[derive(Clone, Debug, PartialEq)]
enum Anchor {
    Id(u64),
    Name(String),
    /// Its place, good only while the list keeps its length.
    Index {
        at: usize,
        of: usize,
    },
}

impl NewerFile {
    /// A file of `app` that `newer` says a newer build saved, nothing left out of it yet.
    pub fn new(app: &'static str, newer: &Newer) -> NewerFile {
        NewerFile {
            app,
            format: newer.format,
            reads_up_to: newer.reads_up_to,
            unknown_changes: newer.unknown_changes.clone(),
            ..NewerFile::default()
        }
    }

    /// What to tell the user, whatever else is said.
    pub fn notice(&self) -> String {
        let (app, reads, format) = (self.app, self.reads_up_to, self.format);
        match self.unknown_changes.first() {
            Some(change) if format <= reads => {
                format!("Saved by a newer {app} (format {format} with `{change}`, which this version doesn't know). If something's missing, update.")
            }
            _ => format!("Saved by a newer {app} (format {format}, this version reads up to {reads}). If something's missing, update."),
        }
    }

    /// Every line to report: the notice, then what couldn't be read, by name.
    pub fn warnings(&self) -> Vec<String> {
        let unread = self.not_understood.iter().map(|what| format!("Not read: {what}. Saving keeps it."));
        std::iter::once(self.notice()).chain(unread).collect()
    }

    /// What a reply says about it.
    pub fn report(&self) -> Value {
        json!({
            "format": self.format,
            "reads_up_to": self.reads_up_to,
            "unknown_changes": self.unknown_changes,
            "not_understood": self.not_understood,
            "notice": self.notice(),
        })
    }

    /// Whether the next save needs a container to keep what the file holds: plain JSON has no room
    /// for parts beside the file's own.
    pub fn needs_container(&self) -> bool {
        !self.parts.is_empty()
    }

    /// Whether anything kept back names this text (a blob's name in a container).
    pub fn mentions(&self, text: &str) -> bool {
        fn kept(k: &Kept, text: &str) -> bool {
            match k {
                Kept::Field(v) => value_mentions(v, text),
                Kept::Fields(m) => m.values().any(|x| kept(x, text)),
                Kept::Items(l) => l.iter().any(|(_, x)| kept(x, text)),
            }
        }
        self.kept.as_ref().is_some_and(|k| kept(k, text))
            || self.sections.iter().any(|s| value_mentions(&s.value, text))
            || self.elements.iter().any(|(_, v)| value_mentions(v, text))
    }

    /// Say that a section the reader couldn't finish reading is left out. `empty` is what the
    /// app writes for the section while it holds nothing, if anything.
    pub fn keep_section(&mut self, key: &str, value: Value, empty: Option<Value>, why: &str) {
        self.not_understood.push(format!("section `{key}` ({why})"));
        self.sections.push(Section { key: key.into(), value, empty });
    }

    /// Say that an element of the top-level list `key` is left out, as `note` names it; it goes
    /// back at the end of the list when the file is saved.
    pub fn keep_element(&mut self, key: &str, element: Value, note: String) {
        self.not_understood.push(note);
        self.elements.push((key.into(), element));
    }

    /// Compare the file as it reads (`file`, after the same upgrade as what was read) with what was
    /// read of it (`known`, its typed record written out): what `known` lacks is kept and named.
    /// An object whose `type` is `placeholder_tag` (an element the app kept whole) and the fields
    /// in `container_only` (what a container adds beside the file's own) are not compared; nor
    /// are the top level's `format` and `format_pending`. The fields are named while their lines
    /// fit in [`NAMED_BYTES`], and the rest are counted.
    pub fn compare(&mut self, file: &Value, known: &Value, placeholder_tag: &str, container_only: &[&str]) {
        let mut walk = Compare::new(placeholder_tag, container_only);
        self.kept = vanished(file, known, &TOP, &mut walk);
        self.not_understood.extend(walk.named.lines("field", "fields"));
        for section in &mut self.sections {
            section.empty = known.get(&section.key).cloned();
        }
    }

    /// Write back what was kept of the file into `value`, the file as this build writes it.
    /// Returns what couldn't go back, because the file was changed around it, named while the
    /// lines fit in [`NAMED_BYTES`] and then counted.
    pub fn restore_into(&self, value: &mut Value) -> Vec<String> {
        let mut put = Restore::default();
        if let Some(kept) = &self.kept {
            apply(kept, value, &TOP, &mut put);
        }
        let mut lost = put.named.lines("field or element", "fields or elements");
        if let Some(top) = value.as_object_mut() {
            for section in &self.sections {
                if top.get(&section.key) == section.empty.as_ref() {
                    top.insert(section.key.clone(), section.value.clone());
                } else {
                    lost.push(format!("section `{}`, replaced by what was edited here", section.key));
                }
            }
            // The names in each list that elements go back into, made the first time and grown with
            // each element put back, so the list isn't looked through again for each element.
            let mut names: HashMap<&str, HashSet<Value>> = HashMap::new();
            for (key, element) in &self.elements {
                let name = element.get("name");
                let back = match top.entry(key.clone()).or_insert_with(|| json!([])) {
                    Value::Array(list) => {
                        let taken = names.entry(key.as_str()).or_insert_with(|| list.iter().filter_map(|x| x.get("name")).cloned().collect());
                        // One whose name the list has already stays out.
                        let free = name.is_none_or(|name| taken.insert(name.clone()));
                        if free {
                            list.push(element.clone());
                        }
                        free
                    }
                    _ => false,
                };
                if !back {
                    lost.push(format!("`{key}` element {}, replaced by what was made here", name.and_then(Value::as_str).unwrap_or("unnamed")));
                }
            }
        }
        lost
    }
}

fn value_mentions(v: &Value, text: &str) -> bool {
    match v {
        Value::String(s) => s == text,
        Value::Array(a) => a.iter().any(|x| value_mentions(x, text)),
        Value::Object(o) => o.values().any(|x| value_mentions(x, text)),
        _ => false,
    }
}

/// The first line of a reader's error, without the list of every kind it knows, and without the
/// `"<noun> file: "` its reader starts it with.
pub fn short<'a>(why: &'a str, noun: &str) -> &'a str {
    let why = why.split(", expected").next().unwrap_or(why);
    why.strip_prefix(&format!("{noun} file: ")).unwrap_or(why)
}

/// Leave a section out and keep it. A section the file needs is left empty.
fn move_out(top: &mut Map<String, Value>, key: &str, why: &str, noun: &str, file: &mut NewerFile) {
    let Some(value) = top.remove(key) else { return };
    file.not_understood.push(format!("section `{key}` ({})", short(why, noun)));
    file.sections.push(Section { key: key.into(), value, empty: None });
}

/// Every top-level section of `top` that doesn't read by itself (`piece`, the app's reading of a
/// file holding just that section) is left out and kept in `file`. `noun` is what a file of the
/// app's is called, which its reader's errors start with.
pub fn leave_out_unreadable<E: std::fmt::Display>(
    top: &mut Map<String, Value>,
    file: &mut NewerFile,
    noun: &str,
    piece: impl Fn(&str, &Value) -> Result<(), E>,
) {
    let keys: Vec<String> = top.keys().cloned().collect();
    for key in &keys {
        let value = top.get(key).cloned().unwrap_or_default();
        if let Err(why) = piece(key, &value) {
            move_out(top, key, &why.to_string(), noun, file);
        }
    }
}

/// Read `top` with `attempt`, leaving out the sections it can't be read with: if it doesn't read,
/// each section but the `core` ones (which the file can't be read without) is tried left out alone,
/// and as a last resort every one of them is. What is left out is kept in `file`. The error is the
/// first attempt's, when even the core alone doesn't read.
pub fn read_around<D, E: std::fmt::Display>(
    top: &mut Map<String, Value>,
    file: &mut NewerFile,
    core: &[&str],
    noun: &str,
    attempt: impl Fn(&Map<String, Value>) -> Result<D, E>,
) -> Result<D, String> {
    let first = match attempt(top) {
        Ok(doc) => return Ok(doc),
        Err(why) => why.to_string(),
    };
    // Sections that are fine by themselves but not together with the rest.
    let keys: Vec<String> = top.keys().filter(|k| !core.contains(&k.as_str())).cloned().collect();
    // Each try reads the whole file: a file of many sections goes straight to the last resort.
    for key in keys.iter().filter(|_| keys.len() <= MAX_SECTION_TRIES) {
        let mut without = top.clone();
        without.remove(key);
        if let Ok(doc) = attempt(&without) {
            move_out(top, key, &first, noun, file);
            return Ok(doc);
        }
    }
    let mut bare = top.clone();
    bare.retain(|key, _| core.contains(&key.as_str()));
    match attempt(&bare) {
        Ok(doc) => {
            for key in &keys {
                move_out(top, key, &first, noun, file);
            }
            Ok(doc)
        }
        Err(_) => Err(first),
    }
}

/// A value a writer leaves out without losing anything: not worth telling anyone about.
pub fn looks_default(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// A comparison of a file with what was read of it ([`NewerFile::compare`]).
struct Compare<'c> {
    placeholder_tag: &'c str,
    container_only: &'c [&'c str],
    named: Named,
    lists: Lists,
}

impl<'c> Compare<'c> {
    fn new(placeholder_tag: &'c str, container_only: &'c [&'c str]) -> Compare<'c> {
        Compare { placeholder_tag, container_only, named: Named::default(), lists: Lists::default() }
    }
}

/// What `raw` has that `known`, what was read of it, doesn't. `walk` names those fields.
fn vanished(raw: &Value, known: &Value, path: &Path<'_>, walk: &mut Compare<'_>) -> Option<Kept> {
    match (raw, known) {
        (Value::Object(raw), Value::Object(known)) => {
            if known.get("type").and_then(Value::as_str) == Some(walk.placeholder_tag) {
                return None;
            }
            let mut kept = BTreeMap::new();
            for (key, value) in raw {
                if walk.container_only.contains(&key.as_str()) || (path.blank && matches!(key.as_str(), "format" | "format_pending")) {
                    continue;
                }
                let inner = path.then(Step::Key(key));
                match known.get(key) {
                    None => {
                        if !looks_default(value) {
                            walk.named.add(format_args!("field `{inner}`"));
                        }
                        kept.insert(key.clone(), Kept::Field(value.clone()));
                    }
                    Some(known) => {
                        if let Some(k) = vanished(value, known, &inner, walk) {
                            kept.insert(key.clone(), k);
                        }
                    }
                }
            }
            (!kept.is_empty()).then_some(Kept::Fields(kept))
        }
        (Value::Array(raw), Value::Array(known)) => {
            // Nothing to look for, or nothing to find it in, so nothing is kept.
            if raw.is_empty() || known.is_empty() {
                return None;
            }
            // Taken when an element looks for its partner by id or name ([`Lists::find`]).
            let mut partners = None;
            let mut items = Vec::new();
            for (at, element) in raw.iter().enumerate() {
                let anchor = anchor_of(element).unwrap_or(Anchor::Index { at, of: raw.len() });
                let Some(partner) = walk.lists.find(&mut partners, known, &anchor).and_then(|place| known.get(place)) else { continue };
                let step = match (element.get("name").and_then(Value::as_str), &anchor) {
                    (Some(name), _) => Step::Name(name),
                    (None, Anchor::Id(id)) => Step::Id(*id),
                    _ => Step::At(at),
                };
                if let Some(k) = vanished(element, partner, &path.then(step), walk) {
                    items.push((anchor, k));
                }
            }
            walk.lists.keep(known, partners);
            (!items.is_empty()).then_some(Kept::Items(items))
        }
        _ => None,
    }
}

fn anchor_of(element: &Value) -> Option<Anchor> {
    if let Some(id) = element.get("id").and_then(Value::as_u64) {
        return Some(Anchor::Id(id));
    }
    element.get("name").and_then(Value::as_str).map(|name| Anchor::Name(name.to_string()))
}

/// Where in a list the first element with each id, and the first with each name, is. [`Lists`]
/// makes it the first time an element looks in a long list by id or name, so the list isn't
/// looked through from the start for each element that looks for its partner there.
struct Partners {
    ids: HashMap<u64, usize>,
    names: HashMap<String, usize>,
}

impl Partners {
    fn of(list: &[Value]) -> Partners {
        let mut partners = Partners { ids: HashMap::new(), names: HashMap::new() };
        for (at, element) in list.iter().enumerate() {
            partners.add(at, element.get("id").and_then(Value::as_u64), element.get("name").and_then(Value::as_str));
        }
        partners
    }

    /// Count `id` and `name`, which the element at `at` has, where no element before it has them.
    fn add(&mut self, at: usize, id: Option<u64>, name: Option<&str>) {
        if let Some(id) = id {
            self.ids.entry(id).and_modify(|first| *first = (*first).min(at)).or_insert(at);
        }
        if let Some(name) = name {
            match self.names.get_mut(name) {
                Some(first) => *first = (*first).min(at),
                None => {
                    self.names.insert(name.to_string(), at);
                }
            }
        }
    }
}

/// A list this long or shorter is looked through for an id or a name, as before lists were
/// indexed, and gets no index. That costs each element looking at most this many comparisons, and
/// saves an index and a place in the store for each of the many short lists a design holds, whose
/// index would take about as much room as the list.
const SEARCHED_LEN: usize = 16;

/// The [`Partners`] of each long list looked in by id or name, made the first time and kept for
/// the next, by where the list's first element is. Comparing changes nothing, and putting kept
/// fields back only adds fields to elements, so no list's elements move or are dropped while they
/// are used, and that tells the lists apart. A list that many elements share as their partner
/// (they repeat one id or name) is then indexed once, not once for each of them. A list no element
/// looks in by id or name, such as a list of numbers, and a list of at most [`SEARCHED_LEN`]
/// elements are neither indexed nor stored.
#[derive(Default)]
struct Lists {
    partners: HashMap<*const Value, Partners>,
    /// How many lists were indexed.
    #[cfg(test)]
    indexed: usize,
}

impl Lists {
    /// Where in `list` the element `anchor` names is. That is the first element with its id, or
    /// the first with its name, or the one at its place while the list keeps its length. A place
    /// needs only the list's length, and a short list is looked through. In a longer one an id or
    /// a name takes the list's partners into `partners` the first time, from the store or made, to
    /// be given back with [`Lists::keep`].
    fn find(&mut self, partners: &mut Option<Partners>, list: &[Value], anchor: &Anchor) -> Option<usize> {
        match anchor {
            Anchor::Index { at, of } => (list.len() == *of && *at < list.len()).then_some(*at),
            Anchor::Id(id) if list.len() <= SEARCHED_LEN => list.iter().position(|x| x.get("id").and_then(Value::as_u64) == Some(*id)),
            Anchor::Name(name) if list.len() <= SEARCHED_LEN => {
                list.iter().position(|x| x.get("name").and_then(Value::as_str) == Some(name.as_str()))
            }
            Anchor::Id(id) => partners.get_or_insert_with(|| self.take(list)).ids.get(id).copied(),
            Anchor::Name(name) => partners.get_or_insert_with(|| self.take(list)).names.get(name).copied(),
        }
    }

    /// The partners of `list`, out of the store until [`Lists::keep`] gives them back.
    fn take(&mut self, list: &[Value]) -> Partners {
        if let Some(partners) = list.first().and_then(|first| self.partners.remove(&std::ptr::from_ref(first))) {
            return partners;
        }
        #[cfg(test)]
        {
            self.indexed += 1;
        }
        Partners::of(list)
    }

    /// Give back the partners of `list` taken while it was looked in, if any were.
    fn keep(&mut self, list: &[Value], partners: Option<Partners>) {
        if let (Some(first), Some(partners)) = (list.first(), partners) {
            self.partners.insert(std::ptr::from_ref(first), partners);
        }
    }
}

/// Where a value is in the file (`components[Part 1].colour`), as the path it goes on from and its
/// last step. It borrows its keys and names, so going a level down copies nothing, and it is
/// written out only for a line that names it.
#[derive(Clone, Copy)]
struct Path<'a> {
    last: Option<(&'a Path<'a>, Step<'a>)>,
    /// Whether it is written as nothing (the top, or empty keys only), so a key after it takes no
    /// dot.
    blank: bool,
}

/// The top of the file.
const TOP: Path<'static> = Path { last: None, blank: true };

/// One step down a [`Path`]. A key, or an element of a list by its name, its id or its place.
#[derive(Clone, Copy)]
enum Step<'a> {
    Key(&'a str),
    Name(&'a str),
    Id(u64),
    At(usize),
}

impl<'a> Path<'a> {
    fn then(&'a self, step: Step<'a>) -> Path<'a> {
        Path { last: Some((self, step)), blank: self.blank && matches!(step, Step::Key("")) }
    }
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some((up, step)) = self.last else { return Ok(()) };
        fmt::Display::fmt(up, f)?;
        match step {
            Step::Key(key) if up.blank => f.write_str(key),
            Step::Key(key) => write!(f, ".{key}"),
            Step::Name(name) => write!(f, "[{name}]"),
            Step::Id(id) => write!(f, "[{id}]"),
            Step::At(at) => write!(f, "[{at}]"),
        }
    }
}

/// Lines that name fields (or elements that couldn't go back), while they fit in [`NAMED_BYTES`]
/// together, and how many more there were.
#[derive(Default)]
struct Named {
    lines: Vec<String>,
    bytes: usize,
    more: usize,
}

impl Named {
    /// Name a field with `line`, or count it when it doesn't fit after the lines before it, and
    /// count every field after that one too. A line that doesn't fit is written no further than
    /// the room left, so a huge name in it isn't copied.
    fn add(&mut self, line: fmt::Arguments<'_>) {
        if self.more == 0 {
            let mut room = Room { text: String::new(), left: NAMED_BYTES.saturating_sub(self.bytes) };
            if fmt::Write::write_fmt(&mut room, line).is_ok() {
                self.bytes = self.bytes.saturating_add(room.text.len());
                self.lines.push(room.text);
                return;
            }
        }
        self.more = self.more.saturating_add(1);
    }

    /// The lines, and after them how many more there were (`1 more field`, `12 more fields`).
    fn lines(mut self, one: &str, many: &str) -> Vec<String> {
        match self.more {
            0 => {}
            1 => self.lines.push(format!("1 more {one}")),
            more => self.lines.push(format!("{more} more {many}")),
        }
        self.lines
    }
}

/// Text written up to a number of bytes. A piece that would pass them is refused, not copied.
struct Room {
    text: String,
    left: usize,
}

impl fmt::Write for Room {
    fn write_str(&mut self, piece: &str) -> fmt::Result {
        self.left = self.left.checked_sub(piece.len()).ok_or(fmt::Error)?;
        self.text.push_str(piece);
        Ok(())
    }
}

/// Putting what was kept back into the file as this build writes it ([`NewerFile::restore_into`]).
#[derive(Default)]
struct Restore {
    named: Named,
    lists: Lists,
}

/// Put the kept fields back. An element that is gone was deleted here, its fields with it. One
/// whose place can't be told any more, and a field whose record is gone, are named by `put`.
fn apply(kept: &Kept, value: &mut Value, path: &Path<'_>, put: &mut Restore) {
    match kept {
        Kept::Field(_) => {}
        Kept::Fields(fields) => {
            let Some(object) = value.as_object_mut() else {
                put.named.add(format_args!("field `{path}`"));
                return;
            };
            for (key, sub) in fields {
                let inner = path.then(Step::Key(key));
                match (sub, object.get_mut(key)) {
                    (Kept::Field(v), None) => {
                        object.insert(key.clone(), v.clone());
                    }
                    (Kept::Field(_), Some(_)) => {}
                    (sub, Some(inner_value)) => apply(sub, inner_value, &inner, put),
                    // The list it was in is empty and not written: its elements were deleted.
                    (Kept::Items(_), None) => {}
                    (_, None) => put.named.add(format_args!("field `{inner}`")),
                }
            }
        }
        Kept::Items(items) => {
            let Some(list) = value.as_array_mut() else {
                put.named.add(format_args!("field `{path}`"));
                return;
            };
            // Taken when an element looks for its partner by id or name ([`Lists::find`]).
            let mut partners = None;
            for (anchor, sub) in items {
                let found = match put.lists.find(&mut partners, list, anchor) {
                    Some(place) => list.get_mut(place).map(|element| (place, element)),
                    None => None,
                };
                match (found, anchor) {
                    (Some((place, element)), _) => {
                        // Fields only go back where the element has none, so it can gain an id or
                        // a name but never change one.
                        let (had_id, had_name) = (element.get("id").is_some(), element.get("name").is_some());
                        apply(sub, element, path, put);
                        // A name put back here is found by the elements after it, as it was when
                        // each of them looked through the list. A short list is looked through
                        // still, and a long one's index learns it.
                        let id = element.get("id").filter(|_| !had_id).and_then(Value::as_u64);
                        let name = element.get("name").filter(|_| !had_name).and_then(Value::as_str).map(str::to_owned);
                        if list.len() > SEARCHED_LEN && (id.is_some() || name.is_some()) {
                            partners.get_or_insert_with(|| put.lists.take(list)).add(place, id, name.as_deref());
                        }
                    }
                    (None, Anchor::Index { at, .. }) => put.named.add(format_args!("element {at} of `{path}`")),
                    (None, _) => {}
                }
            }
            put.lists.keep(list, partners);
        }
    }
}

#[cfg(test)]
#[path = "newer_tests.rs"]
mod tests;
