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
use crate::ui_kit::{
    Mark, Row, Text,
    browser::{Event, Node, NodeMenu, RowOut, Rows},
};

use super::{active_sheet, edit_object, open_sheet_menu, select_sheet};

pub(crate) fn show(app: &mut SolveApp, rows: &mut Rows<'_>) {
    let t = Tokens::get();
    rows.ui().add_space(t.space_4);
    if let Ok(drawing) = app.session.drawing("drawing.inspect").cloned() {
        let key = format!("drawing:browser:{}", encode(drawing.id));
        if branch(app, rows, &key, Row { icon: "ws_drawing", label: &drawing.name, name: true, ..Row::default() }, true) {
            let active = active_sheet(app);
            for sheet in &drawing.sheets {
                show_sheet(app, rows, &drawing, sheet, active == Some(sheet.id));
            }
        }
    } else {
        crate::ui_kit::text(rows.ui(), app.tr_label("No drawing objects"), Text::Note("drawing_objects"));
    }

    let label = app.tr_label("Source design").to_string();
    if branch(app, rows, "drawing:browser:source", Row { icon: "ws_design", label: &label, name: true, ..Row::default() }, false) {
        let name = app.session.doc.name.clone();
        let node = row(
            rows,
            "drawing:browser:source-design",
            Row { depth: 1, icon: "ws_design", label: &name, name: true, ..Row::default() },
            NodeMenu::None,
        );
        if node.clicked || node.double {
            app.start("document.metadata");
        }
    }
    rows.ui().add_space(t.space_4);
}

fn row(rows: &mut Rows<'_>, key: &str, row: Row<'_>, menu: NodeMenu<'_>) -> RowOut {
    let publish = matches!(menu, NodeMenu::Host);
    let result = rows.row(Node { id: crate::browser::node_id(key), key, row, menu, ..Node::default() });
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

fn branch(app: &SolveApp, rows: &mut Rows<'_>, key: &str, mut item: Row<'_>, default_open: bool) -> bool {
    let id = fold_id(app, rows, key);
    let open = rows.ui().data(|data| data.get_temp::<bool>(id)).unwrap_or(default_open);
    item.fold = Some(open);
    for event in row(rows, key, item, NodeMenu::None).events {
        if let Event::Fold { open, .. } = event {
            rows.ui().data_mut(|data| data.insert_temp(id, open));
            return open;
        }
    }
    open
}

fn show_sheet(app: &mut SolveApp, rows: &mut Rows<'_>, drawing: &Drawing, sheet: &Sheet, selected: bool) {
    let key = format!("drawing:browser:sheet:{}", encode(sheet.id));
    let id = fold_id(app, rows, &key);
    let mut open = rows.ui().data(|data| data.get_temp::<bool>(id)).unwrap_or(selected);
    let hit = row(
        rows,
        &key,
        Row {
            depth: 1,
            fold: Some(open),
            icon: "ws_drawing",
            label: &sheet.name,
            name: true,
            selected,
            mark: if selected { Mark::Here } else { Mark::None },
            ..Row::default()
        },
        NodeMenu::Host,
    );
    for event in hit.events {
        match event {
            Event::Fold { open: next, .. } => {
                open = next;
                rows.ui().data_mut(|data| data.insert_temp(id, open));
            }
            Event::Click(_) | Event::Open(_) => select_sheet(app, sheet.id),
            Event::Menu { at, .. } => open_sheet_menu(app, drawing.id, sheet.id, Rect::from_min_max(at, at)),
            _ => {}
        }
    }
    if open {
        show_objects(app, rows, drawing, sheet, &key);
    }
}

fn show_objects(app: &mut SolveApp, rows: &mut Rows<'_>, drawing: &Drawing, sheet: &Sheet, key: &str) {
    for (kind, label, icon, present) in [
        ("paper", "ui.drawing_browser_sheet_border", "lock", true),
        ("border", "ui.drawing_browser_drawing_border", "lock", sheet.border.is_some()),
        ("title-block", "ui.drawing_browser_title_block", "drawing_edit_table", sheet.title_block.is_some()),
    ] {
        if present {
            let label = app.tr(label).to_string();
            row(
                rows,
                &format!("{key}:{kind}"),
                Row { depth: 2, label: &label, icon, name: true, badges: &["lock"], ..Row::default() },
                NodeMenu::None,
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
                &format!("{key}:{kind}:{}", encode(object)),
                Row { depth: 2, label: &label, icon, name: true, ..Row::default() },
                NodeMenu::None,
            );
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
            &key,
            Row { depth, fold: nested.as_ref().map(|_| open), icon: "drawing_base_view", label: &label, name: true, ..Row::default() },
            NodeMenu::None,
        );
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

fn view_name(app: &SolveApp, drawing: &Drawing, view: &View, number: usize) -> String {
    if let Some(label) = &view.label {
        return label.name.clone();
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
