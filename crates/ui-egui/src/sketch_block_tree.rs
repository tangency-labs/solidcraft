//! A sketch's blocks are children of their sketch, beside its patterns and projections: one row
//! per block. A click selects its members while the sketch is edited, a double-click opens Edit
//! Block, Delete explodes it, and its menu has Delete Block, which deletes it with its members.

use serde_json::json;
use solidcraft_engine::Sel;

use crate::{
    SolveApp,
    browser::{Row, Spec, node_row},
    context_menu::Target,
    ui_kit::browser::Rows,
};

pub(crate) fn has_blocks(app: &SolveApp, sketch: u64) -> bool {
    app.session.doc.sketch(sketch).is_ok_and(|sk| !sk.blocks.is_empty())
}

/// A block row clicked, with the selection the click made and the document it was made in:
/// Delete explodes the block while neither has changed.
#[derive(Clone, Debug)]
pub(crate) struct Picked {
    sketch: u64,
    block: String,
    selection: Vec<Sel>,
    doc: std::sync::Weak<solidcraft_engine::doc::Document>,
}

/// One block row of a sketch, as the Browser and the vertical feature history list it.
struct Entry {
    id: String,
    label: String,
    selected: bool,
    /// What a click on it selects while its sketch is edited: its member curves.
    members: Vec<Sel>,
}

/// A block's row label: its name, or "Block" and the number of its id.
pub(crate) fn label(block: &solidcraft_engine::sketch::SketchBlock) -> String {
    match &block.name {
        Some(name) => name.clone(),
        None => format!("{} {}", solidcraft_ui_text::tr("ui.block"), block.id.strip_prefix("blk").unwrap_or(&block.id)),
    }
}

/// The ids of the sketch curves in `selection`.
fn selected_curves(selection: &[Sel]) -> std::collections::HashSet<&str> {
    selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// The sketch's blocks, from the stored document, as patterns are listed.
fn entries(app: &SolveApp, sketch: u64) -> Vec<Entry> {
    let Ok(sk) = app.session.doc.sketch(sketch) else { return Vec::new() };
    let active = app.session.active_sketch == Some(sketch);
    let picked: std::collections::HashSet<&str> = if active { selected_curves(&app.session.selection) } else { Default::default() };
    sk.blocks
        .iter()
        .map(|block| {
            let members: Vec<Sel> = block.curves.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect();
            let selected = active && !block.curves.is_empty() && block.curves.iter().all(|c| picked.contains(c.as_str()));
            Entry { id: block.id.clone(), label: label(block), selected, members }
        })
        .collect()
}

/// A block row's events: a double-click opens the sketch (its placement's component active) and
/// Edit Block; a click selects the members while the sketch is edited, else the sketch; a
/// secondary click opens the block's menu.
fn act(
    app: &mut SolveApp,
    sketch: u64,
    occurrence: Option<(u64, &[u64])>,
    entry: Entry,
    (double, clicked, secondary): (bool, bool, Option<egui::Pos2>),
) {
    if double {
        if let Some((component, path)) = occurrence {
            let _ = app.run("component.activate", json!({"component":component,"occurrence":path.last(),"path":path}));
        }
        if app.session.active_sketch != Some(sketch) {
            app.edit_sketch(sketch);
        }
        if app.session.active_sketch == Some(sketch) {
            crate::sketch_modify_ui::edit_block(app, &entry.id);
        }
    } else if clicked {
        select(app, sketch, &entry.id, entry.members);
    }
    if let Some(at) = secondary {
        crate::context_menu::open_for(app, at, Target::SketchBlock { sketch, block: entry.id });
    }
}

/// Select block `block`'s members while its sketch is edited (else the sketch), and remember the
/// pick for Delete.
fn select(app: &mut SolveApp, sketch: u64, block: &str, members: Vec<Sel>) {
    if app.session.active_sketch != Some(sketch) {
        app.tree.picked_block = None;
        let _ = app.run("select.set", json!({"items":[Sel::Feature { id: sketch }]}));
        return;
    }
    if app.run("select.set", json!({"items":members})).is_ok() {
        app.tree.picked_block = Some(Picked {
            sketch,
            block: block.to_string(),
            selection: app.session.selection.clone(),
            doc: std::sync::Arc::downgrade(&app.session.doc),
        });
    }
}

/// Select every member of block `block` of the sketch being edited (Select Block). Delete then
/// deletes them, as for any selection.
pub(crate) fn select_members(app: &mut SolveApp, block: &str) {
    let Some(sketch) = app.session.active_sketch else { return };
    let members = app
        .session
        .doc
        .sketch(sketch)
        .ok()
        .and_then(|sk| sk.blocks.iter().find(|b| b.id == block))
        .map(|b| b.curves.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None }).collect::<Vec<_>>());
    if let Some(members) = members {
        app.tree.picked_block = None;
        let _ = app.run("select.set", json!({"items":members}));
    }
}

