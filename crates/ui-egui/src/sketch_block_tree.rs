//! A sketch's blocks are children of their sketch, beside its patterns and projections: one row
//! per block. A click selects its members while the sketch is edited (Delete then deletes the
//! block with them), a double-click opens Edit Block, and its menu has Explode Block and Delete
//! Block.

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
        select(app, sketch, entry.members);
    }
    if let Some(at) = secondary {
        crate::context_menu::open_for(app, at, Target::SketchBlock { sketch, block: entry.id });
    }
}

/// Select a block's `members` while its sketch is edited, else the sketch.
fn select(app: &mut SolveApp, sketch: u64, members: Vec<Sel>) {
    let items = if app.session.active_sketch == Some(sketch) { members } else { vec![Sel::Feature { id: sketch }] };
    let _ = app.run("select.set", json!({ "items": items }));
}

/// Select every member of block `block` of the sketch being edited (Select Block, a click on its
/// glyph). Delete then deletes them, and with them the block and its axis.
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
        let _ = app.run("select.set", json!({"items":members}));
    }
}

/// The blocks under their sketch in the Browser. `dim`: the sketch is hidden (or a placement
/// above it), so its blocks fade with it.
pub(crate) fn browser_rows(app: &mut SolveApp, rows: &mut Rows<'_>, sketch: u64, depth: usize, occurrence: Option<(u64, &[u64])>, dim: bool) {
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
