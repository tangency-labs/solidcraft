//! Shared tree content, independent of the panels which present it.
//!
//! A frame has one app-owned begin/finalize pair. Each presentation owns its range order,
//! anchor, geometry and transient input; the document, selection, folds and row cache remain
//! in `SolveApp`. Hosts and future adapters can render the same content through `show`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use egui::{Rect, Ui};

use crate::SolveApp;

/// A presentation identity, independent of its placement or content composition.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentOwner(pub u64);

impl ContentOwner {
    pub const BROWSER: Self = Self(0);
    pub const HISTORY: Self = Self(1);
}

/// Runtime identity of the document and presentation which produced an interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ContentSource {
    pub(crate) session: u64,
    pub(crate) owner: ContentOwner,
}

#[derive(Clone)]
pub(crate) struct FeatureDrag {
    pub(crate) source: ContentSource,
    pub(crate) features: Vec<u64>,
}

/// Existing row compositions. This interface deliberately contains no panel or host policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentRequest {
    Browser { shrink_height: bool },
    ActiveBodiesOrigin,
}

pub type DrawnRow = (String, Rect, Option<Rect>, Option<bool>);
pub type DrawnOccurrence = (Vec<u64>, String, Rect, bool);

/// Geometry belongs to one presentation and expires at the next semantic frame.
#[derive(Clone, Default)]
pub struct Output {
    pub rows: Vec<DrawnRow>,
    pub occurrences: Vec<DrawnOccurrence>,
    pointer_owned: bool,
}

thread_local! {
    // Compatibility/automation snapshots, published by the owner rather than by each row.
    static PUBLISHED: RefCell<BTreeMap<ContentOwner, Output>> = const { RefCell::new(BTreeMap::new()) };
}

pub fn output(owner: ContentOwner) -> Output {
    PUBLISHED.with(|outputs| outputs.borrow().get(&owner).cloned().unwrap_or_default())
}

#[derive(Default)]
pub(crate) struct OwnerState {
    pub(crate) order: Vec<String>,
    pub(crate) previous: Vec<String>,
    pub(crate) anchor: Option<String>,
    pub(crate) feature_drags: HashMap<egui::Id, FeatureDrag>,
    order_len: usize,
    output: Output,
    actions: Option<crate::browser::Actions>,
    presented: bool,
}

impl OwnerState {
    pub(crate) fn remember(&mut self, key: &str) {
        if let Some(slot) = self.order.get_mut(self.order_len) {
            if slot != key {
                slot.clear();
                slot.push_str(key);
            }
        } else {
            self.order.push(key.to_owned());
        }
        self.order_len += 1;
    }

    pub(crate) fn current_order(&self) -> &[String] {
        self.order.get(..self.order_len).unwrap_or(&[])
    }
}

#[derive(Default)]
pub(crate) struct Frame {
    session: Option<u64>,
    context: Option<egui::Context>,
    owners: BTreeMap<ContentOwner, OwnerState>,
    active: Option<(ContentOwner, ContentRequest)>,
    last_bodies: Vec<String>,
    last_plane: Option<String>,
    plane_underlay: Option<String>,
    finalized: bool,
}

impl Frame {
    pub(crate) fn owner(&self, owner: ContentOwner) -> Option<&OwnerState> {
        self.owners.get(&owner)
    }

    pub(crate) fn current_mut(&mut self) -> &mut OwnerState {
        let owner = self.active.map_or(ContentOwner::BROWSER, |(owner, _)| owner);
        self.owners.entry(owner).or_default()
    }

    pub(crate) fn browser_presentation(&self) -> bool {
        matches!(self.active, Some((ContentOwner::BROWSER, ContentRequest::Browser { .. })))
    }
}

fn active_id() -> egui::Id {
    egui::Id::unique("sc_semantic_content_owner")
}

fn output_id(owner: ContentOwner) -> egui::Id {
    egui::Id::unique(("sc_semantic_content_output", owner))
}

fn capture_id() -> egui::Id {
    egui::Id::unique("sc_semantic_content_eye_owner")
}

#[derive(Clone, Copy)]
struct EyeCapture {
    source: ContentSource,
    widget: egui::Id,
    target: bool,
}

#[derive(Clone, Copy)]
struct RowCapture {
    source: ContentSource,
    widget: egui::Id,
}

#[derive(Clone, Default)]
struct InputClaims {
    frame: u64,
    actions: HashSet<egui::Id>,
}

