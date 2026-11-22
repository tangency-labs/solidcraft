//! Drawing objects presented through the shared Browser.
use egui::{Id, Rect};
use solidcraft_engine::doc::drawing::{
    Drawing, Sheet, table,
    view::{View, ViewKind},
};
use solidcraft_engine::doc::ids::encode;
use solidcraft_engine::geom::{PlaneGeom, drawing_view::NamedView};
use solidcraft_theme::Tokens;
use std::collections::HashMap;

use crate::SolveApp;
use crate::browser::Actions;
use crate::ui_kit::{
    Mark, Row, Text,
    browser::{Dragged, Drops, Event, Node, NodeMenu, Payload, RowOut, Rows, Source, Zone, order_before},
};

use super::{MenuRow, active_sheet, edit_object, open_sheet_menu, select_sheet};

/// What a drag of a sheet's row carries: the drawing and the sheet.
#[derive(Clone, Copy, Debug)]
struct SheetDrag {
    drawing: u64,
    sheet: u64,
}

pub(crate) fn show(app: &mut SolveApp, rows: &mut Rows<'_>, acts: &mut Actions) {
    let t = Tokens::get();
    rows.ui().add_space(t.space_4);
    if let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() {
        let key = format!("drawing:browser:{}", encode(drawing.id));
        // A sheet dropped on the drawing's row goes last, as an item dropped on a folder's row does.
        let accept = |dragged: &Dragged, zone: Zone| zone == Zone::Into && takes(app, &drawing, dragged, None);
        let node = Node {
            key: &key,
            row: Row { icon: "ws_drawing", label: &drawing.name, name: true, ..Row::default() },
            drops: Drops { before: false, into: true },
            menu: NodeMenu::Host,
            accept: Some(&accept),
            ..Node::default()
        };
        let (open, events) = branch(app, rows, node, true);
        for event in events {
            match event {
                Event::Menu { at, .. } => open_sheet_menu(app, drawing.id, MenuRow::Drawing, Rect::from_min_max(at, at)),
                Event::Move { source, data, .. } => drop_sheet(acts, &drawing, source, data.as_ref(), None),
                _ => {}
            }
        }
        if open {
            let active = active_sheet(app);
            for sheet in &drawing.sheets {
                show_sheet(app, rows, acts, &drawing, sheet, active == Some(sheet.id));
            }
        }
    } else {
        crate::ui_kit::text(rows.ui(), app.tr_label("No drawing objects"), Text::Note("drawing_objects"));
    }

    let label = app.tr_label("Source design").to_string();
    let source =
        Node { key: "drawing:browser:source", row: Row { icon: "ws_design", label: &label, name: true, ..Row::default() }, ..Node::default() };
    if branch(app, rows, source, false).0 {
        let name = app.session.doc.name.clone();
        let node = row(
            rows,
            Node {
                key: "drawing:browser:source-design",
                row: Row { depth: 1, icon: "ws_design", label: &name, name: true, ..Row::default() },
                ..Node::default()
            },
        );
        if node.clicked || node.double {
            app.start("document.metadata");
        }
    }
    rows.ui().add_space(t.space_4);
}

/// Whether `object` of `drawing` is part of the selection: its row is drawn selected, as a
/// selected body's is in the design's browser.
fn picked(app: &SolveApp, drawing: &Drawing, object: u64) -> bool {
    app.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Drawing { drawing: d, object: o } if *d == drawing.id && *o == object))
}

