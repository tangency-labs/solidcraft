//! What changed from one design to another (`file.diff`, #626). Features added, removed, moved,
//! renamed, suppressed or changed, the rollback marker, parameters, the sketches that changed and
//! imported files, then the names of the other sections that differ. Both files are read and
//! upgraded to this build's format, never evaluated, so a format upgrade alone is no change.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use sha2::{Digest, Sha256};
use solidcraft_doc::newer_file::NewerFile;
use solidcraft_doc::text_params::{TEXT_UNIT, TextParam};
use solidcraft_doc::{Document, Feature, FeatureKind, Param};
use solidcraft_io::limits::{self, Kind};
use solidcraft_sketch::{SPoint, Sketch};

use crate::schema::file::{
    DiffFeature, DiffFeatureAdded, DiffFeatureChanged, DiffFeatureMoved, DiffFeatures, DiffImport, DiffMarker, DiffParameter, DiffParameters,
    DiffPayload, DiffResult, DiffSketch, Pair,
};
use crate::{EngineError, Result};

/// How far a stored sketch point may move and still count as where it was, in mm. Evaluation
/// rounds stored solves to the same quantum.
const MOVED: f64 = 1e-6;

/// The sections of a design that are view state rather than the design itself.
const VIEW: [&str; 3] = ["browser_order", "named_views", "visibility"];

/// The design file at `path`, read and upgraded but not evaluated, and the format it says.
fn read(path: &str) -> Result<(Document, String)> {
    let failed = |e: &dyn std::fmt::Display| EngineError::Other(format!("{path}: {e}"));
    let bytes = limits::read(path, Kind::Design).map_err(|e| failed(&e))?;
    let doc = solidcraft_io::read_design(&bytes).map_err(|e| failed(&e))?;
    let format = solidcraft_io::design_format(&bytes).map_err(|e| failed(&e))?;
    Ok((doc, format))
}

/// What changed from the design file `a` to the design file `b`.
pub fn diff_files(a: &str, b: &str) -> Result<DiffResult> {
    let (da, format_a) = read(a)?;
    let (db, format_b) = read(b)?;
    let mut result = diff(&da, &db);
    result.format = Pair { a: format_a, b: format_b };
    for (path, doc) in [(a, &da), (b, &db)] {
        result.warnings.extend(doc.newer_file.iter().flat_map(|file| file.warnings()).map(|w| format!("{path}: {w}")));
    }
    Ok(result)
}

/// What changed from `a` to `b`. The formats are the caller's to fill in, since reading drops them.
pub fn diff(a: &Document, b: &Document) -> DiffResult {
    let (parameters, reordered) = parameters(a, b);
    let mut r = DiffResult { features: features(a, b), parameters, sketches: sketches(a, b), imports: imports(a, b), ..DiffResult::default() };
    (r.other, r.view) = other_sections(a, b);
    // Parameter rows that only changed order show in no entry, so their section is named.
    r.other.extend(reordered);
    r.other.sort();
    let (f, p) = (&r.features, &r.parameters);
    r.same = f.added.is_empty()
        && f.removed.is_empty()
        && f.moved.is_empty()
        && f.changed.is_empty()
        && f.marker.is_none()
        && p.added.is_empty()
        && p.removed.is_empty()
        && p.changed.is_empty()
        && r.sketches.is_empty()
        && r.imports.is_empty()
        && r.other.is_empty()
        && r.view.is_empty();
    r
}

fn named(f: &Feature) -> DiffFeature {
    DiffFeature { id: f.id, name: f.name.clone(), kind: f.kind.kind_key().to_owned() }
}

/// Each feature id's first feature with its place. A repeated id is hostile and keeps its first.
fn by_id(d: &Document) -> BTreeMap<u64, (usize, &Feature)> {
    let mut ids = BTreeMap::new();
    for (i, f) in d.features.iter().enumerate() {
        ids.entry(f.id).or_insert((i, f));
    }
    ids
}