/// Egui may repeat layout within an input frame. A consumed row action must not execute a
/// second time on that pass (in particular a Ctrl toggle or an eye visibility command).
pub(crate) fn claim_action(ui: &Ui, widget: egui::Id) -> bool {
    let frame = ui.ctx().cumulative_frame_nr();
    ui.ctx().data_mut(|data| {
        let claims = data.get_temp_mut_or_default::<InputClaims>(egui::Id::unique("sc_semantic_content_input_claims"));
        if claims.frame != frame {
            claims.frame = frame;
            claims.actions.clear();
        }
        claims.actions.insert(widget)
    })
}

fn row_capture_id() -> egui::Id {
    egui::Id::unique("sc_semantic_content_row_capture")
}

pub(crate) fn current_source(app: &SolveApp) -> Option<ContentSource> {
    let (owner, _) = app.tree.content.active?;
    (app.tree.content.session == Some(app.session.uid)).then_some(ContentSource { session: app.session.uid, owner })
}

pub(crate) fn same_document(ui: &Ui, source: ContentSource) -> bool {
    ui.ctx().data(|data| data.get_temp::<ContentSource>(active_id())).is_some_and(|active| active.session == source.session)
}

pub(crate) fn source_present(app: &SolveApp, source: ContentSource) -> bool {
    app.tree.content.session == Some(source.session)
        && app.session.uid == source.session
        && app.tree.content.owner(source.owner).is_some_and(|state| state.presented)
}

/// Called while the outgoing session (and its undo stack) is still active. Context is retained
/// solely for cancelling owned egui capture when a tab changes outside the drawing callback.
pub(crate) fn leave_document(app: &mut SolveApp) {
    let context = app.tree.content.context.clone();
    if let (Some(ctx), Some(session)) = (&context, app.tree.content.session) {
        if ctx.data(|data| data.get_temp::<EyeCapture>(capture_id())).is_some_and(|capture| capture.source.session == session) {
            crate::browser::end_eye_drag(app, ctx);
        }
        if let Some(capture) = ctx.data(|data| data.get_temp::<RowCapture>(row_capture_id())).filter(|capture| capture.source.session == session) {
            if ctx.dragged_id() == Some(capture.widget) {
                ctx.stop_dragging();
            }
            ctx.data_mut(|data| data.remove::<RowCapture>(row_capture_id()));
        }
        crate::browser::cancel_content_drag(ctx, session);
        ctx.data_mut(|data| {
            if data.get_temp::<ContentSource>(active_id()).is_some_and(|source| source.session == session) {
                data.remove::<ContentSource>(active_id());
            }
            for owner in app.tree.content.owners.keys() {
                data.remove::<Output>(output_id(*owner));
            }
        });
    }
    crate::browser::discard_eye_press(app);
    let _ = expire_hover(app);
    app.tree.content = Frame { context, ..Default::default() };
    PUBLISHED.with(|outputs| outputs.borrow_mut().clear());
}

/// Also protects callers which replace a Session without passing through Documents.
pub(crate) fn ensure_session(app: &mut SolveApp, ctx: &egui::Context) {
    crate::browser::sync_reveals(app);
    if app.tree.content.session != Some(app.session.uid) {
        leave_document(app);
        app.tree.content.session = Some(app.session.uid);
    }
    app.tree.content.context = Some(ctx.clone());
}

/// An external producer replaced this slot. Its ownership supersedes our previous write even
/// when the new vector contains precisely the same names (Select Other is one such producer).
pub(crate) fn set_external_body_hover(app: &mut SolveApp, bodies: Vec<String>) {
    app.tree.content.last_bodies.clear();
    app.viewport.hover_bodies = bodies;
}

fn expire_hover(app: &mut SolveApp) -> (Vec<String>, Option<String>) {
    let old_bodies = std::mem::take(&mut app.tree.content.last_bodies);
    let old_plane = app.tree.content.last_plane.take();
    for body in &old_bodies {
        if let Some(index) = app.viewport.hover_bodies.iter().position(|candidate| candidate == body) {
            app.viewport.hover_bodies.remove(index);
        }
    }
    let plane_underlay = app.tree.content.plane_underlay.take();
    if old_plane.is_some() && app.viewport.hover_plane == old_plane {
        app.viewport.hover_plane = plane_underlay;
    }
    (old_bodies, old_plane)
}

pub(crate) fn begin(app: &mut SolveApp, ctx: &egui::Context) {
    ensure_session(app, ctx);
    let frame = &mut app.tree.content;
    frame.active = None;
    frame.finalized = false;
    for state in frame.owners.values_mut() {
        if state.presented {
            std::mem::swap(&mut state.previous, &mut state.order);
        }
        state.order_len = 0;
        state.output = Output::default();
        state.actions = None;
        state.presented = false;
    }
    PUBLISHED.with(|outputs| outputs.borrow_mut().clear());
    ctx.data_mut(|data| data.remove::<ContentSource>(active_id()));
    crate::browser::prune_reveals(app);
}

