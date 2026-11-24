//! Remove History (#18698): a body's history replaced by a base feature that holds its B-rep as
//! it is, so it is edited directly from then on (Press Pull, Delete Face, Move Face), as an
//! imported body is.
//!
//! The base feature is an Import (the timeline's base feature), whose STEP the design file keeps
//! as a binary part of its own, as it keeps any import's. The features that built only this body
//! go; one that also builds or changes another body stays, with every feature of this body before
//! it, so the other bodies keep their history and their geometry. When features kept still build
//! the body, a Remove feature takes their body away just before the base feature brings the
//! frozen one in.
//!
//! References to the body's faces and edges (later features, joints, face appearances) are moved
//! to the base feature's faces through the exact correspondence the STEP writer and reader keep
//! for each face. A reference that still fails is reported, never dropped silently.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;
use solidcraft_doc::{BodyName, Document, FeatureKind, Model, ModelBody};

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::schema::browser::{RemoveHistoryBroken, RemoveHistoryParams, RemoveHistoryResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CMD: &str = "body.remove_history";
const CONTRACT: TypedContract<RemoveHistoryParams> = TypedContract::new();
/// How many places in the body's history are tried for the base feature: each is one evaluation.
const MAX_TRIALS: usize = 32;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("body.remove_history", "Remove History", crate::checked!("body.remove_history", remove_history))
    .begins_edit()
    .icon("body")
    .contract(&CONTRACT)
    .output(output_schema_for::<RemoveHistoryResult>)
    .params(
        "body: its name → feature, name: the base feature that now holds the body's B-rep; removed: [the features that went: those that built only this body, and the sketches only they used]; kept: [features that built this body and also build or change other bodies, so they stay]; remove?: the Remove feature added before the base when kept features still build the body; faces, edges, vertices, volume_before, volume_after: the body before and after; broken: [{feature, error}] (features that worked and now fail). One undo step; refused, with why, when nothing can be removed",
    )];

/// The body as it is frozen, and the base feature's STEP with each of its faces' place in it.
struct Frozen {
    step: std::sync::Arc<str>,
    /// For each face of the body (by index), the face of the imported base it became, when the
    /// STEP writer and reader agree on one face exactly.
    faces: Vec<Option<usize>>,
    names: std::sync::Arc<Vec<String>>,
    component: u64,
}

fn freeze(s: &Session, body: &ModelBody) -> Result<Frozen> {
    let exported = solidcraft_kernel::step_export_with_face_ids(&[&body.body], "SolidCraft base body")?;
    let step: std::sync::Arc<str> = exported.text.into();
    let imported = solidcraft_kernel::step_import_shared(&step)?;
    let [read] = imported.bodies.as_slice() else {
        return Err(bad(CMD, format!("{} did not read back as one body from its B-rep ({} bodies)", body.name, imported.bodies.len())));
    };
    // A STEP face split at a seam does not identify just one imported face.
    let mut read_faces: BTreeMap<u64, Option<usize>> = BTreeMap::new();
    for (index, entity) in read.face_origin.iter().enumerate() {
        read_faces.entry(*entity).and_modify(|previous| *previous = None).or_insert(Some(index));
    }
    let written = exported.face_ids.first().map_or(&[][..], Vec::as_slice);
    let faces = (0..body.body.face_count())
        .map(|k| written.get(k).filter(|entity| **entity != 0).and_then(|entity| read_faces.get(entity)).copied().flatten())
        .collect();
    Ok(Frozen { step, faces, names: solidcraft_doc::naming::face_names(body), component: s.doc.model_body_component(body) })
}

/// One way of removing the history: the document, its model and what changed.
struct Trial {
    doc: Document,
    model: Model,
    base: u64,
    remove: Option<u64>,
    removed: Vec<u64>,
    broken: Vec<(String, String)>,
}

/// A body's shape as the checks compare it: faces, edges, vertices, shells and volume.
#[derive(Clone, Copy, Debug)]
struct Shape {
    faces: usize,
    edges: usize,
    vertices: usize,
    shells: usize,
    volume: f64,
}

fn shape(body: &ModelBody) -> Result<Shape> {
    let m = solidcraft_kernel::measure(&body.body)?;
    Ok(Shape { faces: body.body.face_count(), edges: body.body.edge_count(), vertices: body.body.vertex_count(), shells: body.body.shell_count(), volume: m.volume })
}