/// The features of `d` in timeline order, each id once.
fn firsts<'a>(d: &'a Document, ids: &BTreeMap<u64, (usize, &'a Feature)>) -> Vec<(usize, &'a Feature)> {
    d.features.iter().enumerate().filter(|(i, f)| ids.get(&f.id).is_some_and(|(first, _)| first == i)).collect()
}

/// The feature a feature at place `i` follows, none at the start.
fn before(d: &Document, i: usize) -> Option<DiffFeature> {
    i.checked_sub(1).and_then(|j| d.features.get(j)).map(named)
}

/// The rollback marker, placed by the feature it follows. A marker at or past the end is none.
fn marker_at(d: &Document) -> DiffMarker {
    match d.marker {
        Some(n) if n < d.features.len() => before(d, n).map_or(DiffMarker::Start, DiffMarker::After),
        _ => DiffMarker::End,
    }
}

fn same_place(a: &DiffMarker, b: &DiffMarker) -> bool {
    match (a, b) {
        (DiffMarker::After(a), DiffMarker::After(b)) => a.id == b.id,
        (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// The other design's feature with `f`'s id and its place, when it is the same type of feature.
/// An id that names another type on each side is a removal and an addition.
fn shared<'a>(f: &Feature, other: &BTreeMap<u64, (usize, &'a Feature)>) -> Option<(usize, &'a Feature)> {
    other.get(&f.id).copied().filter(|(_, o)| o.kind.kind_key() == f.kind.kind_key())
}

fn features(a: &Document, b: &Document) -> DiffFeatures {
    let (ia, ib) = (by_id(a), by_id(b));
    let mut c = DiffFeatures::default();
    for (_, fa) in firsts(a, &ia) {
        if shared(fa, &ib).is_none() {
            if ib.contains_key(&fa.id) {
                c.reused_ids.push(fa.id);
            }
            c.removed.push(named(fa));
        }
    }
    for (i, fb) in firsts(b, &ib) {
        let Some((_, fa)) = shared(fb, &ia) else {
            c.added.push(DiffFeatureAdded { feature: named(fb), after: before(b, i) });
            continue;
        };
        let renamed_from = (fa.name != fb.name).then(|| fa.name.clone());
        let suppressed = (fa.suppressed != fb.suppressed).then_some(fb.suppressed);
        let keys = changed_keys(fa, fb);
        if renamed_from.is_some() || suppressed.is_some() || !keys.is_empty() {
            c.changed.push(DiffFeatureChanged { feature: named(fb), renamed_from, suppressed, keys });
        }
    }
    // The shared features in `a`'s order, with their places in `b`.
    let order: Vec<(u64, usize)> = firsts(a, &ia).into_iter().filter_map(|(_, fa)| shared(fa, &ib).map(|(at, _)| (fa.id, at))).collect();
    let moved = moved_ids(&order);
    for (i, fb) in firsts(b, &ib) {
        if let (true, Some(&(at, _))) = (moved.contains(&fb.id), ia.get(&fb.id)) {
            c.moved.push(DiffFeatureMoved { feature: named(fb), after_a: before(a, at), after_b: before(b, i) });
        }
    }
    let (ma, mb) = (marker_at(a), marker_at(b));
    if !same_place(&ma, &mb) {
        c.marker = Some(Pair { a: ma, b: mb });
    }
    c
}

/// The shared features that changed places. Those outside the longest run that kept its order
/// moved, found as a longest increasing subsequence of their places in `b`, taken in `a`'s order.
fn moved_ids(order: &[(u64, usize)]) -> BTreeSet<u64> {
    // The last of the smallest-ending run of each length, and each feature's predecessor in its run.
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = Vec::with_capacity(order.len());
    for (i, &(_, at)) in order.iter().enumerate() {
        let len = tails.partition_point(|&t| order.get(t).is_some_and(|&(_, x)| x < at));
        prev.push(len.checked_sub(1).and_then(|l| tails.get(l).copied()));
        match tails.get_mut(len) {
            Some(tail) => *tail = i,
            None => tails.push(i),
        }
    }
    let mut kept = BTreeSet::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        kept.extend(order.get(i).map(|&(id, _)| id));
        at = prev.get(i).copied().flatten();
    }
    order.iter().map(|&(id, _)| id).filter(|id| !kept.contains(id)).collect()
}