/// Forget the picked block row once the document has changed since the pick.
fn forget_stale(app: &mut SolveApp) {
    if app.tree.picked_block.as_ref().is_some_and(|p| !std::sync::Weak::ptr_eq(&p.doc, &std::sync::Arc::downgrade(&app.session.doc))) {
        app.tree.picked_block = None;
    }
}

/// Delete with a block row picked, the document and the selection unchanged since, and that
/// selection still exactly the block's members: the block is exploded, its curves stay. False
/// otherwise, and Delete goes on as usual.
pub(crate) fn delete_picked(app: &mut SolveApp) -> bool {
    let Some(picked) = app.tree.picked_block.take() else { return false };
    let same_doc = std::sync::Weak::ptr_eq(&picked.doc, &std::sync::Arc::downgrade(&app.session.doc));
    if !same_doc || app.session.active_sketch != Some(picked.sketch) || app.session.selection != picked.selection {
        return false;
    }
    let members_picked = app.session.doc.sketch(picked.sketch).ok().and_then(|sk| sk.blocks.iter().find(|b| b.id == picked.block)).is_some_and(|b| {
        let picked_curves = selected_curves(&picked.selection);
        picked_curves.len() == picked.selection.len()
            && b.curves.len() == picked_curves.len()
            && b.curves.iter().all(|c| picked_curves.contains(c.as_str()))
    });
    if !members_picked {
        return false;
    }
    match app.run("sketch.explode_block", json!({"block": picked.block})) {
        Ok(_) => {
            let _ = app.run("select.clear", json!({}));
        }
        Err(e) => app.set_status(e, true),
    }
    true
}

/// The blocks under their sketch in the Browser. `dim`: the sketch is hidden (or a placement
/// above it), so its blocks fade with it.
pub(crate) fn browser_rows(app: &mut SolveApp, rows: &mut Rows<'_>, sketch: u64, depth: usize, occurrence: Option<(u64, &[u64])>, dim: bool) {
    forget_stale(app);
    for entry in entries(app, sketch) {
        let key = format!("block:{sketch}:{}", entry.id);
        let hit = node_row(
            rows,
            Spec { key: &key, ..Default::default() },
            Row { depth, icon: "c_block", label: &entry.label, name: true, selected: entry.selected, dim, ..Default::default() },
        );
        if let Some(rect) = hit.rect {
            crate::scenario::publish_handle(&format!("browser:sketch-block:{sketch}:{}", entry.id), rect.center());
        }
        act(app, sketch, occurrence, entry, (hit.double, hit.clicked, hit.secondary));
    }
}

/// The blocks under their sketch in the vertical feature history.
pub(crate) fn rows(app: &mut SolveApp, ui: &mut egui::Ui, sketch: u64, depth: usize, scope: &str, occurrence: Option<(u64, &[u64])>, dim: bool) {
    forget_stale(app);
    for entry in entries(app, sketch) {
        let response = crate::ui_kit::tree_row(
            ui,
            ui.scope_id().with(("sketch_block", sketch, &entry.id)),
            &Row { depth, icon: "c_block", label: &entry.label, name: true, selected: entry.selected, dim, ..Default::default() },
        );
        if let Some(rect) = response.rect {
            crate::scenario::publish_handle(&format!("{scope}:sketch-block:{sketch}:{}", entry.id), rect.center());
        }
        act(app, sketch, occurrence, entry, (response.double, response.clicked, response.secondary));
    }
}
