//! A pattern of the bodies of whole components offers to pattern those components instead: a
//! component pattern places each again (one shared part, moved), where a body pattern copies
//! every body's geometry, which for a large assembly takes far longer and far more memory.

use crate::dialogs::{Dialog, pick_depth};
use crate::selection::{BODIES, OCCURRENCES};
use crate::ui_kit;
use solidcraft_engine::{Sel, Session};
use std::collections::{BTreeMap, HashMap, HashSet};

/// What [`whole_placements`] said for one selection of one design.
/// Keyed on the session revision (which activating a component also moves: a pick's depth
/// follows the active component) and on the design itself, kept so that its address is not
/// reused by another.
#[derive(Clone, Debug)]
pub(crate) struct WholeCache {
    objects: Vec<Sel>,
    doc: std::sync::Arc<solidcraft_engine::doc::Document>,
    pub(crate) revision: u64,
    pub(crate) placements: Vec<Vec<u64>>,
}

/// The placements (occurrence paths, at the depth a component pick takes) whose bodies `objects`
/// are, all of them and nothing else: empty unless every object is a body of such a placement
/// and each of those placements is picked whole.
pub(crate) fn whole_placements(s: &Session, objects: &[Sel]) -> Vec<Vec<u64>> {
    let mut picked: HashSet<&str> = HashSet::with_capacity(objects.len());
    for o in objects {
        match o {
            Sel::Body { name } => {
                picked.insert(name.as_str());
            }
            _ => return Vec::new(),
        }
    }
    if picked.is_empty() || s.doc.occurrences.is_empty() {
        return Vec::new();
    }
    // Every body's placement at once: looking each up walks every body of the design.
    let model = s.model.state();
    let frames = s.placed_document().body_frames_with_source(&model);
    let paths: HashMap<&str, &[u64]> = frames.iter().map(|(world, _, _, path, _)| (world.as_str(), path.as_slice())).collect();
    let mut groups: BTreeMap<Vec<u64>, usize> = BTreeMap::new();
    for name in &picked {
        let Some(path) = paths.get(name) else { return Vec::new() };
        let depth = pick_depth(s, path);
        let path: Vec<u64> = path.iter().take(depth).copied().collect();
        if path.is_empty() {
            return Vec::new();
        }
        *groups.entry(path).or_default() += 1;
    }
    for (path, count) in &groups {
        let Ok(bodies) = s.occurrence_bodies_shared(path, true) else { return Vec::new() };
        if bodies.len() != *count || !bodies.iter().all(|b| picked.contains(b.as_str())) {
            return Vec::new();
        }
    }
    groups.into_keys().collect()
}

/// The placements the dialog's body objects are, worked out again only when they or the design
/// change.
fn cached_whole(d: &mut Dialog, s: &Session) -> Vec<Vec<u64>> {
    let objects = d.inputs.first().map(|i| i.items.as_slice()).unwrap_or_default();
    if let Some(c) = &d.whole_components
        && c.revision == s.revision
        && std::sync::Arc::ptr_eq(&c.doc, &s.doc)
        && c.objects == objects
    {
        return c.placements.clone();
    }
    let placements = whole_placements(s, objects);
    d.whole_components = Some(WholeCache { objects: objects.to_vec(), doc: s.doc.clone(), revision: s.revision, placements: placements.clone() });
    placements
}

/// Under a pattern's Object Type: when its bodies are whole components, say so and offer to
/// pattern the components.
pub(crate) fn offer_row(d: &mut Dialog, ui: &mut egui::Ui, s: &Session) {
    if d.inputs.first().is_none_or(|i| i.accept != BODIES || i.items.is_empty()) {
        return;
    }
    let placements = cached_whole(d, s);
    if placements.is_empty() {
        return;
    }
    let note = match placements.len() {
        1 => solidcraft_ui_text::tr("pattern.whole_component").to_string(),
        n => solidcraft_ui_text::format("pattern.whole_components", &[("count", &n.to_string())]),
    };
    ui.add(egui::Label::new(note).wrap());
    if ui_kit::button(ui, solidcraft_ui_text::tr("pattern.use_components"), ui_kit::Button::Normal).clicked()
        && let Some(inp) = d.inputs.first_mut()
    {
        let bodies = std::mem::replace(&mut inp.items, placements.into_iter().map(|path| Sel::Occurrence { path }).collect());
        inp.accept = OCCURRENCES;
        inp.label = "Components";
        // Switching back to Bodies restores the bodies.
        d.other_objects[0] = bodies;
        d.active = 0;
        d.error = None;
    }
}

/// Objects of a pattern picked as Bodies, as the placements they are when switched to
/// Components: each body as the placement a component pick there takes (a body of the design
/// itself, in no placement, is left out).
pub(crate) fn as_placements(s: &Session, objects: &[Sel]) -> Vec<Sel> {
    let model = s.model.state();
    let frames = s.placed_document().body_frames_with_source(&model);
    let paths: HashMap<&str, &[u64]> = frames.iter().map(|(world, _, _, path, _)| (world.as_str(), path.as_slice())).collect();
    let mut out: Vec<Sel> = Vec::new();
    for o in objects {
        let path = match o {
            Sel::Occurrence { path } => Some(path.clone()),
            Sel::Body { name } => paths.get(name.as_str()).map(|path| path.iter().take(pick_depth(s, path)).copied().collect()),
            _ => None,
        };
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            let sel = Sel::Occurrence { path };
            if !out.contains(&sel) {
                out.push(sel);
            }
        }
    }
    out
}