/// The feature without what other sections report (its sketch, its imported file's contents),
/// and without its name and suppression, which are reported on their own.
fn shell(f: &Feature) -> Feature {
    let mut f = Feature { name: String::new(), suppressed: false, ..f.clone() };
    match &mut f.kind {
        FeatureKind::Sketch { sketch, .. } => *sketch = Sketch::default(),
        FeatureKind::Import { step, records, .. } => {
            *step = "".into();
            *records = None;
        }
        FeatureKind::MeshImport { meshes, .. } => {
            for mesh in meshes {
                (mesh.positions, mesh.triangles, mesh.corner_normals) = (Vec::new().into(), Vec::new().into(), Vec::new().into());
            }
        }
        _ => {}
    }
    f
}

/// The design-file keys whose values differ between two versions of one feature.
fn changed_keys(a: &Feature, b: &Feature) -> Vec<String> {
    let (a, b) = (shell(a), shell(b));
    if a == b {
        return Vec::new();
    }
    let keys = differing_keys(&serde_json::to_value(&a).unwrap_or_default(), &serde_json::to_value(&b).unwrap_or_default(), &[]);
    // Never a silent change, even of something the design's JSON leaves out.
    if keys.is_empty() { vec!["other".to_owned()] } else { keys }
}

/// The top-level keys whose values differ between two JSON objects, sorted, leaving out `skip`.
fn differing_keys(a: &Value, b: &Value, skip: &[&str]) -> Vec<String> {
    let none = serde_json::Map::new();
    let (a, b) = (a.as_object().unwrap_or(&none), b.as_object().unwrap_or(&none));
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).filter(|k| !skip.contains(&k.as_str())).collect();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

/// A number parameter row as stored. `sketch` is a row a dimension or feature made (its `model` flag).
fn number(p: &Param, row: Option<usize>) -> DiffParameter {
    let source = if p.model { "sketch" } else { "user" };
    DiffParameter {
        name: p.name.clone(),
        source: source.to_owned(),
        expression: p.expr.clone(),
        unit: p.unit.clone(),
        comment: p.comment.clone(),
        row,
        ..DiffParameter::default()
    }
}

/// A text parameter row as stored, its text in place of an expression.
fn text(p: &TextParam, row: Option<usize>) -> DiffParameter {
    DiffParameter {
        name: p.name.clone(),
        source: "text".to_owned(),
        expression: p.text.clone(),
        unit: TEXT_UNIT.to_owned(),
        comment: p.comment.clone(),
        row,
        ..DiffParameter::default()
    }
}

fn number_name(p: &Param) -> &str {
    &p.name
}

fn text_name(p: &TextParam) -> &str {
    &p.name
}

/// Each row with its name and its place among the rows of that name, from 0.
fn places<T>(rows: &[T], name: fn(&T) -> &str) -> Vec<(&T, &str, usize)> {
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    rows.iter()
        .map(|r| {
            let n = name(r);
            let k = seen.entry(n).or_insert(0);
            *k += 1;
            (r, n, *k - 1)
        })
        .collect()
}

/// The rows of each name, in order.
fn by_name<'a, T>(places: &[(&'a T, &'a str, usize)]) -> BTreeMap<&'a str, Vec<&'a T>> {
    let mut rows: BTreeMap<&str, Vec<&T>> = BTreeMap::new();
    for &(r, n, _) in places {
        rows.entry(n).or_default().push(r);
    }
    rows
}

