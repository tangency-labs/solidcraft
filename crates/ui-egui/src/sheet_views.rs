//! Sheet view identities and opening policy. Authored drawings stay in the document.
use crate::SolveApp;
use solidcraft_engine::doc::ids;
use solidcraft_views::{DesignView, ViewKind};

pub(crate) fn subject(drawing: u64, sheet: u64) -> String {
    format!("{}/{}", ids::encode(drawing), ids::encode(sheet))
}

pub(crate) fn resolve_for_session(session: &solidcraft_engine::Session, subject: Option<&str>) -> Option<(u64, u64)> {
    let subject = subject?;
    if let Some((drawing, sheet)) = subject.split_once('/') {
        let (drawing, sheet) = (ids::decode(drawing)?, ids::decode(sheet)?);
        return session.doc.drawing(drawing)?.sheets.iter().any(|record| record.id == sheet).then_some((drawing, sheet));
    }
    let sheet = ids::decode(subject)?;
    let mut owners = session.doc.drawings.iter().filter(|drawing| drawing.sheets.iter().any(|record| record.id == sheet));
    let owner = owners.next()?;
    owners.next().is_none().then_some((owner.id, sheet))
}

pub(crate) fn filter_selection(session: &solidcraft_engine::Session, owner: (u64, u64), picks: &mut Vec<solidcraft_engine::Sel>) {
    let on = session.doc.drawing(owner.0).map(|drawing| drawing.on_sheet(owner.1));
    picks.retain(|pick| match pick {
        solidcraft_engine::Sel::Drawing { drawing, object } => {
            *drawing == owner.0
                && on.as_ref().is_some_and(|on| {
                    [&on.views, &on.dimensions, &on.balloons, &on.images, &on.tags, &on.tables, &on.notes, &on.sketches, &on.fills]
                        .iter()
                        .any(|objects| objects.contains(object))
                })
        }
        _ => true,
    });
}

/// Resolve an exact sheet identity, with an explicit error for deleted or ambiguous subjects.
pub(crate) fn resolve(app: &SolveApp, subject: Option<&str>) -> Result<(u64, u64), String> {
    if let Some(subject) = subject {
        if let Some((drawing, sheet)) = subject.split_once('/') {
            let drawing = ids::decode(drawing).ok_or_else(|| app.tr("sheet_views.missing").to_owned())?;
            let sheet = ids::decode(sheet).ok_or_else(|| app.tr("sheet_views.missing").to_owned())?;
            if app.session.doc.drawing(drawing).is_some_and(|record| record.sheets.iter().any(|record| record.id == sheet)) {
                return Ok((drawing, sheet));
            }
            return Err(app.tr("sheet_views.missing").to_owned());
        }
        let sheet = ids::decode(subject).ok_or_else(|| app.tr("sheet_views.missing").to_owned())?;
        let mut owners = app.session.doc.drawings.iter().filter(|drawing| drawing.sheets.iter().any(|record| record.id == sheet));
        let owner = owners.next().ok_or_else(|| app.tr("sheet_views.missing").to_owned())?;
        if owners.next().is_some() {
            return Err(app.tr("sheet_views.ambiguous").to_owned());
        }
        return Ok((owner.id, sheet));
    }
    app.session
        .open_drawing
        .and_then(|id| app.session.doc.drawing(id))
        .or_else(|| app.session.doc.drawings.first())
        .and_then(|drawing| drawing.sheets.first().map(|sheet| (drawing.id, sheet.id)))
        .ok_or_else(|| app.tr("sheet_views.missing").to_owned())
}

/// Borrow a Sheet receiver without committing a selection or focus transition.
pub(crate) fn bind(app: &mut SolveApp, view: &DesignView) -> Result<(), String> {
    if view.kind != ViewKind::Sheet {
        return Ok(());
    }
    let (drawing, sheet) = resolve(app, view.subject.as_deref())?;
    crate::drawing_sheet_page::activate_view(app, view.id);
    app.session.open_drawing = Some(drawing);
    crate::drawing_sheet_page::select_sheet(app, sheet);
    Ok(())
}

/// Bind a real focus transition to its drawing receiver, without fitting or authoring anything.
pub(crate) fn focus(app: &mut SolveApp, view: &DesignView) -> Result<(), String> {
    if view.kind != ViewKind::Sheet {
        return Ok(());
    }
    bind(app, view)?;
    crate::drawing_workspace::focus_sheet_selection(app);
    Ok(())
}