/// The same topology and, within the kernel's measuring accuracy, the same volume.
fn same_shape(a: Shape, b: Shape) -> bool {
    let tolerance = 1e-6 * a.volume.abs().max(b.volume.abs()).max(1.0);
    (a.faces, a.edges, a.vertices, a.shells) == (b.faces, b.edges, b.vertices, b.shells) && (a.volume - b.volume).abs() <= tolerance
}

/// Each string in `v` that `f` rewrites, rewritten; whether any was.
fn rewrite_strings(v: &mut Value, f: &dyn Fn(&str) -> Option<String>) -> bool {
    match v {
        Value::String(text) => match f(text) {
            Some(new) => {
                *text = new;
                true
            }
            None => false,
        },
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| rewrite_strings(item, f) | changed),
        Value::Object(map) => map.values_mut().fold(false, |changed, item| rewrite_strings(item, f) | changed),
        _ => false,
    }
}

/// `item` with every face or edge name in it moved through `faces`; unchanged when none is.
fn moved<T: serde::Serialize + serde::de::DeserializeOwned>(item: &mut T, rename: &dyn Fn(&str) -> Option<String>) -> Result<()> {
    let mut value = serde_json::to_value(&*item).map_err(|e| bad(CMD, e.to_string()))?;
    if rewrite_strings(&mut value, rename) {
        *item = serde_json::from_value(value).map_err(|e| bad(CMD, e.to_string()))?;
    }
    Ok(())
}

fn remove_history(s: &mut Session, p: &Value) -> Result<Value> {
    let asked = str_(p, "body").map(str::trim).filter(|b| !b.is_empty()).ok_or_else(|| bad(CMD, "`body` must name the body"))?;
    // A placed copy's name means the body it shows.
    let name = s.own_body_name(asked).ok_or_else(|| bad(CMD, format!("no body `{asked}`")))?;
    if s.doc.marker.is_some_and(|m| m < s.doc.features.len()) {
        return Err(bad(CMD, "the history marker is rolled back: move it to the end first"));
    }
    let state = s.model.state();
    let before = state.body(&name).ok_or_else(|| bad(CMD, format!("no body `{name}`")))?.clone();
    if before.body.is_mesh() {
        return Err(bad(CMD, "not supported yet: removing the history of a mesh body"));
    }
    let all = s.body_features();
    let built = all.get(&name).cloned().unwrap_or_default();
    let order: HashMap<u64, usize> = s.doc.features.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
    let shared: BTreeSet<u64> = all.iter().filter(|(body, _)| **body != name).flat_map(|(_, list)| list.iter().copied()).collect();
    // A linked design's or an inserted file's import keeps its component and its link.
    let fixed = |id: u64| {
        s.doc.feature(id).is_none_or(|f| {
            f.inserted || s.doc.links.values().any(|l| l.feature == id) || s.doc.feature_owns_component(f) || matches!(f.kind, FeatureKind::MeshImport { .. })
        })
    };
    let place = |id: &u64| order.get(id).copied().unwrap_or(usize::MAX);
    if let [only] = built.as_slice()
        && s.doc.feature(*only).is_some_and(|f| matches!(f.kind, FeatureKind::Import { .. }))
    {
        return Err(bad(CMD, format!("{name} has no history to remove: it is a base body already")));
    }
    // Every feature of this body before the last one that stays feeds what that one does to the
    // other bodies: those stay too.
    let last_kept = built.iter().filter(|id| shared.contains(id) || fixed(**id)).map(place).max();
    let mut removable: Vec<u64> = built.iter().copied().filter(|id| last_kept.is_none_or(|k| place(id) > k)).collect();
    removable.sort_by_key(place);
    let feature_name = |id: &u64| s.doc.feature(*id).map(|f| f.name.clone()).unwrap_or_default();
    let kept_builders: Vec<u64> = built.iter().copied().filter(|id| !removable.contains(id)).collect();
    if removable.is_empty() {
        let kept: Vec<String> = kept_builders.iter().map(feature_name).collect();
        return Err(bad(CMD, format!("nothing to remove: every feature that built {name} also builds other bodies ({})", kept.join(", "))));
    }
    let last = *removable.last().ok_or_else(|| bad(CMD, "nothing to remove"))?;
    // The body as the last feature that built it left it: anything after that changed none of its
    // geometry.
    let at_last = s.model.result(last).and_then(|r| r.state.body(&name).cloned()).ok_or_else(|| bad(CMD, format!("{name} is not built")))?;
    let frozen = freeze(s, &at_last)?;
    let others: BTreeMap<String, (Shape, u64)> = state
        .bodies
        .iter()
        .filter(|b| b.name != name)
        .map(|b| Ok((b.name.clone(), (shape(b)?, s.doc.model_body_component(b)))))
        .collect::<Result<_>>()?;
    let want = (shape(&before)?, s.doc.model_body_component(&before));
    // The earliest start that keeps every other body as it is wins; a start later keeps more of
    // this body's history.
    let mut refusal = None;
    let mut chosen = None;
    for start in 0..removable.len().min(MAX_TRIALS) {
        let mut best: Option<Trial> = None;
        // With the sketches only the removed features used, then (when that breaks something)
        // without removing them.
        for take_sketches in [true, false] {
            match trial(s, &name, &removable[start..], take_sketches, &frozen, &others, want) {
                Ok(t) => {
                    let fewer = best.as_ref().is_none_or(|b| t.broken.len() < b.broken.len());
                    let clean = t.broken.is_empty();
                    if fewer {
                        best = Some(t);
                    }
                    if clean {
                        break;
                    }
                }
                Err(e) => {
                    refusal.get_or_insert(e);
                    break;
                }
            }
        }
        if let Some(t) = best {
            chosen = Some(t);
            break;
        }
    }
    let Some(t) = chosen else {
        return Err(refusal.unwrap_or_else(|| bad(CMD, "the history could not be removed")));
    };
    let removed: Vec<String> = t.removed.iter().map(feature_name).collect();
    let kept: Vec<String> = built.iter().filter(|id| !t.removed.contains(id)).map(feature_name).collect();
    let base_name = t.doc.feature(t.base).map(|f| f.name.clone()).unwrap_or_default();
    let remove = t.remove.and_then(|id| t.doc.feature(id)).map(|f| f.name.clone());
    let after = t.model.state().body(&name).map(shape).transpose()?.ok_or_else(|| bad(CMD, format!("{name} is not built")))?;
    let broken = t.broken.iter().map(|(feature, error)| RemoveHistoryBroken { feature: feature.clone(), error: error.clone() }).collect();
    let base = t.base;
    s.set_doc(t.doc);
    s.selection.clear();
    s.refresh();
    let reply = RemoveHistoryResult {
        feature: base,
        name: base_name,
        removed,
        kept,
        remove,
        faces: after.faces,
        edges: after.edges,
        vertices: after.vertices,
        volume_before: want.0.volume,
        volume_after: after.volume,
        broken,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| bad(CMD, e.to_string()))
}