/// One section of stored parameter rows, compared row by row into `c`. Rows match by name and then
/// by their place among the rows of that name, so every row of a repeated name counts, whichever
/// one evaluation takes (reading keeps them all). True when the rows both sides have are in another
/// order, which no entry shows.
fn section<T: PartialEq>(a: &[T], b: &[T], name: fn(&T) -> &str, show: fn(&T, Option<usize>) -> DiffParameter, c: &mut DiffParameters) -> bool {
    if a == b {
        return false;
    }
    let (pa, pb) = (places(a, name), places(b, name));
    let (ra, rb) = (by_name(&pa), by_name(&pb));
    let count = |rows: &BTreeMap<&str, Vec<&T>>, n: &str| rows.get(n).map_or(0, Vec::len);
    // A name the section has once on each side keeps the reply as it was, with no row number.
    let row = |n: &str, k: usize| (count(&ra, n).max(count(&rb, n)) > 1).then_some(k + 1);
    let (mut order_a, mut order_b) = (Vec::new(), Vec::new());
    for &(r, n, k) in &pb {
        match ra.get(n).and_then(|rows| rows.get(k)) {
            None => c.added.push(show(r, row(n, k))),
            Some(&old) => {
                order_b.push((n, k));
                if old != r {
                    c.changed.push(Pair { a: show(old, row(n, k)), b: show(r, row(n, k)) });
                }
            }
        }
    }
    for &(r, n, k) in &pa {
        if k < count(&rb, n) {
            order_a.push((n, k));
        } else {
            c.removed.push(show(r, row(n, k)));
        }
    }
    order_a != order_b
}

