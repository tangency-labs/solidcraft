//! A model as a design file keeps it: every feature's result and the state after it, so a design
//! opens without evaluating its timeline again (docs/design-container.md, "Model cache").
//!
//! The records are JSON; the kernel bodies they hold are a separate list ([`SavedModel::bodies`]),
//! which the file stores in SolidCraft's exact native B-rep encoding. A body that several states
//! hold (one that later features pass through unchanged) is recorded once, as are its face names
//! and the solved sketches the states share. Face names are kept as worked out, not as the
//! history they were worked out from: a model taken from a file names every face as the model
//! that was saved did. A file without room for every state keeps fewer
//! ([`SavedModel::thinned`]); opening evaluates the features whose state it left out again.

use super::*;
use crate::history_names::{FaceInfo, Names};
use serde_json::{Value as Json, json};
use std::collections::HashMap;

/// A model's results and states as records, and the kernel bodies they refer to by index.
pub struct SavedModel {
    pub records: Json,
    pub bodies: Vec<Body>,
}

/// Records refer to these at most.
const MAX_ITEMS: usize = 1_000_000;
/// Bodies, sketches, sheets, sketch keys and named face points all the states hold together, at
/// most (a state is a list of clones).
const MAX_HELD: usize = 50_000_000;
/// Longest text a record holds (bytes). A feature's warning can quote every generated name of
/// the edges it picked: 122,701 characters in one design, whose saved model was refused, so it
/// evaluated again on every open (#8541).
const MAX_TEXT: usize = 1 << 24;

/// Whether two tap markings are the same to the bit: then they write the same record. (`==` takes
/// 0.0 and -0.0 for the same number, whose records differ.) Every field is named, so a new one
/// cannot be left out.
fn same_marking(a: &ThreadInfo, b: &ThreadInfo) -> bool {
    let numbers = |t: &ThreadInfo| {
        let ThreadInfo {
            feature: _,
            body: _,
            designation: _,
            major_diameter,
            radius,
            pitch,
            internal: _,
            axis_point,
            axis,
            start,
            end,
            form: _,
            hand: _,
            starts: _,
        } = t;
        [*major_diameter, *radius, *pitch, axis_point.x, axis_point.y, axis_point.z, axis.x, axis.y, axis.z, *start, *end]
    };
    a.feature == b.feature
        && a.body == b.body
        && a.designation == b.designation
        && a.internal == b.internal
        && a.form == b.form
        && a.hand == b.hand
        && a.starts == b.starts
        && numbers(a).iter().zip(numbers(b).iter()).all(|(x, y)| x.to_bits() == y.to_bits())
}

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn unhex(v: &Json) -> Result<u64> {
    v.as_str().and_then(|s| u64::from_str_radix(s, 16).ok()).ok_or_else(|| bad("saved model: key"))
}

/// A sketch-key set's list `key` (`set` or `drop`).
fn keys_list<'a>(set: &'a Json, key: &str) -> Result<&'a Vec<Json>> {
    set.get(key).and_then(Json::as_array).filter(|l| l.len() <= MAX_ITEMS).ok_or_else(|| bad("saved model: sketch keys"))
}

fn bad(what: &str) -> DocError {
    DocError::Invalid(what.into())
}

/// A design holding a face or edge name saved before format 14 (marked
/// [`crate::naming::LEGACY_MARK`]): it saves without a model.
fn has_legacy_names(doc: &Document) -> bool {
    let mark = format!("\"{}", crate::naming::LEGACY_MARK);
    doc.features.iter().any(|f| match &f.kind {
        FeatureKind::Import { .. } => f.face_names.iter().any(|n| n.starts_with(crate::naming::LEGACY_MARK)),
        _ => serde_json::to_string(f).is_ok_and(|j| j.contains(&mark)),
    })
}

/// What tells two model bodies apart in a record: a body a feature passes through unchanged is a
/// clone sharing its mesh cell and names, so this finds it without writing its record again.
#[derive(PartialEq, Eq, Hash)]
struct BodyKey {
    mesh: usize,
    names: usize,
    imported: usize,
    key: u64,
    feature: u64,
    component: u64,
    transferred: bool,
    envelope: bool,
    frame_member_line: Option<Arc<str>>,
    name: String,
    source_name: Option<String>,
    source_transform: [u64; 16],
}

impl BodyKey {
    /// Deduplicate states only when their geometry, names and member provenance agree.
    fn of(b: &ModelBody) -> Self {
        let mut source_transform = [0; 16];
        for (bits, v) in source_transform.iter_mut().zip(b.source_transform.iter().flatten()) {
            *bits = v.to_bits();
        }
        BodyKey {
            mesh: Arc::as_ptr(&b.mesh) as usize,
            names: b.names.id(),
            imported: b.imported.as_ref().map_or(0, |l| Arc::as_ptr(l) as usize),
            key: b.key,
            feature: b.feature,
            component: b.component,
            transferred: b.transferred,
            envelope: b.envelope,
            frame_member_line: b.frame_member_line.clone(),
            name: b.name.clone(),
            source_name: b.source_name.clone(),
            source_transform,
        }
    }
}

/// The place of a sheet's record in `list`, written the first time a state holds it. Keyed by the
/// sheet written as text, straight from the sheet: every state asks for each of its sheets, and
/// building the record (a JSON tree) each time only to find it written already took most of a
/// sheet-metal design's save (906 states, 300 sheets: 0.8 s). Two sheets are the same text exactly
/// when they are the same record.
fn shared<T: serde::Serialize>(list: &mut Vec<Json>, index: &mut HashMap<String, usize>, sheet: &T) -> Option<usize> {
    let key = serde_json::to_string(sheet).ok()?;
    if let Some(i) = index.get(&key) {
        return Some(*i);
    }
    list.push(serde_json::to_value(sheet).ok()?);
    index.insert(key, list.len() - 1);
    Some(list.len() - 1)
}

/// Indices as runs of consecutive ones (`[first, count]`): a state lists mostly the bodies of the
/// state before it, in order.
fn runs(indices: &[usize]) -> Vec<[usize; 2]> {
    let mut out: Vec<[usize; 2]> = Vec::new();
    for &i in indices {
        match out.last_mut() {
            Some([first, count]) if *first + *count == i => *count += 1,
            _ => out.push([i, 1]),
        }
    }
    out
}

#[derive(Default)]
struct Writer {
    bodies: Vec<Body>,
    model_bodies: Vec<Json>,
    model_body_index: HashMap<BodyKey, usize>,
    names: Vec<Json>,
    names_index: HashMap<usize, usize>,
    sketches: Vec<Json>,
    sketch_index: HashMap<usize, usize>,
    /// Sheets made from surfaces by their record: most features leave the sheets as they were, so
    /// the states share them (writing them with every state made a sheet-metal design's model too
    /// large to keep).
    surface_sheets: Vec<Json>,
    surface_sheet_index: HashMap<String, usize>,
    /// Tap markings (cosmetic threads) by their record, as the sheets: every state lists the
    /// markings of all the features before it, so writing them with each state grew with the
    /// square of the features (400 marked cylinders: 34 of a 37 MB manifest, past its limit).
    threads: Vec<Json>,
    thread_index: HashMap<String, usize>,
    /// The state written last and the places of its markings: a state mostly keeps the markings
    /// of the one before it, which are found again without writing them as text.
    last_threads: Option<(Arc<ModelState>, Vec<usize>)>,
    states: Vec<Json>,
    state_index: HashMap<usize, usize>,
    /// Each kernel body's place in `bodies`, by its record.
    body_index: HashMap<usize, usize>,
    /// The rounds that made bodies (`Body::rounds`): the body, the body they were made on (both
    /// places in `bodies`), and the edges they rounded there with their radii.
    rounds: Vec<Json>,
    /// The states' sketch keys, each distinct map once, written as the changes from the set
    /// written before it: every state used to repeat the whole map (one entry per solved sketch),
    /// 85 % of a 5,484-feature design's records, which then did not fit the cache's manifest.
    sketch_key_sets: Vec<Json>,
    sketch_key_index: HashMap<BTreeMap<u64, u64>, usize>,
    last_sketch_keys: Option<(BTreeMap<u64, u64>, usize)>,
    /// Face points as resolved before each feature ran, each distinct value once. A state's
    /// map includes earlier features too; writing those points per state grows quadratically.
    named_faces: Vec<Json>,
    named_face_index: HashMap<(u64, Vec<[u64; 3]>), usize>,
}