/// The row of something on a sheet takes the clicks its place on the sheet takes: a click
/// selects it (Shift and Ctrl add and toggle, as in the model), and a right-click opens the
/// sheet's menu on it. A double-click is the caller's: it opens the object's options.
fn object_clicks(app: &mut SolveApp, rows: &mut Rows<'_>, drawing: &Drawing, sheet: u64, object: u64, hit: &RowOut) {
    if !drawing.has_object(object) {
        return;
    }
    let sel = solidcraft_engine::Sel::Drawing { drawing: drawing.id, object };
    let held = crate::tool_modifiers::Held::from_modifiers(rows.ui().input(|input| input.modifiers));
    if let Some(at) = hit.secondary {
        select_sheet(app, sheet);
        crate::context_menu::open_on_sheet(app, at, held, Some(sel));
    } else if hit.clicked && !hit.double {
        select_sheet(app, sheet);
        let (add, toggle) = crate::tool_modifiers::selection_click(held, app.preferences.selection_shift_add_only);
        crate::viewport::select(app, Some(sel), add, toggle);
    }
}

/// Draw `node`, its id made from its key.
fn row(rows: &mut Rows<'_>, node: Node<'_>) -> RowOut {
    let (key, publish) = (node.key, matches!(node.menu, NodeMenu::Host));
    let result = rows.row(Node { id: crate::browser::node_id(key), ..node });
    if publish
        && result.widget.is_some()
        && let Some(rect) = result.rect
    {
        crate::scenario::publish_target(key, rect);
    }
    result
}

fn fold_id(app: &SolveApp, rows: &mut Rows<'_>, key: &str) -> Id {
    rows.ui().make_persistent_id((app.session.uid, key))
}

/// A folding row: whether it is open, and its other events.
fn branch(app: &SolveApp, rows: &mut Rows<'_>, mut node: Node<'_>, default_open: bool) -> (bool, Vec<Event>) {
    let id = fold_id(app, rows, node.key);
    let mut open = rows.ui().data(|data| data.get_temp::<bool>(id)).unwrap_or(default_open);
    node.row.fold = Some(open);
    let mut events = Vec::new();
    for event in row(rows, node).events {
        if let Event::Fold { open: next, .. } = event {
            rows.ui().data_mut(|data| data.insert_temp(id, next));
            open = next;
        } else {
            events.push(event);
        }
    }
    (open, events)
}

fn show_sheet(app: &mut SolveApp, rows: &mut Rows<'_>, acts: &mut Actions, drawing: &Drawing, sheet: &Sheet, selected: bool) {
    let key = format!("drawing:browser:sheet:{}", encode(sheet.id));
    let id = fold_id(app, rows, &key);
    let mut open = rows.ui().data(|data| data.get_temp::<bool>(id)).unwrap_or(selected);
    // A sheet dropped on this row goes just before this sheet.
    let before = Some(sheet.id);
    let accept = |dragged: &Dragged, zone: Zone| zone == Zone::Before && takes(app, drawing, dragged, before);
    let hit = row(
        rows,
        Node {
            key: &key,
            row: Row {
                depth: 1,
                fold: Some(open),
                icon: "ws_drawing",
                label: &sheet.name,
                name: true,
                selected,
                mark: if selected { Mark::Here } else { Mark::None },
                ..Row::default()
            },
            drag: Some(crate::browser::DRAG_SHEETS),
            drops: Drops { before: true, into: false },
            menu: NodeMenu::Host,
            accept: Some(&accept),
            ..Node::default()
        },
    );
    if hit.drag_started {
        let dragged = SheetDrag { drawing: drawing.id, sheet: sheet.id };
        rows.drag_with(vec![crate::browser::node_id(&key)], Some(Payload::new(dragged)));
    }
    for event in hit.events {
        match event {
            Event::Fold { open: next, .. } => {
                open = next;
                rows.ui().data_mut(|data| data.insert_temp(id, open));
            }
            Event::Click(_) | Event::Open(_) => select_sheet(app, sheet.id),
            Event::Menu { at, .. } => open_sheet_menu(app, drawing.id, MenuRow::Sheet(sheet.id), Rect::from_min_max(at, at)),
            Event::Move { source, data, .. } => drop_sheet(acts, drawing, source, data.as_ref(), before),
            _ => {}
        }
    }
    if open {
        show_objects(app, rows, drawing, sheet, &key);
    }
}