/// Remove the features `removable` (with the sketches only they use, when `take_sketches`) and
/// put the base feature where the last of them was. Refused when another body, or this one,
/// would not come out as it is.
fn trial(
    s: &Session,
    name: &str,
    removable: &[u64],
    take_sketches: bool,
    frozen: &Frozen,
    others: &BTreeMap<String, (Shape, u64)>,
    want: (Shape, u64),
) -> Result<Trial> {
    let mut doc = (*s.doc).clone();
    let mut gone: BTreeSet<u64> = removable.iter().copied().collect();
    if take_sketches {
        let sketches: Vec<u64> = doc
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }) && !gone.contains(&f.id))
            .filter(|sketch| {
                let users: Vec<u64> = doc.features.iter().filter(|f| f.kind.input_sketches().contains(&sketch.id)).map(|f| f.id).collect();
                !users.is_empty() && users.iter().all(|u| gone.contains(u))
            })
            .map(|f| f.id)
            .collect();
        gone.extend(sketches);
    }
    let last = removable.last().copied().ok_or_else(|| bad(CMD, "nothing to remove"))?;
    // The body's creator stays: its body goes just before the base brings the frozen one in.
    let creator = s.model.state().body(name).map(|b| b.feature);
    let needs_remove = creator.is_some_and(|c| !gone.contains(&c));
    let taken = |doc: &Document, n: &str| doc.features.iter().any(|f| f.name == n);
    let base_name = solidcraft_doc::copy_name(&format!("{name} Base"), |n| taken(&doc, n));
    let base = doc.add_feature(
        FeatureKind::Import { file: name.to_string(), step: frozen.step.clone(), components: Vec::new(), skip: Vec::new(), records: None },
        Some(&base_name),
    )?;
    if let Some(f) = doc.feature_mut(base) {
        f.component = frozen.component;
        f.body_names = vec![name.to_string()];
    }
    let remove = if needs_remove {
        let remove_name = solidcraft_doc::copy_name(&format!("Remove {name}"), |n| taken(&doc, n));
        let id = doc.add_feature(FeatureKind::Remove { bodies: vec![BodyName(name.to_string())] }, Some(&remove_name))?;
        if let Some(f) = doc.feature_mut(id) {
            f.component = frozen.component;
        }
        Some(id)
    } else {
        None
    };
    // Both were added at the end: they go right after the last feature removed, the Remove first.
    let added: Vec<_> = doc.features.iter().filter(|f| f.id == base || Some(f.id) == remove).cloned().collect();
    doc.features.retain(|f| f.id != base && Some(f.id) != remove);
    let at = doc.feature_index(last).map_or(doc.features.len(), |i| i + 1);
    let mut added = added;
    added.sort_by_key(|f| f.id != remove.unwrap_or(u64::MAX));
    for (k, f) in added.into_iter().enumerate() {
        doc.features.insert(at + k, f);
    }
    // The body's face names, each to the base feature's face it became; a name another body's
    // face also has is left alone.
    let other_names: BTreeSet<String> = s
        .model
        .state()
        .bodies
        .iter()
        .filter(|b| b.name != name && !b.body.is_mesh())
        .flat_map(|b| solidcraft_doc::naming::face_names(b).iter().cloned().collect::<Vec<_>>())
        .collect();
    let mut map: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (k, old) in frozen.names.iter().enumerate() {
        if old.is_empty() || other_names.contains(old) {
            continue;
        }
        let new = frozen.faces.get(k).copied().flatten().map(|i| format!("F{base}:face:{i}"));
        map.entry(old.clone()).and_modify(|previous| *previous = None).or_insert(new);
    }
    let map: BTreeMap<String, Option<String>> = map.into_iter().filter(|(_, new)| new.is_some()).collect();
    let none = BTreeMap::new();
    let none_bodies = BTreeMap::new();
    let rename = |text: &str| -> Option<String> {
        if map.is_empty() || !text.contains(':') || text.len() > 64 * 1024 {
            return None;
        }
        // Only a name the map moves: anything else is left as it was written.
        if solidcraft_doc::remap_topology(text, &none, &none_bodies) != text {
            return None;
        }
        solidcraft_doc::remap_topology_faces(text, &none, &none_bodies, &map).ok().filter(|new| new != text)
    };
    for f in doc.features.iter_mut().filter(|f| !gone.contains(&f.id) && f.id != base && Some(f.id) != remove) {
        moved(f, &rename)?;
    }
    for joint in &mut doc.assembly.joints {
        moved(joint, &rename)?;
    }
    for face in doc.appearances.faces.iter_mut().filter(|face| face.body == name) {
        if let Some(new) = face.name.as_deref().and_then(&rename) {
            face.name = Some(new);
        }
    }
    let mut removed = Vec::new();
    for id in gone.iter().copied().collect::<Vec<_>>() {
        if doc.feature(id).is_some() {
            removed.extend(doc.delete_feature(id)?);
        }
    }
    let mut model = s.model.clone();
    model.evaluate(&doc);
    if let Some(error) = model.result(base).and_then(|r| r.error.clone()) {
        return Err(bad(CMD, format!("the base feature fails: {error}")));
    }
    let after = model.state();
    let mut names: Vec<&str> = after.bodies.iter().map(|b| b.name.as_str()).filter(|n| *n != name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = others.keys().map(String::as_str).collect();
    expected.sort_unstable();
    if names != expected {
        return Err(bad(CMD, format!("removing the history of {name} would change which other bodies the design has")));
    }
    for b in after.bodies.iter().filter(|b| b.name != name) {
        let Some((was, component)) = others.get(&b.name) else { continue };
        if !same_shape(*was, shape(b)?) || doc.model_body_component(b) != *component {
            return Err(bad(CMD, format!("{} is built from {name}'s history, so it would change", b.name)));
        }
    }
    let body = after.body(name).ok_or_else(|| bad(CMD, format!("{name} would not be built")))?;
    let got = shape(body)?;
    if !same_shape(want.0, got) {
        return Err(bad(
            CMD,
            format!(
                "{name}'s B-rep does not come back the same (faces {} → {}, edges {} → {}, vertices {} → {}, volume {} → {})",
                want.0.faces, got.faces, want.0.edges, got.edges, want.0.vertices, got.vertices, want.0.volume, got.volume
            ),
        ));
    }
    if doc.model_body_component(body) != want.1 {
        return Err(bad(CMD, format!("{name} would move to another component")));
    }
    let broken = super::edit::newly_broken_in_history(&s.doc, &s.model, &doc, &model);
    Ok(Trial { doc, model, base, remove, removed, broken })
}
