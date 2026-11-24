//! Keep the parts of a newer drawing this build can't read, apart from the editable record.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, value::RawValue};
use solidcraft_geom::revision::{self, RevisionRow, Tag};

use super::image::{self, ImageFile, SheetImage};
use super::table::{Kind, SheetTable};
use super::view::{View, ViewKind};
use super::{
    Anchor, Balloon, Dimension, Drawing, FillPaint, Note, Owner, PatternSnapshot, RECORD_FIELDS, RevisionRecord, Sheet, SheetSketch, SketchFill,
    Source, Symbol, damaged, ids,
};
use crate::blob::{self, Blob};
use crate::format::{CheckNumbers as _, DRAWING, Newer};
use crate::{DocError, Result};

/// A newer drawing's format and the records it must carry through a save unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct NewerDrawing {
    /// The number the file says.
    pub format: u32,
    /// The number this build reads up to.
    pub reads_up_to: u32,
    /// The changes the file names that this build doesn't have.
    pub unknown_changes: Vec<String>,
    /// What this build couldn't read, by name.
    pub not_understood: Vec<String>,
    fields: Vec<(String, Retained)>,
    elements: BTreeMap<&'static str, RetainedList>,
    protected: BTreeSet<u64>,
    pub(super) reserved: BTreeSet<u64>,
    parts: BTreeMap<String, Blob>,
}

/// The parsed value is used for validation and references. The original text travels with it.
#[derive(Clone, Debug)]
struct Retained {
    value: Value,
    raw: Option<Arc<RawValue>>,
}

impl PartialEq for Retained {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.raw.as_deref().map(RawValue::get) == other.raw.as_deref().map(RawValue::get)
    }
}

impl Retained {
    fn new(value: Value, raw: Option<&RawValue>) -> Self {
        // An upgrade can move or rewrite a legacy record. Only reuse text for the same value.
        let raw = raw.filter(|raw| serde_json::from_str::<Value>(raw.get()).is_ok_and(|original| original == value));
        Self { value, raw: raw.map(|raw| Arc::from(raw.to_owned())) }
    }

