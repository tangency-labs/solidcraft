//! Runtime projection of the design's sketch editor onto its owning view.
use crate::SolveApp;
use solidcraft_engine::{ViewContext, ViewDescriptor};
use solidcraft_views::{SketchSession, ViewKind};

pub(crate) fn active_sketch(app: &SolveApp) -> Option<u64> {
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    (app.session.sketching_view == Some(painted)).then_some(app.session.active_sketch).flatten()
}

pub(crate) fn sync(app: &mut SolveApp) -> Result<(), String> {
    let open: Vec<_> = crate::view_windows::layout_views(app)
        .into_iter()
        .map(|(window, view)| ViewDescriptor { id: view.id, kind: view.kind, window: u64::from(window), subject: view.subject.clone() })
        .collect();
    let focused = crate::view_windows::focused_view(app);
    let mut recent = vec![focused];
    recent.extend(app.session.view_context.recent().iter().copied().filter(|id| *id != focused && open.iter().any(|view| view.id == *id)));
    app.session.set_view_context(ViewContext::new(open, focused, recent)?)?;
    project(app);
    Ok(())
}

pub(crate) fn prepare_start(app: &mut SolveApp, explicit: Option<u64>) -> Result<u64, String> {
    sync(app)?;
    if explicit.is_none() && !app.session.view_context.descriptors().iter().any(|view| view.kind == ViewKind::Model) {
        if !app.docs.views.active().kind.can_tile() {
            return Err(app.tr("sketch_mode.model_card_unavailable").into());
        }
        let mut next = app.docs.views.clone();
        let previous = next.active().id;
        let id = next.open(ViewKind::Model, None, false);
        next.activate(previous);
        if !next.get(id).is_some_and(|view| view.kind == ViewKind::Model)
            || !(next.fill_empty(0, id) || next.show_beside(id, solidcraft_views::Edge::Right))
        {
            return Err(app.tr("sketch_mode.model_card_unavailable").into());
        }
        crate::documents::change_views(app, |views| *views = next);
        sync(app)?;
    }
    app.session.view_context.resolve_sketch_target(explicit)
}

/// Lifecycle transitions do not participate in linked navigation. Once the user moves the
/// owner camera away from this target, normal camera linking resumes.
pub(crate) fn camera_transition(app: &mut SolveApp, source: u64, camera: solidcraft_viewport::Camera) -> bool {
    let painted = app.viewport.compass_view.unwrap_or_else(|| app.docs.views.active().id);
    if painted != source {
        return false;
    }
    let transition = app.sketch_camera_target.is_some_and(|target| target == camera || app.cam_anim.is_some_and(|animation| animation.to == target));
    if !transition {
        app.sketch_camera_target = None;
    }
    transition
}

pub(crate) fn before_close(app: &mut SolveApp, closing: &[u64]) -> bool {
    if !app.session.sketching_view.is_some_and(|view| closing.contains(&view)) {
        return true;
    }
    app.prepare_sketch_exit();
    app.run("sketch.finish", serde_json::json!({})).is_ok()
}

/// Passive painting updates presentation only; it never records focus or changes the MRU.
pub(crate) fn project(app: &mut SolveApp) {
    let editing = app.session.sketching_view.zip(app.session.active_sketch).map(|(view, sketch)| (view, SketchSession { sketch }));
    let local = editing.filter(|(id, _)| app.docs.views.window_of(*id).is_some());
    app.docs.views.set_editing(local);
    crate::view_windows::sync_sketch_editing(app, editing);
}