/// The sheet a drag carries, when it carries one.
fn sheet_drag(data: Option<&Payload>) -> Option<&SheetDrag> {
    data?.get::<SheetDrag>()
}

/// Whether `dragged` is a sheet of this drawing, from this document, whose drop before the sheet
/// `before` (last without one) changes the order.
fn takes(app: &SolveApp, drawing: &Drawing, dragged: &Dragged, before: Option<u64>) -> bool {
    crate::browser::same_document(app, dragged)
        && sheet_drag(dragged.data.as_ref()).is_some_and(|drag| drag.drawing == drawing.id && moved_order(drawing, drag.sheet, before).is_some())
}

/// A sheet dropped on a row (its drop rule took it): the new order goes with the Browser's other
/// drops, run once the frame is drawn.
fn drop_sheet(acts: &mut Actions, drawing: &Drawing, source: Source, data: Option<&Payload>, before: Option<u64>) {
    if let Some(order) = sheet_drag(data).and_then(|drag| moved_order(drawing, drag.sheet, before)) {
        acts.drop_sheets(source, order);
    }
}

/// The drawing's sheet ids with `sheet` moved to just before `before` (last without one), when
/// the drawing has that sheet and the order changes.
fn moved_order(drawing: &Drawing, sheet: u64, before: Option<u64>) -> Option<Vec<u64>> {
    let order: Vec<u64> = drawing.sheets.iter().map(|s| s.id).collect();
    let moved = order_before(&order, &[sheet], before.as_ref());
    (order.contains(&sheet) && moved != order).then_some(moved)
}

fn show_objects(app: &mut SolveApp, rows: &mut Rows<'_>, drawing: &Drawing, sheet: &Sheet, key: &str) {
    for (kind, label, icon, present) in [
        ("paper", "ui.drawing_browser_sheet_border", "", true),
        ("border", "ui.drawing_browser_drawing_border", "", sheet.border.is_some()),
        ("title-block", "ui.drawing_browser_title_block", "drawing_edit_table", sheet.title_block.is_some()),
    ] {
        if present {
            let label = app.tr(label).to_string();
            row(
                rows,
                Node { key: &format!("{key}:{kind}"), row: Row { depth: 2, label: &label, icon, name: true, ..Row::default() }, ..Node::default() },
            );
        }
    }

    show_views(app, rows, drawing, sheet.id, key);
    let objects = drawing.on_sheet(sheet.id);
    for (kind, icon, ids) in [
        ("table", "drawing_edit_table", objects.tables),
        ("dimension", "dimension", objects.dimensions),
        ("balloon", "", objects.balloons),
        ("note", "", objects.notes),
        ("image", "", objects.images),
        ("revision_tag", "", objects.tags),
    ] {
        for (index, object) in ids.into_iter().enumerate() {
            let table_kind = (kind == "table").then(|| drawing.tables.by_id(object)).flatten().map(|table| &table.kind);
            let (label, editor) = match table_kind {
                Some(table::Kind::PartsList(_)) => (app.tr("ui.drawing_browser_parts_list").to_string(), Some("parts_list")),
                Some(table::Kind::Revision) => (app.tr("ui.drawing_browser_revision_table").to_string(), None),
                None => {
                    let msg = format!("ui.drawing_browser_{kind}");
                    let label = solidcraft_ui_text::format(&msg, &[("number", &(index + 1).to_string())]);
                    (label, matches!(kind, "dimension" | "note").then_some(kind))
                }
            };
            let hit = row(
                rows,
                Node {
                    key: &format!("{key}:{kind}:{}", encode(object)),
                    row: Row { depth: 2, label: &label, icon, name: true, selected: picked(app, drawing, object), ..Row::default() },
                    menu: NodeMenu::Host,
                    ..Node::default()
                },
            );
            object_clicks(app, rows, drawing, sheet.id, object, &hit);
            if let Some(editor) = editor
                && hit.events.iter().any(|event| matches!(event, Event::Open(_)))
            {
                select_sheet(app, sheet.id);
                edit_object(app, editor, object);
            }
        }
    }
}