    fn saved(&self) -> SavedValue<'_> {
        self.raw.as_deref().map_or_else(|| SavedValue::Value(self.value.clone()), SavedValue::Raw)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum SavedValue<'a> {
    Value(Value),
    Raw(&'a RawValue),
    List(Vec<SavedValue<'a>>),
    Object(BTreeMap<String, SavedValue<'a>>),
}

type RecordKey = (u64, Option<String>);

#[derive(Clone, Debug, PartialEq)]
struct RetainedList {
    limit: usize,
    // Original readable neighbours, in file order. Unread records have no anchor of their own.
    original: Vec<Option<RecordKey>>,
    held: Vec<(usize, Retained)>,
}

impl RetainedList {
    fn weave<'a, T>(&'a self, readable: Vec<Value>, live: impl Fn(Value) -> T, hold: impl Fn(&'a Retained) -> T) -> Vec<T> {
        let current: BTreeMap<_, _> = readable.iter().enumerate().filter_map(|(i, value)| record_key(value).map(|key| (key, i))).collect();
        let positions: Vec<_> = self.original.iter().map(|key| key.as_ref().and_then(|key| current.get(key)).copied()).collect();
        let mut next = None;
        let mut slots: Vec<_> = positions
            .iter()
            .rev()
            .map(|position| {
                next = position.iter().copied().chain(next).min();
                next
            })
            .collect();
        slots.reverse();
        let mut previous = None;
        for (slot, position) in slots.iter_mut().zip(positions) {
            previous = position.into_iter().chain(previous).max();
            *slot = slot.or(previous.map(|i| i + 1));
        }
        let mut remaining = readable.into_iter();
        let mut combined = Vec::with_capacity(remaining.len() + self.held.len());
        let mut copied = 0;
        for (i, record) in &self.held {
            let slot = slots.get(*i).copied().flatten().unwrap_or(0);
            let count = slot.saturating_sub(copied);
            combined.extend(remaining.by_ref().take(count).map(&live));
            copied += count;
            combined.push(hold(record));
        }
        combined.extend(remaining.map(live));
        combined
    }
}

type RawFields<'a> = BTreeMap<String, &'a RawValue>;

impl NewerDrawing {
    pub(super) fn new(newer: Newer) -> Self {
        Self {
            format: newer.format,
            reads_up_to: newer.reads_up_to,
            unknown_changes: newer.unknown_changes,
            not_understood: Vec::new(),
            fields: Vec::new(),
            elements: BTreeMap::new(),
            protected: BTreeSet::new(),
            reserved: BTreeSet::new(),
            parts: BTreeMap::new(),
        }
    }

    /// What to tell the user.
    pub fn notice(&self) -> String {
        format!(
            "Saved by a newer SolidCraft (format {}, this version reads up to {}). If something's missing, update.",
            self.format, self.reads_up_to
        )
    }

    pub(super) fn salvage(&mut self, value: &mut Value, text: Option<&str>) -> Result<()> {
        let raw: RawFields<'_> = text.map(serde_json::from_str).transpose().map_err(damaged)?.unwrap_or_default();
        let top = value.as_object_mut().ok_or_else(|| damaged("expected a JSON object"))?;
        let keys: Vec<_> = top.keys().filter(|key| !RECORD_FIELDS.contains(&key.as_str())).cloned().collect();
        for key in keys {
            if let Some(value) = top.remove(&key) {
                self.not_understood.push(format!("field `{key}`"));
                let record = Retained::new(value, raw.get(&key).copied());
                self.fields.push((key, record));
            }
        }

        // Accept the older inline image representation too. Keep all image records in the form
        // used by the writer before deciding which files, placements or logos can be read.
        if !top.contains_key(image::FILES) && top.get("images").is_some_and(Value::is_array) {
            image::set_aside(value);
        }
        super::sketch::check_raw_bounds(value).map_err(damaged)?;
        super::fill::check_raw(value).map_err(damaged)?;
        let mut plan = Salvage::default();
        plan.read(value, "sources", "source", super::MAX_SOURCES, |_: Source| [None, None])?;
        plan.read(value, "sheets", "sheet", super::MAX_SHEETS, |sheet: Sheet| [sheet.documents.map(|d| d.source), None])?;
        plan.read(value, "views", "view", super::view::MAX_VIEWS, |view: View| {
            let model = match view.kind {
                ViewKind::Base { source, .. } | ViewKind::Flat { source, .. } => Some(source),
                kind => kind.parent(),
            };
            [Some(view.sheet), model]
        })?;
        plan.read(value, "dimensions", "dimension", super::dimension::MAX_DIMENSIONS, |d: Dimension| [Some(d.view), None])?;
        plan.read(value, "balloons", "balloon", super::balloon::MAX_BALLOONS, |b: Balloon| [Some(b.view), b.parts_list])?;
        plan.read(value, "notes", "note", super::note::MAX_NOTES, |note: Note| {
            let owner = match note.anchor {
                Anchor::Sheet(sheet) => Some(sheet),
                anchor => anchor.view(),
            };
            [owner, None]
        })?;
        plan.read(value, "tables", "table", super::table::MAX_TABLES, |table: SheetTable| {
            let source = match table.kind {
                Kind::PartsList(list) => Some(list.of.source),
                Kind::Revision => None,
            };
            [Some(table.sheet), source]
        })?;
        plan.read(value, "sketches", "sketch", super::MAX_SHEET_SKETCHES, |sketch: SheetSketch| {
            let (Owner::Sheet(owner) | Owner::View(owner)) = sketch.owner;
            [Some(owner), None]
        })?;
        plan.read(value, "fills", "fill", super::MAX_SKETCH_FILLS, |fill: SketchFill| {
            let pattern = match fill.paint {
                FillPaint::Hatch { pattern, .. } => Some(pattern),
                FillPaint::Solid => None,
            };
            [Some(fill.sketch), pattern]
        })?;
        plan.read(value, "patterns", "pattern", super::MAX_PATTERN_SNAPSHOTS, |_: PatternSnapshot| [None, None])?;
        plan.read(value, "symbols", "symbol", super::symbol::MAX_SYMBOLS, |_: Symbol| [None, None])?;
        plan.read(value, image::FILES, "image", image::MAX_IMAGES, |_: ImageFile| [None, None])?;
        plan.read(value, image::PLACED, "placed image", image::MAX_SHEET_IMAGES, |image: SheetImage| [Some(image.sheet), Some(image.image)])?;
        plan.read(value, image::LOGOS, "title block logo", super::MAX_SHEETS * solidcraft_geom::title_block::MAX_CELLS, |logo: image::LogoCell| {
            [Some(logo.sheet), Some(logo.image)]
        })?;
        plan.read(value, "/revisions/rows", "revision row", revision::MAX_ROWS, |row: RevisionRow| [row.sheet, None])?;
        plan.read(value, "/revisions/issued", "issued revision", revision::MAX_ISSUED, |row: RevisionRow| [row.sheet, None])?;
        plan.read(value, "/revisions/tags", "revision tag", revision::MAX_ISSUED, |tag: Tag| {
            [Some(tag.sheet), tag.lost.is_none().then_some(tag.row)]
        })?;
        plan.follow_dependencies();
        plan.keep_unused_images(value);
        plan.take(value, &raw, self);

        if let Some(revisions) = value.get("revisions")
            && RevisionRecord::deserialize(revisions).map_or(true, |record| record.per_sheet)
            && let Some(mut revisions) = value.as_object_mut().and_then(|top| top.remove("revisions"))
        {
            // A newer revision header makes the entire record unreadable. Its rows and tags
            // still go back into it, and edits here must not replace it.
            for path in ["/revisions/rows", "/revisions/issued", "/revisions/tags"] {
                if let Some(elements) = self.elements.remove(path)
                    && let Some((_, field)) = path.rsplit_once('/')
                {
                    let readable = take_list(&mut revisions[field]);
                    revisions[field] = Value::Array(elements.weave(readable, |value| value, |record| record.value.clone()));
                }
            }
            self.fields.push(("revisions".into(), Retained::new(revisions, raw.get("revisions").copied())));
            self.not_understood.push("revision record".into());
        }
        Ok(())
    }

    pub(super) fn remember_references(&mut self, drawing: &Drawing) {
        let visible: BTreeSet<_> = drawing.ids().collect();
        // We cannot know the fields of an opaque record. Treat every exact drawing id in it
        // as a potential reference, and never give its ids to a newly authored object.
        for value in
            self.fields.iter().map(|(_, value)| value).chain(self.elements.values().flat_map(|list| list.held.iter().map(|(_, record)| record)))
        {
            visit_strings(&value.value, &mut |text| {
                if let Some(id) = ids::decode(text) {
                    if visible.contains(&id) {
                        self.protected.insert(id);
                    } else {
                        self.reserved.insert(id);
                    }
                } else if let Some(part) = blob::part(text) {
                    self.parts.entry(text.into()).or_insert(part);
                }
            });
        }
    }

    pub(super) fn check_edits(&self, drawing: &Drawing, visible: &BTreeSet<u64>) -> Result<()> {
        if let Some(id) = self.protected.difference(visible).next() {
            return Err(DocError::Invalid(format!(
                "object {} is referenced by data saved by a newer SolidCraft. Update SolidCraft to remove it",
                ids::encode(*id)
            )));
        }
        if let Some(id) = self.reserved.intersection(visible).next() {
            return Err(DocError::Invalid(format!("object {} is already used by data saved by a newer SolidCraft", ids::encode(*id))));
        }
        if self.fields.iter().any(|(field, _)| field == "revisions") && !super::no_revisions(&drawing.revisions) {
            return Err(DocError::Invalid("the revision record was saved by a newer SolidCraft. Update SolidCraft to edit it".into()));
        }
        for (&path, list) in &self.elements {
            let count = editable_count(drawing, path) + list.held.len();
            if count > list.limit {
                return Err(DocError::Invalid(format!(
                    "{path} must be a list of at most {} records, including those from a newer SolidCraft",
                    list.limit
                )));
            }
        }
        self.check_sketch_budget(drawing.sketches.iter().map(AsRef::as_ref))?;
        self.check_fill_budget(drawing.fills.iter().map(AsRef::as_ref))?;
        self.check_combined("patterns", drawing.patterns.iter().map(AsRef::as_ref))
    }

    pub(super) fn check_sketch_budget<'a>(&self, records: impl Iterator<Item = &'a SheetSketch>) -> Result<()> {
        self.check_combined("sketches", records)
    }

    pub(super) fn check_fill_budget<'a>(&self, records: impl Iterator<Item = &'a SketchFill>) -> Result<()> {
        self.check_combined("fills", records)
    }

    /// The reader applies these budgets to the entire file. An edit must leave
    /// room for the records it will write back alongside the editable ones.
    fn check_combined<'a, T: Serialize + 'a>(&self, path: &str, records: impl Iterator<Item = &'a T>) -> Result<()> {
        let Some(retained) = self.elements.get(path) else { return Ok(()) };
        let live = records.map(serde_json::to_value).collect::<serde_json::Result<Vec<_>>>().map_err(damaged)?;
        let combined = live.iter().chain(retained.held.iter().map(|(_, record)| &record.value));
        match path {
            "sketches" => super::sketch::check_raw_records(combined),
            "fills" => super::fill::check_raw_fills(combined),
            _ => super::fill::check_raw_patterns(combined),
        }
    }

    /// Container parts referred to by records this build leaves unread.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.keys().map(String::as_str)
    }