impl Writer {
    fn named_faces(&mut self, faces: &BTreeMap<u64, Vec<Vec3>>) -> Vec<[usize; 2]> {
        let mut indices = Vec::with_capacity(faces.len());
        for (&feature, points) in faces {
            // Signed zero writes differently, so share only points that are identical to the bit.
            let key = (feature, points.iter().map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]).collect());
            let at = match self.named_face_index.get(&key) {
                Some(at) => *at,
                None => {
                    let at = self.named_faces.len();
                    self.named_faces.push(json!([feature, points]));
                    self.named_face_index.insert(key, at);
                    at
                }
            };
            indices.push(at);
        }
        // A map has no order. Reordering features can put their ids in reverse timeline order;
        // sorting by table position keeps the growing set as runs even in that case.
        indices.sort_unstable();
        runs(&indices)
    }

    /// The place of `keys` among the sketch-key sets, written the first time a state holds them
    /// (as the changes from the set written last: a later state mostly adds a sketch or two).
    fn sketch_key_set(&mut self, keys: &BTreeMap<u64, u64>) -> usize {
        if let Some(i) = self.sketch_key_index.get(keys) {
            return *i;
        }
        let record = match &self.last_sketch_keys {
            Some((base, at)) => {
                let set: Vec<Json> = keys.iter().filter(|(f, k)| base.get(f) != Some(k)).map(|(f, k)| json!([f, hex(*k)])).collect();
                let drop: Vec<u64> = base.keys().filter(|f| !keys.contains_key(f)).copied().collect();
                json!({"base": at, "set": set, "drop": drop})
            }
            None => json!({"set": keys.iter().map(|(f, k)| json!([f, hex(*k)])).collect::<Vec<_>>(), "drop": []}),
        };
        self.sketch_key_sets.push(record);
        let i = self.sketch_key_sets.len() - 1;
        self.sketch_key_index.insert(keys.clone(), i);
        self.last_sketch_keys = Some((keys.clone(), i));
        i
    }

    fn names(&mut self, b: &ModelBody) -> usize {
        let id = b.names.id();
        if let Some(i) = self.names_index.get(&id) {
            return *i;
        }
        let names = crate::naming::names(b);
        // A Split Face piece's former name follows (only then, so other records read as before).
        let info: Vec<Json> = names
            .info
            .iter()
            .map(|i| match &i.former {
                Some(former) => json!([i.aliases, i.bounds, i.verified, i.synonyms, former]),
                None => json!([i.aliases, i.bounds, i.verified, i.synonyms]),
            })
            .collect();
        self.names.push(json!({"faces": *names.faces, "info": info}));
        self.names_index.insert(id, self.names.len() - 1);
        self.names.len() - 1
    }

    fn model_body(&mut self, b: &ModelBody) -> Option<usize> {
        let key = BodyKey::of(b);
        if let Some(i) = self.model_body_index.get(&key) {
            return Some(*i);
        }
        let names = self.names(b);
        let mut r = json!({
            "name": b.name, "feature": b.feature, "component": b.component, "key": hex(b.key), "names": names,
        });
        if let Some(depth) = b.names.depth() {
            r["depth"] = json!(depth);
        }
        if let Some(source) = &b.source_name {
            r["source_name"] = json!(source);
        }
        if b.source_transform != crate::IDENTITY {
            r["source_transform"] = json!(b.source_transform);
        }
        if let Some(line) = &b.frame_member_line {
            r["frame_member_line"] = json!(line.as_ref());
        }
        if b.transferred {
            r["transferred"] = json!(true);
        }
        if let Ok(data) = b.round_member() {
            r["round_member"] = serde_json::to_value(data).ok()?;
        }
        if b.envelope {
            r["envelope"] = json!(true);
        }
        if let Some(look) = &b.imported {
            let faces: Vec<Json> = look.faces.iter().map(|(name, (color, opacity))| json!([name, color, opacity])).collect();
            r["imported"] = json!({"color": look.color, "opacity": look.opacity, "faces": faces});
            if look.name.is_some() || !look.face_names.is_empty() {
                r["imported"]["name"] = json!(look.name);
                r["imported"]["face_names"] = json!(look.face_names);
            }
        }
        // The kernel body's own encoding keeps no colour names: the file's names for its paint.
        if let Some(paint) = b.body.paint() {
            let faces: Vec<Json> = paint.faces.iter().filter_map(|f| Some(json!([f.face, f.name.as_ref()?]))).collect();
            if paint.name.is_some() || !faces.is_empty() {
                r["paint_names"] = json!({"body": paint.name, "faces": faces});
            }
        }
        r["body"] = json!(self.body(&b.body));
        self.model_bodies.push(r);
        self.model_body_index.insert(key, self.model_bodies.len() - 1);
        Some(self.model_bodies.len() - 1)
    }

    /// The place of a model body's kernel body: its own (a placed copy shares its record with
    /// the part's other copies, but not its display), with the bodies the rounds that made it
    /// were made on, each written once though no state holds it: a later round at a corner where
    /// they meet blends them again, in a model opened from the file as in one evaluated anew.
    fn body(&mut self, body: &Body) -> usize {
        let key = |b: &Body| std::ptr::from_ref(b.record()).addr();
        self.bodies.push(body.clone());
        let first = self.bodies.len() - 1;
        self.body_index.entry(key(body)).or_insert(first);
        let (mut at, mut of) = (body.clone(), first);
        // Down the rounds, until a body made on one already written.
        while let Some((before, edges)) = at.rounds().map(|(b, e)| (b.clone(), e.to_vec())) {
            let (i, new) = match self.body_index.get(&key(&before)) {
                Some(i) => (*i, false),
                None => {
                    self.bodies.push(before.clone());
                    self.body_index.insert(key(&before), self.bodies.len() - 1);
                    (self.bodies.len() - 1, true)
                }
            };
            self.rounds.push(json!({"body": of, "before": i, "edges": edges}));
            if !new {
                break;
            }
            (at, of) = (before, i);
        }
        first
    }

    fn sketch(&mut self, s: &Arc<SolvedSketch>) -> Option<usize> {
        let id = Arc::as_ptr(s) as usize;
        if let Some(i) = self.sketch_index.get(&id) {
            return Some(*i);
        }
        let r = json!({
            "feature": s.feature, "name": s.name, "plane": serde_json::to_value(s.plane).ok()?,
            "sketch": serde_json::to_value(&s.sketch).ok()?, "report": serde_json::to_value(&s.report).ok()?,
        });
        self.sketches.push(r);
        self.sketch_index.insert(id, self.sketches.len() - 1);
        Some(self.sketches.len() - 1)
    }

    fn surface_sheet(&mut self, sheet: &crate::SurfaceSheet) -> Option<usize> {
        shared(&mut self.surface_sheets, &mut self.surface_sheet_index, sheet)
    }

    /// The places of `st`'s markings among the written ones. A marking at the same place in the
    /// state written last, the same to the bit, is that one (no text built); any other is keyed by
    /// its text, as a sheet is.
    fn threads(&mut self, st: &Arc<ModelState>) -> Option<Vec<usize>> {
        let mut out = Vec::with_capacity(st.threads.len());
        for (i, t) in st.threads.iter().enumerate() {
            let before = self.last_threads.as_ref().and_then(|(s, at)| Some((s.threads.get(i)?, *at.get(i)?)));
            if let Some((_, at)) = before.filter(|(b, _)| same_marking(b, t)) {
                out.push(at);
                continue;
            }
            let key = serde_json::to_string(t).ok()?;
            let at = match self.thread_index.get(&key) {
                Some(at) => *at,
                None => {
                    self.threads.push(serde_json::to_value(t).ok()?);
                    self.thread_index.insert(key, self.threads.len() - 1);
                    self.threads.len() - 1
                }
            };
            out.push(at);
        }
        self.last_threads = Some((st.clone(), out.clone()));
        Some(out)
    }

    fn state(&mut self, st: &Arc<ModelState>) -> Option<usize> {
        let id = Arc::as_ptr(st) as usize;
        if let Some(i) = self.state_index.get(&id) {
            return Some(*i);
        }
        let bodies = st.bodies.iter().map(|b| self.model_body(b)).collect::<Option<Vec<_>>>()?;
        let sketches = st.sketches.iter().map(|s| self.sketch(s)).collect::<Option<Vec<_>>>()?;
        let surface_sheets = st.surface_sheets.iter().map(|s| self.surface_sheet(s)).collect::<Option<Vec<_>>>()?;
        let threads = self.threads(st)?;
        // Shape identity is process-local. Persist the decision in each timeline state,
        // then restore its shared shape from that state's body after decoding the cache.
        let members = st
            .frame_members
            .iter()
            .map(|m| {
                let mut record = serde_json::to_value(m).ok()?;
                record["unmodified"] = json!(st.body(&m.body).is_some_and(|b| b.feature == m.feature && !b.transferred && m.is_unchanged(&b.body)));
                Some(record)
            })
            .collect::<Option<Vec<_>>>()?;
        let r = json!({
            "bodies": runs(&bodies), "sketches": runs(&sketches), "body_counter": st.body_counter,
            "reserved_body_names": st.reserved_body_names.iter().collect::<Vec<_>>(),
            "surface_sheets": surface_sheets, "construct": serde_json::to_value(&st.construct).ok()?,
            "threads": runs(&threads),
            "frame_members": members,
            "sketch_key_set": self.sketch_key_set(&st.sketch_keys),
            "named_faces": self.named_faces(&st.named_faces),
            "blend_edges": serde_json::to_value(&st.blend_edges).ok()?, "fitted": serde_json::to_value(&st.fitted).ok()?,
            "tapered": serde_json::to_value(&st.tapered).ok()?,
            "pattern_left_out": serde_json::to_value(&st.pattern_left_out).ok()?,
        });
        self.states.push(r);
        self.state_index.insert(id, self.states.len() - 1);
        Some(self.states.len() - 1)
    }
}