/// The id of the feature and the input label that give each of `names` to a feature input of `d`,
/// when exactly one input has that name. A name no input or several inputs have is `None`.
fn inputs_named<'a>(d: &'a Document, names: &BTreeSet<&str>) -> BTreeMap<&'a str, Option<(u64, &'static str)>> {
    let mut found: BTreeMap<&str, Option<(u64, &'static str)>> = BTreeMap::new();
    for f in d.features.iter().filter(|f| f.param_names.iter().any(|n| names.contains(n.as_str()))) {
        for (n, (label, _, _)) in f.param_names.iter().zip(f.kind.inputs()) {
            if names.contains(n.as_str()) {
                found.entry(n.as_str()).and_modify(|one| *one = None).or_insert(Some((f.id, label)));
            }
        }
    }
    found
}

/// The comments of feature inputs, by name, compared into `c`. A feature input's expression is a
/// key of its feature, reported under `features`. Each entry names its feature by id and never
/// copies the feature's name, since one feature may have any number of inputs (a Fillet reads each
/// of its `edge_radii` as one) and nothing limits the length of its name. The reply then grows
/// with the files, not with a name's length once for each input.
fn comments(a: &Document, b: &Document, c: &mut DiffParameters) {
    let (ca, cb) = (&a.param_comments, &b.param_comments);
    let names: BTreeSet<&str> = ca
        .iter()
        .filter(|&(n, comment)| cb.get(n) != Some(comment))
        .chain(cb.iter().filter(|&(n, comment)| ca.get(n) != Some(comment)))
        .map(|(n, _)| n.as_str())
        .collect();
    if names.is_empty() {
        return;
    }
    let (fa, fb) = (inputs_named(a, &names), inputs_named(b, &names));
    let show = |n: &str, comment: &str, found: &BTreeMap<&str, Option<(u64, &str)>>| {
        let (feature, input) = found.get(n).copied().flatten().map_or((None, None), |(id, i)| (Some(id), Some(i.to_owned())));
        DiffParameter { name: n.to_owned(), source: "feature".to_owned(), feature, input, comment: comment.to_owned(), ..DiffParameter::default() }
    };
    for n in names {
        match (ca.get(n), cb.get(n)) {
            (Some(x), Some(y)) => c.changed.push(Pair { a: show(n, x, &fa), b: show(n, y, &fb) }),
            (None, Some(y)) => c.added.push(show(n, y, &fb)),
            (Some(x), None) => c.removed.push(show(n, x, &fa)),
            (None, None) => {}
        }
    }
}

/// The parameters as the design stores them, compared row by row (number parameters, text
/// parameters, then the comments of feature inputs), and the sections whose shared rows changed
/// order, to be named under `other`.
fn parameters(a: &Document, b: &Document) -> (DiffParameters, Vec<String>) {
    let mut c = DiffParameters::default();
    let mut reordered = Vec::new();
    if section(&a.params, &b.params, number_name, number, &mut c) {
        reordered.push("params".to_owned());
    }
    if section(&a.text_params, &b.text_params, text_name, text, &mut c) {
        reordered.push("text_params".to_owned());
    }
    comments(a, b, &mut c);
    (c, reordered)
}

fn sketches(a: &Document, b: &Document) -> Vec<DiffSketch> {
    let (ia, ib) = (by_id(a), by_id(b));
    let mut out = Vec::new();
    for (_, fb) in firsts(b, &ib) {
        let (FeatureKind::Sketch { sketch: sb, .. }, Some((_, Feature { kind: FeatureKind::Sketch { sketch: sa, .. }, .. }))) =
            (&fb.kind, ia.get(&fb.id))
        else {
            continue;
        };
        if sa == sb {
            continue;
        }
        let value = |s: &Sketch| serde_json::to_value(s).unwrap_or_default();
        // The id counters are bookkeeping, and a point that moved no more than the quantum is
        // where it was.
        let mut keys = differing_keys(&value(sa), &value(sb), &["counters"]);
        if same_points(&sa.points, &sb.points) {
            keys.retain(|k| k != "points");
        }
        if !keys.is_empty() {
            let count = |n: fn(&Sketch) -> usize| Pair { a: n(sa), b: n(sb) };
            let (points, curves, constraints) = (count(|s| s.points.len()), count(|s| s.curves.len()), count(|s| s.constraints.len()));
            out.push(DiffSketch { feature: named(fb), points, curves, constraints, keys });
        }
    }
    out
}

/// Whether two sketches' points are the same, a point that moved no more than [`MOVED`] counting
/// as where it was.
fn same_points(a: &[SPoint], b: &[SPoint]) -> bool {
    let near = |pa: &SPoint, pb: &SPoint| (pa.pos.x - pb.pos.x).abs() <= MOVED && (pa.pos.y - pb.pos.y).abs() <= MOVED;
    a.len() == b.len() && a.iter().zip(b).all(|(pa, pb)| near(pa, pb) && SPoint { pos: pb.pos, ..pa.clone() } == *pb)
}

fn payload(kind: &str, bytes: &[u8]) -> DiffPayload {
    DiffPayload { kind: kind.to_owned(), sha256: format!("{:x}", Sha256::digest(bytes)), bytes: bytes.len() as u64 }
}

/// What the design keeps of an imported file, as a container stores it. `None` for a feature that
/// imports nothing.
fn payloads(f: &Feature) -> Option<Vec<DiffPayload>> {
    match &f.kind {
        FeatureKind::Import { step, records, .. } => {
            let mut parts: Vec<DiffPayload> = (!step.is_empty()).then(|| payload("step", step.as_bytes())).into_iter().collect();
            parts.extend(records.as_deref().and_then(|r| solidcraft_kernel::records_text(r).ok()).map(|text| payload("records", text.as_bytes())));
            Some(parts)
        }
        FeatureKind::MeshImport { meshes, .. } => Some(
            meshes
                .iter()
                .map(|m| payload("mesh", &solidcraft_io::mesh_part_bytes(m).unwrap_or_else(|_| serde_json::to_vec(m).unwrap_or_default())))
                .collect(),
        ),
        _ => None,
    }
}

/// Whether two features keep the same imported contents, checked before anything is hashed.
fn same_contents(a: &Feature, b: &Feature) -> bool {
    match (&a.kind, &b.kind) {
        (FeatureKind::Import { step: s, records: r, .. }, FeatureKind::Import { step, records, .. }) => s == step && r == records,
        (FeatureKind::MeshImport { meshes: m, .. }, FeatureKind::MeshImport { meshes, .. }) => {
            m.len() == meshes.len()
                && m.iter().zip(meshes).all(|(x, y)| x.positions == y.positions && x.triangles == y.triangles && x.corner_normals == y.corner_normals)
        }
        _ => false,
    }
}

fn file_name(f: &Feature) -> String {
    match &f.kind {
        FeatureKind::Import { file, .. } | FeatureKind::MeshImport { file, .. } => file.clone(),
        _ => String::new(),
    }
}

/// Imported files, matched as features are, by id and type. An import whose id names another type
/// of feature on the other side is an addition or a removal, under its own name and file.
fn imports(a: &Document, b: &Document) -> Vec<DiffImport> {
    let (ia, ib) = (by_id(a), by_id(b));
    let mut out = Vec::new();
    for (_, fb) in firsts(b, &ib) {
        let fa = shared(fb, &ia).map(|(_, f)| f);
        if fa.is_some_and(|fa| same_contents(fa, fb)) {
            continue;
        }
        let (pa, pb) = (fa.and_then(payloads), payloads(fb));
        if pa != pb {
            out.push(DiffImport { feature: named(fb), file: file_name(fb), a: pa.unwrap_or_default(), b: pb.unwrap_or_default() });
        }
    }
    for (_, fa) in firsts(a, &ia).into_iter().filter(|(_, f)| shared(f, &ib).is_none()) {
        if let Some(pa) = payloads(fa) {
            out.push(DiffImport { feature: named(fa), file: file_name(fa), a: pa, b: Vec::new() });
        }
    }
    out
}

/// What a newer SolidCraft saved in the design that this build kept aside without reading it
/// (unknown fields and sections, parameters, container parts). The format it names is left out,
/// so a format difference alone is no change, and a design that keeps nothing aside compares as
/// one this build saved. Signatures are left out too, as they are for a file this build saved.
fn kept_aside(d: &Document) -> NewerFile {
    let mut kept = d.newer_file.as_deref().cloned().unwrap_or_default();
    (kept.app, kept.format, kept.reads_up_to) = ("", 0, 0);
    // The names of what was kept follow from it, and a newer feature is compared on the timeline.
    kept.unknown_changes.clear();
    kept.not_understood.clear();
    // Each signed save adds to the signature chain under `provenance/` with no edit. The parts of
    // a container have no order, so they are compared by name.
    kept.parts.retain(|(name, _)| !name.starts_with("provenance/"));
    kept.parts.sort_by(|(x, _), (y, _)| x.cmp(y));
    // The container note's format is the file's format as the container names it, and a note
    // that says nothing else is no note.
    if let Some(Value::Object(note)) = &mut kept.container {
        note.remove("format");
    }
    if kept.container.as_ref().and_then(Value::as_object).is_some_and(serde_json::Map::is_empty) {
        kept.container = None;
    }
    kept
}

/// The other sections of the design that differ, then the view state that does. The sections
/// reported above and bookkeeping (the format and the id counters) are left out. What a newer
/// SolidCraft saved that this build kept aside is named `newer_file` when it differs.
fn other_sections(a: &Document, b: &Document) -> (Vec<String>, Vec<String>) {
    let rest = |d: &Document| Document {
        features: Vec::new(),
        marker: None,
        params: Vec::new(),
        text_params: Vec::new(),
        param_comments: BTreeMap::new(),
        next_id: 0,
        next_component_color_index: 0,
        format: String::new(),
        format_pending: Vec::new(),
        newer_file: None,
        ..d.clone()
    };
    let (ra, rb) = (rest(a), rest(b));
    let newer = kept_aside(a) != kept_aside(b);
    if ra == rb && !newer {
        return (Vec::new(), Vec::new());
    }
    let value = |d: &Document| serde_json::to_value(d).unwrap_or_default();
    let mut keys = differing_keys(&value(&ra), &value(&rb), &[]);
    // What a design's JSON leaves out (images, points, plugin files) is compared as it is.
    let canvases = ("canvases", ra.canvases != rb.canvases);
    let clouds = ("point_clouds", ra.point_clouds != rb.point_clouds);
    for (key, differs) in [canvases, clouds, ("plugin_data", ra.plugin_data != rb.plugin_data), ("render", ra.render != rb.render)] {
        if differs && !keys.iter().any(|k| k == key) {
            keys.push(key.to_owned());
        }
    }
    if keys.is_empty() && ra != rb {
        keys.push("other".to_owned());
    }
    if newer {
        keys.push("newer_file".to_owned());
    }
    keys.sort();
    keys.into_iter().partition(|k| !VIEW.contains(&k.as_str()))
}

#[cfg(test)]
#[path = "design_diff_tests.rs"]
mod tests;