    pub(super) fn saved_record(&self, mut value: Value) -> Result<SavedValue<'_>> {
        let mut revisions = value.get_mut("revisions").map(Value::take).unwrap_or_else(|| serde_json::json!({"rows": []}));
        let mut saved_revisions = BTreeMap::new();
        let mut restored = BTreeMap::new();
        for (&path, list) in &self.elements {
            if let Some((_, field)) = path.rsplit_once('/') {
                let readable = take_list(&mut revisions[field]);
                saved_revisions.insert(field.into(), SavedValue::List(list.weave(readable, SavedValue::Value, Retained::saved)));
            } else {
                let readable = take_list(&mut value[path]);
                restored.insert(path.into(), SavedValue::List(list.weave(readable, SavedValue::Value, Retained::saved)));
            }
        }
        if !saved_revisions.is_empty() {
            merge_fields(revisions, &mut saved_revisions);
            restored.insert("revisions".into(), SavedValue::Object(saved_revisions));
        } else if let Some(revision) = value.get_mut("revisions") {
            *revision = revisions;
        }
        merge_fields(value, &mut restored);
        for (key, record) in &self.fields {
            restored.insert(key.clone(), record.saved());
        }
        for (name, part) in &self.parts {
            blob::keep_part(name, part);
        }
        Ok(SavedValue::Object(restored))
    }
}