impl Model {
    /// The model as a design file keeps it: `None` unless it is `doc`'s evaluation without an
    /// error and `doc` keeps no names saved before format 14. Works out
    /// the face names not asked for yet (a saved model keeps them as worked out).
    ///
    /// An import whose SMLib check of its parts is still running is saved with what the check
    /// found: this waits for it ([`Self::saved_now`] does not). `None` when the check healed a
    /// part: the states are of the import as first shown ([`PendingChecks::settle`]).
    pub fn saved(&self, doc: &Document) -> Option<SavedModel> {
        let (mut saved, pending) = self.saved_now(doc)?;
        pending.settle(&mut saved).then_some(saved)
    }

    /// [`Self::saved`] when no import's SMLib check of its parts is still running
    /// ([`Self::checking_imports`]); `None` while one is, without waiting for it (#20133). A
    /// check can take many times as long as the import's read, and what it finds (a warning, a
    /// healed part) is part of the model a design keeps: a model kept before the check ends
    /// would open without it. A design saved meanwhile keeps no model, and is rebuilt when it
    /// opens, as a design saved without the cache is; a save once the check has ended keeps it.
    pub fn saved_if_checked(&self, doc: &Document) -> Option<SavedModel> {
        if self.checking_imports() {
            return None;
        }
        self.saved(doc)
    }

    /// Why [`Self::saved_if_checked`] would leave the model cache out for `doc`, if it cannot
    /// save one right now (`None` when the model is ready to save or a feature has an error,
    /// which the caller names specifically).
    pub fn cache_skip_reason(&self, doc: &Document) -> Option<&'static str> {
        if self.checking_imports() {
            return Some("an imported file's parts were still being checked (a save once the check ends keeps it)");
        }
        if self.results.is_empty() {
            return Some("the model has not been evaluated yet");
        }
        if self.checking_features() {
            return Some("a feature's geometry check was still running (a save once it ends keeps it)");
        }
        if self.results.iter().any(|r| r.error.is_some()) {
            return None;
        }
        if !self.is_current(doc) {
            return Some("the model was not up to date with the design");
        }
        if has_legacy_names(doc) {
            return Some("the design still uses legacy face or edge names");
        }
        if self.results.iter().any(|r| {
            r.import_report.as_ref().is_some_and(|report| {
                !report.checks.is_empty() && report.checks.heals().is_some_and(|h| self.import_digests.heal_digest(r.id) != Some(h.digest()))
            })
        }) {
            return Some("an imported file's parts were healed after evaluation (a save once it updates keeps it)");
        }
        None
    }

    /// [`Self::saved`] without waiting for the imports' checks: the records hold what each import
    /// found before its check, and [`PendingChecks::settle`] adds what the check finds.
    pub fn saved_now(&self, doc: &Document) -> Option<(SavedModel, PendingChecks)> {
        // Nor while the SMLib checks of features still run ([`Model::set_background_checks`]): a
        // model kept before they pass would open with results no check has passed.
        if self.results.is_empty()
            || self.checking_features()
            || self.results.iter().any(|r| r.error.is_some())
            || !self.is_current(doc)
            || has_legacy_names(doc)
        {
            return None;
        }
        kernel::guard("save the model", || {
            let mut w = Writer::default();
            let mut features = Vec::with_capacity(self.results.len());
            let mut pending = Vec::new();
            for (k, (feature, r)) in doc.features.iter().zip(&self.results).enumerate() {
                if let Some(report) = r.import_report.as_ref().filter(|i| !i.checks.is_empty()) {
                    pending.push((k, self.import_digests.heal_digest(feature.id), report.clone()));
                }
                let state = w.state(&r.state).ok_or_else(|| kernel::KernelError::Invalid("not kept".into()))?;
                let mut f = json!({"id": r.id, "fingerprint": hex(r.fingerprint), "ms": r.ms, "skipped": r.skipped, "state": state});
                // The pose a pose-only feature was evaluated at, which its fingerprint leaves out:
                // opening finds it the same and keeps the result (#12139).
                if pose_only(feature)
                    && let Some(pose) = self.poses.get(&r.id)
                {
                    f["pose"] = json!(hex(*pose));
                }
                if let Some(w) = &r.warning {
                    f["warning"] = json!(w);
                }
                if let Some(report) = &r.import_report {
                    f["import_report"] = serde_json::to_value(report.as_ref()).map_err(|e| kernel::KernelError::Invalid(e.to_string()))?;
                }
                features.push(f);
            }
            // The check mode the results were evaluated under: they are another mode's results.
            let mut records = json!({
                "features": features, "states": w.states, "model_bodies": w.model_bodies, "names": w.names, "sketches": w.sketches,
                "surface_sheets": w.surface_sheets, "threads": w.threads, "validation": self.validation().id(),
                "rounds": w.rounds,
                "sketch_key_sets": w.sketch_key_sets,
                "named_faces": w.named_faces,
            });
            // Evaluated with the tolerance heal off: recorded only then, so the models saved
            // with it on (the default) are as before.
            if !self.heals()
                && let Some(out) = records.as_object_mut()
            {
                out.insert("tolerance_heal".into(), json!(false));
            }
            // Healed imports' digests: recorded only when any import was healed, so models without
            // healed imports keep the same manifest layout.
            if !self.import_digests.heals.is_empty()
                && let Some(out) = records.as_object_mut()
            {
                let mut heals: Vec<(u64, String)> = self.import_digests.heals.iter().map(|(&id, (_, h))| (id, hex(h.digest()))).collect();
                heals.sort_by_key(|(id, _)| *id);
                out.insert("import_heals".into(), json!(heals));
            }
            if doc.features.iter().any(|f| matches!(f.kind, FeatureKind::FrameMember { .. } | FeatureKind::FrameCope { .. })) {
                records["round_member_records"] = json!(1);
            }
            Ok((SavedModel { records, bodies: w.bodies }, PendingChecks(pending)))
        })
        .ok()
    }
}

/// Imports of a saved model ([`Model::saved_now`]) whose SMLib check of their parts may still
/// be running: by feature record, the heal digest already evaluated into the model (if any),
/// and the report that holds the check.
pub struct PendingChecks(Vec<(usize, Option<u64>, Arc<kernel::ImportReport>)>);

impl PendingChecks {
    /// No import's check is left to wait for.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Wait for the checks and write what they found into `saved`'s feature records: each
    /// import's report and warning as an import that waited for its check has them. A check a
    /// token stopped leaves its record as it was.
    ///
    /// Returns whether `saved` stands: not when a check healed a part that the saved states do
    /// not yet carry ([`kernel::ImportChecks::heals`]), since its states hold the import as it
    /// was first shown, and the model evaluates it again as its healed bodies (with other
    /// fingerprints). `saved` is then left as it was.
    #[must_use]
    pub fn settle(self, saved: &mut SavedModel) -> bool {
        if self.0.iter().any(|(_, known, report)| report.settled().is_some() && report.checks.heals().is_some_and(|h| *known != Some(h.digest()))) {
            return false;
        }
        for (k, _, report) in self.0 {
            let Some(settled) = report.settled() else { continue };
            let Some(f) = saved.records.get_mut("features").and_then(|f| f.get_mut(k)).and_then(Json::as_object_mut) else { continue };
            match crate::eval::import_warning(&settled.issues.iter().map(|i| i.detail.as_str()).collect::<Vec<_>>()) {
                Some(w) => f.insert("warning".into(), json!(w)),
                None => f.remove("warning"),
            };
            match serde_json::to_value(&settled) {
                Ok(v) if !settled.issues.is_empty() => f.insert("import_report".into(), v),
                _ => f.remove("import_report"),
            };
        }
        true
    }
}

