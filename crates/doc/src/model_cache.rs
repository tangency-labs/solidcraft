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
/// Bodies, sketches and sheet bodies all the states hold together, at most (a state is a list of clones).
const MAX_HELD: usize = 50_000_000;

fn hex(v: u64) -> String {
    format!("{v:016x}")
}

fn unhex(v: &Json) -> Result<u64> {
    v.as_str().and_then(|s| u64::from_str_radix(s, 16).ok()).ok_or_else(|| bad("saved model: key"))
}

fn bad(what: &str) -> DocError {
    DocError::Invalid(what.into())
}

/// A design whose stored face or edge names were saved before format 14 (marked
/// [`crate::naming::LEGACY_MARK`]) finds them through names worked out from geometry on demand,
/// which a saved model does not keep.
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
    name: String,
    source_name: Option<String>,
    source_transform: [u64; 16],
}

impl BodyKey {
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
            name: b.name.clone(),
            source_name: b.source_name.clone(),
            source_transform,
        }
    }
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
    /// Sheet bodies by their record: most features leave the sheets as they were, so the states
    /// share them (writing them with every state made a sheet-metal design's model too large to keep).
    sheets: Vec<Json>,
    sheet_index: HashMap<String, usize>,
    states: Vec<Json>,
    state_index: HashMap<usize, usize>,
}

impl Writer {
    fn names(&mut self, b: &ModelBody) -> usize {
        let id = b.names.id();
        if let Some(i) = self.names_index.get(&id) {
            return *i;
        }
        let names = crate::naming::names(b);
        let info: Vec<Json> = names.info.iter().map(|i| json!([i.aliases, i.bounds, i.verified])).collect();
        self.names.push(json!({"faces": *names.faces, "info": info}));
        self.names_index.insert(id, self.names.len() - 1);
        self.names.len() - 1
    }