fn merge_fields<'a>(value: Value, fields: &mut BTreeMap<String, SavedValue<'a>>) {
    if let Value::Object(object) = value {
        for (key, value) in object {
            fields.entry(key).or_insert(SavedValue::Value(value));
        }
    }
}

fn take_list(list: &mut Value) -> Vec<Value> {
    match list.take() {
        Value::Array(records) => records,
        _ => Vec::new(),
    }
}

fn editable_count(d: &Drawing, path: &str) -> usize {
    match path {
        "sources" => d.sources.len(),
        "sheets" => d.sheets.len(),
        "views" => d.views.len(),
        "dimensions" => d.dimensions.len(),
        "balloons" => d.balloons.len(),
        "notes" => d.notes.len(),
        "tables" => d.tables.len(),
        "sketches" => d.sketches.len(),
        "fills" => d.fills.len(),
        "patterns" => d.patterns.len(),
        "symbols" => d.symbols.len(),
        image::FILES => d.images.len(),
        image::PLACED => d.sheet_images.len(),
        image::LOGOS => d.logos().count(),
        "/revisions/rows" => d.revisions.rows().len(),
        "/revisions/issued" => d.revisions.issued().len(),
        "/revisions/tags" => d.revisions.tags().len(),
        _ => 0,
    }
}

fn visit_strings(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(text) => visit(text),
        Value::Array(list) => list.iter().for_each(|item| visit_strings(item, visit)),
        Value::Object(map) => map.iter().for_each(|(key, value)| {
            visit(key);
            visit_strings(value, visit);
        }),
        _ => {}
    }
}

type Position = (&'static str, usize);

/// Dependencies are indexed by drawing-global id, so a projected chain can be processed in
/// either order. Validation still rejects missing ids that were never in the file.
#[derive(Default)]
struct Salvage {
    lists: BTreeMap<&'static str, (&'static str, usize)>,
    unavailable: BTreeMap<Position, String>,
    ids: BTreeMap<Position, u64>,
    users: BTreeMap<u64, Vec<Position>>,
}

impl Salvage {
    fn read<T: for<'de> Deserialize<'de>>(
        &mut self,
        value: &Value,
        path: &'static str,
        one: &'static str,
        limit: usize,
        references: impl Fn(T) -> [Option<u64>; 2],
    ) -> Result<()> {
        let Some(field) = field(value, path) else { return Ok(()) };
        let records = field
            .as_array()
            .filter(|list| list.len() <= limit)
            .ok_or_else(|| damaged(format!("{path} must be a list of at most {limit} records")))?;
        self.lists.insert(path, (one, limit));
        for (index, raw) in records.iter().enumerate() {
            let position = (path, index);
            // An issued copy deliberately shares its row's id. Losing the copy doesn't remove
            // the row a tag names, and a row it can't read doesn't remove readable history.
            if path != "/revisions/issued"
                && let Some(id) = record_id(raw)
            {
                self.ids.insert(position, id);
            }
            let readable = DRAWING.check_numbers(raw, "properties").and_then(|()| {
                if path == "sketches" {
                    super::sketch::check_raw_support(raw).map_err(damaged)?;
                }
                T::deserialize(raw).map_err(damaged)
            });
            match readable {
                Ok(record) => {
                    for id in references(record).into_iter().flatten() {
                        self.users.entry(id).or_default().push(position);
                    }
                }
                Err(why) => {
                    self.unavailable.insert(position, why.to_string());
                }
            }
        }
        Ok(())
    }