/// Open beside the focused view, or focus this window's existing view of the sheet.
pub(crate) fn open_in(app: &mut SolveApp, drawing: u64, sheet: u64, new: bool) -> Result<u64, String> {
    let window = crate::view_windows::opening_window(app);
    crate::view_windows::with_strip(app, Some(window), |app| open_local(app, drawing, sheet, new))
}

fn open_local(app: &mut SolveApp, drawing: u64, sheet: u64, new: bool) -> Result<u64, String> {
    let subject = subject(drawing, sheet);
    resolve(app, Some(&subject))?;
    if !new
        && let Some(id) = app
            .docs
            .views
            .views()
            .iter()
            .find(|view| view.kind == ViewKind::Sheet && resolve(app, view.subject.as_deref()) == Ok((drawing, sheet)))
            .map(|view| view.id)
    {
        crate::view_windows::focus_view(app, id, None)?;
        return Ok(id);
    }
    if (app.preferences.workspace_follows_view || app.preferences.design_sub_tab_sheets)
        && crate::view_windows::layout_views(app).len() >= solidcraft_views::MAX_VIEWS
    {
        return Err(app.tr("sheet_views.limit").to_owned());
    }
    let previous = app.docs.views.active().id;
    let window = app.view_windows.current_window();
    let reply = app.run("tab.view.open", serde_json::json!({"kind":"sheet","subject":subject,"reuse":false,"window":window}))?;
    let id = reply.get("id").and_then(serde_json::Value::as_u64).ok_or_else(|| app.tr("sheet_views.missing").to_owned())?;
    // The existing ground keeps its place; the newly opened Sheet gets a neighbouring card.
    if previous != id && app.docs.views.get(previous).is_some_and(|view| view.kind.can_tile()) {
        crate::documents::view_activate(app, previous);
        crate::documents::change_views(app, |views| {
            let mut next = views.clone();
            let placed = next.fill_empty(0, id) || next.show_beside(id, solidcraft_views::Edge::Right);
            let placed = placed || {
                let park = next
                    .tiles()
                    .filter(|tiles| tiles.len() >= solidcraft_views::MAX_TILES)
                    .and_then(|tiles| tiles.views().into_iter().rev().find(|other| *other != previous && *other != solidcraft_views::EMPTY));
                park.is_some_and(|park| next.unshow(park) && next.show_beside(id, solidcraft_views::Edge::Right))
            };
            if placed {
                *views = next;
            }
        });
        crate::view_windows::focus_view(app, id, None)?;
    }
    Ok(id)
}

/// Window effects of public drawing commands. Headless Session execution has no UI effects.
pub(crate) fn command_opened(app: &mut SolveApp, id: &str, reply: &serde_json::Value) {
    if !app.preferences.design_sub_tab_sheets || !matches!(id, "drawing.open" | "drawing.create" | "drawing.sheet.add") {
        return;
    }
    let drawing =
        if id == "drawing.sheet.add" { app.session.open_drawing } else { reply.get("id").and_then(serde_json::Value::as_str).and_then(ids::decode) };
    let sheet = if id == "drawing.sheet.add" {
        reply.get("sheet")
    } else {
        reply.get("sheets").and_then(|sheets| sheets.get(0)).and_then(|sheet| sheet.get("id"))
    }
    .and_then(serde_json::Value::as_str)
    .and_then(ids::decode);
    if let Some((drawing, sheet)) = drawing.zip(sheet)
        && let Err(error) = open_in(app, drawing, sheet, false)
    {
        app.set_status(error, true);
    }
}

pub(crate) fn menu_entries(app: &SolveApp) -> Vec<(String, String)> {
    app.session
        .doc
        .drawings
        .iter()
        .flat_map(|drawing| {
            drawing.sheets.iter().map(|sheet| (format!("sheet.open:{}", subject(drawing.id, sheet.id)), format!("{} · {}", drawing.name, sheet.name)))
        })
        .collect()
}

pub(crate) fn menu_action(app: &mut SolveApp, action: &str) -> bool {
    if action == "sheet.new" {
        if let Err(error) = crate::drawing_workspace::new_sheet_from_view_menu(app) {
            app.set_status(error, true);
        }
        return true;
    }
    let Some(subject) = action.strip_prefix("sheet.open:") else { return false };
    let result = resolve(app, Some(subject)).and_then(|(drawing, sheet)| open_in(app, drawing, sheet, false));
    if let Err(error) = result {
        app.set_status(error, true);
    }
    true
}
