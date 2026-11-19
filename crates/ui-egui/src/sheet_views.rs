//! Exact Sheet receiver binding shared with the per-view page renderer.
use crate::SolveApp;
use solidcraft_views::DesignView;

pub(crate) fn subject(drawing: u64, sheet: u64) -> String {
    format!("{}/{}", solidcraft_engine::doc::ids::encode(drawing), solidcraft_engine::doc::ids::encode(sheet))
}

pub(crate) fn resolve(app: &SolveApp, subject: Option<&str>) -> Result<(u64, u64), String> {
    let unavailable = || solidcraft_ui_text::tr("workspace_focus.sheet_unavailable").to_string();
    let subject = subject.ok_or_else(unavailable)?;
    if let Some((drawing, sheet)) = subject.split_once('/') {
        let drawing = solidcraft_engine::doc::ids::decode(drawing).ok_or_else(unavailable)?;
        let sheet = solidcraft_engine::doc::ids::decode(sheet).ok_or_else(unavailable)?;
        return app
            .session
            .doc
            .drawing(drawing)
            .filter(|drawing| drawing.sheets.iter().any(|candidate| candidate.id == sheet))
            .map(|_| (drawing, sheet))
            .ok_or_else(unavailable);
    }
    let sheet = solidcraft_engine::doc::ids::decode(subject).ok_or_else(unavailable)?;
    let mut owners = app
        .session
        .doc
        .drawings
        .iter()
        .filter_map(|drawing| drawing.sheets.iter().any(|candidate| candidate.id == sheet).then_some((drawing.id, sheet)));
    let owner = owners.next().ok_or_else(unavailable)?;
    if owners.next().is_some() {
        return Err(unavailable());
    }
    Ok(owner)
}

pub(crate) fn focus(app: &mut SolveApp, view: &DesignView) -> Result<(), String> {
    let (drawing, sheet) = resolve(app, view.subject.as_deref())?;
    app.session.open_drawing = Some(drawing);
    crate::drawing_sheet_page::select_sheet(app, sheet);
    Ok(())
}