    fn model_body(&mut self, b: &ModelBody) -> Option<usize> {
        if b.body.is_mesh() {
            return None;
        }
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
        if b.transferred {
            r["transferred"] = json!(true);
        }
        if let Some(look) = &b.imported {
            let faces: Vec<Json> = look.faces.iter().map(|(name, (color, opacity))| json!([name, color, opacity])).collect();
            r["imported"] = json!({"color": look.color, "opacity": look.opacity, "faces": faces});
        }
        r["body"] = json!(self.bodies.len());
        self.bodies.push(b.body.clone());
        self.model_bodies.push(r);
        self.model_body_index.insert(key, self.model_bodies.len() - 1);
        Some(self.model_bodies.len() - 1)
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

    fn sheet(&mut self, sheet: &crate::sheet::SheetBody) -> Option<usize> {
        let record = serde_json::to_value(sheet).ok()?;
        let key = record.to_string();
        if let Some(i) = self.sheet_index.get(&key) {
            return Some(*i);
        }
        self.sheets.push(record);
        self.sheet_index.insert(key, self.sheets.len() - 1);
        Some(self.sheets.len() - 1)
    }

    fn state(&mut self, st: &Arc<ModelState>) -> Option<usize> {
        let id = Arc::as_ptr(st) as usize;
        if let Some(i) = self.state_index.get(&id) {
            return Some(*i);
        }
        let bodies = st.bodies.iter().map(|b| self.model_body(b)).collect::<Option<Vec<_>>>()?;
        let sketches = st.sketches.iter().map(|s| self.sketch(s)).collect::<Option<Vec<_>>>()?;
        let sheets = st.sheets.iter().map(|s| self.sheet(s)).collect::<Option<Vec<_>>>()?;
        let r = json!({
            "bodies": runs(&bodies), "sketches": runs(&sketches), "body_counter": st.body_counter,
            "reserved_body_names": st.reserved_body_names.iter().collect::<Vec<_>>(),
            "sheets": sheets, "construct": serde_json::to_value(&st.construct).ok()?,
            "threads": serde_json::to_value(&st.threads).ok()?,
            "sketch_keys": st.sketch_keys.iter().map(|(f, k)| json!([f, hex(*k)])).collect::<Vec<_>>(),
            "blend_edges": serde_json::to_value(&st.blend_edges).ok()?, "fitted": serde_json::to_value(&st.fitted).ok()?,
            "tapered": serde_json::to_value(&st.tapered).ok()?,
        });
        self.states.push(r);
        self.state_index.insert(id, self.states.len() - 1);
        Some(self.states.len() - 1)
    }
}

impl Model {
    /// The model as a design file keeps it: `None` unless it is `doc`'s evaluation without an
    /// error, holds no mesh bodies, and `doc` keeps no names saved before format 14. Works out
    /// the face names not asked for yet (a saved model keeps them as worked out).
    pub fn saved(&self, doc: &Document) -> Option<SavedModel> {
        if self.results.is_empty() || self.results.iter().any(|r| r.error.is_some()) || !self.is_current(doc) || has_legacy_names(doc) {
            return None;
        }
        kernel::guard("save the model", || {
            let mut w = Writer::default();
            let mut features = Vec::with_capacity(self.results.len());
            for r in &self.results {
                let state = w.state(&r.state).ok_or_else(|| kernel::KernelError::Invalid("not kept".into()))?;
                let mut f = json!({"id": r.id, "fingerprint": hex(r.fingerprint), "ms": r.ms, "skipped": r.skipped, "state": state});
                if let Some(w) = &r.warning {
                    f["warning"] = json!(w);
                }
                if let Some(report) = &r.import_report {
                    f["import_report"] = serde_json::to_value(report.as_ref()).map_err(|e| kernel::KernelError::Invalid(e.to_string()))?;
                }
                features.push(f);
            }
            let records = json!({
                "features": features, "states": w.states, "model_bodies": w.model_bodies, "names": w.names, "sketches": w.sketches,
                "sheets": w.sheets,
            });
            Ok(SavedModel { records, bodies: w.bodies })
        })
        .ok()
    }
}

impl SavedModel {
    /// The states the records hold, every record checked.
    fn states(&self) -> Result<Vec<Arc<ModelState>>> {
        let (records, bodies) = (&self.records, &self.bodies);
        let list = |key: &str| records.get(key).and_then(Json::as_array).filter(|l| l.len() <= MAX_ITEMS).ok_or_else(|| bad("saved model: list"));
        let index = |v: Option<&Json>, len: usize| {
            v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < len).ok_or_else(|| bad("saved model: index"))
        };
        let text =
            |v: Option<&Json>| v.and_then(Json::as_str).filter(|s| s.len() <= 1 << 16).map(str::to_owned).ok_or_else(|| bad("saved model: text"));
        let parse = |v: Option<&Json>| v.cloned().ok_or_else(|| bad("saved model: missing"));
        let mut names = Vec::new();
        for n in list("names")? {
            let faces: Vec<String> = serde_json::from_value(parse(n.get("faces"))?).map_err(|_| bad("saved model: names"))?;
            let info: Vec<(Vec<String>, std::collections::BTreeSet<String>, bool)> =
                serde_json::from_value(parse(n.get("info"))?).map_err(|_| bad("saved model: names"))?;
            if info.len() != faces.len() {
                return Err(bad("saved model: names"));
            }
            let info = info.into_iter().map(|(aliases, bounds, verified)| FaceInfo { aliases, bounds, verified }).collect();
            names.push(Arc::new(Names { faces: Arc::new(faces), info }));
        }
        let mut sketches = Vec::new();
        for s in list("sketches")? {
            let sketch: solidcraft_sketch::Sketch = serde_json::from_value(parse(s.get("sketch"))?).map_err(|_| bad("saved model: sketch"))?;
            sketches.push(Arc::new(SolvedSketch {
                feature: s.get("feature").and_then(Json::as_u64).ok_or_else(|| bad("saved model: sketch"))?,
                name: text(s.get("name"))?,
                plane: serde_json::from_value(parse(s.get("plane"))?).map_err(|_| bad("saved model: sketch"))?,
                report: serde_json::from_value(parse(s.get("report"))?).map_err(|_| bad("saved model: sketch"))?,
                profiles: find_profiles(&sketch),
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
            b.transferred = r.get("transferred").and_then(Json::as_bool).unwrap_or(false);
            // Measured once here, so every state holding it adds it up for the cache's budget.
            b.bytes = b.body.retained_bytes();
            if let Some(look) = r.get("imported") {
                let faces: Vec<(String, [f32; 3], f32)> = serde_json::from_value(parse(look.get("faces"))?).map_err(|_| bad("saved model: look"))?;
                b.imported = Some(Arc::new(crate::appearance::ImportedLook {
                    color: serde_json::from_value(parse(look.get("color"))?).map_err(|_| bad("saved model: look"))?,
                    opacity: serde_json::from_value(parse(look.get("opacity"))?).map_err(|_| bad("saved model: look"))?,
                    faces: faces.into_iter().map(|(name, color, opacity)| (name, (color, opacity))).collect(),
                }));
            }
            model_bodies.push(b);
        }
        let mut sheets = Vec::new();
        for s in list("sheets")? {
            sheets.push(serde_json::from_value::<crate::sheet::SheetBody>(s.clone()).map_err(|_| bad("saved model: sheet"))?);
        }
        let mut states = Vec::new();
        let mut held = 0usize;
        for s in list("states")? {
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
            let mut sketch_keys = BTreeMap::new();
            for k in s.get("sketch_keys").and_then(Json::as_array).ok_or_else(|| bad("saved model: state"))? {
                sketch_keys
                    .insert(k.get(0).and_then(Json::as_u64).ok_or_else(|| bad("saved model: state"))?, unhex(k.get(1).unwrap_or(&Json::Null))?);
            }
            let st = ModelState {
                frame_bodies: Default::default(),
                bodies: ids("bodies", model_bodies.len())?.into_iter().filter_map(|i| model_bodies.get(i).cloned()).collect(),
                sketches: ids("sketches", sketches.len())?.into_iter().filter_map(|i| sketches.get(i).cloned()).collect(),
                body_counter: index(s.get("body_counter"), usize::MAX)?,
                reserved_body_names: s
                    .get("reserved_body_names")
                    .and_then(Json::as_array)
                    .map(|a| a.iter().filter_map(|n| n.as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
                sheets: s
                    .get("sheets")
                    .and_then(Json::as_array)
                    .ok_or_else(|| bad("saved model: state"))?
                    .iter()
                    .map(|i| index(Some(i), sheets.len()).and_then(|i| sheets.get(i).cloned().ok_or_else(|| bad("saved model: sheet"))))
                    .collect::<Result<Vec<_>>>()?,
                construct: serde_json::from_value(field("construct")?).map_err(invalid)?,
                threads: serde_json::from_value(field("threads")?).map_err(invalid)?,
                sketch_keys,
                blend_edges: serde_json::from_value(field("blend_edges")?).map_err(invalid)?,
                fitted: serde_json::from_value(field("fitted")?).map_err(invalid)?,
                tapered: serde_json::from_value(field("tapered")?).map_err(invalid)?,
                // A join's notice is for the evaluation that made it, not kept with the cache.
                kept_separate: BTreeMap::new(),
            };
            held = held.saturating_add(st.bodies.len()).saturating_add(st.sketches.len()).saturating_add(st.sheets.len());
            if held > MAX_HELD {
                return Err(bad("saved model: states"));
            }
            states.push(Arc::new(st));
        }
        Ok(states)
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
        // The sheet bodies, each written once and named by place in the states: all kept.
        let sheets = list("sheets").cloned().unwrap_or_default();
        let mut used = bytes(sheets.as_slice()).iter().fold(0usize, |t, b| t.saturating_add(*b));
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
                    for &(piece, bytes) in pieces.get(*body_of.get(m)?)? {
                        if !stored.contains(&piece) {
                            adds.insert(piece, bytes);
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
            *kernel_used.get_mut(*body_of.get(m)?)? = true;
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
        let records = json!({
            "features": out_features, "states": out_states, "model_bodies": out_bodies, "names": kept_of(names.as_slice(), &names_used),
            "sketches": kept_of(sketches.as_slice(), &sketches_used), "sheets": sheets,
        });
        let kept: Vec<usize> = (0..self.bodies.len()).filter(|i| kernel_used.get(*i) == Some(&true)).collect();
        let bodies = kept.iter().filter_map(|i| self.bodies.get(*i).cloned()).collect();
        Some((SavedModel { records, bodies }, kept, used))
    }
}

impl Model {
    /// The model a design file kept for `doc` ([`Model::saved`]), taken as `doc`'s evaluation:
    /// every feature's result as saved, nothing evaluated but the features whose state the file
    /// did not keep ([`SavedModel::thinned`]). Refused (an error) unless every feature's
    /// fingerprint is the one `doc` gives it now, and every record is well formed.
    pub fn adopt(doc: &Document, saved: &SavedModel) -> Result<Model> {
        let mut states = saved.states()?;
        let frame_bodies = Arc::new(std::sync::Mutex::new(FrameBodies::default()));
        for state in &mut states {
            Arc::make_mut(state).frame_bodies = Arc::downgrade(&frame_bodies);
        }
        let records = &saved.records;
        let index = |v: Option<&Json>, len: usize| {
            v.and_then(Json::as_u64).and_then(|i| usize::try_from(i).ok()).filter(|i| *i < len).ok_or_else(|| bad("saved model: index"))
        };
        let text =
            |v: Option<&Json>| v.and_then(Json::as_str).filter(|s| s.len() <= 1 << 16).map(str::to_owned).ok_or_else(|| bad("saved model: text"));
        let features = records.get("features").and_then(Json::as_array).ok_or_else(|| bad("saved model: features"))?;
        if features.len() != doc.features.len() {
            return Err(bad("saved model: features"));
        }
        let (vals, param_errors) = evaluation_values(doc);
        let step_digests = StepDigests::of(doc, &StepDigests::default());
        let marker = doc.marker.unwrap_or(usize::MAX);
        let ctx = context_digest(doc, &vals);
        let mut before = Arc::new(ModelState::default());
        // Whether `before` is the state before the feature: one the file left out only stands in.
        let mut exact = true;
        let mut prev = 0;
        let mut results = Vec::with_capacity(features.len());
        for (i, (f, r)) in doc.features.iter().zip(features).enumerate() {
            let saved = unhex(r.get("fingerprint").unwrap_or(&Json::Null))?;
            // A fingerprint reads the state before its feature (where the bodies it takes lie).
            // After a state the file left out, the saved one is taken: `evaluate` works that state
            // out, checks the fingerprints after it, and evaluates again from one that differs.
            prev = if exact { chain_fingerprint(doc, &vals, f, prev, i >= marker, &step_digests, Some(&before)) } else { saved };
            if r.get("id").and_then(Json::as_u64) != Some(f.id) || saved != prev {
                return Err(bad("saved model: the document changed"));
            }
            let ms = r.get("ms").and_then(Json::as_f64).filter(|ms| ms.is_finite()).ok_or_else(|| bad("saved model: time"))?;
            // A file without room for every state keeps some features without one (#3220).
            let state =
                r.get("state").map(|s| states.get(index(Some(s), states.len())?).cloned().ok_or_else(|| bad("saved model: state"))).transpose()?;
            let result = FeatureResult {
                id: f.id,
                name: f.name.clone(),
                error: None,
                warning: r.get("warning").map(|w| text(Some(w))).transpose()?,
                import_report: r
                    .get("import_report")
                    .map(|v| serde_json::from_value::<kernel::ImportReport>(v.clone()).map(Arc::new).map_err(|_| bad("saved model: import report")))
                    .transpose()?,
                ms,
                skipped: r.get("skipped").and_then(Json::as_bool).ok_or_else(|| bad("saved model: skipped"))?,
                stale: false,
                fingerprint: prev,
                unkept: state.is_none(),
                state: state.unwrap_or_else(|| before.clone()),
            };
            if !result.skipped {
                exact = !result.unkept;
                before = result.state.clone();
            }
            results.push(result);
        }
        let mut model = Model { results, param_errors, step_digests, context: ctx, frame_bodies, ..Model::default() };
        let unkept = model.results.iter().filter(|r| r.unkept).count();
        if unkept > 0 {
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
        if let Ok(mut cache) = model.cache.lock() {
            for (f, r) in doc.features.iter().zip(&model.results).filter(|(_, r)| !r.skipped && r.error.is_none()) {
                let local = fingerprint(doc, 0, f, &vals, false, &model.step_digests) ^ placement_digest(doc, f, Some(&before));
                cache.put(
                    cache_key(f, r.fingerprint, local, ctx, &before),
                    CachedEval {
                        error: None,
                        warning: r.warning.clone(),
                        import_report: r.import_report.clone(),
                        ms: r.ms,
                        bytes: r.state.retained_bytes(),
                        state: r.state.clone(),
                    },
                );
                before = r.state.clone();
            }
        }
        Ok(model)
    }
}