    fn follow_dependencies(&mut self) {
        let mut lost: Vec<_> = self.unavailable.keys().filter_map(|position| self.ids.get(position).copied()).collect();
        while let Some(id) = lost.pop() {
            for &position in self.users.get(&id).into_iter().flatten() {
                if let std::collections::btree_map::Entry::Vacant(entry) = self.unavailable.entry(position) {
                    entry.insert(format!("refers to unavailable object {}", ids::encode(id)));
                    lost.extend(self.ids.get(&position).copied());
                }
            }
        }
    }

    fn keep_unused_images(&mut self, value: &Value) {
        let mut used = BTreeSet::new();
        for path in [image::PLACED, image::LOGOS] {
            for (index, raw) in list(value, path).into_iter().flatten().enumerate() {
                if !self.unavailable.contains_key(&(path, index)) {
                    used.extend(raw.get("image").and_then(Value::as_str).and_then(ids::decode));
                }
            }
        }
        for (index, file) in list(value, image::FILES).into_iter().flatten().enumerate() {
            if record_id(file).is_some_and(|id| !used.contains(&id)) {
                self.unavailable.entry((image::FILES, index)).or_insert_with(|| "has no readable placement".into());
            }
        }
    }

    fn take(self, value: &mut Value, raw: &RawFields<'_>, file: &mut NewerDrawing) {
        let mut lists = BTreeMap::<&str, BTreeMap<usize, String>>::new();
        for ((path, index), why) in self.unavailable {
            lists.entry(path).or_default().insert(index, why);
        }
        for (path, mut held) in lists {
            let list = if path.starts_with('/') { value.pointer_mut(path) } else { value.get_mut(path) };
            let Some(list) = list.and_then(Value::as_array_mut) else { continue };
            let Some(&(one, limit)) = self.lists.get(path) else { continue };
            let originals = raw_list(raw, path);
            let mut retained = RetainedList { limit, original: Vec::with_capacity(list.len()), held: Vec::with_capacity(held.len()) };
            for (index, raw) in std::mem::take(list).into_iter().enumerate() {
                if let Some(why) = held.remove(&index) {
                    retained.original.push(None);
                    let name = ["name", "path", "id", "cell"].into_iter().find_map(|key| raw.get(key).and_then(Value::as_str)).unwrap_or("unnamed");
                    let name: String = name.chars().take(80).collect();
                    file.not_understood.push(format!("{one} `{name}` ({why})"));
                    retained.held.push((index, Retained::new(raw, originals.get(index).copied())));
                } else {
                    retained.original.push(record_key(&raw));
                    list.push(raw);
                }
            }
            file.elements.insert(path, retained);
        }
    }
}

fn raw_list<'a>(raw: &RawFields<'a>, path: &str) -> Vec<&'a RawValue> {
    let raw = if let Some((_, field)) = path.rsplit_once('/') {
        raw.get("revisions").and_then(|value| serde_json::from_str::<RawFields<'a>>(value.get()).ok()).and_then(|fields| fields.get(field).copied())
    } else {
        raw.get(path).copied()
    };
    raw.and_then(|value| serde_json::from_str(value.get()).ok()).unwrap_or_default()
}

fn record_key(record: &Value) -> Option<RecordKey> {
    if let Some(id) = record_id(record) {
        return Some((id, None));
    }
    let sheet = record.get("sheet").and_then(Value::as_str).and_then(ids::decode)?;
    Some((sheet, Some(record.get("cell")?.as_str()?.into())))
}

fn record_id(record: &Value) -> Option<u64> {
    record.get("id").and_then(Value::as_str).and_then(ids::decode)
}

fn field<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') { value.pointer(path) } else { value.get(path) }
}

fn list<'a>(value: &'a Value, path: &str) -> Option<&'a [Value]> {
    field(value, path).and_then(Value::as_array).map(Vec::as_slice)
}
