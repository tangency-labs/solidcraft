//! A sketch's blocks are children of their sketch, beside its patterns and projections: one row
//! per block. A click selects its members while the sketch is edited (Delete then deletes the
//! block with them), a double-click opens Edit Block, and its menu has Edit Contents, Explode
//! Block and Delete Block. A closed block is taken whole: a click on any of its curves or points
//! selects all its members, and a box selects all of them or none.

use std::collections::HashSet;

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

/// Open block `block` of the sketch being edited to edit its contents in place (Edit Contents).
/// The constraints it held that no longer hold where it is, and were removed, are named.
pub(crate) fn edit_contents(app: &mut SolveApp, block: &str) {
    match app.run("sketch.block_open", json!({"block": block})) {
        Ok(reply) => {
            let _ = app.run("select.clear", json!({}));
            notice(app, block, &reply["dropped"], "ui.block_open_dropped_one", "ui.block_open_dropped_other");
        }
        Err(error) => app.set_status(error, true),
    }
}

/// Close the block open for editing its contents (Close Block). The curves that could not join
/// it are named.
pub(crate) fn close_contents(app: &mut SolveApp, block: &str) {
    let name = block_label(app, block);
    match app.run("sketch.block_close", json!({"block": block})) {
        Ok(reply) => notice_named(app, &name, &reply["left_out"], "ui.block_close_left_out_one", "ui.block_close_left_out_other"),
        Err(error) => app.set_status(error, true),
    }
}

/// Block `block`'s label in the sketch being edited.
fn block_label(app: &SolveApp, block: &str) -> String {
    app.session
        .active_sketch
        .and_then(|id| app.session.doc.sketch(id).ok())
        .and_then(|sk| sk.blocks.iter().find(|b| b.id == block).map(label))
        .unwrap_or_else(|| block.to_string())
}

/// A status message naming the ids in `ids` (a reply's list), when there are any.
fn notice(app: &mut SolveApp, block: &str, ids: &serde_json::Value, one: &str, other: &str) {
    let name = block_label(app, block);
    notice_named(app, &name, ids, one, other);
}

fn notice_named(app: &mut SolveApp, name: &str, ids: &serde_json::Value, one: &str, other: &str) {
    let ids: Vec<&str> = ids.as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).collect();
    if ids.is_empty() {
        return;
    }
    let text = solidcraft_ui_text::format(
        if ids.len() == 1 { one } else { other },
        &[("name", name), ("count", &ids.len().to_string()), ("ids", &ids.join(", "))],
    );
    app.set_status(text, false);
}

/// The closed block of the sketch being edited that the picked curve or point `sel` belongs to.
fn closed_block_of<'a>(sk: &'a solidcraft_engine::sketch::Sketch, sel: &Sel) -> Option<&'a solidcraft_engine::sketch::SketchBlock> {
    match sel {
        Sel::SketchCurve { id, sketch: None } | Sel::SketchPoint { id } | Sel::SketchMidpoint { id } => sk.closed_block_with(id),
        _ => None,
    }
}

/// The member curves of `block`, as a selection.
fn member_sels(block: &solidcraft_engine::sketch::SketchBlock) -> impl Iterator<Item = Sel> + '_ {
    block.curves.iter().map(|id| Sel::SketchCurve { id: id.clone(), sketch: None })
}

/// `items` (what a click picks) with each curve or point of a closed block of the sketch being
/// edited put in place of all the block's members: a click takes a block whole, never one of its
/// members. A block open for editing its contents is not taken whole.
pub(crate) fn whole_blocks(session: &solidcraft_engine::Session, items: Vec<Sel>) -> Vec<Sel> {
    let Some(sk) = session.active_sketch.and_then(|id| session.doc.sketch(id).ok()) else { return items };
    if sk.blocks.iter().all(|b| b.is_open()) {
        return items;
    }
    let mut taken = HashSet::new();
    let mut out = Vec::with_capacity(items.len());
    for sel in items {
        match closed_block_of(sk, &sel) {
            Some(block) => {
                if taken.insert(block.id.as_str()) {
                    out.extend(member_sels(block));
                }
            }
            None => out.push(sel),
        }
    }
    out
}

/// What a box over the sketch being edited takes (`items`), with the closed blocks taken whole or
/// not at all: a window or a lasso (`crossing` false) takes a block when it takes every one of
/// its shown curves, a crossing box when it takes any of its curves or points.
pub(crate) fn box_blocks(session: &solidcraft_engine::Session, items: Vec<Sel>, crossing: bool) -> Vec<Sel> {
    let Some(sk) = session.active_sketch.and_then(|id| session.doc.sketch(id).ok()) else { return items };
    if sk.blocks.iter().all(|b| b.is_open()) {
        return items;
    }
    let curves: HashSet<&str> = items
        .iter()
        .filter_map(|s| match s {
            Sel::SketchCurve { id, sketch: None } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let mut touched: HashSet<&str> = HashSet::new();
    let mut out: Vec<Sel> = Vec::with_capacity(items.len());
    for sel in &items {
        match closed_block_of(sk, sel) {
            Some(block) => {
                touched.insert(block.id.as_str());
            }
            None => out.push(sel.clone()),
        }
    }
    let index: std::collections::HashMap<&str, usize> = sk.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    for block in sk.blocks.iter().filter(|b| touched.contains(b.id.as_str())) {
        let shown = |id: &String| index.get(id.as_str()).is_some_and(|i| solidcraft_engine::view::sketch_curve_visible(sk, *i));
        if crossing || block.curves.iter().filter(|id| shown(id)).all(|id| curves.contains(id.as_str())) {
            out.extend(member_sels(block));
        }
    }
    out
}

/// The label of the block open in the sketch being edited, and its id.
pub(crate) fn open_block(app: &SolveApp) -> Option<(String, String)> {
    let sk = app.session.active_sketch.and_then(|id| app.session.doc.sketch(id).ok())?;
    sk.open_block_now().map(|b| (label(b), b.id.clone()))
}