/// A face's [`FaceInfo`] as the model cache keeps it: its aliases, bounds, whether exact, its
/// synonyms and, for a Split Face piece, the name it had before (absent in records without one).
#[derive(serde::Deserialize)]
struct SavedFaceInfo(Vec<String>, std::collections::BTreeSet<String>, bool, Vec<String>, #[serde(default)] Option<String>);

impl SavedModel {
    /// The states the records hold, every record checked.
    fn states(&self) -> Result<Vec<Arc<ModelState>>> {
        let bodies = &self.bodies_with_rounds()?;
        let records = &self.records;
        let list = |key: &str| records.get(key).and_then(Json::as_array).filter(|l| l.len() <= MAX_ITEMS).ok_or_else(|| bad("saved model: list"));
        let index = |v: Option<&Json>, len: usize| {
            v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < len).ok_or_else(|| bad("saved model: index"))
        };
        let text =
            |v: Option<&Json>| v.and_then(Json::as_str).filter(|s| s.len() <= MAX_TEXT).map(str::to_owned).ok_or_else(|| bad("saved model: text"));
        let parse = |v: Option<&Json>| v.cloned().ok_or_else(|| bad("saved model: missing"));
        let mut names = Vec::new();
        for n in list("names")? {
            let faces: Vec<String> = serde_json::from_value(parse(n.get("faces"))?).map_err(|_| bad("saved model: names"))?;
            let info: Vec<SavedFaceInfo> = serde_json::from_value(parse(n.get("info"))?).map_err(|_| bad("saved model: names"))?;
            if info.len() != faces.len() {
                return Err(bad("saved model: names"));
            }
            let info = info
                .into_iter()
                .map(|SavedFaceInfo(aliases, bounds, verified, synonyms, former)| FaceInfo { aliases, synonyms, bounds, verified, former })
                .collect();
            names.push(Arc::new(Names { faces: Arc::new(faces), info }));
        }
        let mut sketches = Vec::new();
        for s in list("sketches")? {
            let sketch: solidcraft_sketch::Sketch = serde_json::from_value(parse(s.get("sketch"))?).map_err(|_| bad("saved model: sketch"))?;
            let plane: Plane = serde_json::from_value(parse(s.get("plane"))?).map_err(|_| bad("saved model: sketch"))?;
            sketches.push(Arc::new(SolvedSketch {
                feature: s.get("feature").and_then(Json::as_u64).ok_or_else(|| bad("saved model: sketch"))?,
                name: text(s.get("name"))?,
                plane,
                report: serde_json::from_value(parse(s.get("report"))?).map_err(|_| bad("saved model: sketch"))?,
                profiles: find_profiles(&sketch),
                profiles_3d: solidcraft_sketch::find_planar_3d_profiles(&sketch, plane),
                ends: solidcraft_sketch::loose_ends(&sketch),
                sketch,
            }));
        }
        let mut model_bodies = Vec::new();
        for r in list("model_bodies")? {
            let body = bodies.get(index(r.get("body"), bodies.len())?).cloned().ok_or_else(|| bad("saved model: body"))?;
            let mut b =
                ModelBody::new(text(r.get("name"))?, body, r.get("feature").and_then(Json::as_u64).ok_or_else(|| bad("saved model: feature"))?);
            b.component = r.get("component").and_then(Json::as_u64).ok_or_else(|| bad("saved model: component"))?;
            b.key = unhex(r.get("key").unwrap_or(&Json::Null))?;
            let depth =
                r.get("depth").map(|d| d.as_u64().and_then(|d| usize::try_from(d).ok()).ok_or_else(|| bad("saved model: depth"))).transpose()?;
            b.names = crate::naming::NameCell::saved(
                names.get(index(r.get("names"), names.len())?).cloned().ok_or_else(|| bad("saved model: names"))?,
                depth,
            );
            b.source_name = r.get("source_name").map(|s| text(Some(s))).transpose()?;
            if let Some(m) = r.get("source_transform") {
                let m: crate::Mat = serde_json::from_value(m.clone()).map_err(|_| bad("saved model: transform"))?;
                if m.iter().flatten().any(|x| !x.is_finite()) {
                    return Err(bad("saved model: transform"));
                }
                b.source_transform = m;
            }
            b.frame_member_line = r.get("frame_member_line").map(|v| text(Some(v)).map(Arc::from)).transpose()?;
            b.transferred = r.get("transferred").and_then(Json::as_bool).unwrap_or(false);
            if let Some(record) = r.get("round_member") {
                let data: crate::frame_cope::RoundMember = serde_json::from_value(record.clone()).map_err(|_| bad("saved model: round member"))?;
                data.validate()?;
                b.certify_round_member(data);
            }
            b.envelope = r.get("envelope").and_then(Json::as_bool).unwrap_or(false);
            // Measured once here, so every state holding it adds it up for the cache's budget.
            b.bytes = b.body.retained_bytes();
            if let Some(look) = r.get("imported") {
                let faces: Vec<(String, [f32; 3], f32)> = serde_json::from_value(parse(look.get("faces"))?).map_err(|_| bad("saved model: look"))?;
                let colour_name = |v: Option<&Json>| -> Result<Option<String>> {
                    let name: Option<String> = serde_json::from_value(v.cloned().unwrap_or(Json::Null)).map_err(|_| bad("saved model: look"))?;
                    Ok(name.as_deref().and_then(solidcraft_kernel::colour_name))
                };
                let face_names: BTreeMap<String, String> =
                    serde_json::from_value(look.get("face_names").cloned().unwrap_or_else(|| json!({}))).map_err(|_| bad("saved model: look"))?;
                b.imported = Some(Arc::new(crate::appearance::ImportedLook {
                    color: serde_json::from_value(parse(look.get("color"))?).map_err(|_| bad("saved model: look"))?,
                    opacity: serde_json::from_value(parse(look.get("opacity"))?).map_err(|_| bad("saved model: look"))?,
                    faces: faces.into_iter().map(|(name, color, opacity)| (name, (color, opacity))).collect(),
                    name: colour_name(look.get("name"))?,
                    face_names: face_names.into_iter().filter_map(|(face, name)| Some((face, solidcraft_kernel::colour_name(&name)?))).collect(),
                }));
            }
            if let Some(names) = r.get("paint_names")
                && let Some(paint) = b.body.paint()
            {
                let body: Option<String> =
                    serde_json::from_value(names.get("body").cloned().unwrap_or(Json::Null)).map_err(|_| bad("saved model: paint"))?;
                let faces: Vec<(usize, String)> =
                    serde_json::from_value(names.get("faces").cloned().unwrap_or_else(|| json!([]))).map_err(|_| bad("saved model: paint"))?;
                let mut paint = paint.clone();
                paint.name = body.as_deref().and_then(solidcraft_kernel::colour_name);
                for f in &mut paint.faces {
                    f.name = faces.iter().rfind(|(face, _)| *face == f.face).and_then(|(_, name)| solidcraft_kernel::colour_name(name));
                }
                b.body = b.body.clone().with_paint(Some(paint));
            }
            model_bodies.push(b);
        }
        // Sheets made from surfaces: none in a model that holds none. Each one's weight is what it
        // costs to clone (its items and text), as a marking's is.
        let (mut surface_sheets, mut surface_weight) = (Vec::new(), Vec::new());
        if records.get("surface_sheets").is_some() {
            for s in list("surface_sheets")? {
                surface_sheets.push(serde_json::from_value::<crate::SurfaceSheet>(s.clone()).map_err(|_| bad("saved model: sheet"))?);
                surface_weight.push(1usize.saturating_add(serde_json::to_vec(s).map_or(usize::MAX, |b| b.len()) / 64));
            }
        }
        let mut threads = Vec::new();
        for t in list("threads")? {
            threads.push(serde_json::from_value::<ThreadInfo>(t.clone()).map_err(|_| bad("saved model: tap marking"))?);
        }
        // Bound the whole table before decoding any point lists, including unused entries.
        // Older caches lack this table: refuse them so opening evaluates the missing points.
        let face_records = list("named_faces")?;
        let mut point_count = 0usize;
        for r in face_records {
            let points = r.get(1).and_then(Json::as_array).ok_or_else(|| bad("saved model: face points"))?;
            point_count = point_count.saturating_add(points.len());
            if point_count > MAX_ITEMS {
                return Err(bad("saved model: face points"));
            }
        }
        let mut face_points = Vec::with_capacity(face_records.len());
        let mut face_weight = vec![0usize];
        for r in face_records {
            let feature = r.get(0).and_then(Json::as_u64).ok_or_else(|| bad("saved model: face points"))?;
            let points: Vec<Vec3> = serde_json::from_value(parse(r.get(1))?).map_err(|_| bad("saved model: face points"))?;
            if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite()) {
                return Err(bad("saved model: face points"));
            }
            // Even an empty list costs a map entry in every state that holds it.
            face_weight.push(face_weight.last().copied().unwrap_or(0).saturating_add(1).saturating_add(points.len()));
            face_points.push((feature, points));
        }
        // The sketch-key sets, each the changes from an earlier one (`base`): a short record can
        // stand for a large set, so what they hold counts towards MAX_HELD as they are built.
        let mut key_sets: Vec<BTreeMap<u64, u64>> = Vec::new();
        let mut held = point_count.saturating_add(face_points.len());
        for k in list("sketch_key_sets")? {
            // A set is the changes from one before it.
            let base = match k.get("base") {
                Some(b) => Some(key_sets.get(index(Some(b), key_sets.len())?).ok_or_else(|| bad("saved model: sketch keys"))?),
                None => None,
            };
            let (drop, set) = (keys_list(k, "drop")?, keys_list(k, "set")?);
            // Counted before the base is copied: at most what the set can hold.
            held = held.saturating_add(base.map_or(0, BTreeMap::len)).saturating_add(set.len());
            if held > MAX_HELD {
                return Err(bad("saved model: sketch keys"));
            }
            let mut keys = base.cloned().unwrap_or_default();
            for f in drop {
                keys.remove(&f.as_u64().ok_or_else(|| bad("saved model: sketch keys"))?);
            }
            for e in set {
                keys.insert(e.get(0).and_then(Json::as_u64).ok_or_else(|| bad("saved model: sketch keys"))?, unhex(e.get(1).unwrap_or(&Json::Null))?);
            }
            key_sets.push(keys);
        }
        // Running totals of what each marking costs to clone (its items and text), by place.
        let mut thread_weight = vec![0usize];
        for t in &threads {
            let w = 1usize.saturating_add((t.body.len().saturating_add(t.designation.len())) / 64);
            thread_weight.push(thread_weight.last().copied().unwrap_or(0).saturating_add(w));
        }
        let records = list("states")?;
        // What every state holds, counted from its runs and its sketch-key set before any is
        // built (a hostile file cannot make the states clone more than MAX_HELD items).
        for s in records {
            for key in ["bodies", "sketches"] {
                for run in s.get(key).and_then(Json::as_array).ok_or_else(|| bad("saved model: state"))? {
                    held = held.saturating_add(run.get(1).and_then(Json::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or(usize::MAX));
                }
            }
            // These are cloned deeply into every state: count a marking's text and a face
            // entry's points, so short runs of large records cannot exhaust memory.
            for (key, weights, len) in [("threads", &thread_weight, threads.len()), ("named_faces", &face_weight, face_points.len())] {
                for run in s.get(key).and_then(Json::as_array).ok_or_else(|| bad("saved model: state"))? {
                    let first = index(run.get(0), len)?;
                    let count = run.get(1).and_then(Json::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("saved model: run"))?;
                    let end = first.checked_add(count).filter(|end| *end <= len).ok_or_else(|| bad("saved model: run"))?;
                    let weight = weights.get(end).zip(weights.get(first)).ok_or_else(|| bad("saved model: run"))?;
                    held = held.saturating_add(weight.0.saturating_sub(*weight.1));
                }
            }
            // A state that holds a sheet of the flat model, which nothing makes now: the model is
            // stale, and the design is evaluated again.
            if s.get("sheets").and_then(Json::as_array).is_some_and(|l| !l.is_empty()) {
                return Err(bad("saved model: a flat sheet"));
            }
            // A sheet made from surfaces is cloned deeply into every state that lists it, as a
            // marking is, so each counts by its weight: a large sheet listed again and again cannot
            // exhaust memory.
            for i in s.get("surface_sheets").and_then(Json::as_array).map_or(&[][..], Vec::as_slice) {
                held = held.saturating_add(surface_weight.get(index(Some(i), surface_weight.len())?).copied().unwrap_or(usize::MAX));
            }
            held = held.saturating_add(key_sets.get(index(s.get("sketch_key_set"), key_sets.len())?).map_or(0, BTreeMap::len));
            if held > MAX_HELD {
                return Err(bad("saved model: states"));
            }
        }
        // Each state is independent of the others once the records they share are read: built
        // on threads (a 5,484-feature design spent 3.5 s building them one after another).
        let built = kernel::map_on_threads(records, 16, |s: &Json| -> Result<ModelState> {
            // Runs of consecutive indices, each within the list it indexes.
            let ids = |key: &str, len: usize| -> Result<Vec<usize>> {
                let mut out = Vec::new();
                for run in s.get(key).and_then(Json::as_array).ok_or_else(|| bad("saved model: state"))? {
                    let first = index(run.get(0), len)?;
                    let count = run.get(1).and_then(Json::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("saved model: run"))?;
                    if first.checked_add(count).is_none_or(|end| end > len) || out.len().saturating_add(count) > MAX_ITEMS {
                        return Err(bad("saved model: run"));
                    }
                    out.extend(first..first + count);
                }
                Ok(out)
            };
            let field = |key: &str| parse(s.get(key));
            let invalid = |_| bad("saved model: state");
            let sketch_keys = key_sets.get(index(s.get("sketch_key_set"), key_sets.len())?).cloned().ok_or_else(|| bad("saved model: state"))?;
            let mut named_faces = BTreeMap::new();
            for i in ids("named_faces", face_points.len())? {
                let (feature, points) = face_points.get(i).ok_or_else(|| bad("saved model: face points"))?;
                if named_faces.contains_key(feature) {
                    return Err(bad("saved model: duplicate face points"));
                }
                named_faces.insert(*feature, points.clone());
            }
            let mut st = ModelState {
                frame_bodies: Default::default(),
                origin_frames: Default::default(),
                bodies: ids("bodies", model_bodies.len())?.into_iter().filter_map(|i| model_bodies.get(i).cloned()).collect(),
                sketches: ids("sketches", sketches.len())?.into_iter().filter_map(|i| sketches.get(i).cloned()).collect(),
                // A world state's alone: a model's own state has none.
                sketch_copies: BTreeMap::new(),
                body_counter: index(s.get("body_counter"), usize::MAX)?,
                reserved_body_names: s
                    .get("reserved_body_names")
                    .and_then(Json::as_array)
                    .map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
                surface_sheets: match s.get("surface_sheets") {
                    Some(v) => v
                        .as_array()
                        .ok_or_else(|| bad("saved model: state"))?
                        .iter()
                        .map(|i| {
                            index(Some(i), surface_sheets.len()).and_then(|i| surface_sheets.get(i).cloned().ok_or_else(|| bad("saved model: sheet")))
                        })
                        .collect::<Result<Vec<_>>>()?,
                    None => Vec::new(),
                },
                construct: serde_json::from_value(field("construct")?).map_err(invalid)?,
                threads: ids("threads", threads.len())?.into_iter().filter_map(|i| threads.get(i).cloned()).collect(),
                // Kept since frame trims (#4003); a model saved before keeps no members.
                frame_members: match s.get("frame_members") {
                    None => Vec::new(),
                    v => serde_json::from_value(parse(v)?).map_err(invalid)?,
                },
                sketch_keys,
                blend_edges: serde_json::from_value(field("blend_edges")?).map_err(invalid)?,
                fitted: serde_json::from_value(field("fitted")?).map_err(invalid)?,
                tapered: serde_json::from_value(field("tapered")?).map_err(invalid)?,
                // Older saved models have none: their patterns left no body out.
                pattern_left_out: match s.get("pattern_left_out") {
                    Some(v) => serde_json::from_value(v.clone()).map_err(invalid)?,
                    None => BTreeMap::new(),
                },
                named_faces,
                // A join's notice is for the evaluation that made it, not kept with the cache.
                kept_separate: BTreeMap::new(),
                // Not kept either: a saved model's states carry no lineage, so the check of a
                // legacy split's picks (`lineage::legacy_picks_digest`) finds no pieces in them
                // and takes them as the design's evaluation they were saved as.
                legacy_picks: BTreeMap::new(),
                // What a cut was drawn as is read by the evaluation that made it.
                cut_extents: BTreeMap::new(),
            };
            for (member, record) in st.frame_members.iter_mut().zip(s.get("frame_members").and_then(Json::as_array).into_iter().flatten()) {
                // A pre-identity cache cannot prove a member safe to rebuild, so re-evaluate.
                let unmodified = record.get("unmodified").and_then(Json::as_bool).ok_or_else(|| bad("saved model: frame member identity"))?;
                if unmodified {
                    member.built = Some(
                        st.bodies
                            .iter()
                            .find(|b| b.name == member.body && b.feature == member.feature)
                            .ok_or_else(|| bad("saved model: frame member body"))?
                            .body
                            .clone(),
                    );
                }
            }
            Ok(st)
        });
        let states = built.into_iter().map(|r| r.unwrap_or_else(|| Err(bad("saved model: state"))).map(Arc::new)).collect::<Result<Vec<_>>>()?;
        Ok(states)
    }

    /// The kernel bodies with the rounds that made them ([`kernel::Body::rounds`]) as the records
    /// list them, each body's before it is taken as another's body before its rounds.
    fn bodies_with_rounds(&self) -> Result<Vec<Body>> {
        let refused = || bad("saved model: rounds");
        let list = self.records.get("rounds").and_then(Json::as_array).filter(|l| l.len() <= self.bodies.len()).ok_or_else(refused)?;
        let place = |v: Option<&Json>| v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < self.bodies.len());
        // Per body, the body its rounds were made on and their edges.
        let mut made: Vec<Option<(usize, Vec<(usize, f64)>)>> = vec![None; self.bodies.len()];
        for r in list {
            let (body, before) = (place(r.get("body")).ok_or_else(refused)?, place(r.get("before")).ok_or_else(refused)?);
            let edges: Vec<(usize, f64)> = serde_json::from_value(r.get("edges").cloned().ok_or_else(refused)?).map_err(|_| refused())?;
            match made.get_mut(body) {
                Some(slot @ None) => *slot = Some((before, edges)),
                _ => return Err(refused()),
            }
        }
        // Each body after the one its rounds were made on, down each chain once (a cycle is refused).
        let mut bodies = self.bodies.clone();
        let (mut done, mut on_chain) = (vec![false; bodies.len()], vec![false; bodies.len()]);
        for start in 0..bodies.len() {
            let mut chain = Vec::new();
            let mut at = start;
            while let Some(seen @ false) = done.get_mut(at) {
                *seen = true;
                chain.push(at);
                if let Some(on) = on_chain.get_mut(at) {
                    *on = true;
                }
                match made.get(at).and_then(Option::as_ref) {
                    Some((before, _)) => at = *before,
                    None => break,
                }
                if on_chain.get(at).copied().unwrap_or(false) {
                    return Err(refused());
                }
            }
            for &k in chain.iter().rev() {
                if let Some(on) = on_chain.get_mut(k) {
                    *on = false;
                }
                let Some(Some((before, edges))) = made.get(k) else { continue };
                let before = bodies.get(*before).cloned().ok_or_else(refused)?;
                let body = bodies.get(k).cloned().ok_or_else(refused)?;
                *bodies.get_mut(k).ok_or_else(refused)? = body.with_rounds_of(&before, edges.clone())?;
            }
        }
        Ok(bodies)
    }

    /// The model's final state (what a picture of it shows), without checking that it is a
    /// document's evaluation.
    pub fn final_state(&self) -> Result<Arc<ModelState>> {
        let states = self.states()?;
        let features = self.records.get("features").and_then(Json::as_array).ok_or_else(|| bad("saved model: features"))?;
        let last = features.iter().rev().find(|f| f.get("skipped").and_then(Json::as_bool) == Some(false));
        match last {
            Some(f) => f
                .get("state")
                .and_then(Json::as_u64)
                .and_then(|i| states.get(usize::try_from(i).ok()?))
                .cloned()
                .ok_or_else(|| bad("saved model: state")),
            None => Ok(Arc::new(ModelState::default())),
        }
    }

    /// This model with fewer states, for a design file without room for all of them (#3220): the
    /// final state (what the design shows, kept whatever it costs), then the others, the slowest
    /// feature's first (the ones opening would take longest to evaluate again; of equal ones the
    /// later first), each while what it adds still fits in `budget` bytes: the bodies no state kept
    /// so far holds, and the records of the state, those bodies, their names and sketches.
    /// `pieces[i]` is what keeping body `i` stores, as ids and bytes: a piece that several bodies
    /// or states need counts once. A feature whose state is left out keeps none, and opening
    /// evaluates it again ([`Model::adopt`]). With the model, the index here of each body it keeps,
    /// and the bytes it counted: over `budget` when the final state alone is. `None` only for
    /// records it cannot read.
    pub fn thinned(&self, pieces: &[Vec<(usize, usize)>], budget: usize) -> Option<(SavedModel, Vec<usize>, usize)> {
        let list = |key: &str| self.records.get(key).and_then(Json::as_array);
        let (features, states, model_bodies) = (list("features")?, list("states")?, list("model_bodies")?);
        let (names, sketches) = (list("names")?, list("sketches")?);
        let at = |v: Option<&Json>, len: usize| v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < len);
        // The indices a state's runs list.
        let held = |s: &Json, key: &str, len: usize| -> Option<Vec<usize>> {
            let mut out = Vec::new();
            for run in s.get(key)?.as_array()? {
                let first = at(run.get(0), len)?;
                out.extend(first..first.checked_add(at(run.get(1), len.saturating_add(1))?).filter(|end| *end <= len)?);
            }
            Some(out)
        };
        let state_bodies = states.iter().map(|s| held(s, "bodies", model_bodies.len())).collect::<Option<Vec<_>>>()?;
        let state_sketches = states.iter().map(|s| held(s, "sketches", sketches.len())).collect::<Option<Vec<_>>>()?;
        let body_of = model_bodies.iter().map(|r| at(r.get("body"), self.bodies.len())).collect::<Option<Vec<_>>>()?;
        let names_of = model_bodies.iter().map(|r| at(r.get("names"), names.len())).collect::<Option<Vec<_>>>()?;
        // The body each body's rounds were made on: kept with it, though no kept state holds it.
        let rounds = list("rounds")?;
        let mut before_of = HashMap::new();
        for r in rounds {
            before_of.insert(at(r.get("body"), self.bodies.len())?, at(r.get("before"), self.bodies.len())?);
        }
        // A kernel body and the bodies down its rounds (a cycle, refused on reading, ends it).
        let with_before = |k: usize| -> Vec<usize> {
            let mut out = vec![k];
            while let Some(b) = out.last().and_then(|k| before_of.get(k)).filter(|b| !out.contains(b)) {
                out.push(*b);
            }
            out
        };
        // What each record takes in the file.
        let bytes = |list: &[Json]| list.iter().map(|r| serde_json::to_vec(r).map_or(usize::MAX, |b| b.len().saturating_add(1))).collect::<Vec<_>>();
        let (state_bytes, body_bytes) = (bytes(states.as_slice()), bytes(model_bodies.as_slice()));
        let (name_bytes, sketch_bytes) = (bytes(names.as_slice()), bytes(sketches.as_slice()));
        // What each state's feature took to evaluate, and the final state.
        let mut ms = vec![0.0f64; states.len()];
        let mut last = None;
        for f in features.iter().filter(|f| f.get("skipped").and_then(Json::as_bool) == Some(false)) {
            let s = at(f.get("state"), states.len())?;
            if let Some(t) = ms.get_mut(s) {
                *t += f.get("ms").and_then(Json::as_f64).filter(|t| t.is_finite() && *t > 0.0).unwrap_or(0.0);
            }
            last = Some(s);
        }
        let last = last?;
        let time = |s: &usize| ms.get(*s).copied().unwrap_or(0.0);
        let mut order: Vec<usize> = (0..states.len()).filter(|s| *s != last).collect();
        order.sort_by(|a, b| time(b).total_cmp(&time(a)).then(b.cmp(a)));
        let mut keep = vec![false; states.len()];
        // The records and pieces kept so far, and their bytes.
        let mut kept_bodies = vec![false; model_bodies.len()];
        let (mut names_used, mut sketches_used) = (vec![false; names.len()], vec![false; sketches.len()]);
        let mut stored = std::collections::HashSet::new();
        // The sheets made from surfaces and the tap markings, each written once and named by place in
        // the states: all kept.
        let surface_sheets = list("surface_sheets").cloned().unwrap_or_default();
        let threads = list("threads").cloned().unwrap_or_default();
        let key_sets = list("sketch_key_sets").cloned().unwrap_or_default();
        let named_faces = list("named_faces")?.clone();
        let mut used =
            [&surface_sheets, &threads, &key_sets, &named_faces].iter().flat_map(|l| bytes(l.as_slice())).fold(0usize, |t, b| t.saturating_add(b));
        for s in std::iter::once(last).chain(order) {
            let (mut adds, mut new_names, mut new_sketches) = (HashMap::new(), std::collections::HashSet::new(), std::collections::HashSet::new());
            let mut records = *state_bytes.get(s)?;
            for &m in state_bodies.get(s)? {
                if kept_bodies.get(m) == Some(&false) {
                    records = records.saturating_add(*body_bytes.get(m)?);
                    let n = *names_of.get(m)?;
                    if names_used.get(n) == Some(&false) && new_names.insert(n) {
                        records = records.saturating_add(*name_bytes.get(n)?);
                    }
                    for k in with_before(*body_of.get(m)?) {
                        for &(piece, bytes) in pieces.get(k)? {
                            if !stored.contains(&piece) {
                                adds.insert(piece, bytes);
                            }
                        }
                    }
                }
            }
            for &k in state_sketches.get(s)? {
                if sketches_used.get(k) == Some(&false) && new_sketches.insert(k) {
                    records = records.saturating_add(*sketch_bytes.get(k)?);
                }
            }
            let total = adds.values().fold(used.saturating_add(records), |t, b| t.saturating_add(*b));
            if total > budget && s != last {
                continue;
            }
            used = total;
            stored.extend(adds.into_keys());
            for &m in state_bodies.get(s)? {
                *kept_bodies.get_mut(m)? = true;
            }
            for n in new_names {
                *names_used.get_mut(n)? = true;
            }
            for k in new_sketches {
                *sketches_used.get_mut(k)? = true;
            }
            *keep.get_mut(s)? = true;
        }
        // What the states kept refer to, numbered again in the order it had.
        let mut kernel_used = vec![false; self.bodies.len()];
        for m in (0..model_bodies.len()).filter(|m| kept_bodies.get(*m) == Some(&true)) {
            for k in with_before(*body_of.get(m)?) {
                *kernel_used.get_mut(k)? = true;
            }
        }
        let renumber = |used: &[bool]| -> Vec<Option<usize>> {
            let mut next = 0;
            used.iter()
                .map(|&u| {
                    u.then(|| {
                        next += 1;
                        next - 1
                    })
                })
                .collect()
        };
        let (new_body, new_kernel, new_names) = (renumber(&kept_bodies), renumber(&kernel_used), renumber(&names_used));
        let (new_sketch, new_state) = (renumber(&sketches_used), renumber(&keep));
        let new = |map: &[Option<usize>], i: usize| map.get(i).copied().flatten();
        let kept_of = |list: &[Json], used: &[bool]| list.iter().zip(used).filter(|(_, u)| **u).map(|(v, _)| v.clone()).collect::<Vec<_>>();
        let mut out_states = Vec::new();
        for (s, st) in states.iter().enumerate().filter(|(s, _)| keep.get(*s) == Some(&true)) {
            let bodies = state_bodies.get(s)?.iter().map(|m| new(&new_body, *m)).collect::<Option<Vec<_>>>()?;
            let sketches = state_sketches.get(s)?.iter().map(|i| new(&new_sketch, *i)).collect::<Option<Vec<_>>>()?;
            let mut st = st.clone();
            let o = st.as_object_mut()?;
            o.insert("bodies".into(), json!(runs(&bodies)));
            o.insert("sketches".into(), json!(runs(&sketches)));
            out_states.push(st);
        }
        let mut out_bodies = Vec::new();
        for (m, r) in model_bodies.iter().enumerate().filter(|(m, _)| kept_bodies.get(*m) == Some(&true)) {
            let (body, named) = (new(&new_kernel, *body_of.get(m)?)?, new(&new_names, at(r.get("names"), names.len())?)?);
            let mut r = r.clone();
            let o = r.as_object_mut()?;
            o.insert("body".into(), json!(body));
            o.insert("names".into(), json!(named));
            out_bodies.push(r);
        }
        let mut out_features = Vec::with_capacity(features.len());
        for f in features {
            let state = new(&new_state, at(f.get("state"), states.len())?);
            let mut f = f.clone();
            let o = f.as_object_mut()?;
            match state {
                Some(s) => o.insert("state".into(), json!(s)),
                None => o.remove("state"),
            };
            out_features.push(f);
        }
        let mut out_rounds = Vec::new();
        for r in rounds {
            let Some(body) = new(&new_kernel, at(r.get("body"), self.bodies.len())?) else { continue };
            let before = new(&new_kernel, at(r.get("before"), self.bodies.len())?)?;
            let mut r = r.clone();
            let o = r.as_object_mut()?;
            o.insert("body".into(), json!(body));
            o.insert("before".into(), json!(before));
            out_rounds.push(r);
        }
        let mut records = json!({
            "features": out_features, "states": out_states, "model_bodies": out_bodies, "names": kept_of(names.as_slice(), &names_used),
            "sketches": kept_of(sketches.as_slice(), &sketches_used), "threads": threads, "rounds": out_rounds,
            "sketch_key_sets": key_sets,
            "named_faces": named_faces,
        });
        if let (Some(_), Some(out)) = (self.records.get("surface_sheets"), records.as_object_mut()) {
            out.insert("surface_sheets".into(), json!(surface_sheets));
        }
        // The check mode the results were evaluated under, as the model records it (a model
        // that records none stays without one: adopting it is refused either way).
        if let (Some(validation), Some(out)) = (self.records.get("validation"), records.as_object_mut()) {
            out.insert("validation".into(), validation.clone());
        }
        if let (Some(heal), Some(out)) = (self.records.get("tolerance_heal"), records.as_object_mut()) {
            out.insert("tolerance_heal".into(), heal.clone());
        }
        if let (Some(heals), Some(out)) = (self.records.get("import_heals"), records.as_object_mut()) {
            out.insert("import_heals".into(), heals.clone());
        }
        if let Some(version) = self.records.get("round_member_records") {
            records["round_member_records"] = version.clone();
        }
        let kept: Vec<usize> = (0..self.bodies.len()).filter(|i| kernel_used.get(*i) == Some(&true)).collect();
        let bodies = kept.iter().filter_map(|i| self.bodies.get(*i).cloned()).collect();
        Some((SavedModel { records, bodies }, kept, used))
    }
}