/// Render into an existing UI. Stable item IDs and command paths belong to the row renderers.
pub(crate) fn show(app: &mut SolveApp, ui: &mut Ui, owner: ContentOwner, request: ContentRequest) {
    render_scope(app, ui, owner, request, |app, ui| crate::browser::render_content(app, ui, request));
}

pub(crate) fn render_scope(
    app: &mut SolveApp,
    ui: &mut Ui,
    owner: ContentOwner,
    request: ContentRequest,
    render: impl FnOnce(&mut SolveApp, &mut Ui) -> crate::browser::Actions,
) {
    ensure_session(app, ui.ctx());
    if app.tree.content.owners.get(&owner).is_some_and(|state| state.presented) {
        return;
    }
    app.tree.content.active = Some((owner, request));
    app.tree.content.owners.entry(owner).or_default().presented = presented(ui);
    ui.ctx().data_mut(|data| {
        data.insert_temp(active_id(), ContentSource { session: app.session.uid, owner });
        data.insert_temp(output_id(owner), Output::default());
    });
    let session = app.session.uid;
    let actions = ui.push_id(("semantic_content", session, owner), |ui| render(app, ui)).inner;
    if session != app.session.uid {
        ensure_session(app, ui.ctx());
        return;
    }
    let output = ui.ctx().data_mut(|data| {
        data.remove::<ContentSource>(active_id());
        data.remove_temp::<Output>(output_id(owner)).unwrap_or_default()
    });
    PUBLISHED.with(|outputs| outputs.borrow_mut().insert(owner, output.clone()));
    let state = app.tree.content.owners.entry(owner).or_default();
    state.order.truncate(state.order_len);
    state.output = output;
    state.actions = Some(actions);
    app.tree.content.active = None;
}

pub(crate) fn record_row(ui: &Ui, row: DrawnRow, response: &crate::ui_kit::RowResp) {
    if !presented(ui) {
        return;
    }
    let Some(source) = ui.ctx().data(|data| data.get_temp::<ContentSource>(active_id())) else { return };
    let owner = source.owner;
    ui.ctx().data_mut(|data| {
        let output = data.get_temp_mut_or_default::<Output>(output_id(owner));
        output.rows.push(row);
        output.pointer_owned |= response.hovered || response.resp.as_ref().is_some_and(|response| response.contains_pointer());
    });
    if !response.on_control
        && let Some(response) = &response.resp
    {
        record_capture(ui, response);
    }
}

pub(crate) fn record_capture(ui: &Ui, response: &egui::Response) {
    if response.drag_started()
        && let Some(source) = ui.ctx().data(|data| data.get_temp::<ContentSource>(active_id()))
    {
        ui.ctx().data_mut(|data| data.insert_temp(row_capture_id(), RowCapture { source, widget: response.id }));
    }
}

pub(crate) fn record_occurrence(ui: &Ui, row: DrawnOccurrence) {
    if !presented(ui) {
        return;
    }
    let Some(source) = ui.ctx().data(|data| data.get_temp::<ContentSource>(active_id())) else { return };
    let owner = source.owner;
    ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<Output>(output_id(owner)).occurrences.push(row));
}

/// The kit's eye gesture keeps its existing shared target value; its capture belongs to the
/// presentation where it began, so another copy of a body cannot apply the same drag again.
pub(crate) fn route_response(ui: &Ui, widget: egui::Id, response: &mut crate::ui_kit::RowResp) {
    let Some(source) = ui.ctx().data(|data| data.get_temp::<ContentSource>(active_id())) else { return };
    response.hovered &= response.resp.as_ref().is_some_and(|response| response.contains_pointer());
    let action =
        response.clicked || response.double || response.secondary.is_some() || response.fold || response.eye || response.radio || response.action;
    let visible = presented(ui);
    if !visible {
        response.hovered = false;
        response.resp = None;
    }
    if !visible || (action && !claim_action(ui, widget)) {
        response.clicked = false;
        response.double = false;
        response.secondary = None;
        response.fold = false;
        response.eye = false;
        response.radio = false;
        response.action = false;
    }
    let capture = ui.ctx().data(|data| data.get_temp::<EyeCapture>(capture_id()));
    if let Some(capture) = capture {
        // A layout repeat may run the kit's drag-start branch again after visibility changed.
        // Keep the gesture's original target rather than letting that branch invert it.
        ui.ctx().data_mut(|data| data.insert_temp(crate::ui_kit::eye_drag_id(), capture.target));
    }
    if capture.is_some_and(|capture| capture.source != source) {
        response.eye = false;
    } else if response.eye
        && let Some(response) = response.resp.as_ref().filter(|response| response.drag_started())
    {
        ui.ctx().data_mut(|data| {
            if let Some(target) = data.get_temp::<bool>(crate::ui_kit::eye_drag_id()) {
                data.insert_temp(capture_id(), EyeCapture { source, widget: response.id, target });
            }
        });
    }
}