fn show_views(app: &mut SolveApp, rows: &mut Rows<'_>, drawing: &Drawing, sheet: u64, key: &str) {
    let mut children: HashMap<Option<u64>, Vec<(usize, &View)>> = HashMap::new();
    for (index, view) in drawing.views.iter().filter(|view| view.sheet == sheet).enumerate() {
        children.entry(view.kind.parent()).or_default().push((index + 1, view));
    }
    let mut stack = Vec::new();
    if let Some(roots) = children.remove(&None) {
        stack.extend(roots.into_iter().rev().map(|(number, view)| (2, number, view)));
    }
    while let Some((depth, number, view)) = stack.pop() {
        let key = format!("{key}:view:{}", encode(view.id));
        let id = fold_id(app, rows, &key);
        let nested = children.remove(&Some(view.id));
        let open = rows.ui().data(|data| data.get_temp::<bool>(id)).unwrap_or(true);
        let label = view_name(app, drawing, view, number);
        let hit = row(
            rows,
            Node {
                key: &key,
                row: Row {
                    depth,
                    fold: nested.as_ref().map(|_| open),
                    icon: "drawing_base_view",
                    label: &label,
                    name: true,
                    selected: picked(app, drawing, view.id),
                    ..Row::default()
                },
                menu: NodeMenu::Host,
                ..Node::default()
            },
        );
        object_clicks(app, rows, drawing, sheet, view.id, &hit);
        let mut open = open;
        for event in hit.events {
            match event {
                Event::Open(_) => {
                    select_sheet(app, sheet);
                    edit_object(app, "view", view.id);
                }
                Event::Fold { open: next, .. } => {
                    open = next;
                    rows.ui().data_mut(|data| data.insert_temp(id, open));
                }
                _ => {}
            }
        }
        if open && let Some(nested) = nested {
            stack.extend(nested.into_iter().rev().map(|(number, view)| ((depth + 1).min(5), number, view)));
        }
    }
}

/// View `id` of the open drawing as the browser calls it, for anything that names a view to the
/// person at the screen: one name for a view everywhere.
pub(crate) fn name_of(app: &SolveApp, id: u64) -> Option<String> {
    let drawing = app.session.drawing("drawing.inspect").ok()?;
    let view = drawing.view(id)?;
    let number = drawing.views.iter().filter(|other| other.sheet == view.sheet).position(|other| other.id == id)? + 1;
    Some(view_name(app, drawing, view, number))
}

fn view_name(app: &SolveApp, drawing: &Drawing, view: &View, number: usize) -> String {
    // A view with a label is called what its label says: its name, else its words and letter
    // (`DETAIL A`), never nothing.
    if let Some(label) = &view.label {
        if !label.name.is_empty() {
            return label.name.clone();
        }
        let called = format!("{} {}", label.words, label.letter);
        if !called.trim().is_empty() {
            return called.trim().to_string();
        }
    }
    if matches!(view.kind, ViewKind::Base { .. } | ViewKind::Projected { .. })
        && let Ok(resolved) = drawing.resolve_view(view.id)
        && let Some(named) = NamedView::ALL.into_iter().find(|named| named.basis().normal() == resolved.basis.normal())
    {
        let key = match named {
            NamedView::Front => "cmd.view_front",
            NamedView::Back => "cmd.view_back",
            NamedView::Top => "cmd.view_top",
            NamedView::Bottom => "cmd.view_bottom",
            NamedView::Left => "cmd.view_left",
            NamedView::Right => "cmd.view_right",
            _ => "cmd.view_isometric",
        };
        return app.tr(key).to_string();
    }
    solidcraft_ui_text::format("ui.drawing_browser_view", &[("number", &number.to_string())])
}