impl Model {
    /// The model a design file kept for `doc` ([`Model::saved`]), taken as `doc`'s evaluation:
    /// every feature's result as saved, nothing evaluated but the features whose state the file
    /// did not keep ([`SavedModel::thinned`]), under the check mode it was evaluated in
    /// ([`Model::validation`]). Refused (an error) unless every feature's fingerprint is the one
    /// `doc` gives it now, and every record is well formed.
    pub fn adopt(doc: &Document, saved: &SavedModel) -> Result<Model> {
        if doc.features.iter().any(|f| matches!(f.kind, FeatureKind::FrameMember { .. } | FeatureKind::FrameCope { .. }))
            && saved.records.get("round_member_records").and_then(Json::as_u64) != Some(1)
        {
            return Err(bad("saved model: member records need reevaluation"));
        }
        let mut states = saved.states()?;
        // A saved/thinned stock marker proves only record presence, not eligibility. Inspect
        // the originating feature before any retained certificate can be used for an export.
        if states
            .iter()
            .flat_map(|state| &state.bodies)
            .any(|body| body.round_member.is_some() && !doc.feature(body.feature).is_some_and(|f| crate::frame_cope::stock_eligible(&f.kind)))
        {
            return Err(bad("saved model: finished frame stock records need reevaluation"));
        }
        let frame_bodies = Arc::new(std::sync::Mutex::new(FrameBodies::default()));
        for state in &mut states {
            Arc::make_mut(state).frame_bodies = Arc::downgrade(&frame_bodies);
        }
        let records = &saved.records;
        let index = |v: Option<&Json>, len: usize| {
            v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < len).ok_or_else(|| bad("saved model: index"))
        };
        let text =
            |v: Option<&Json>| v.and_then(Json::as_str).filter(|s| s.len() <= MAX_TEXT).map(str::to_owned).ok_or_else(|| bad("saved model: text"));
        let features = records.get("features").and_then(Json::as_array).ok_or_else(|| bad("saved model: features"))?;
        let validation =
            records.get("validation").and_then(Json::as_str).and_then(kernel::Validation::parse).ok_or_else(|| bad("saved model: validation"))?;
        if features.len() != doc.features.len() {
            return Err(bad("saved model: features"));
        }
        let (vals, param_errors) = evaluation_values(doc);
        // As `evaluate` digests the context under the mode, so the next evaluation reuses these.
        // And under the tolerance heal preference it was evaluated with (on when not recorded).
        let heal = records.get("tolerance_heal").and_then(Json::as_bool).unwrap_or(true);
        let mut import_digests = ImportDigests::of(doc, &ImportDigests::default(), heal);
        if let Some(heals) = records.get("import_heals") {
            let entries = heals.as_array().filter(|l| l.len() <= MAX_ITEMS).ok_or_else(|| bad("saved model: import heals"))?;
            for entry in entries {
                let pair = entry.as_array().filter(|p| p.len() == 2).ok_or_else(|| bad("saved model: import heals"))?;
                let id = pair.first().and_then(Json::as_u64).ok_or_else(|| bad("saved model: import heals"))?;
                let digest = unhex(pair.get(1).unwrap_or(&Json::Null))?;
                if digest == 0 {
                    return Err(bad("saved model: import heals"));
                }
                if heal && let Some((_, step_digest)) = import_digests.steps.get(&id) {
                    import_digests.heals.insert(id, (*step_digest, ImportHeal::Digest(digest)));
                }
            }
        }
        let marker = doc.marker.unwrap_or(usize::MAX);
        let ctx = context_digest(doc, &vals) ^ validation_digest(validation) ^ heal_digest(heal);
        let shared = shared_context_digest(doc, &vals) ^ validation_digest(validation) ^ heal_digest(heal);
        let mut before = Arc::new(ModelState::default());
        // Whether `before` is the state before the feature: one the file left out only stands in.
        let mut exact = true;
        let mut prev = 0;
        let mut results = Vec::with_capacity(features.len());
        let mut construction = HashMap::new();
        let mut prepared_states = std::collections::HashSet::new();
        // The pose-only features saved at the pose `doc` gives them: their results stand.
        let mut poses = HashMap::new();
        let mut step_doc_owned = super::init_capture_step_doc(doc, &vals);
        let mut resolved_joints = std::collections::BTreeMap::new();
        for (i, (f, r)) in doc.features.iter().zip(features).enumerate() {
            let eval_doc = step_doc_owned.as_ref().unwrap_or(doc);
            let saved = unhex(r.get("fingerprint").unwrap_or(&Json::Null))?;
            // A fingerprint reads the state before its feature (where the bodies it takes lie).
            // After a state the file left out, the saved one is taken: `evaluate` works that state
            // out, checks the fingerprints after it, and evaluates again from one that differs.
            prev = if exact { chain_fingerprint(eval_doc, &vals, f, prev, i >= marker, &import_digests, Some(&before)) } else { saved };
            if r.get("id").and_then(Json::as_u64) != Some(f.id) || saved != prev {
                return Err(bad("saved model: the document changed"));
            }
            let ms = r.get("ms").and_then(Json::as_f64).filter(|ms| ms.is_finite()).ok_or_else(|| bad("saved model: time"))?;
            // A file without room for every state keeps some features without one (#3220).
            let state = r.get("state").map(|s| index(Some(s), states.len())).transpose()?;
            if let Some(at) = state
                && prepared_states.insert(at)
            {
                let original = matches!(
                    &f.kind,
                    FeatureKind::Box { operation: Operation::NewBody, .. } | FeatureKind::Extrude { operation: Operation::NewBody, .. }
                );
                let current = states.get_mut(at).ok_or_else(|| bad("saved model: state"))?;
                // Each state is prepared before its first shared clone. Record and NameCell
                // identity prove this is still the original geometry and its exact names.
                for body in &mut Arc::make_mut(current).bodies {
                    let key = (std::ptr::from_ref(body.body.record()).addr(), body.names.id());
                    if original && body.feature == f.id && before.body(&body.name).is_none() {
                        construction.insert(key, f.id);
                    }
                    body.names.cached_construction = construction.get(&key).copied();
                }
            }
            let state = state.map(|at| states.get(at).cloned().ok_or_else(|| bad("saved model: state"))).transpose()?;
            if exact
                && pose_only(f)
                && let Some(pose) = r.get("pose").map(unhex).transpose()?
                && pose == placement_digest(eval_doc, f, Some(&before))
            {
                poses.insert(f.id, pose);
            }
            let result = FeatureResult {
                id: f.id,
                name: f.name.clone(),
                error: None,
                error_code: None,
                internal: false,
                warning: r.get("warning").map(|w| text(Some(w))).transpose()?,
                import_report: r
                    .get("import_report")
                    .map(|v| serde_json::from_value::<kernel::ImportReport>(v.clone()).map(Arc::new).map_err(|_| bad("saved model: import report")))
                    .transpose()?,
                repair: None,
                lost_projections: None,
                ms,
                skipped: r.get("skipped").and_then(Json::as_bool).ok_or_else(|| bad("saved model: skipped"))?,
                stale: false,
                fingerprint: prev,
                unkept: state.is_none(),
                state: state.unwrap_or_else(|| before.clone()),
            };
            // A model saved before rebuildable ordinary members were recorded (#4003) needs
            // reevaluation for trims. Auto and path bodies intentionally emit no such record.
            if !result.skipped
                && !result.unkept
                && super::frame_member::needs_trim_records(&result.state, &f.kind)?
                && !result.state.frame_members.iter().any(|m| m.feature == f.id)
            {
                return Err(bad("saved model: frame members"));
            }
            if !result.skipped {
                exact = !result.unkept;
                let changed = super::bodies_changed(&before, &result.state);
                before = result.state.clone();
                if result.error.is_none() && i < marker && !f.suppressed {
                    super::advance_capture_step(&mut step_doc_owned, &mut resolved_joints, &vals, f, &before, changed);
                }
            }
            results.push(result);
        }
        // The retained-outline notes as the model is now, whatever the file kept (an older one
        // has none; a sketch may have been renamed since).
        for (f, r) in doc.features.iter().zip(results.iter_mut()).filter(|(_, r)| !r.skipped && !r.unkept) {
            r.warning = crate::eval::with_retained_notes(doc, r.warning.take(), f, &r.state);
        }
        let resolved_occurrences = step_doc_owned.map(|d| d.occurrences.into_iter().map(|o| (o.id, o.transform)).collect()).unwrap_or_default();
        let mut model = Model {
            results,
            param_errors,
            import_digests,
            context: ctx,
            frame_bodies,
            validation,
            heal_off: !heal,
            resolved_occurrences,
            resolved_joints,
            ..Model::default()
        };
        let unkept = model.results.iter().filter(|r| r.unkept).count();
        // A pose-only feature's fingerprint leaves its pose out, so its saved result may be from
        // another pose than `doc`'s: evaluated again, with what reads none of it kept (#12139).
        // One saved with the pose `doc` gives it is `doc`'s result as it is.
        let posed = doc.features.iter().zip(&model.results).any(|(f, r)| !r.skipped && pose_only(f) && !poses.contains_key(&f.id));
        model.poses = poses;
        if unkept > 0 || posed {
            // The features whose state the file did not keep, each evaluated again from the state
            // before it: nothing else in the model is missing then.
            model.evaluate(doc);
            log::info!(
                "the saved model kept the states of {} of {} features: {} evaluated again",
                model.results.len().saturating_sub(unkept),
                model.results.len(),
                model.last_recomputed
            );
        }
        // Each result where an evaluation would have put it, so an edit undone (or set back)
        // finds it again rather than evaluating the feature anew.
        let mut before = Arc::new(ModelState::default());
        let mut cache_step_doc = super::init_capture_step_doc(doc, &vals);
        let mut cache_joints = std::collections::BTreeMap::new();
        let component_digests = super::ComponentDigests::of(doc);
        if let Ok(mut cache) = model.cache.lock() {
            for (i, (f, r)) in doc.features.iter().zip(&model.results).enumerate().filter(|(_, (_, r))| !r.skipped && r.error.is_none()) {
                let eval_doc = cache_step_doc.as_ref().unwrap_or(doc);
                // A pose-only feature's fingerprint leaves its pose out, so its saved result may be
                // from another pose than `doc`'s: it is no result for `doc`'s key (#12139).
                if pose_only(f) {
                    let changed = super::bodies_changed(&before, &r.state);
                    before = r.state.clone();
                    if i < marker && !f.suppressed {
                        super::advance_capture_step(&mut cache_step_doc, &mut cache_joints, &vals, f, &before, changed);
                    }
                    continue;
                }
                let key = EvalKey::new(eval_doc, &vals, f, r.fingerprint, ctx, shared, &before, &model.import_digests, &component_digests);
                model.component_keys.insert(f.id, key.reached);
                let whole = CachedEval {
                    error: None,
                    error_code: None,
                    warning: crate::eval::without_retained_notes(r.warning.clone()),
                    import_report: r.import_report.clone(),
                    repair: r.repair.clone(),
                    lost_projections: None,
                    ms: r.ms,
                    state: r.state.clone(),
                };
                // And what it changed, by what it read, as an evaluation keeps it: the first edit
                // after opening then evaluates only the features that read what the edit changed,
                // as every later edit does, not every feature after the edit. Features that name
                // their pieces as later features pick them are left out, as is any feature whose
                // bodies were not stamped with this key (saved by a version that keyed them
                // otherwise, or stamped by the whole model): `stamp` would work their names out
                // afresh, with no lineage to work them from. It is evaluated again instead.
                if let Some((k, read)) = &key.reads
                    && !f.shell_piece_order
                    && let Some(change) = r.state.change_from(&before, f, read)
                {
                    // A change that holds whatever the bodies it did not read was stamped with the
                    // key without them; one saved before that key, with the other.
                    let relative = key.relative.filter(|_| change.relative().is_some());
                    if let Some(k) = relative.into_iter().chain([*k]).find(|k| change.made_bodies_stamped(*k)) {
                        let bytes = change.retained_bytes();
                        cache.changes.put(k, whole.with(change), bytes, r.ms);
                    }
                }
                cache.states.put(key.whole, whole, r.state.retained_bytes(), r.ms);
                let changed = super::bodies_changed(&before, &r.state);
                before = r.state.clone();
                if i < marker && !f.suppressed {
                    super::advance_capture_step(&mut cache_step_doc, &mut cache_joints, &vals, f, &before, changed);
                }
            }
        }
        Ok(model)
    }
}