pub(crate) fn release_eye(ctx: &egui::Context) -> Option<ContentSource> {
    let capture = ctx.data_mut(|data| {
        let capture = data.get_temp::<EyeCapture>(capture_id());
        data.remove::<EyeCapture>(capture_id());
        capture
    })?;
    if ctx.dragged_id() == Some(capture.widget) {
        ctx.stop_dragging();
    }
    Some(capture.source)
}

/// A reveal is acknowledged only after its row is presented in the scroll viewport. An
/// offscreen target requests scrolling again on the next frame until that happens.
pub(crate) fn scroll_acknowledged(ui: &Ui, rect: Rect) -> bool {
    scroll_acknowledged_at(ui, rect, egui::Align::Center)
}

/// [`scroll_acknowledged`] placing `rect` at `align` in the viewport.
pub(crate) fn scroll_acknowledged_at(ui: &Ui, rect: Rect, align: egui::Align) -> bool {
    ui.scroll_to_rect(rect, Some(align));
    presented(ui) && ui.is_rect_visible(rect) && ui.clip_rect().contains(rect.center())
}

fn presented(ui: &Ui) -> bool {
    ui.is_visible() && ui.opacity() > 0.0
}

pub(crate) fn finalize(app: &mut SolveApp, ctx: &egui::Context) {
    ensure_session(app, ctx);
    if app.tree.content.finalized {
        return;
    }
    app.tree.content.finalized = true;
    let mut capture = ctx.data(|data| data.get_temp::<EyeCapture>(capture_id())).map(|capture| capture.source);
    if capture.is_some_and(|source| !source_present(app, source)) {
        crate::browser::end_eye_drag(app, ctx);
        capture = None;
    }
    if let Some(capture) = ctx.data(|data| data.get_temp::<RowCapture>(row_capture_id())) {
        if !source_present(app, capture.source) {
            crate::browser::cancel_content_drag(ctx, capture.source.session);
            if ctx.dragged_id() == Some(capture.widget) {
                ctx.stop_dragging();
            }
            ctx.data_mut(|data| data.remove::<RowCapture>(row_capture_id()));
        } else if !ctx.input(|input| input.pointer.primary_down()) {
            ctx.data_mut(|data| data.remove::<RowCapture>(row_capture_id()));
        }
    }
    // Keep the previous contribution until finalization: floating viewports paint before
    // their overlaid panels. Expire only our contribution, leaving other producers intact.
    let (old_bodies, old_plane) = expire_hover(app);
    // Egui's response hit testing supplies pointer ownership, including occlusion and capture.
    // Stable owner iteration only breaks ties; submission order never decides the winner.
    let winner = app.tree.content.owners.iter().find_map(|(owner, state)| {
        let actions = state.actions.as_ref()?;
        (state.presented
            && actions.session == Some(app.session.uid)
            && capture.is_none_or(|capture| capture.owner == *owner && capture.session == app.session.uid)
            && (state.output.pointer_owned || actions.hovered || actions.hover_plane.is_some()))
        .then_some(*owner)
    });
    let mut effect = None;
    for (owner, state) in &mut app.tree.content.owners {
        if !state.presented {
            state.order.clear();
            state.feature_drags.clear();
        }
        let actions = state.actions.take();
        if Some(*owner) == winner {
            effect = actions;
        }
    }
    if let Some(actions) = effect {
        for body in &actions.hover_bodies {
            if !app.viewport.hover_bodies.contains(body) {
                app.viewport.hover_bodies.push(body.clone());
                app.tree.content.last_bodies.push(body.clone());
            }
        }
        if actions.hover_plane.is_some() && actions.hover_plane != app.viewport.hover_plane {
            app.tree.content.plane_underlay = app.viewport.hover_plane.take();
            app.tree.content.last_plane.clone_from(&actions.hover_plane);
            app.viewport.hover_plane.clone_from(&actions.hover_plane);
        }
        crate::browser::apply_content_actions(app, actions);
    }
    if old_bodies != app.tree.content.last_bodies || old_plane != app.tree.content.last_plane {
        ctx.request_repaint();
    }
}
