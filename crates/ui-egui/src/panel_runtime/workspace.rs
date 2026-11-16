//! Shared opt-in workspace renderer. The only mutation path is the retained
//! runtime action sink; resolved geometry and drag previews are observational.
use std::collections::BTreeMap;

use solidcraft_engine::panels::*;

use super::drag_paint::{CardPaint, DragPaint, shape_count};
use super::runtime::{RuntimeHostProposal, standalone_header_action};
use super::*;
use crate::ui_kit::{self, BackingTooltipExt as _, HostChrome};

// Raw input plus per-control focus, gesture and command outcomes, with room for
// pass diagnostics. Repeated passes replace a snapshot rather than growing it.
const MAX_HOST_FRAME_OUTCOMES: usize = 4096 + 3 * (MAX_INSTANCES + MAX_SECTIONS * (MAX_HEADER_ACTIONS + 8)) + 2;

/// The command executor stages the injected catalogue before dispatch takes
/// this exact proposal. No reducer, allocator or controller factory runs here.
fn commit_catalogued_proposal(
    runtime: &mut PanelRuntime,
    proposal: RuntimeHostProposal,
    native: &mut dyn NativeHostAdapter,
    services: &mut ContentServices<'_>,
    catalogue: &mut dyn commands::LayoutCatalogue,
) -> PanelResult<LayoutReply> {
    let action = proposal.action().clone();
    let registry = runtime.layout().registry().clone();
    let mut sink = RetainedCandidateSink { runtime, native, services, proposal: Some(proposal) };
    commands::execute_layout_action(
        action,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue, web: cfg!(target_arch = "wasm32") },
    )
}
fn commit_registered_action(
    runtime: &mut PanelRuntime,
    action: LayoutAction,
    native: &mut dyn NativeHostAdapter,
    services: &mut ContentServices<'_>,
    catalogue: &mut dyn commands::LayoutCatalogue,
) -> PanelResult<LayoutReply> {
    let registry = runtime.layout().registry().clone();
    let mut sink = RuntimeActionSink { runtime, native, services };
    // The shared executor dispatches this standalone owner action before any
    // catalogue access. Its runtime sink still validates the owning binding.
    commands::execute_layout_action(
        action,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue, web: cfg!(target_arch = "wasm32") },
    )
}
struct RetainedCandidateSink<'a, 'services> {
    runtime: &'a mut PanelRuntime,
    native: &'a mut dyn NativeHostAdapter,
    services: &'a mut ContentServices<'services>,
    proposal: Option<RuntimeHostProposal>,
}
impl ActionSink for RetainedCandidateSink<'_, '_> {
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        self.proposal.as_ref().ok_or_else(|| error(Rule::Revision, "host proposal already consumed"))?.validate_preparation_context(context)?;
        self.project(action)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        self.project_in_context(action.clone(), context)?;
        self.dispatch(action)
    }
    fn inspect(&self) -> LayoutReply {
        super::runtime::normalize_native_reply(self.runtime.layout().inspect(), &self.native.capabilities())
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        let proposal = self.proposal.as_ref().ok_or_else(|| error(Rule::Revision, "host proposal already consumed"))?;
        if proposal.action() != &action {
            return Err(error(Rule::Revision, "host proposal intent changed"));
        }
        proposal.validate_captured(self.runtime, self.services.models)?;
        Ok(proposal.layout().clone())
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.project(action)?;
        let proposal = self.proposal.take().ok_or_else(|| error(Rule::Revision, "host proposal already consumed"))?;
        let reply = self.runtime.commit_host_proposal(proposal, self.native, self.services)?;
        Ok(super::runtime::normalize_native_reply(reply, &self.native.capabilities()))
    }
}

#[derive(Clone, Debug)]
pub struct HostOptions {
    pub metrics: HostMetrics,
    pub geometry: HostGeometryPolicy,
    /// Content toolbar space, separate from the shared title and tabs. The
    /// content/body split may fit transiently within the bounded outer host.
    pub content_header_height: f32,
}
impl Default for HostOptions {
    fn default() -> Self {
        Self {
            metrics: HostMetrics { inline_max_height: solidcraft_theme::PANEL_INLINE_MAX_HEIGHT, ..HostMetrics::default() },
            geometry: HostGeometryPolicy::default(),
            content_header_height: solidcraft_theme::Tokens::get().control_height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostSharedAction {
    Focus,
    Close,
    Collapse,
    Move(Zone),
    Join(SectionId),
    Popout,
}
impl std::hash::Hash for HostSharedAction {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Move(zone) => (*zone as u8).hash(state),
            Self::Join(section) => section.hash(state),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HostControlTarget {
    Pointer(HostPointerTarget),
    Shared { instance: InstanceId, action: HostSharedAction },
    Content { instance: InstanceId, action: ControlId },
    PanelMenu { instance: InstanceId },
    Scroll { horizontal: bool },
}

#[derive(Clone, Debug)]
pub struct HostFrameControl {
    pub id: egui::Id,
    pub target: HostControlTarget,
    pub bounds: LogicalRect,
    pub role: InputRole,
    pub accessible_name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub enum HostEvent {
    Regions {
        scope: LocalScope,
        revision: u64,
        painted: Vec<LogicalRect>,
        input: Vec<LogicalRect>,
        occlusion: Vec<LogicalRect>,
        reservations: Vec<LogicalRect>,
    },
    Committed {
        action: LayoutAction,
        reply: LayoutReply,
    },
    PreparedCommitted {
        reply: LayoutReply,
    },
    Rejected {
        action: Option<LayoutAction>,
        error: PanelError,
    },
    ContentAction {
        instance: InstanceId,
        action: ControlId,
        result: PanelResult<()>,
    },
    Focus {
        target: HostControlTarget,
    },
}

/// `layout`/controls describe what was actually painted in this pass. `reply`
/// always describes the authoritative post-dispatch state. A successful change
/// also exposes `next_layout`; repaint is requested immediately, without drawing
/// content twice or inventing widgets at unpainted coordinates.
#[derive(Clone, Debug)]
pub struct HostFrame {
    pub layout: HostLayout,
    pub controls: Vec<HostFrameControl>,
    pub events: Vec<HostEvent>,
    pub preview: Option<HostPreview>,
    pub reply: LayoutReply,
    pub next_layout: Option<HostLayout>,
    pub menu_bounds: Option<LogicalRect>,
    pub owned_overlays: Vec<InputRegion>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HostMenu {
    Panel(InstanceId),
    Tabs(SectionId),
}

enum HostCommand {
    Contextual(ContextualPanelAction, egui::Id),
    Layout(LayoutAction),
    Prepared(Box<PreparedHostLayout>),
    Scroll { horizontal: bool, offset: f32, source: egui::Id },
    WidgetLayout(LayoutAction, egui::Id),
    Content(InstanceId, ControlId, egui::Id),
    OpenMenu { menu: HostMenu, anchor: LogicalRect, source: egui::Id, secondary: bool },
}

struct PendingHostCommands {
    scope: LocalScope,
    window: HostId,
    frame_number: u64,
    allow_actions: bool,
    commands: Vec<HostCommand>,
    pointer_events: Vec<(egui::Id, HostPointerEvent)>,
    mount: PanelResult<()>,
    drag_cards: Vec<CardPaint>,
}

#[derive(Clone)]
struct RegisteredOverlay {
    region: InputRegion,
    layer: Option<egui::LayerId>,
}

#[derive(Clone)]
struct ContentFocus {
    key: ControllerKey,
    scope: LocalScope,
    id: egui::Id,
    visible: egui::Rect,
    // egui collapses a text selection during an unfocused render. Retain only
    // the cursor range, never a copy of the editor's undo/IME/scroll state.
    selection: Option<egui::text::CCursorRange>,
}
impl ContentFocus {
    fn logically_presented(&self, runtime: &PanelRuntime) -> bool {
        let service = runtime.layout();
        self.scope == runtime_scope(runtime)
            && runtime.instance_owner(&self.key.instance).is_ok_and(|owner| owner == self.key.owner)
            && service.availability(&self.key.instance) == Availability::Available
            && service.layout().instance(&self.key.instance).is_ok_and(|instance| instance.requested_open)
            && service.layout().containing(&self.key.instance).is_ok_and(|section| {
                let availability: AvailabilityMap = section.tabs.iter().map(|instance| (instance.clone(), service.availability(instance))).collect();
                !section.collapsed && presented_tab(section, &availability) == Some(&self.key.instance)
            })
    }
    fn restore_selection(&self, ctx: &egui::Context) {
        if let Some(selection) = self.selection
            && let Some(mut state) = egui::TextEdit::load_state(ctx, self.id)
        {
            state.cursor.set_char_range(Some(selection));
            state.store(ctx, self.id);
            ctx.request_repaint();
        }
    }
}

pub struct WorkspacePanelHost {
    options: HostOptions,
    native_capabilities: HostCapabilities,
    interaction: HostInteraction,
    frame: Option<HostFrame>,
    // Executed outcomes survive geometry invalidation only within this egui
    // frame. Regions and provisional candidates never enter the journal.
    outcome_frame: Option<u64>,
    outcomes: Vec<HostEvent>,
    scope: Option<(LocalScope, HostId)>,
    menu: Option<HostMenu>,
    menu_anchor: Option<LogicalRect>,
    last_input_frame: Option<u64>,
    last_mount_frame: Option<u64>,
    last_mount_revision: Option<u64>,
    preview_started: Option<f64>,
    live_drag: bool,
    drag_paint: DragPaint,
    pending_escape: bool,
    last_rejection: Option<PanelError>,
    overlays: BTreeMap<InstanceId, Vec<RegisteredOverlay>>,
    observed_focus: Option<InstanceId>,
    focus_request: Option<InstanceId>,
    egui_context: Option<egui::Context>,
    owned_focus: Option<egui::Id>,
    content_focus: Option<ContentFocus>,
    content_bookmarks: BTreeMap<ControllerKey, ContentFocus>,
    content_reports: egui::IdMap<ContentFocus>,
    restore_content_focus: bool,
    menu_opener: Option<egui::Id>,
    menu_entry: Option<egui::Id>,
    menu_reveal: Option<egui::Id>,
    menu_items: Vec<egui::Id>,
    keyboard_shortcuts: Vec<(ContextualPanelAction, egui::KeyboardShortcut)>,
    pending: Option<PendingHostCommands>,
    last_capacity: Option<LogicalRect>,
    last_domain: Option<LogicalRect>,
    last_chrome: Vec<LogicalRect>,
}

fn validate_requested_opacity(opacity: Option<f32>) -> PanelResult<()> {
    if opacity.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
        return Err(error(Rule::Bounds, "invalid requested panel opacity"));
    }
    Ok(())
}

impl WorkspacePanelHost {
    /// Override host paint opacity for this runtime only, without changing saved layout.
    pub fn set_requested_opacity(&mut self, opacity: Option<f32>) -> PanelResult<()> {
        validate_requested_opacity(opacity)?;
        if self.options.geometry.requested_opacity != opacity {
            self.invalidate_geometry();
            self.options.geometry.requested_opacity = opacity;
            self.options.geometry.surfaces = None;
            self.options.geometry.surface_alpha = None;
        }
        Ok(())
    }

    pub(crate) fn set_header_action_visible(&mut self, instance: InstanceId, action: ControlId, visible: bool) {
        let hidden = &mut self.options.geometry.hidden_header_actions;
        let changed = if visible {
            hidden.get_mut(&instance).is_some_and(|actions| actions.remove(&action))
        } else {
            hidden.entry(instance).or_default().insert(action)
        };
        hidden.retain(|_, actions| !actions.is_empty());
        if changed {
            self.invalidate_geometry();
        }
    }

    pub(crate) fn set_live_drag(&mut self, enabled: bool) {
        self.live_drag = enabled;
        self.interaction.set_live_drag(enabled);
        if !enabled {
            self.drag_paint.clear();
        }
    }

    pub(crate) fn set_gap(&mut self, gap: f32) -> PanelResult<()> {
        let metrics = HostMetrics { gap, ..self.options.metrics };
        metrics.validate()?;
        if metrics != self.options.metrics {
            self.invalidate_geometry();
            self.options.metrics = metrics;
        }
        Ok(())
    }

    pub fn new(options: HostOptions) -> PanelResult<Self> {
        options.metrics.validate()?;
        if !options.content_header_height.is_finite() || options.content_header_height < 0.0 || options.content_header_height > 256.0 {
            return Err(error(Rule::Bounds, "invalid content header extent"));
        }
        if options.geometry.effective_surfaces.len() > MAX_SECTIONS {
            return Err(error(Rule::CollectionLimit, "effective surface policy bound"));
        }
        for section in options.geometry.effective_surfaces.keys() {
            valid_id(&section.0)?;
        }
        validate_requested_opacity(options.geometry.requested_opacity)?;
        Ok(Self {
            options,
            native_capabilities: HostCapabilities::default(),
            interaction: HostInteraction::default(),
            frame: None,
            outcome_frame: None,
            outcomes: Vec::new(),
            scope: None,
            menu: None,
            menu_anchor: None,
            last_input_frame: None,
            last_mount_frame: None,
            last_mount_revision: None,
            preview_started: None,
            live_drag: false,
            drag_paint: DragPaint::default(),
            pending_escape: false,
            last_rejection: None,
            overlays: BTreeMap::new(),
            observed_focus: None,
            focus_request: None,
            egui_context: None,
            owned_focus: None,
            content_focus: None,
            content_bookmarks: BTreeMap::new(),
            content_reports: egui::IdMap::default(),
            restore_content_focus: false,
            menu_opener: None,
            menu_entry: None,
            menu_reveal: None,
            menu_items: Vec::new(),
            keyboard_shortcuts: [
                (ContextualPanelAction::ResizeLeft, egui::Key::ArrowLeft),
                (ContextualPanelAction::ResizeRight, egui::Key::ArrowRight),
                (ContextualPanelAction::ResizeUp, egui::Key::ArrowUp),
                (ContextualPanelAction::ResizeDown, egui::Key::ArrowDown),
            ]
            .into_iter()
            .map(|(action, key)| (action, egui::KeyboardShortcut::new(egui::Modifiers::NONE, key)))
            .collect(),
            pending: None,
            last_capacity: None,
            last_domain: None,
            last_chrome: Vec::new(),
        })
    }
    pub(crate) fn set_keyboard_shortcuts(&mut self, shortcuts: Vec<(ContextualPanelAction, egui::KeyboardShortcut)>) {
        self.keyboard_shortcuts = shortcuts.into_iter().take(ContextualPanelAction::ALL.len() * 8).collect();
    }

    pub(crate) fn contextual_action(&self, runtime: &PanelRuntime, action: ContextualPanelAction) -> PanelResult<LayoutAction> {
        let frame = self.frame.as_ref().ok_or_else(|| error(Rule::Availability, "panel host is not presented"))?;
        let reference = self.command_reference(runtime)?;
        if reference != frame.layout {
            return Err(error(Rule::Revision, "presented panel geometry is stale"));
        }
        self.contextual_in_frame(runtime, frame, action)
    }

    fn contextual_in_frame(&self, runtime: &PanelRuntime, frame: &HostFrame, action: ContextualPanelAction) -> PanelResult<LayoutAction> {
        let ctx = self.egui_context.as_ref().ok_or_else(|| error(Rule::Availability, "panel UI context unavailable"))?;
        if self.scope.as_ref() != Some(&(runtime_scope(runtime), runtime.environment().window.clone()))
            || frame.layout.revision != runtime.layout().inspect().revision
            || ui_kit::Container::modal_input_blocked(ctx)
            || !ctx.input(|input| input.focused)
        {
            return Err(error(Rule::Availability, "panel focus context is interrupted"));
        }
        let focused = ctx.memory(|memory| memory.focused()).ok_or_else(|| error(Rule::Availability, "no panel control focused"))?;
        let response = ctx
            .read_response(focused)
            .filter(|response| response.enabled() && response.interact_rect.is_positive())
            .ok_or_else(|| error(Rule::Availability, "focused panel control unavailable"))?;
        let focus_at = self.content_reports.get(&focused).map_or(response.interact_rect.center(), |report| report.visible.center());
        if !owns_current_layer_at(ctx, response.layer_id, focus_at) {
            return Err(error(Rule::Availability, "panel focus layer changed"));
        }
        let control = frame.controls.iter().find(|control| control.id == focused && control.enabled);
        let instance = control
            .and_then(|control| match &control.target {
                HostControlTarget::Pointer(target) => pointer_instance(target).cloned(),
                HostControlTarget::Shared { instance, .. }
                | HostControlTarget::Content { instance, .. }
                | HostControlTarget::PanelMenu { instance } => Some(instance.clone()),
                _ => None,
            })
            .or_else(|| {
                self.content_reports
                    .get(&focused)
                    .filter(|report| {
                        report.scope == runtime_scope(runtime)
                            && runtime.instance_owner(&report.key.instance).is_ok_and(|owner| owner == report.key.owner)
                    })
                    .map(|report| report.key.instance.clone())
            });
        use ContextualPanelAction::*;
        if matches!(action, ResizeLeft | ResizeRight | ResizeUp | ResizeDown | ResetSash) {
            let target = control
                .and_then(|control| if let HostControlTarget::Pointer(target) = &control.target { Some(target) } else { None })
                .ok_or_else(|| error(Rule::Availability, "focused control is not a sash"))?;
            let candidate = super::interaction::keyboard_resize_action(runtime.layout(), &frame.layout, target, action)?;
            prepare_host_layout(runtime.layout(), &runtime_scope(runtime), &frame.layout, candidate.clone())?;
            return Ok(candidate);
        }
        let instance = instance.ok_or_else(|| error(Rule::Availability, "focus does not belong to panel content or chrome"))?;
        runtime.layout().availability(&instance).require()?;

        match action {
            FocusNext | FocusPrevious => {
                let eligible: Vec<_> = frame
                    .layout
                    .sections
                    .iter()
                    .flat_map(|card| runtime.layout().layout().section(&card.section).into_iter().flat_map(|section| section.tabs.iter()))
                    .filter(|id| runtime.layout().availability(id) == Availability::Available)
                    .collect();
                let index =
                    eligible.iter().position(|id| **id == instance).ok_or_else(|| error(Rule::Availability, "focused panel is not presented"))?;
                let next = if action == FocusNext { (index + 1) % eligible.len() } else { (index + eligible.len() - 1) % eligible.len() };
                Ok(apply_action(frame.layout.revision, vec![Mutation::Focus { instance: eligible[next].clone() }]))
            }
            Toggle => {
                let candidate = shared_action(
                    runtime,
                    &instance,
                    &HostSharedAction::Collapse,
                    &self.native_capabilities,
                    frame.layout.capacity,
                    &frame.layout.geometry_policy.identity_reservations,
                )?;
                prepare_host_layout(runtime.layout(), &runtime_scope(runtime), &frame.layout, candidate.clone())?;
                Ok(candidate)
            }
            MoveNextZone => {
                let current = runtime.layout().layout().containing(&instance)?.location.zone();
                let zones = [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating];
                let start = zones.iter().position(|zone| *zone == current).unwrap_or(0);
                for step in 1..=zones.len() {
                    let zone = zones[(start + step) % zones.len()];
                    if zone == current {
                        continue;
                    }
                    if let Ok(candidate) = shared_action(
                        runtime,
                        &instance,
                        &HostSharedAction::Move(zone),
                        &self.native_capabilities,
                        frame.layout.capacity,
                        &frame.layout.geometry_policy.identity_reservations,
                    ) && prepare_host_layout(runtime.layout(), &runtime_scope(runtime), &frame.layout, candidate.clone()).is_ok()
                    {
                        return Ok(candidate);
                    }
                }
                Err(error(Rule::Availability, "panel has no allowed next zone"))
            }
            _ => Err(error(Rule::Availability, "unsupported contextual panel action")),
        }
    }

    pub fn frame(&self) -> Option<&HostFrame> {
        self.frame.as_ref()
    }
    // A pass copies this snapshot without consuming it. The next frame (or
    // context teardown) expires it even if no HostFrame geometry remains.
    fn renew_outcomes(&mut self, frame_number: u64) {
        if self.outcome_frame != Some(frame_number) {
            self.outcomes.clear();
            self.outcome_frame = Some(frame_number);
        }
    }
    pub(crate) fn note_native_capabilities(&mut self, capabilities: HostCapabilities) {
        self.native_capabilities = capabilities;
    }
    pub(crate) fn capture_metrics(&mut self, runtime: &PanelRuntime, models: &dyn PanelModels) -> PanelResult<()> {
        let metrics = runtime.capture_metrics(models)?;
        if self.options.geometry.measure.as_ref().is_some_and(|old| old.revision() == metrics.revision()) {
            return Ok(());
        }
        if self.options.geometry.measure.as_ref().is_some_and(|old| old.revision() != metrics.revision()) {
            self.invalidate_geometry();
        }
        self.options.geometry.content_revision = metrics.revision();
        self.options.geometry.measure = Some(metrics);
        self.options.geometry.content.clear();
        Ok(())
    }
    /// Programmatic changes use the exact allocator and admission of gestures.
    /// No host or captured environment means no structural admission.
    pub(crate) fn command_reference(&self, runtime: &PanelRuntime) -> PanelResult<HostLayout> {
        let (domain, capacity, chrome) = if let Some(frame) = &self.frame {
            (frame.layout.render_domain, frame.layout.capacity, frame.layout.chrome_obstacles.as_slice())
        } else {
            (
                self.last_domain.ok_or_else(|| error(Rule::Availability, "panel host environment is not prepared"))?,
                self.last_capacity.ok_or_else(|| error(Rule::Availability, "panel host environment is not prepared"))?,
                self.last_chrome.as_slice(),
            )
        };
        resolve_frame_layout(runtime, domain, capacity, chrome, &self.options)
    }
    /// Capture the injected catalogue's full envelope, including opaque/dormant
    /// reservations, before either widget or pointer identity generation.
    pub(crate) fn capture_reservations(&mut self, runtime: &PanelRuntime, catalogue: &dyn commands::LayoutCatalogue) -> PanelResult<()> {
        let service = runtime.layout();
        let envelope = commands::current_envelope_snapshot(catalogue, &service.inspect(), service.registry())?;
        let mut reservations = host_identity_reservations(&envelope)?;
        reservations.include_layout(service.layout())?;
        if self.options.geometry.identity_reservations != reservations {
            self.invalidate_geometry();
            self.options.geometry.identity_reservations = reservations;
        }
        Ok(())
    }
    pub(crate) fn cancel_gesture(&mut self) {
        if self.interaction.move_source().is_some()
            && let Some(ctx) = &self.egui_context
        {
            ctx.request_repaint();
        }
        self.interaction.cancel();
        self.pending = None;
        if let Some(frame) = &mut self.frame {
            frame.preview = None;
        }
    }
    pub fn interaction(&self) -> &HostInteraction {
        &self.interaction
    }
    pub(crate) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub(crate) fn prepared_geometry(&self) -> Option<(LogicalRect, LogicalRect, Vec<LogicalRect>)> {
        Some((self.last_domain?, self.last_capacity?, self.last_chrome.clone()))
    }
    pub(crate) fn wants_escape(&self, runtime: &PanelRuntime) -> bool {
        let _ = runtime;
        self.interaction.is_captured() || self.menu.is_some()
    }
    pub(crate) fn request_escape(&mut self) {
        self.pending_escape = true;
    }
    /// Discard geometry and provisional input without tearing down an eligible
    /// editor's keyboard focus or retained egui caret/selection state.
    pub(crate) fn invalidate_geometry(&mut self) {
        if self.interaction.move_source().is_some()
            && let Some(ctx) = &self.egui_context
        {
            ctx.request_repaint();
        }
        self.interaction.cancel();
        self.pending = None;
        self.frame = None;
        self.clear_menu();
        self.overlays.clear();
        self.preview_started = None;
        self.pending_escape = false;
        self.focus_request = None;
        self.last_mount_frame = None;
        self.last_mount_revision = None;
    }
    /// Owner/scope loss and application hiding also release owned keyboard input.
    pub(crate) fn invalidate_context(&mut self) {
        self.drag_paint.clear();
        self.invalidate_geometry();
        self.content_focus = None;
        self.content_bookmarks.clear();
        self.content_reports.clear();
        self.restore_content_focus = false;
        self.outcomes.clear();
        self.outcome_frame = None;
        if let (Some(ctx), Some(focused)) = (&self.egui_context, self.owned_focus.take()) {
            ctx.memory_mut(|memory| memory.surrender_focus(focused));
        }
    }
    pub(crate) fn reconcile_scope(&mut self, runtime: &PanelRuntime) {
        let identity = (runtime_scope(runtime), runtime.environment().window.clone());
        if self.scope.as_ref() != Some(&identity) {
            let replacing_owner = self.scope.is_some();
            self.invalidate_context();
            self.last_rejection = None;
            if replacing_owner {
                self.observed_focus = runtime.layout().interaction().focused.clone();
            }
            self.scope = Some(identity);
        }
        if let Some(bookmark) = &self.content_focus
            && self.owned_focus == Some(bookmark.id)
            && !bookmark.logically_presented(runtime)
        {
            self.restore_content_focus = false;
            self.owned_focus = None;
            if let Some(ctx) = &self.egui_context {
                ctx.memory_mut(|memory| memory.surrender_focus(bookmark.id));
            }
        }
    }
    pub(crate) fn note_reply(&mut self, reply: &LayoutReply) {
        for effect in &reply.effects {
            if let HostEffect::Focus { instance } = effect {
                self.focus_request = Some(instance.clone());
            }
        }
    }
    /// App ticks/programmatic forwarding use the same overlay ledger as render
    /// and pointer dispatch, so background cleanup cannot leave stale input.
    pub(crate) fn with_local_services<R>(
        &mut self,
        scope: &LocalScope,
        services: &mut ContentServices<'_>,
        use_services: impl FnOnce(&mut ContentServices<'_>) -> R,
    ) -> R {
        let capacity = self
            .frame
            .as_ref()
            .map_or(LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 0.0, height: 0.0 } }, |frame| frame.layout.capacity);
        with_local_tracking(services, scope, capacity, &mut self.overlays, use_services)
    }
    pub(crate) fn with_command_services<R>(
        &mut self,
        scope: &LocalScope,
        services: &mut ContentServices<'_>,
        use_services: impl FnOnce(&mut Self, &mut ContentServices<'_>) -> R,
    ) -> R {
        let capacity = self
            .frame
            .as_ref()
            .map_or(LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 0.0, height: 0.0 } }, |frame| frame.layout.capacity);
        let mut overlays = std::mem::take(&mut self.overlays);
        let result = with_local_tracking(services, scope, capacity, &mut overlays, |services| use_services(self, services));
        self.overlays = overlays;
        result
    }
    pub fn prepare(&mut self, runtime: &PanelRuntime, capacity: LogicalRect) -> PanelResult<()> {
        self.prepare_in_domain(runtime, capacity, capacity, &[])
    }

    pub(crate) fn note_egui_context(&mut self, ctx: &egui::Context, force_opaque: bool) {
        self.egui_context = Some(ctx.clone());
        let surfaces = HostSurfaceStyle::capture(ctx, force_opaque, self.options.geometry.requested_opacity, self.options.geometry.surfaces.as_ref());
        let surface_alpha = Some(HostSurfaceAlpha {
            docked: surfaces.appearance(PanelSurface::FollowZone, false).fill.a(),
            floating: surfaces.appearance(PanelSurface::FollowZone, true).fill.a(),
        });
        if self.options.geometry.surfaces.as_ref() != Some(&surfaces) || self.options.geometry.surface_alpha != surface_alpha {
            self.invalidate_geometry();
            self.options.geometry.surfaces = Some(surfaces);
            self.options.geometry.surface_alpha = surface_alpha;
        }
    }

    pub fn prepare_in_domain(
        &mut self,
        runtime: &PanelRuntime,
        render_domain: LogicalRect,
        capacity: LogicalRect,
        chrome: &[LogicalRect],
    ) -> PanelResult<()> {
        self.reconcile_scope(runtime);
        if let Some(frame_number) = self.egui_context.as_ref().map(egui::Context::cumulative_frame_nr) {
            self.renew_outcomes(frame_number);
        }
        let mut layout = match resolve_frame_layout(runtime, render_domain, capacity, chrome, &self.options) {
            Ok(layout) => layout,
            Err(reason) => {
                if self.interaction.move_source().is_some()
                    && let Some(ctx) = &self.egui_context
                {
                    ctx.request_repaint();
                }
                self.last_capacity = None;
                self.last_domain = None;
                self.last_chrome.clear();
                self.frame = None;
                self.interaction.cancel();
                self.clear_menu();
                self.overlays.clear();
                return Err(reason);
            }
        };
        let mut unmeasured_instances = Vec::new();
        // Predict only the kit's forced invisible Area measurement pass before
        // scene navigation runs. Retain intended sections/resolved mount geometry.
        for section in &layout.sections {
            if !(section.peek || section.zone == Zone::Floating) {
                continue;
            }
            let area = section_surface_id(&runtime_scope(runtime), &runtime.environment().window, &section.section).with("area");
            let measured =
                self.egui_context.as_ref().is_some_and(|ctx| egui::containers::AreaState::load(ctx, area).is_some_and(|state| state.size.is_some()));
            if !measured {
                unmeasured_instances.push(section.instance.clone());
                layout.controls.retain(|control| match &control.target {
                    HostPointerTarget::Header { instance }
                    | HostPointerTarget::Surface { instance }
                    | HostPointerTarget::Menu { instance }
                    | HostPointerTarget::Collapse { instance }
                    | HostPointerTarget::Close { instance }
                    | HostPointerTarget::HeaderAction { instance, .. } => *instance != section.instance,
                    HostPointerTarget::Tab { instance } => !section.tabs.iter().any(|tab| tab.instance == *instance),
                    HostPointerTarget::Resize { section: id, .. } | HostPointerTarget::TabOverflow { section: id } => *id != section.section,
                    _ => true,
                });
                let clipped = logical_rect(egui_rect(section.outer).intersect(egui_rect(capacity)));
                for regions in [&mut layout.painted, &mut layout.input, &mut layout.occlusion] {
                    if let Some(index) = regions.iter().position(|rect| *rect == clipped) {
                        regions.remove(index);
                    }
                }
            }
        }
        layout.reservations = layout.painted.clone();
        let obstacles: Vec<_> = layout.chrome_obstacles.iter().chain(&layout.painted).copied().collect();
        layout.usable = resolve_usable_region(layout.render_domain, &obstacles)?;
        self.overlays.retain(|instance, _| layout.resolved.panels.iter().any(|panel| panel.instance == *instance));
        self.last_capacity = Some(capacity);
        self.last_domain = Some(render_domain);
        self.last_chrome = chrome.to_vec();
        let preview = self.frame.as_ref().and_then(|frame| frame.preview.clone());
        self.frame = Some(HostFrame {
            layout,
            controls: Vec::new(),
            events: self.outcomes.clone(),
            preview,
            reply: super::runtime::normalize_native_reply(runtime.layout().inspect(), &self.native_capabilities),
            next_layout: None,
            menu_bounds: None,
            owned_overlays: overlay_regions(&self.overlays, self.egui_context.as_ref(), &unmeasured_instances),
        });
        Ok(())
    }

    /// Mount the complete in-app set, including an empty set. Repeated egui
    /// passes at the same revision repaint without another reconciliation. A
    /// new committed revision must reconcile before painting its new content.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        runtime: &mut PanelRuntime,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
        catalogue: &mut dyn commands::LayoutCatalogue,
    ) -> PanelResult<()> {
        let painted = self.show_deferred(ui, runtime, native, services);
        let finished = self.finish(ui, runtime, native, services, catalogue);
        painted.and(finished)
    }

    /// Paint/mount now; the embedding app calls finish AFTER its other windows
    /// and modals have registered their actual layers and widget responses.
    pub fn show_deferred(
        &mut self,
        ui: &mut egui::Ui,
        runtime: &mut PanelRuntime,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        self.show_deferred_with_content(ui, runtime, native, services, &mut super::host::ControllerFrameContent)
    }

    /// Paint using content borrowed only for this frame. Layout, chrome and
    /// current-pass focus provenance remain owned by the shared host.
    pub fn show_deferred_with_content(
        &mut self,
        ui: &mut egui::Ui,
        runtime: &mut PanelRuntime,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
        frame_content: &mut dyn FramePanelContent,
    ) -> PanelResult<()> {
        if !ui.is_visible() || ui.is_sizing_pass() {
            self.invalidate_context();
            return runtime.mount_in_app(&ResolvedLayout::default(), &mut EmptyPanelHost, services);
        }
        self.note_native_capabilities(native.capabilities());
        let frame_number = ui.ctx().cumulative_frame_nr();
        self.renew_outcomes(frame_number);
        let Some(mut frame) = self.frame.take() else {
            return Err(error(Rule::Lifecycle, "prepare host geometry before painting"));
        };
        frame.reply.capabilities = runtime.layout().inspect().capabilities;
        frame.reply = super::runtime::normalize_native_reply(frame.reply, &self.native_capabilities);
        self.egui_context = Some(ui.ctx().clone());
        let allow_actions = self.last_input_frame != Some(frame_number) && ui.is_enabled();
        frame.events.clone_from(&self.outcomes);
        frame.controls.clear();
        frame.menu_bounds = None;
        // Resolve each pass, including the pass after an invisible Area sizing
        // pass. Actual paint/input arrays below may have omitted sizing areas;
        // intended geometry must be restored before the next attempt to paint.
        if frame.layout.revision != runtime.layout().inspect().revision {
            frame.reply = super::runtime::normalize_native_reply(runtime.layout().inspect(), &self.native_capabilities);
        }
        let scope = runtime_scope(runtime);
        let window = runtime.environment().window.clone();
        let locale = solidcraft_ui_text::Locale::for_language(solidcraft_ui_text::language());
        let _resolved_labels = solidcraft_ui_text::resolved_labels();
        let mut metadata = BTreeMap::new();
        let mut owners = BTreeMap::new();
        let mut header_icons = BTreeMap::new();
        for instance in &runtime.layout().layout().instances {
            if let Ok(value) = runtime.instance_presentation(&instance.id) {
                metadata.insert(instance.id.clone(), value);
            }
            if let Ok(owner) = runtime.instance_owner(&instance.id) {
                owners.insert(instance.id.clone(), owner);
            }
            for action in &runtime.layout().registry().get(&instance.kind)?.header_actions {
                header_icons.insert((instance.id.clone(), action.id.clone()), action.icon.clone());
            }
        }
        let t = solidcraft_theme::Tokens::get();
        let heading_font = ui_kit::type_face::semibold(t.font_heading);
        self.options.geometry.header_title_widths = ui.fonts_mut(|fonts| {
            metadata
                .iter()
                .map(|(instance, presentation)| {
                    let text = fonts.layout_no_wrap(presentation.title.clone(), heading_font.clone(), t.form_ink).size().x;
                    let title = text.ceil() + 2.0 * t.space_12 + t.space_10 + t.space_8;
                    (instance.clone(), title)
                })
                .collect()
        });
        self.options.geometry.tab_title_widths = ui.fonts_mut(|fonts| {
            let regular = egui::FontId::proportional(t.font_button);
            let active = ui_kit::type_face::semibold(t.font_button);
            metadata
                .iter()
                .map(|(instance, presentation)| {
                    let mut width = |font| fonts.layout_no_wrap(presentation.title.clone(), font, t.app_bar_text).size().x;
                    let width = width(regular.clone()).max(width(active.clone())).ceil() + 2.0 * t.space_8 + t.space_2;
                    (instance.clone(), width.min(MAX_LOGICAL))
                })
                .collect()
        });
        frame.layout =
            resolve_frame_layout(runtime, frame.layout.render_domain, frame.layout.capacity, &frame.layout.chrome_obstacles, &self.options)?;
        let requested = self.focus_request.clone().or_else(|| {
            let focused = runtime.layout().interaction().focused.clone();
            (focused != self.observed_focus).then_some(focused).flatten()
        });
        if let Some(instance) = requested.as_ref()
            && let Some(card) = frame
                .layout
                .sections
                .iter()
                .find(|card| runtime.layout().layout().section(&card.section).is_ok_and(|section| section.tabs.contains(instance)))
        {
            let target = egui_rect(card.header);
            let visible = egui_rect(frame.layout.capacity);
            let delta_x = if target.left() < visible.left() { target.left() - visible.left() } else { (target.right() - visible.right()).max(0.0) };
            let delta_y = if target.top() < visible.top() { target.top() - visible.top() } else { (target.bottom() - visible.bottom()).max(0.0) };
            if delta_x != 0.0 || delta_y != 0.0 {
                self.options.geometry.scroll.x =
                    (self.options.geometry.scroll.x + delta_x).clamp(0.0, (frame.layout.extent.width - frame.layout.capacity.size.width).max(0.0));
                self.options.geometry.scroll.y =
                    (self.options.geometry.scroll.y + delta_y).clamp(0.0, (frame.layout.extent.height - frame.layout.capacity.size.height).max(0.0));
                frame.layout =
                    resolve_frame_layout(runtime, frame.layout.render_domain, frame.layout.capacity, &frame.layout.chrome_obstacles, &self.options)?;
            }
        }
        for control in &frame.layout.controls {
            let target = HostControlTarget::Pointer(control.target.clone());
            let instance = pointer_instance(&control.target);
            let name = if matches!(&control.target, HostPointerTarget::Collapse { instance }
                if runtime.layout().layout().containing(instance).is_ok_and(|section| section.collapsed))
            {
                locale.text("ui.panel_host.expand").to_owned()
            } else {
                chrome_label(&control.target, &control.accessible_name, &metadata, locale)
            };
            let binding_enabled = match &control.target {
                HostPointerTarget::HeaderAction { instance, action } => metadata
                    .get(instance)
                    .is_some_and(|presentation| presentation.actions.iter().any(|binding| binding.id == *action && binding.enabled)),
                HostPointerTarget::Close { instance } => runtime
                    .layout()
                    .layout()
                    .instance(instance)
                    .and_then(|panel| runtime.layout().registry().get(&panel.kind))
                    .is_ok_and(|spec| spec.features.closable),
                HostPointerTarget::Collapse { instance } => runtime.layout().layout().containing(instance).is_ok_and(|section| {
                    section.tabs.iter().all(|id| {
                        runtime
                            .layout()
                            .layout()
                            .instance(id)
                            .and_then(|panel| runtime.layout().registry().get(&panel.kind))
                            .is_ok_and(|spec| spec.collapsible_in(section.location.zone()))
                    })
                }),
                _ => true,
            };
            let enabled = ui.is_enabled()
                && binding_enabled
                && (instance.is_none_or(|id| runtime.layout().availability(id) == Availability::Available)
                    || matches!(&control.target, HostPointerTarget::Menu { .. }));
            frame.controls.push(HostFrameControl {
                id: control_id(runtime, &owners, &target),
                target,
                bounds: control.bounds,
                role: control.role,
                accessible_name: name,
                enabled,
            });
        }
        let focused = runtime.layout().interaction().focused.clone();
        if focused != self.observed_focus {
            self.focus_request = focused.clone();
            self.observed_focus = focused;
        }
        let mut commands = Vec::new();
        let mut pointer_events = Vec::new();
        let mut unpainted = Vec::new();
        self.content_reports.clear();
        let mut drag_cards = Vec::new();
        let capture_drag =
            self.live_drag && (self.interaction.is_captured() || self.drag_paint.active() || ui.input(|input| input.pointer.primary_down()));
        let drag_layout = self
            .live_drag
            .then(|| {
                frame
                    .preview
                    .as_ref()
                    .filter(|preview| preview.rejection.is_none())
                    .and_then(|preview| preview.projected.as_ref())
                    .or(self.interaction.detached_layout())
            })
            .flatten();
        let mut renderer = WorkspaceContentHost {
            ui,
            frame_content,
            layout: &frame.layout,
            move_source: (!self.live_drag).then_some(self.interaction.move_source()).flatten(),
            drag_cards: capture_drag.then_some(&mut drag_cards),
            drag_layout,
            tab_drop: self
                .live_drag
                .then(|| {
                    let preview = frame.preview.as_ref().filter(|preview| preview.rejection.is_none())?;
                    let Destination::JoinTab { section, index } = preview.destination.as_ref()? else { return None };
                    Some((section, self.interaction.move_source()?, *index))
                })
                .flatten(),
            open_menu: self.menu.as_ref(),
            controls: &mut frame.controls,
            metadata: &metadata,
            header_icons: &header_icons,
            scope: &scope,
            window: &window,
            owned_focus: &mut self.owned_focus,
            content_focus: &mut self.content_focus,
            content_bookmarks: &mut self.content_bookmarks,
            content_reports: &mut self.content_reports,
            motion: runtime.environment().motion,
            commands: &mut commands,
            allow_actions,
            unpainted: &mut unpainted,
        };
        let mut overlays = std::mem::take(&mut self.overlays);
        let reconcile = self.last_mount_frame != Some(frame_number) || self.last_mount_revision != Some(frame.layout.revision);
        if reconcile {
            self.last_mount_frame = Some(frame_number);
            self.last_mount_revision = Some(frame.layout.revision);
        }
        let mount = with_local_tracking(services, &scope, frame.layout.capacity, &mut overlays, |services| {
            if reconcile {
                runtime.mount_in_app(&frame.layout.resolved, &mut renderer, services)
            } else {
                runtime.repaint_in_app(&frame.layout.resolved, &mut renderer, services)
            }
        });
        if mount.is_err() {
            self.content_reports.clear();
        }
        // An Area's first sizing pass is invisible and non-interactable. Keep
        // the intended resolved geometry, but never publish it as actual paint
        // or an input/occlusion shield, even if no further egui pass is allowed.
        let mut unpainted_instances = Vec::new();
        for (layer, bounds, sizing) in &unpainted {
            let hidden_sections: Vec<_> = frame
                .layout
                .sections
                .iter()
                .filter(|section| section_surface_id(&scope, &window, &section.section).with("area") == layer.id)
                .collect();
            let hidden_instances: Vec<_> = hidden_sections
                .iter()
                .flat_map(|section| std::iter::once(section.instance.clone()).chain(section.tabs.iter().map(|tab| tab.instance.clone())))
                .collect();
            // A new Area measures before it paints. Preserve only the editor
            // that still owns focus; restore it after a fresh response returns.
            if *sizing && ui.is_enabled() && ui.is_visible() && ui.input(|input| input.focused) && !ui_kit::Container::modal_input_blocked(ui.ctx()) {
                self.restore_content_focus |= self.content_focus.as_ref().is_some_and(|bookmark| {
                    self.owned_focus == Some(bookmark.id)
                        && ui.memory(|memory| memory.focused()) == Some(bookmark.id)
                        && bookmark.logically_presented(runtime)
                        && hidden_sections.iter().any(|section| section.instance == bookmark.key.instance && !section.collapsed)
                });
            }
            unpainted_instances.extend(hidden_instances.iter().cloned());
            let clipped = logical_rect(egui_rect(*bounds).intersect(egui_rect(frame.layout.capacity)));
            // Remove one occurrence: a visible card can occupy the same rect.
            for regions in [&mut frame.layout.painted, &mut frame.layout.input, &mut frame.layout.occlusion] {
                if let Some(index) = regions.iter().position(|rect| *rect == clipped) {
                    regions.remove(index);
                }
            }
            let hidden: Vec<_> = frame
                .controls
                .iter()
                .filter(|control| {
                    let owned = match &control.target {
                        HostControlTarget::Pointer(pointer) => {
                            pointer_instance(pointer).is_some_and(|instance| hidden_instances.contains(instance))
                                || matches!(pointer, HostPointerTarget::Resize { section, .. } | HostPointerTarget::TabOverflow { section }
                                if hidden_sections.iter().any(|hidden| hidden.section == *section))
                        }
                        _ => false,
                    };
                    owned || ui.ctx().read_response(control.id).is_some_and(|response| response.layer_id == *layer)
                })
                .map(|control| control.target.clone())
                .collect();
            for target in &hidden {
                if let HostControlTarget::Pointer(HostPointerTarget::Surface { instance }) = target {
                    unpainted_instances.push(instance.clone());
                }
            }
            frame.layout.controls.retain(|control| !hidden.contains(&HostControlTarget::Pointer(control.target.clone())));
            frame.controls.retain(|control| !hidden.contains(&control.target));
        }
        if !unpainted_instances.is_empty() {
            let mut actual = frame.layout.resolved.clone();
            actual.panels.retain(|panel| !unpainted_instances.contains(&panel.instance));
            with_local_tracking(services, &scope, frame.layout.capacity, &mut overlays, |services| {
                runtime.reconcile_in_app_presentation(&actual, services.local);
            });
        }
        frame.owned_overlays = overlay_regions(&overlays, Some(ui.ctx()), &unpainted_instances);
        self.paint_scrollbars(ui, &scope, &window, &mut frame, &mut commands, allow_actions);
        // Usable space follows this pass's actual card/chrome footprints. Popup
        // input remains separate and never becomes a camera framing target.
        let obstacles: Vec<_> = frame.layout.chrome_obstacles.iter().chain(&frame.layout.painted).copied().collect();
        frame.layout.reservations = frame.layout.painted.clone();
        frame.layout.usable = resolve_usable_region(frame.layout.render_domain, &obstacles)?;
        // Rails, dock sashes and overflow exist even with no mounted content.
        // Section resize widgets are painted on their card's own layer.
        self.paint_remaining(ui, runtime, &metadata, &mut frame, &mut commands, &mut pointer_events, allow_actions, locale);
        if let Some(instance) = self.focus_request.as_ref().filter(|_| ui.is_enabled() && !ui_kit::Container::modal_input_blocked(ui.ctx())) {
            if runtime.layout().layout().containing(instance).is_ok_and(|section| section.location.zone() == Zone::Window) {
                self.focus_request = None;
            } else {
                // A first Area sizing pass has no actual enabled Header. Keep
                // the typed request until its visible widget exists; animation
                // opacity never delays this focus transfer.
                let actual =
                    |control: &&HostFrameControl| control.enabled && ui.ctx().read_response(control.id).is_some_and(|response| response.enabled());
                let target = frame.controls.iter().filter(actual).find(|control| matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Header { instance: id }) if id == instance))
                    .or_else(|| frame.controls.iter().filter(actual).find(|control| matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id }) if id == instance)))
                    .or_else(|| frame.controls.iter().filter(actual).find(|control| control.target == HostControlTarget::Pointer(HostPointerTarget::OverflowAll)));
                if let Some(control) = target {
                    let bookmark = runtime
                        .instance_owner(instance)
                        .ok()
                        .and_then(|owner| self.content_bookmarks.get(&ControllerKey { instance: instance.clone(), owner }));
                    let eligible = bookmark.filter(|bookmark| {
                        bookmark.scope == scope
                            && frame.layout.sections.iter().any(|section| section.instance == *instance && !section.collapsed)
                            && self
                                .content_reports
                                .get(&bookmark.id)
                                .is_some_and(|report| report.key == bookmark.key && report.scope == bookmark.scope)
                    });
                    // A typed focus request is deliberate. Passive restoration
                    // below is the path that must yield to foreign focus.
                    let target = eligible.map_or(control.id, |bookmark| bookmark.id);
                    if let Some(bookmark) = eligible {
                        bookmark.restore_selection(ui.ctx());
                    }
                    ui.memory_mut(|memory| memory.request_focus(target));
                    self.owned_focus = Some(target);
                    self.focus_request = None;
                }
            }
        }
        let logical_focus_eligible = self.restore_content_focus
            && ui.input(|input| input.focused)
            && self.content_focus.as_ref().is_some_and(|bookmark| bookmark.logically_presented(runtime));
        // A passive return cannot outlive deliberate hiding or owner/focus loss.
        // A modal may still wait while that logical presentation remains valid.
        self.restore_content_focus &= logical_focus_eligible;
        if self.restore_content_focus && !ui_kit::Container::modal_input_blocked(ui.ctx()) {
            let current = ui.memory(|memory| memory.focused());
            let deliberate = current.is_some_and(|id| ui.ctx().read_response(id).is_some_and(|response| response.gained_focus()));
            if deliberate {
                self.restore_content_focus = false;
            } else if current.is_none()
                && let Some(bookmark) = &self.content_focus
            {
                let eligible =
                    self.content_reports.get(&bookmark.id).is_some_and(|report| report.key == bookmark.key && report.scope == bookmark.scope);
                if eligible {
                    bookmark.restore_selection(ui.ctx());
                    ui.memory_mut(|memory| memory.request_focus(bookmark.id));
                    self.owned_focus = Some(bookmark.id);
                    self.restore_content_focus = false;
                }
            }
        }
        self.paint_menu(ui, runtime, native.capabilities(), &metadata, &owners, &mut frame, &mut commands, allow_actions, locale);
        self.pending =
            Some(PendingHostCommands { scope, window, frame_number, allow_actions, commands, pointer_events, mount: mount.clone(), drag_cards });
        self.overlays = overlays;
        self.frame = Some(frame);
        mount
    }

    pub fn finish(
        &mut self,
        ui: &mut egui::Ui,
        runtime: &mut PanelRuntime,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
        catalogue: &mut dyn commands::LayoutCatalogue,
    ) -> PanelResult<()> {
        self.note_native_capabilities(native.capabilities());
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        // Chrome has already painted this pass. Threshold/cancellation changes
        // need another paint even when there is no admitted landing to animate.
        let painted_move_source = self.interaction.move_source().cloned();
        if pending.scope != runtime_scope(runtime)
            || pending.window != runtime.environment().window
            || pending.frame_number != ui.ctx().cumulative_frame_nr()
        {
            self.invalidate_context();
            ui.ctx().request_repaint();
            return Err(error(Rule::Placement, "host context changed after paint"));
        }
        self.capture_metrics(runtime, services.models)?;
        self.capture_reservations(runtime, catalogue)?;
        let PendingHostCommands { scope, frame_number, allow_actions, mut commands, mut pointer_events, mount, drag_cards, .. } = pending;
        let Some(mut frame) = self.frame.take() else {
            ui.ctx().request_repaint();
            return Ok(());
        };
        let painted_landing = frame.preview.as_ref().and_then(|preview| preview.target.clone());
        frame.reply.capabilities = runtime.layout().inspect().capabilities;
        frame.reply = super::runtime::normalize_native_reply(frame.reply, &self.native_capabilities);
        let stale = frame.layout.revision != runtime.layout().inspect().revision;
        if stale {
            self.interaction.cancel();
            commands.clear();
            pointer_events.clear();
            frame.preview = None;
            frame.reply = super::runtime::normalize_native_reply(runtime.layout().inspect(), &self.native_capabilities);
            frame.next_layout =
                resolve_frame_layout(runtime, frame.layout.render_domain, frame.layout.capacity, &frame.layout.chrome_obstacles, &self.options).ok();
            let reason = error(Rule::Revision, "host layout changed after paint");
            if !frame.events.iter().any(|event| matches!(event, HostEvent::Rejected { action: None, error } if error == &reason)) {
                frame.events.push(HostEvent::Rejected { action: None, error: reason });
            }
            ui.ctx().request_repaint();
        }
        let hard_modal = ui_kit::Container::modal_input_blocked(ui.ctx());
        if hard_modal {
            // Modal backdrops own the entire input context, including points
            // outside the card and releases from an earlier host capture.
            // This is not a content lifecycle or layout/peek mutation.
            self.interaction.cancel();
            frame.preview = None;
            commands.clear();
            pointer_events.clear();
            self.clear_menu();
            self.pending_escape = false;
            self.preview_started = None;
            self.focus_request = None;
            self.last_input_frame = Some(frame_number);
            self.restore_content_focus |= self.content_focus.as_ref().is_some_and(|bookmark| {
                ui.memory(|memory| memory.focused()) == Some(bookmark.id) || ui.memory(|memory| memory.had_focus_last_frame(bookmark.id))
            });
            let owned = self.owned_focus.take();
            ui.memory_mut(|memory| {
                if let Some(focused) = memory.focused()
                    && (owned == Some(focused) || frame.controls.iter().any(|control| control.id == focused))
                {
                    memory.surrender_focus(focused);
                }
            });
            // Report what this pass painted; remove private menu/preview state
            // on the next paint without changing viewport reservations.
            ui.ctx().request_repaint();
        }
        if !hard_modal
            && self.restore_content_focus
            && ui.memory(|memory| memory.focused()).is_some_and(|id| ui.ctx().read_response(id).is_some_and(|response| response.gained_focus()))
        {
            self.restore_content_focus = false;
        }
        let allow_actions =
            allow_actions && !stale && mount.is_ok() && ui.is_enabled() && ui.is_visible() && ui.input(|input| input.focused) && !hard_modal;
        let dismiss_menu = self.pending_escape || ui.input(|input| input.key_pressed(egui::Key::Escape));
        if mount.is_err() {
            commands.clear();
            self.interaction.cancel();
            frame.preview = None;
        }
        if allow_actions {
            self.last_input_frame = Some(frame_number);
            if self.scroll_from_wheel(ui, &frame) {
                self.interaction.cancel();
                frame.preview = None;
                commands.clear();
                pointer_events.clear();
                ui.ctx().request_repaint();
            }
            if commands.iter().any(|command| {
                matches!(
                    command,
                    HostCommand::Layout(_) | HostCommand::WidgetLayout(_, _) | HostCommand::Content(_, _, _) | HostCommand::Scroll { .. }
                )
            }) {
                self.interaction.cancel();
                frame.preview = None;
                pointer_events.clear();
            }
            self.menu_keyboard(ui, &frame);
            for (action, shortcut) in &self.keyboard_shortcuts {
                let pressed = ui.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(event,
                            egui::Event::Key { key, modifiers, pressed: true, .. }
                            if *key == shortcut.logical_key && modifiers.matches_logically(shortcut.modifiers)
                        )
                    })
                });
                if !pressed {
                    continue;
                }
                // TextEdit keeps its own editing chords; explicit registered
                // panel commands still resolve the same owned content context.
                if ui.ctx().text_edit_focused() {
                    break;
                }
                let available = self.contextual_in_frame(runtime, &frame, *action);
                // Editors have already had first refusal. A bounded/no-op split
                // still owns its axis key; an unrelated context never consumes it.
                let owned_bound = available.as_ref().err().is_some_and(|reason| reason.rule == Rule::Bounds);
                if (available.is_ok() || owned_bound)
                    && ui.input_mut(|input| input.consume_shortcut(shortcut))
                    && let Some(source) = ui.memory(|memory| memory.focused())
                {
                    commands.push(HostCommand::Contextual(*action, source));
                }
            }
            self.pointer_input(ui, runtime, &scope, &mut frame, &mut commands, pointer_events);
            for control in &frame.controls {
                if ui.ctx().read_response(control.id).is_some_and(|response| response.has_focus() && response.gained_focus()) {
                    self.owned_focus = Some(control.id);
                    frame.events.push(HostEvent::Focus { target: control.target.clone() });
                }
            }
        } else if !ui.is_enabled() || !ui.is_visible() || !ui.input(|input| input.focused) {
            self.interaction.cancel();
            frame.preview = None;
        }
        if let Some(preview) = &frame.preview
            && !(self.live_drag && self.interaction.move_source().is_some())
            && preview.rejection.is_none()
            && preview.projected.is_some()
        {
            let now = ui.input(|input| input.time);
            let started = *self.preview_started.get_or_insert(now);
            let duration = solidcraft_theme::panel_motion_duration(ui.ctx(), runtime.environment().motion == Motion::Reduced);
            let progress = if duration <= 0.0 { 1.0 } else { ((now - started) as f32 / duration).clamp(0.0, 1.0) };
            let opacity = solidcraft_theme::panel_motion_ease(progress);
            if let Some(projected) = &preview.projected {
                for section in &projected.sections {
                    if section.outer != preview.bounds
                        && frame
                            .layout
                            .sections
                            .iter()
                            .find(|before| before.section == section.section)
                            .is_none_or(|before| before.outer != section.outer)
                    {
                        ui_kit::panel_host_neighbour_preview(ui, egui_rect(section.outer), false, opacity);
                    }
                }
                for section in &frame.layout.sections {
                    if !projected.sections.iter().any(|after| after.section == section.section) {
                        ui_kit::panel_host_neighbour_preview(ui, egui_rect(section.outer), true, opacity);
                    }
                }
            }
            ui_kit::panel_host_preview(ui, egui_rect(preview.bounds), true, None, opacity);
            if let Some(marker) = preview.tab_marker {
                ui_kit::panel_host_tab_insertion(ui, egui_rect(marker), opacity);
            }
            if progress < 1.0 {
                ui.ctx().request_repaint();
            }
        } else {
            self.preview_started = None;
        }
        if let Err(reason) = &mount
            && !frame.events.iter().any(|event| matches!(event, HostEvent::Rejected { action: None, error } if error == reason))
        {
            frame.events.push(HostEvent::Rejected { action: None, error: reason.clone() });
        }
        // Dispatch AFTER painting, with the same provider/native adapter that
        // created the retained runtime. No content is recreated for this borrow.
        let mut overlays = std::mem::take(&mut self.overlays);
        let mut accepted = Vec::new();
        let mut dismiss_popup_peek = false;
        for command in commands {
            let (command, source) = match command {
                HostCommand::Contextual(action, source) => {
                    if !owns_focused_action(ui.ctx(), source) {
                        continue;
                    }
                    match self.contextual_in_frame(runtime, &frame, action) {
                        Ok(action) => (HostCommand::Layout(action), Some(source)),
                        Err(reason) => {
                            frame.events.push(HostEvent::Rejected { action: None, error: reason });
                            continue;
                        }
                    }
                }
                HostCommand::WidgetLayout(action, source) => {
                    if !owns_widget_action(ui.ctx(), source) {
                        continue;
                    }
                    (HostCommand::Layout(action), Some(source))
                }
                HostCommand::Content(instance, action, source) => {
                    if !owns_widget_action(ui.ctx(), source) {
                        continue;
                    }
                    (HostCommand::Content(instance, action, source), Some(source))
                }
                HostCommand::Scroll { horizontal, offset, source } => {
                    if !owns_widget_action(ui.ctx(), source)
                        && !ui.ctx().read_response(source).is_some_and(|response| response.enabled() && response.dragged())
                        && !owns_focused_action(ui.ctx(), source)
                    {
                        continue;
                    }
                    (HostCommand::Scroll { horizontal, offset, source }, Some(source))
                }
                HostCommand::OpenMenu { menu, anchor, source, secondary } => {
                    if !allow_actions || dismiss_menu || !owns_menu_action(ui.ctx(), source, secondary) {
                        continue;
                    }
                    (HostCommand::OpenMenu { menu, anchor, source, secondary }, Some(source))
                }
                other => (other, None),
            };
            dismiss_popup_peek |= source.is_some_and(|source| self.popup_action_dismisses_peek(runtime, &frame, ui.ctx(), source));
            accepted.push(command);
        }
        if dismiss_popup_peek
            && !accepted.iter().any(|command| {
                matches!(command,
            HostCommand::Layout(action) if standalone_header_action(action))
            })
        {
            // Preserve an unrelated popup through press so its actual row can
            // complete its click. Dismiss on the accepted activation, atomically
            // with the first structural Apply; never merge unrelated actions.
            if let Some(mutations) = accepted.iter_mut().find_map(|command| match command {
                HostCommand::Layout(LayoutAction::Apply { mutations, .. }) => Some(mutations),
                _ => None,
            }) {
                if !mutations.contains(&Mutation::DismissPeek) {
                    mutations.insert(0, Mutation::DismissPeek);
                }
            } else {
                accepted.insert(0, HostCommand::Layout(apply_action(frame.layout.revision, vec![Mutation::DismissPeek])));
            }
        }
        let mut sent_layout = false;
        for command in accepted {
            match command {
                HostCommand::Scroll { horizontal, offset, .. } => {
                    if horizontal {
                        self.options.geometry.scroll.x = offset;
                    } else {
                        self.options.geometry.scroll.y = offset;
                    }
                    self.interaction.cancel();
                    frame.preview = None;
                    ui.ctx().request_repaint();
                }
                HostCommand::Prepared(candidate) if !sent_layout => {
                    sent_layout = true;
                    let result = with_local_tracking(services, &scope, frame.layout.capacity, &mut overlays, |services| {
                        let proposal = runtime.retain_host_candidate(*candidate, frame.layout.clone(), services.models)?;
                        commit_catalogued_proposal(runtime, proposal, native, services, catalogue)
                    });
                    match result {
                        Ok(reply) => {
                            let reply = super::runtime::normalize_native_reply(reply, &self.native_capabilities);
                            self.note_reply(&reply);
                            frame.events.push(HostEvent::PreparedCommitted { reply: reply.clone() });
                            frame.reply = reply;
                            self.last_rejection = None;
                            self.clear_menu();
                            frame.next_layout = resolve_frame_layout(
                                runtime,
                                frame.layout.render_domain,
                                frame.layout.capacity,
                                &frame.layout.chrome_obstacles,
                                &self.options,
                            )
                            .ok();
                            ui.ctx().request_repaint();
                        }
                        Err(reason) => {
                            self.last_rejection = Some(reason.clone());
                            frame.events.push(HostEvent::Rejected { action: None, error: reason });
                        }
                    }
                    self.interaction.cancel();
                    frame.preview = None;
                }
                HostCommand::Layout(action) if !sent_layout => {
                    sent_layout = true;
                    let result: PanelResult<LayoutReply> = with_local_tracking(services, &scope, frame.layout.capacity, &mut overlays, |services| {
                        let reply = if standalone_header_action(&action) {
                            commit_registered_action(runtime, action.clone(), native, services, catalogue)?
                        } else {
                            let proposal = runtime.prepare_host_proposal(frame.layout.clone(), action.clone(), services.models)?;
                            commit_catalogued_proposal(runtime, proposal, native, services, catalogue)?
                        };
                        Ok(super::runtime::normalize_native_reply(reply, &self.native_capabilities))
                    });
                    match result {
                        Ok(reply) => {
                            self.last_rejection = None;
                            for effect in &reply.effects {
                                if let HostEffect::Focus { instance } = effect {
                                    self.focus_request = Some(instance.clone());
                                }
                            }
                            frame.events.push(HostEvent::Committed { action, reply: reply.clone() });
                            frame.reply = reply;
                            self.clear_menu();
                            self.interaction.cancel();
                            frame.preview = None;
                            frame.next_layout = resolve_frame_layout(
                                runtime,
                                frame.layout.render_domain,
                                frame.layout.capacity,
                                &frame.layout.chrome_obstacles,
                                &self.options,
                            )
                            .ok();
                            ui.ctx().request_repaint();
                        }
                        Err(reason) => {
                            self.last_rejection = Some(reason.clone());
                            if let Some(preview) = &mut frame.preview {
                                preview.rejection = Some(reason.clone());
                                preview.action = None;
                            }
                            frame.events.push(HostEvent::Rejected { action: Some(action), error: reason });
                            frame.reply = super::runtime::normalize_native_reply(runtime.layout().inspect(), &self.native_capabilities);
                            self.interaction.cancel();
                        }
                    }
                }
                HostCommand::Content(instance, action, _) => {
                    let result = with_local_tracking(services, &scope, frame.layout.capacity, &mut overlays, |services| {
                        runtime.content_action(&instance, &action, services)
                    });
                    if let Err(reason) = &result {
                        self.last_rejection = Some(reason.clone());
                    }
                    frame.events.push(HostEvent::ContentAction { instance, action, result });
                }
                HostCommand::OpenMenu { menu, anchor, source, secondary } => {
                    if allow_actions && !dismiss_menu && owns_menu_action(ui.ctx(), source, secondary) {
                        // A submenu row replaces its parent menu, so retain the
                        // live root launcher for Escape while tracking the row
                        // separately for the next menu's keyboard entry.
                        let nested = self.menu.is_some() && self.menu_items.contains(&source);
                        if !nested {
                            self.menu_opener = Some(source);
                        }
                        self.menu_entry = Some(source);
                        self.menu_reveal = None;
                        self.menu = Some(menu);
                        self.menu_anchor = Some(anchor);
                        // This pass still reports only the menu actually painted.
                        ui.ctx().request_repaint();
                    }
                }
                HostCommand::Layout(_) | HostCommand::WidgetLayout(_, _) | HostCommand::Prepared(_) | HostCommand::Contextual(_, _) => {}
            }
        }
        if painted_move_source.as_ref() != self.interaction.move_source() {
            ui.ctx().request_repaint();
        }
        let source = self.interaction.move_source();
        if self.live_drag && painted_landing.as_ref() != frame.preview.as_ref().and_then(|preview| preview.target.as_ref()) {
            ui.ctx().request_repaint();
        }
        let landing = frame.preview.as_ref().filter(|preview| preview.rejection.is_none()).and_then(|preview| preview.projected.as_ref());
        if source.is_none() && drag_cards.is_empty() {
            self.drag_paint.clear();
        }
        if self.live_drag && !drag_cards.is_empty() && (source.is_some() || self.drag_paint.active()) {
            let visual = landing.or(self.interaction.detached_layout()).or(frame.next_layout.as_ref()).unwrap_or(&frame.layout);
            let landing_preview = frame.preview.as_ref().filter(|_| landing.is_some() && source.is_some());
            let preview_bounds =
                self.drag_paint.show(ui, &drag_cards, visual, source, landing_preview, runtime.environment().motion == Motion::Reduced);
            if source.is_some() {
                for section in &visual.sections {
                    if !drag_cards.iter().any(|card| card.instance == section.instance)
                        && let Ok(presentation) = runtime.instance_presentation(&section.instance)
                    {
                        // A tab activated only by a proposal has no body paint
                        // in this pass. Keep its projected card and title visible.
                        super::drag_paint::paint_unmounted(ui, visual, section, &presentation.title, source == Some(&section.instance));
                    }
                }
            }
            if let Some(preview) = landing_preview {
                let destination = egui_rect(preview.bounds);
                let bounds = preview_bounds.unwrap_or(destination);
                ui_kit::panel_host_preview(ui, bounds, true, None, 1.0);
            }
        }
        if (!self.live_drag || landing.is_none())
            && let (Some(instance), Some(at)) = (source, self.interaction.move_position())
            && let Ok(presentation) = runtime.instance_presentation(instance)
        {
            ui_kit::panel_host_drag_title(
                ui,
                &presentation.title,
                egui::pos2(at.x, at.y),
                egui_rect(frame.layout.render_domain),
                frame.layout.direction == Direction::Rtl,
            );
        }
        let input = self.input_rects(&frame);
        let occlusion = self.occlusion_rects(&frame);
        frame.events.retain(|event| !matches!(event, HostEvent::Regions { .. }));
        frame.events.truncate(MAX_HOST_FRAME_OUTCOMES);
        // Replace the complete frame outcome snapshot; never append the copy
        // restored by show_deferred. Publish only this pass's Regions below.
        self.outcomes.clone_from(&frame.events);
        frame.events.insert(
            0,
            HostEvent::Regions {
                scope,
                revision: frame.layout.revision,
                painted: frame
                    .layout
                    .painted
                    .iter()
                    .copied()
                    .chain(frame.menu_bounds)
                    .chain(frame.owned_overlays.iter().map(|region| region.bounds))
                    .collect(),
                input,
                occlusion,
                reservations: frame.layout.reservations.clone(),
            },
        );
        self.overlays = overlays;
        self.frame = Some(frame);
        mount
    }

    fn clear_menu(&mut self) {
        self.menu = None;
        self.menu_anchor = None;
        self.menu_opener = None;
        self.menu_entry = None;
        self.menu_reveal = None;
        self.menu_items.clear();
    }

    fn menu_keyboard(&mut self, ui: &mut egui::Ui, frame: &HostFrame) {
        let Some(_bounds) = frame.menu_bounds else {
            return;
        };
        let items: Vec<_> =
            self.menu_items.iter().copied().filter(|id| ui.ctx().read_response(*id).is_some_and(|response| response.enabled())).collect();
        if items.is_empty() {
            return;
        }
        let focused = ui.memory(|memory| memory.focused());
        let index = items.iter().position(|control| Some(*control) == focused);
        if let Some(source) = self.menu_entry.take()
            && (focused == Some(source) || focused.is_none())
        {
            ui.memory_mut(|memory| memory.request_focus(items[0]));
            self.menu_reveal = Some(items[0]);
            ui.ctx().request_repaint();
        }
        let focused = ui.memory(|memory| memory.focused());
        let index = index.or_else(|| items.iter().position(|control| Some(*control) == focused));
        if let Some(index) = index {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(items[index], egui::EventFilter { vertical_arrows: true, escape: true, ..Default::default() })
            });
            let destination = ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) {
                    Some((index + 1) % items.len())
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) {
                    Some((index + items.len() - 1) % items.len())
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                    Some(0)
                } else if input.consume_key(egui::Modifiers::NONE, egui::Key::End) {
                    Some(items.len() - 1)
                } else {
                    None
                }
            });
            if let Some(destination) = destination {
                ui.memory_mut(|memory| memory.request_focus(items[destination]));
                self.menu_reveal = Some(items[destination]);
                // The next row paint requests scrolling while its ScrollArea
                // is active; a request after ScrollArea::show is too late.
                ui.ctx().request_repaint();
            }
            if self.pending_escape || ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                let opener = self.menu_opener;
                self.clear_menu();
                self.pending_escape = false;
                if let Some(opener) = opener.filter(|opener| {
                    frame.controls.iter().any(|control| control.id == *opener && control.enabled)
                        && ui.ctx().read_response(*opener).is_some_and(|response| response.enabled())
                }) {
                    ui.memory_mut(|memory| memory.request_focus(opener));
                    self.owned_focus = Some(opener);
                }
                ui.ctx().request_repaint();
            }
        }
    }

    fn input_rects(&self, frame: &HostFrame) -> Vec<LogicalRect> {
        frame.layout.input.iter().copied().chain(frame.menu_bounds).chain(frame.owned_overlays.iter().map(|region| region.bounds)).collect()
    }

    fn popup_action_dismisses_peek(&self, runtime: &PanelRuntime, frame: &HostFrame, ctx: &egui::Context, source: egui::Id) -> bool {
        let Some(peek) = runtime.layout().interaction().peek.as_ref() else {
            return false;
        };
        let Ok(peek_section) = runtime.layout().layout().containing(peek) else {
            return false;
        };
        let protects = match self.menu.as_ref() {
            Some(HostMenu::Panel(instance)) => runtime.layout().layout().containing(instance).is_ok_and(|section| section.id == peek_section.id),
            Some(HostMenu::Tabs(section)) => *section == peek_section.id,
            None => return false,
        };
        if protects {
            return false;
        }
        let Some(control) = frame.controls.iter().find(|control| control.id == source) else {
            return false;
        };
        if !frame.menu_bounds.is_some_and(|bounds| bounds.contains_rect(control.bounds)) {
            return false;
        }
        let at = ctx
            .read_response(source)
            .and_then(|response| {
                (response.clicked_by(egui::PointerButton::Primary) || response.secondary_clicked()).then(|| response.interact_pointer_pos()).flatten()
            })
            .map_or_else(|| center(control.bounds), point);
        !frame.layout.sections.iter().any(|section| section.peek && section.instance == *peek && section.outer.contains(at))
            && !frame.layout.rails.iter().any(|rail| rail.bounds.contains(at))
    }
    fn occlusion_rects(&self, frame: &HostFrame) -> Vec<LogicalRect> {
        frame.layout.occlusion.iter().copied().chain(frame.menu_bounds).chain(frame.owned_overlays.iter().map(|region| region.bounds)).collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn scroll_from_wheel(&mut self, ui: &egui::Ui, frame: &HostFrame) -> bool {
        if self.menu.is_some() {
            return false;
        }
        let maximum = egui::vec2(
            (frame.layout.extent.width - frame.layout.capacity.size.width).max(0.0),
            (frame.layout.extent.height - frame.layout.capacity.size.height).max(0.0),
        );
        if maximum == egui::Vec2::ZERO {
            return false;
        }
        let hovered = ui.input(|input| input.pointer.hover_pos()).is_some_and(|position| {
            frame.controls.iter().filter(|control| matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Surface { .. }))).any(
                |control| {
                    control.enabled
                        && egui_rect(control.bounds).contains(position)
                        && ui.ctx().read_response(control.id).is_some_and(|response| {
                            response.enabled()
                                && response.interact_rect.contains(position)
                                && owns_current_layer_at(ui.ctx(), response.layer_id, position)
                        })
                },
            )
        });
        if !hovered {
            return false;
        }
        let previous = self.options.geometry.scroll;
        let next = crate::navigation::panel_scroll_offset(ui.ctx(), egui::vec2(previous.x, previous.y), maximum);
        self.options.geometry.scroll = Point { x: next.x, y: next.y };
        previous != self.options.geometry.scroll
    }

    fn paint_scrollbars(
        &self,
        ui: &mut egui::Ui,
        scope: &LocalScope,
        window: &HostId,
        frame: &mut HostFrame,
        commands: &mut Vec<HostCommand>,
        allow_actions: bool,
    ) {
        for (horizontal, bounds) in scroll_tracks(&frame.layout) {
            let target = HostControlTarget::Scroll { horizontal };
            let id = scoped_id(scope, window, None, &target);
            let label = solidcraft_ui_text::tr(if horizontal { "ui.panel_host.scroll_horizontal" } else { "ui.panel_host.scroll_vertical" });
            let (visible, total, offset) = if horizontal {
                (frame.layout.capacity.size.width, frame.layout.extent.width, frame.layout.allocation_environment.scroll.x)
            } else {
                (frame.layout.capacity.size.height, frame.layout.extent.height, frame.layout.allocation_environment.scroll.y)
            };
            let (response, next) = ui_kit::panel_host_scrollbar(ui, id, egui_rect(bounds), horizontal, visible, total, offset, label);
            record_control(frame, &response, target, label, true, ui.clip_rect());
            if allow_actions && let Some(offset) = next {
                commands.push(HostCommand::Scroll { horizontal, offset, source: response.id });
            }
        }
    }

    fn paint_remaining(
        &self,
        ui: &mut egui::Ui,
        _runtime: &PanelRuntime,
        _metadata: &BTreeMap<InstanceId, PanelPresentation>,
        frame: &mut HostFrame,
        _commands: &mut Vec<HostCommand>,
        pointer_events: &mut Vec<(egui::Id, HostPointerEvent)>,
        allow_actions: bool,
        _locale: solidcraft_ui_text::Locale,
    ) {
        for control in &frame.controls {
            let HostControlTarget::Pointer(target @ HostPointerTarget::Sash { first, .. }) = &control.target else {
                continue;
            };
            let Some(panel) = frame.layout.sections.iter().find(|panel| panel.section == *first) else {
                continue;
            };
            let horizontal = panel.zone != Zone::Bottom;
            let extent = if horizontal { panel.outer.size.height } else { panel.outer.size.width };
            let response = ui_kit::panel_host_sash(ui, control.id, egui_rect(control.bounds), &control.accessible_name, horizontal, extent);
            if allow_actions && response.double_clicked() {
                pointer_events.push((response.id, HostPointerEvent::DoubleClick { target: target.clone() }));
            }
        }
    }
    fn pointer_input(
        &mut self,
        ui: &egui::Ui,
        runtime: &PanelRuntime,
        scope: &LocalScope,
        frame: &mut HostFrame,
        commands: &mut Vec<HostCommand>,
        extra: Vec<(egui::Id, HostPointerEvent)>,
    ) {
        let raw = ui.input(|input| input.events.iter().take(4097).cloned().collect::<Vec<_>>());
        if raw.len() > 4096 {
            self.interaction.cancel();
            frame.preview = None;
            commands.clear();
            frame.events.push(HostEvent::Rejected { action: None, error: error(Rule::CollectionLimit, "host input event limit") });
            return;
        }
        let mut events = Vec::new();
        // egui reports end-of-frame button state. Recover the state before the
        // first raw button transition so a real release frame can still commit.
        // PointerGone itself preserves egui's held buttons.
        let mut primary_down = raw
            .iter()
            .find_map(|event| match event {
                egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed, .. } => Some(!*pressed),
                _ => None,
            })
            .unwrap_or_else(|| ui.input(|input| input.pointer.primary_down()));
        if self.interaction.is_captured() && !primary_down {
            events.push(HostPointerEvent::Cancel);
        }
        let reserved_escape = std::mem::take(&mut self.pending_escape);
        if reserved_escape {
            if !self.interaction.is_captured() && self.menu.is_some() {
                self.clear_menu();
            } else {
                events.push(HostPointerEvent::Escape);
            }
        }
        for event in raw.into_iter().take(4096) {
            match event {
                egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                    if reserved_escape {
                        continue;
                    }
                    if !self.interaction.is_captured() && self.menu.is_some() {
                        self.clear_menu();
                    } else {
                        events.push(HostPointerEvent::Escape);
                    }
                }
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } => {
                    primary_down = pressed;
                    let at = point(pos);
                    if pressed {
                        let in_menu = frame.menu_bounds.is_some_and(|bounds| bounds.contains(at));
                        let in_owned_overlay =
                            runtime.layout().interaction().peek.as_ref().and_then(|peek| self.overlays.get(peek)).is_some_and(|regions| {
                                regions.iter().any(|entry| frame.owned_overlays.contains(&entry.region) && entry.region.bounds.contains(at))
                            });
                        let other_overlay_press = !ui_kit::Container::modal_input_blocked(ui.ctx())
                            && self.overlays.iter().any(|(instance, entries)| {
                                runtime.layout().interaction().peek.as_ref() != Some(instance)
                                    && entries.iter().any(|entry| {
                                        entry.region.bounds.contains(at)
                                            && frame.owned_overlays.contains(&entry.region)
                                            && entry.layer.is_some_and(|layer| {
                                                current_area_contains(ui.ctx(), layer, pos) && owns_current_layer_at(ui.ctx(), layer, pos)
                                            })
                                    })
                            });
                        let geometric_target = frame.layout.hit_test(at);
                        let target = geometric_target.as_ref().and_then(|target| {
                            frame
                                .controls
                                .iter()
                                .find(|control| control.enabled && control.target == HostControlTarget::Pointer(target.clone()))
                                .filter(|control| owns_primary_press(ui.ctx(), control.id, pos))
                                .map(|_| target.clone())
                        });
                        // A body leaf (e.g. TextEdit) owns its press instead of
                        // the enclosing Surface response. Its actual card layer
                        // still owns outside-dismissal, without becoming a drag
                        // source or consuming/retargeting the leaf's input.
                        let body_press = matches!(&geometric_target, Some(HostPointerTarget::Surface { .. }))
                            && geometric_target.as_ref().is_some_and(|pointer| {
                                frame.controls.iter().any(|control| {
                                    control.enabled
                                        && control.target == HostControlTarget::Pointer(pointer.clone())
                                        && ui.ctx().read_response(control.id).is_some_and(|response| {
                                            response.enabled()
                                                && response.interact_rect.contains(pos)
                                                && owns_current_layer_at(ui.ctx(), response.layer_id, pos)
                                        })
                                })
                            });
                        let content_control = frame.controls.iter().rev().any(|control| {
                            control.enabled
                                && control.bounds.contains(at)
                                && matches!(&control.target, HostControlTarget::Content { .. })
                                && owns_primary_press(ui.ctx(), control.id, pos)
                        });
                        if in_owned_overlay {
                            events.push(HostPointerEvent::OutsidePress { at, inside_owned_overlay: true });
                        } else if !in_menu || other_overlay_press {
                            // A foreign widget/window owns covered geometry. It
                            // must not begin a drag or dismiss peek underneath.
                            if geometric_target.is_some() && target.is_none() && !content_control && !body_press && !other_overlay_press {
                                continue;
                            }
                            if target.is_none()
                                && !body_press
                                && !content_control
                                && !other_overlay_press
                                && !owns_current_layer_at(ui.ctx(), ui.layer_id(), pos)
                            {
                                continue;
                            }
                            if self.menu.is_some() {
                                self.clear_menu();
                            }
                            if body_press || content_control || other_overlay_press {
                                events.push(HostPointerEvent::OutsidePress { at, inside_owned_overlay: false });
                            } else if target.is_some() {
                                events.push(HostPointerEvent::Press { at, target });
                            } else {
                                events.push(HostPointerEvent::OutsidePress { at, inside_owned_overlay: false });
                            }
                        }
                    } else {
                        events.push(HostPointerEvent::Release { at });
                    }
                }
                egui::Event::PointerMoved(pos) => events.push(HostPointerEvent::Move { at: point(pos) }),
                egui::Event::PointerGone => events.push(if primary_down { HostPointerEvent::Leave } else { HostPointerEvent::Cancel }),
                egui::Event::WindowFocused(false) => events.push(HostPointerEvent::Cancel),
                _ => {}
            }
        }
        // Validate a captured scope/revision/source even when the pointer stands
        // still. Pointer cancellation must never depend on receiving another drag.
        if events.is_empty() && self.interaction.is_captured() {
            events.push(HostPointerEvent::Reconcile);
        }
        // Widget-derived gestures are provisional until all later windows have
        // painted, just like menu and content actions. Existing raw captures
        // retain their independent Move/Release routing above.
        events.extend(
            extra
                .into_iter()
                .filter(|(source, _)| owns_widget_action(ui.ctx(), *source) || owns_focused_action(ui.ctx(), *source))
                .map(|(_, event)| event),
        );
        for event in events {
            let update = self.interaction.update(runtime.layout(), scope, &frame.layout, event);
            frame.preview = update.preview;
            if let Some(candidate) = update.candidate {
                commands.push(HostCommand::Prepared(Box::new(candidate)));
            }
            if let Some(error) = update.rejection {
                frame.events.push(HostEvent::Rejected { action: None, error });
            }
        }
    }
}

pub(crate) fn runtime_scope(runtime: &PanelRuntime) -> LocalScope {
    let environment = runtime.environment();
    LocalScope { workspace: environment.workspace.clone(), document: environment.document.clone(), viewport: environment.viewport.clone() }
}

fn overlay_regions(overlays: &BTreeMap<InstanceId, Vec<RegisteredOverlay>>, ctx: Option<&egui::Context>, hidden: &[InstanceId]) -> Vec<InputRegion> {
    overlays
        .iter()
        .filter(|(instance, _)| !hidden.contains(instance))
        .flat_map(|(_, entries)| entries.iter())
        .filter(|entry| {
            entry.layer.is_none_or(|layer| {
                ctx.is_some_and(|ctx| {
                    let bounds = egui_rect(entry.region.bounds);
                    current_area_contains(ctx, layer, bounds.center())
                        && egui::containers::AreaState::load(ctx, layer.id).is_some_and(|state| {
                            ctx.layer_transform_to_global(layer).map_or(state.rect(), |transform| transform * state.rect()).contains_rect(bounds)
                        })
                })
            })
        })
        .map(|entry| entry.region.clone())
        .collect()
}

/// Observe accepted local overlay requests while forwarding to the exact same
/// injected local provider. This is routing metadata, never a second service or
/// a guess based on popup titles/egui IDs. Lifecycle release clears the record.
struct HostLocalBridge<'a> {
    inner: &'a mut dyn PanelLocalServices,
    scope: &'a LocalScope,
    capacity: LogicalRect,
    overlays: &'a mut BTreeMap<InstanceId, Vec<RegisteredOverlay>>,
}

impl HostLocalBridge<'_> {
    fn prepare_overlay(
        &self,
        scope: &LocalScope,
        instance: &InstanceId,
        region: &InputRegion,
        layer: Option<egui::LayerId>,
    ) -> PanelResult<Option<RegisteredOverlay>> {
        LocalRequest::Overlay { instance: instance.clone(), region: region.clone() }.validate()?;
        if scope != self.scope {
            return Ok(None);
        }
        if !self.capacity.contains_rect(region.bounds) {
            return Err(error(Rule::Bounds, "panel overlay outside workspace"));
        }
        let entries = self.overlays.get(instance);
        if entries.is_none() && self.overlays.len() >= MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "panel overlay owner limit"));
        }
        if entries.is_some_and(|entries| entries.len() >= MAX_TABS && !entries.iter().any(|entry| entry.region.control == region.control)) {
            return Err(error(Rule::CollectionLimit, "panel overlay limit"));
        }
        Ok(Some(RegisteredOverlay { region: region.clone(), layer }))
    }

    fn record_overlay(&mut self, instance: &InstanceId, overlay: RegisteredOverlay) {
        let entries = self.overlays.entry(instance.clone()).or_default();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.region.control == overlay.region.control) {
            *entry = overlay;
        } else {
            entries.push(overlay);
        }
    }
}

impl PanelLocalServices for HostLocalBridge<'_> {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        request.validate()?;
        let overlay = if let LocalRequest::Overlay { instance, region } = &request {
            self.prepare_overlay(scope, instance, region, None)?.map(|overlay| (instance.clone(), overlay))
        } else {
            None
        };
        let clear = if let LocalRequest::ClearOverlays { instance } = &request { Some(instance.clone()) } else { None };
        self.inner.request(scope, request)?;
        if scope == self.scope {
            if let Some(instance) = clear {
                self.overlays.remove(&instance);
            }
            if let Some((instance, overlay)) = overlay {
                self.record_overlay(&instance, overlay);
            }
        }
        Ok(())
    }
    fn request_overlay_layer(&mut self, scope: &LocalScope, instance: &InstanceId, region: InputRegion, layer: egui::LayerId) -> PanelResult<()> {
        let overlay = self.prepare_overlay(scope, instance, &region, Some(layer))?;
        self.inner.request_overlay_layer(scope, instance, region, layer)?;
        if let Some(overlay) = overlay {
            self.record_overlay(instance, overlay);
        }
        Ok(())
    }
    fn register_focus_response(&mut self, scope: &LocalScope, instance: &InstanceId, response: &egui::Response) -> PanelResult<()> {
        self.inner.register_focus_response(scope, instance, response)
    }
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId) {
        self.inner.release(scope, instance);
        if scope == self.scope {
            self.overlays.remove(instance);
        }
    }
}

fn with_local_tracking<R>(
    services: &mut ContentServices<'_>,
    scope: &LocalScope,
    capacity: LogicalRect,
    overlays: &mut BTreeMap<InstanceId, Vec<RegisteredOverlay>>,
    use_services: impl FnOnce(&mut ContentServices<'_>) -> R,
) -> R {
    let mut local = HostLocalBridge { inner: &mut *services.local, scope, capacity, overlays };
    let mut adapted = ContentServices { models: services.models, commands: &mut *services.commands, local: &mut local };
    use_services(&mut adapted)
}

fn pointer_instance(target: &HostPointerTarget) -> Option<&InstanceId> {
    match target {
        HostPointerTarget::Header { instance }
        | HostPointerTarget::Tab { instance }
        | HostPointerTarget::Rail { instance }
        | HostPointerTarget::Surface { instance }
        | HostPointerTarget::Collapse { instance }
        | HostPointerTarget::Close { instance }
        | HostPointerTarget::HeaderAction { instance, .. }
        | HostPointerTarget::Menu { instance } => Some(instance),
        _ => None,
    }
}

fn chrome_label(
    target: &HostPointerTarget,
    fallback: &str,
    metadata: &BTreeMap<InstanceId, PanelPresentation>,
    locale: solidcraft_ui_text::Locale,
) -> String {
    match target {
        HostPointerTarget::Menu { .. } => locale.text("ui.panel_host.menu").to_owned(),
        HostPointerTarget::Collapse { .. } => locale.text(fallback).to_owned(),
        HostPointerTarget::Close { .. } => locale.text("ui.panel_host.close").to_owned(),
        HostPointerTarget::HeaderAction { instance, action } => metadata
            .get(instance)
            .and_then(|presentation| presentation.actions.iter().find(|binding| binding.id == *action))
            .map_or_else(|| locale.text(fallback).to_owned(), |binding| binding.label.clone()),
        HostPointerTarget::TabOverflow { .. } => locale.text("ui.panel_host.tabs").to_owned(),
        _ => pointer_instance(target)
            .and_then(|instance| metadata.get(instance))
            .map_or_else(|| locale.text(fallback).to_owned(), |presentation| presentation.title.clone()),
    }
}

/// Geometry selects a candidate, but egui decides who actually owns a press.
/// Release/Move for an existing host capture intentionally do not use this gate.
fn owns_current_layer_at(ctx: &egui::Context, source: egui::LayerId, at: egui::Pos2) -> bool {
    let Some(raised) = ui_kit::Container::current_layer_raises(ctx) else {
        return false;
    };
    // egui stable-sorts (Order, was-raised) at end_pass. A returning Area raises
    // itself; explicit application raises go through the shared Container seam.
    // Two raised layers retain their previous relative order, not call order.
    let (source_returning, source_rank, candidates) = ctx.memory(|memory| {
        let source_rank = memory.layer_ids().position(|layer| layer == source);
        (
            !memory.areas().visible_last_frame(&source),
            source_rank,
            memory
                .layer_ids()
                .enumerate()
                .filter_map(|(rank, layer)| {
                    (layer != source && layer.order >= source.order).then_some((layer, rank, !memory.areas().visible_last_frame(&layer)))
                })
                .collect::<Vec<_>>(),
        )
    });
    let source_raised = raised.contains(&source) || source_returning && current_area_contains(ctx, source, at);
    !candidates.into_iter().any(|(layer, rank, returning)| {
        // A stale prior-frame Area, a noninteractive overlay or an invisible
        // sizing pass cannot shield input. Bounds include the layer transform.
        if !current_area_contains(ctx, layer, at) {
            return false;
        }
        if layer.order > source.order {
            return true;
        }
        let candidate_raised = raised.contains(&layer) || returning;
        match (candidate_raised, source_raised) {
            (true, false) => true,
            (false, true) => false,
            // Read stored pair order directly. A stale third Area can mask
            // both candidates in layer_id_at, but cannot change their ranks.
            _ => source_rank.is_none_or(|source_rank| rank > source_rank),
        }
    })
}

fn current_area_contains(ctx: &egui::Context, layer: egui::LayerId, at: egui::Pos2) -> bool {
    if !ctx.graphics(|graphics| {
        graphics.get(layer).is_some_and(|paint| {
            paint.all_entries().any(|entry| {
                // Native backing reserves a Noop before deciding whether its Area is shown.
                let bounds = (entry.transform_after_tessellation * entry.shape.visual_bounding_rect()).intersect(entry.clip_rect);
                bounds.is_positive() && bounds.is_finite()
            })
        })
    }) {
        return false;
    }
    egui::containers::AreaState::load(ctx, layer.id).is_some_and(|state| {
        let bounds = ctx.layer_transform_to_global(layer).map_or(state.rect(), |transform| transform * state.rect());
        state.interactable && bounds.contains(at)
    })
}

fn owns_primary_press(ctx: &egui::Context, id: egui::Id, at: egui::Pos2) -> bool {
    ctx.read_response(id).is_some_and(|response| {
        response.enabled()
            && response.interact_rect.contains(at)
            && owns_current_layer_at(ctx, response.layer_id, at)
            && (response.is_pointer_button_down_on()
                || response.clicked_by(egui::PointerButton::Primary)
                || response.drag_started_by(egui::PointerButton::Primary)
                || response.dragged_by(egui::PointerButton::Primary)
                || response.drag_stopped_by(egui::PointerButton::Primary))
    })
}

fn owns_widget_action(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.read_response(id).is_some_and(|response| {
        if !response.enabled() {
            return false;
        }
        if !response.clicked_by(egui::PointerButton::Primary) {
            return response.clicked() && ctx.memory(|memory| memory.focused()) == Some(id);
        }
        response
            .interact_pointer_pos()
            .or_else(|| ctx.input(|input| input.pointer.interact_pos()))
            .is_some_and(|at| owns_current_layer_at(ctx, response.layer_id, at))
    })
}

fn owns_focused_action(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.memory(|memory| memory.focused()) == Some(id)
        && ctx.read_response(id).is_some_and(|response| response.enabled() && response.interact_rect.is_positive())
}

fn owns_menu_action(ctx: &egui::Context, id: egui::Id, secondary: bool) -> bool {
    if !secondary {
        return owns_widget_action(ctx, id);
    }
    ctx.read_response(id).is_some_and(|response| {
        response.enabled()
            && response.secondary_clicked()
            && response
                .interact_pointer_pos()
                .or_else(|| ctx.input(|input| input.pointer.interact_pos()))
                .is_some_and(|at| response.interact_rect.contains(at) && owns_current_layer_at(ctx, response.layer_id, at))
    })
}

fn section_surface_id(scope: &LocalScope, window: &HostId, section: &SectionId) -> egui::Id {
    egui::Id::unique(("universal_panel_section", &scope.workspace, &scope.document, &scope.viewport, window, section))
}

fn scoped_id(scope: &LocalScope, window: &HostId, owner: Option<&ContentOwner>, target: &HostControlTarget) -> egui::Id {
    egui::Id::unique(("universal_panel_host", &scope.workspace, &scope.document, &scope.viewport, window, owner, target))
}
fn control_id(runtime: &PanelRuntime, owners: &BTreeMap<InstanceId, ContentOwner>, target: &HostControlTarget) -> egui::Id {
    let instance = match target {
        HostControlTarget::Pointer(pointer) => pointer_instance(pointer),
        HostControlTarget::Shared { instance, .. } | HostControlTarget::Content { instance, .. } | HostControlTarget::PanelMenu { instance } => {
            Some(instance)
        }
        HostControlTarget::Scroll { .. } => None,
    };
    scoped_id(&runtime_scope(runtime), &runtime.environment().window, instance.and_then(|id| owners.get(id)), target)
}
pub(crate) fn egui_rect(rect: LogicalRect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(rect.origin.x, rect.origin.y), egui::vec2(rect.size.width, rect.size.height))
}
pub(crate) fn logical_rect(rect: egui::Rect) -> LogicalRect {
    LogicalRect { origin: point(rect.min), size: Size { width: rect.width(), height: rect.height() } }
}
fn point(at: egui::Pos2) -> Point {
    Point { x: at.x, y: at.y }
}
fn center(rect: LogicalRect) -> Point {
    Point { x: rect.origin.x + rect.size.width / 2.0, y: rect.origin.y + rect.size.height / 2.0 }
}

pub(crate) struct EmptyPanelHost;
impl PanelHost for EmptyPanelHost {
    fn mount(
        &mut self,
        _geometry: &ResolvedPanel,
        _presentation: &PanelPresentation,
        _context: &PanelContext,
        _content: &mut dyn PanelController,
        _services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        Err(error(Rule::Placement, "empty host cannot mount content"))
    }
}

pub(super) fn rule_key(rule: Rule) -> &'static str {
    match rule {
        Rule::Availability => "ui.panel_host.rule.availability",
        Rule::Bounds => "ui.panel_host.rule.bounds",
        Rule::NoSpace => "ui.panel_host.rule.no_space",
        Rule::Capability => "ui.panel_host.rule.capability",
        Rule::CollectionLimit => "ui.panel_host.rule.collection_limit",
        Rule::DuplicateId => "ui.panel_host.rule.duplicate_id",
        Rule::ExactlyOnce => "ui.panel_host.rule.exactly_once",
        Rule::FeatureDisabled => "ui.panel_host.rule.feature_disabled",
        Rule::Id => "ui.panel_host.rule.id",
        Rule::IncompatibleTabs => "ui.panel_host.rule.incompatible_tabs",
        Rule::Lifecycle => "ui.panel_host.rule.lifecycle",
        Rule::MissingTarget => "ui.panel_host.rule.missing_target",
        Rule::PinConflict => "ui.panel_host.rule.pin_conflict",
        Rule::PinOrder => "ui.panel_host.rule.pin_order",
        Rule::Placement => "ui.panel_host.rule.placement",
        Rule::Revision => "ui.panel_host.rule.revision",
        Rule::Schema => "ui.panel_host.rule.schema",
        Rule::Version => "ui.panel_host.rule.version",
    }
}

struct WorkspaceContentHost<'a> {
    ui: &'a mut egui::Ui,
    frame_content: &'a mut dyn FramePanelContent,
    layout: &'a HostLayout,
    move_source: Option<&'a InstanceId>,
    drag_cards: Option<&'a mut Vec<CardPaint>>,
    drag_layout: Option<&'a HostLayout>,
    tab_drop: Option<(&'a SectionId, &'a InstanceId, usize)>,
    open_menu: Option<&'a HostMenu>,
    controls: &'a mut Vec<HostFrameControl>,
    metadata: &'a BTreeMap<InstanceId, PanelPresentation>,
    header_icons: &'a BTreeMap<(InstanceId, ControlId), String>,
    scope: &'a LocalScope,
    window: &'a HostId,
    owned_focus: &'a mut Option<egui::Id>,
    content_focus: &'a mut Option<ContentFocus>,
    content_bookmarks: &'a mut BTreeMap<ControllerKey, ContentFocus>,
    content_reports: &'a mut egui::IdMap<ContentFocus>,
    motion: Motion,
    commands: &'a mut Vec<HostCommand>,
    allow_actions: bool,
    unpainted: &'a mut Vec<(egui::LayerId, LogicalRect, bool)>,
}

impl PanelHost for WorkspaceContentHost<'_> {
    fn mount(
        &mut self,
        geometry: &ResolvedPanel,
        presentation: &PanelPresentation,
        context: &PanelContext,
        content: &mut dyn PanelController,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        geometry.validate()?;
        presentation.validate()?;
        context.validate()?;
        if geometry.instance != context.instance || geometry.outer != context.outer || geometry.body != context.usable_body {
            return Err(error(Rule::Placement, "content context differs from resolved host"));
        }
        if geometry.painted.is_empty() {
            return Ok(());
        }
        let section = self
            .layout
            .sections
            .iter()
            .find(|section| section.instance == geometry.instance)
            .ok_or_else(|| error(Rule::MissingTarget, "resolved section metadata missing"))?;
        let surface_target = HostControlTarget::Pointer(HostPointerTarget::Surface { instance: geometry.instance.clone() });
        let surface_id = scoped_id(self.scope, self.window, Some(&context.owner), &surface_target);
        let mut failure = None;
        let wanted_focus = self
            .content_bookmarks
            .get(&ControllerKey { instance: context.instance.clone(), owner: context.owner.clone() })
            .map(|bookmark| bookmark.id);
        let fallback;
        let style = if let Some(style) = self.layout.geometry_policy.surfaces.as_ref() {
            style
        } else {
            fallback = HostSurfaceStyle::capture(self.ui.ctx(), false, self.layout.geometry_policy.requested_opacity, None);
            &fallback
        };
        let appearance = style.appearance(section.surface, section.peek || section.zone == Zone::Floating);
        let panel_id = section_surface_id(self.scope, self.window, &section.section);
        let layer = if section.peek || section.zone == Zone::Floating {
            egui::LayerId::new(egui::Order::Background, panel_id.with("area"))
        } else {
            self.ui.layer_id()
        };
        // Lay out the real body once at its landing size. Capture the resulting
        // paint and animate from the committed position without stretching widgets.
        let committed_bounds = egui_rect(section.outer);
        let projected = self.drag_layout.and_then(|layout| {
            layout.sections.iter().find(|target| {
                target.instance == section.instance
                    && target.inline == section.inline
                    && target.collapsed == section.collapsed
                    && target.outer.size.width == section.outer.size.width
                    && (target.zone == Zone::Floating) == (section.zone == Zone::Floating)
            })
        });
        let paint_section = projected.unwrap_or(section);
        let tab_drop = self.tab_drop.filter(|(receiver, _, _)| *receiver == &section.section && paint_section.tab_strip.is_some());
        let paint_bounds = egui_rect(paint_section.outer);
        let chrome_offset = paint_bounds.min - committed_bounds.min;
        let paint_context = projected.filter(|target| target.outer != section.outer || target.body != section.body).map(|target| PanelContext {
            outer: target.outer,
            usable_body: target.body.or(target.inline_content),
            preview_geometry: true,
            ..context.clone()
        });
        let content_context = paint_context.as_ref().unwrap_or(context);
        let start = self.drag_cards.as_ref().map(|_| shape_count(self.ui.ctx(), layer));
        ui_kit::panel_host_surface(
            self.ui,
            panel_id,
            paint_bounds,
            section.peek || section.zone == Zone::Floating,
            appearance.fill,
            matches!(appearance.contrast, Some(ContrastOutcome::RetainReadableFallback { .. })).then(|| style.palette(appearance)),
            |ui, content_clip| {
                if (section.peek || section.zone == Zone::Floating) && (!ui.is_visible() || ui.is_sizing_pass()) {
                    self.unpainted.push((ui.layer_id(), section.outer, ui.is_sizing_pass()));
                    return;
                }
                // A surface response provides semantic focus and owns only the actual
                // painted card; child controls retain their own accessible responses.
                let response = ui.interact(paint_bounds, surface_id, egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), &presentation.title));
                if section.tabs.len() + section.hidden_tabs.len() > 1 {
                    let mut tablist = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("tablist_semantics")
                            .scope_id(surface_id.with("tablist"))
                            .max_rect(egui_rect(paint_section.header))
                            .accessibility_role(egui::accesskit::Role::TabList)
                            .accessibility_label(&presentation.title),
                    );
                    tablist.set_clip_rect(ui.clip_rect());
                }
                if let Some((_, dragged, index)) = tab_drop {
                    let order =
                        self.layout.source_state.layout.section(&section.section).ok().map(|section| section.tabs.as_slice()).unwrap_or_default();
                    paint_drop_tabs(ui, paint_section, dragged, index, order, self.metadata, self.layout.direction);
                }
                for control in self.controls.iter() {
                    let HostControlTarget::Pointer(target) = &control.target else {
                        continue;
                    };
                    let belongs = match target {
                        HostPointerTarget::Header { instance }
                        | HostPointerTarget::Menu { instance }
                        | HostPointerTarget::Collapse { instance }
                        | HostPointerTarget::Close { instance }
                        | HostPointerTarget::HeaderAction { instance, .. } => *instance == section.instance,
                        HostPointerTarget::Tab { instance } => section.tabs.iter().any(|tab| tab.instance == *instance),
                        HostPointerTarget::TabOverflow { section: id } => *id == section.section,
                        _ => false,
                    };
                    if !belongs {
                        continue;
                    }
                    if tab_drop.is_some() && matches!(target, HostPointerTarget::Tab { .. } | HostPointerTarget::TabOverflow { .. }) {
                        continue;
                    }
                    let data = pointer_instance(target).and_then(|instance| self.metadata.get(instance));
                    let label = &control.accessible_name;
                    let bounds = match target {
                        HostPointerTarget::Header { .. } if tab_drop.is_some() => {
                            let Some(bounds) = paint_section.title else { continue };
                            egui_rect(bounds)
                        }
                        HostPointerTarget::HeaderAction { action, .. } if projected.is_some() => {
                            let Some((_, bounds)) = paint_section.actions.iter().find(|(id, _)| id == action) else { continue };
                            egui_rect(*bounds)
                        }
                        _ => egui_rect(control.bounds).translate(chrome_offset),
                    };
                    let mut tab_ui;
                    let target_ui = if matches!(target, HostPointerTarget::Tab { .. }) {
                        tab_ui = ui
                            .new_child(egui::UiBuilder::new().id_salt(control.id).max_rect(bounds).accessibility_parent(surface_id.with("tablist")));
                        &mut tab_ui
                    } else {
                        &mut *ui
                    };
                    let response = if let HostPointerTarget::Tab { instance } = target {
                        ui_kit::panel_tab(
                            target_ui,
                            control.id,
                            bounds,
                            label,
                            *instance == section.instance,
                            control.enabled,
                            self.layout.direction == Direction::Rtl,
                            if self.move_source == Some(instance) { 0.5 } else { 1.0 },
                        )
                    } else if matches!(target, HostPointerTarget::Collapse { .. }) {
                        ui_kit::panel_host_disclosure(
                            target_ui,
                            control.id,
                            bounds,
                            label,
                            control.enabled,
                            section.collapsed,
                            self.motion == Motion::Reduced,
                        )
                    } else {
                        ui_kit::panel_host_control_label_opacity(
                            target_ui,
                            control.id,
                            bounds,
                            HostChrome {
                                label,
                                icon: if matches!(target, HostPointerTarget::Menu { .. } | HostPointerTarget::TabOverflow { .. }) {
                                    Some("dots")
                                } else if matches!(target, HostPointerTarget::Close { .. }) {
                                    Some("close")
                                } else if let HostPointerTarget::HeaderAction { instance, action } = target {
                                    self.header_icons.get(&(instance.clone(), action.clone())).map(String::as_str)
                                } else {
                                    None
                                },
                                icon_only: matches!(
                                    target,
                                    HostPointerTarget::Menu { .. }
                                        | HostPointerTarget::TabOverflow { .. }
                                        | HostPointerTarget::Close { .. }
                                        | HostPointerTarget::HeaderAction { .. }
                                ),
                                attention: data.is_some_and(|value| value.attention),
                                selected: matches!(target, HostPointerTarget::Tab { instance } if *instance == section.instance),
                                enabled: control.enabled,
                                drag: matches!(target, HostPointerTarget::Header { .. } | HostPointerTarget::Tab { .. }),
                                title: matches!(target, HostPointerTarget::Header { .. }),
                                grip_only: matches!(target, HostPointerTarget::Header { .. })
                                    && (tab_drop.is_some() || section.tabs.len() + section.hidden_tabs.len() > 1),
                                rtl: self.layout.direction == Direction::Rtl,
                            },
                            if matches!(target, HostPointerTarget::Header { instance } | HostPointerTarget::Tab { instance }
                            if self.move_source == Some(instance))
                            {
                                0.5
                            } else {
                                1.0
                            },
                        )
                    };
                    ui.ctx().accesskit_node_builder(response.id, |node| match target {
                        HostPointerTarget::Tab { instance } => {
                            node.set_role(egui::accesskit::Role::Tab);
                            node.set_selected(*instance == section.instance);
                        }
                        HostPointerTarget::Header { .. } | HostPointerTarget::Collapse { .. } => node.set_expanded(!section.collapsed),
                        HostPointerTarget::Menu { instance } => {
                            node.set_has_popup(egui::accesskit::HasPopup::Menu);
                            node.set_expanded(self.open_menu == Some(&HostMenu::Panel(instance.clone())));
                        }
                        HostPointerTarget::TabOverflow { section } => {
                            node.set_has_popup(egui::accesskit::HasPopup::Menu);
                            node.set_expanded(self.open_menu == Some(&HostMenu::Tabs(section.clone())));
                        }
                        HostPointerTarget::HeaderAction { action, .. } => {
                            if let Some(pressed) =
                                data.and_then(|data| data.actions.iter().find(|entry| entry.id == *action)).and_then(|entry| entry.pressed)
                            {
                                node.set_toggled(if pressed { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
                            }
                        }
                        _ => {}
                    });
                    if let HostPointerTarget::HeaderAction { instance, action } = target
                        && instance.0 == "history"
                        && response.interact_rect.is_positive()
                    {
                        crate::scenario::publish_target(&format!("history:{}", action.0), response.interact_rect);
                    }
                    let clicked = response.clicked();
                    let secondary = response.secondary_clicked();
                    if let Some(status) = data.and_then(|value| value.status.as_deref()) {
                        response.clone().backing_on_hover_text(status);
                    }
                    if let HostPointerTarget::HeaderAction { action, .. } = target
                        && let Some(reason) = data
                            .and_then(|presentation| presentation.actions.iter().find(|binding| binding.id == *action))
                            .and_then(|binding| binding.disabled_reason.as_deref())
                    {
                        response.clone().backing_on_disabled_hover_text(reason);
                    }
                    if self.allow_actions && projected.is_none() && (clicked || secondary) {
                        match target {
                            HostPointerTarget::Collapse { .. } if clicked => self.commands.push(HostCommand::WidgetLayout(
                                apply_action(
                                    self.layout.revision,
                                    vec![Mutation::Collapse { section: section.section.clone(), collapsed: !section.collapsed }],
                                ),
                                control.id,
                            )),
                            HostPointerTarget::Close { instance } if clicked => self.commands.push(HostCommand::WidgetLayout(
                                apply_action(self.layout.revision, vec![Mutation::Close { instance: instance.clone() }]),
                                control.id,
                            )),
                            HostPointerTarget::HeaderAction { instance, action } if clicked => self.commands.push(HostCommand::WidgetLayout(
                                apply_action(
                                    self.layout.revision,
                                    vec![Mutation::HeaderAction { instance: instance.clone(), action: action.clone() }],
                                ),
                                control.id,
                            )),
                            HostPointerTarget::HeaderAction { instance, .. } if secondary => {
                                if let Some(at) = response.interact_pointer_pos() {
                                    self.frame_content.header_action_context_menu(instance, at);
                                }
                            }
                            HostPointerTarget::Menu { instance } => self.commands.push(HostCommand::OpenMenu {
                                menu: HostMenu::Panel(instance.clone()),
                                anchor: control.bounds,
                                source: response.id,
                                secondary,
                            }),
                            HostPointerTarget::TabOverflow { section } => self.commands.push(HostCommand::OpenMenu {
                                menu: HostMenu::Tabs(section.clone()),
                                anchor: control.bounds,
                                source: response.id,
                                secondary,
                            }),
                            HostPointerTarget::Header { instance } | HostPointerTarget::Tab { instance } if secondary => {
                                self.commands.push(HostCommand::OpenMenu {
                                    menu: HostMenu::Panel(instance.clone()),
                                    anchor: control.bounds,
                                    source: response.id,
                                    secondary: true,
                                });
                            }
                            HostPointerTarget::Tab { instance } if !ui.input(|input| input.pointer.primary_released()) => {
                                self.commands.push(HostCommand::WidgetLayout(
                                    apply_action(self.layout.revision, vec![Mutation::Activate { instance: instance.clone() }]),
                                    control.id,
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                let t = solidcraft_theme::Tokens::get();
                // The title and its controls share the card background. One
                // kit separator spans the header, rather than per-control tiles.
                if !section.inline {
                    let header = egui_rect(paint_section.header);
                    if !section.collapsed {
                        t.secondary_hline(ui.painter(), header.x_range(), header.bottom(), t.dialog_rule);
                    }
                    if paint_section.action_bar.is_none() && !paint_section.actions.is_empty() {
                        let menu = egui_rect(paint_section.menu);
                        let x = if self.layout.direction == Direction::Rtl { menu.right() } else { menu.left() };
                        let inset = t.space_8.min(header.height() / 2.0);
                        t.secondary_vline(ui.painter(), x, egui::emath::Rangef::new(header.top() + inset, header.bottom() - inset), t.dialog_rule);
                    }
                }
                // Content scrolls only where it chooses. Each slot is an isolated
                // bounded child, so malicious/minimum-size content cannot enlarge
                // the outer host or cover the shared menu and tab strip. Keep
                // content fills inside the frame's rounded corners as well.
                let mut content_ui = ui.new_child(egui::UiBuilder::new().id_salt("content_clip").max_rect(paint_bounds));
                content_ui.set_clip_rect(content_clip);
                let content_ui = &mut content_ui;
                if !section.collapsed && section.inline {
                    if let Some(body) = paint_section.inline_content {
                        render_slot(
                            content_ui,
                            ContentSlot::Inline,
                            body,
                            content_context,
                            content,
                            services,
                            self.frame_content,
                            self.content_reports,
                            wanted_focus,
                            &mut failure,
                        );
                    }
                } else if !section.collapsed {
                    if let Some(header) = content_header_bounds(paint_section) {
                        render_slot(
                            content_ui,
                            ContentSlot::Header,
                            header,
                            content_context,
                            content,
                            services,
                            self.frame_content,
                            self.content_reports,
                            wanted_focus,
                            &mut failure,
                        );
                    }
                    if let Some(body) = paint_section.body {
                        render_slot(
                            content_ui,
                            ContentSlot::Body,
                            body,
                            content_context,
                            content,
                            services,
                            self.frame_content,
                            self.content_reports,
                            wanted_focus,
                            &mut failure,
                        );
                    }
                    if let Some(footer) = paint_section.footer {
                        let has_footer_actions = presentation.actions.iter().any(|action| !section.actions.iter().any(|(id, _)| *id == action.id));
                        let row_height = if has_footer_actions { self.layout.metrics.control_extent.min(footer.size.height) } else { 0.0 };
                        if row_height > 0.0 && egui_rect(footer).intersect(content_ui.clip_rect()).is_positive() {
                            let inset = solidcraft_theme::Tokens::get().space_4.min(footer.size.width / 2.0);
                            let row = LogicalRect {
                                origin: Point { x: footer.origin.x + inset, y: footer.origin.y },
                                size: Size { width: footer.size.width - 2.0 * inset, height: row_height },
                            };
                            let actions_id = egui::Id::unique(("panel-actions", &context.instance, &context.owner));
                            let mut actions = content_ui.new_child(egui::UiBuilder::new().scope_id(actions_id).max_rect(egui_rect(row)));
                            actions.set_clip_rect(egui_rect(row).intersect(content_ui.clip_rect()));
                            egui::ScrollArea::horizontal().id_salt("scroll").max_height(row_height).show(&mut actions, |ui| {
                                ui.horizontal(|ui| {
                                    for action in presentation.actions.iter().filter(|action| !section.actions.iter().any(|(id, _)| *id == action.id))
                                    {
                                        let target = HostControlTarget::Content { instance: context.instance.clone(), action: action.id.clone() };
                                        let id = scoped_id(self.scope, self.window, Some(&context.owner), &target);
                                        let response = ui
                                            .push_id(id, |ui| {
                                                ui.add_enabled_ui(action.enabled, |ui| {
                                                    ui_kit::button(ui, action.label.as_str(), ui_kit::Button::Small)
                                                })
                                                .inner
                                            })
                                            .inner;
                                        if let Some(pressed) = action.pressed {
                                            ui.ctx().accesskit_node_builder(response.id, |node| node.set_toggled(pressed.into()));
                                        }
                                        let visible = response.rect.intersect(ui.clip_rect()).intersect(egui_rect(row));
                                        if projected.is_none() && visible.is_positive() {
                                            self.controls.push(HostFrameControl {
                                                id: response.id,
                                                target,
                                                bounds: logical_rect(visible),
                                                role: InputRole::Action,
                                                accessible_name: action.label.clone(),
                                                enabled: action.enabled && response.enabled(),
                                            });
                                        }
                                        if self.allow_actions && projected.is_none() && response.clicked() {
                                            self.commands.push(HostCommand::Content(context.instance.clone(), action.id.clone(), response.id));
                                        }
                                        if let Some(reason) = &action.disabled_reason {
                                            response.backing_on_disabled_hover_text(reason);
                                        }
                                    }
                                });
                            });
                        }
                        let remaining = footer.size.height - row_height;
                        render_slot(
                            content_ui,
                            ContentSlot::Footer,
                            LogicalRect {
                                origin: Point { x: footer.origin.x, y: footer.origin.y + row_height },
                                size: Size { width: footer.size.width, height: remaining },
                            },
                            content_context,
                            content,
                            services,
                            self.frame_content,
                            self.content_reports,
                            wanted_focus,
                            &mut failure,
                        );
                    }
                }
                // Resize must be above content on the card's actual UI/layer. Its
                // published bounds and scoped ID remain the layout authority's.
                for control in self.controls.iter() {
                    if let HostControlTarget::Pointer(HostPointerTarget::Resize { section: id, edge }) = &control.target
                        && *id == section.section
                    {
                        let horizontal = !matches!(edge, HostResizeEdge::Left | HostResizeEdge::Right);
                        let extent = if horizontal { paint_section.outer.size.height } else { paint_section.outer.size.width };
                        ui_kit::panel_host_sash(
                            ui,
                            control.id,
                            egui_rect(control.bounds).translate(chrome_offset),
                            &control.accessible_name,
                            horizontal,
                            extent,
                        );
                    }
                }
                if let Some(focused) = ui.memory(|memory| memory.focused())
                    && let Some(bookmark) = self
                        .content_reports
                        .get(&focused)
                        .filter(|report| report.key.instance == context.instance && report.key.owner == context.owner)
                        .cloned()
                {
                    *self.owned_focus = Some(focused);
                    if self.content_bookmarks.len() >= MAX_INSTANCES
                        && !self.content_bookmarks.contains_key(&bookmark.key)
                        && let Some(oldest) = self.content_bookmarks.keys().next().cloned()
                    {
                        self.content_bookmarks.remove(&oldest);
                    }
                    self.content_bookmarks.insert(bookmark.key.clone(), bookmark.clone());
                    *self.content_focus = Some(bookmark);
                }
            },
        );
        if let (Some(cards), Some(start)) = (self.drag_cards.as_mut(), start) {
            let end = shape_count(self.ui.ctx(), layer);
            if start < end {
                cards.push(CardPaint { instance: geometry.instance.clone(), layer, bounds: paint_bounds, committed_bounds, start, end });
            }
        }
        if let Some(reason) = failure { Err(reason) } else { Ok(()) }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_slot(
    ui: &mut egui::Ui,
    slot: ContentSlot,
    bounds: LogicalRect,
    context: &PanelContext,
    content: &mut dyn PanelController,
    services: &mut ContentServices<'_>,
    frame_content: &mut dyn FramePanelContent,
    reports: &mut egui::IdMap<ContentFocus>,
    wanted_focus: Option<egui::Id>,
    failure: &mut Option<PanelError>,
) {
    if failure.is_some() || bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return;
    }
    let bounds = egui_rect(bounds);
    let t = solidcraft_theme::Tokens::get();
    let inset = bounds.shrink2(egui::vec2(t.space_4.min(bounds.width().max(0.0) / 2.0), t.space_2.min(bounds.height().max(0.0) / 2.0)));
    let clip = ui.clip_rect().intersect(inset);
    if !clip.is_positive() {
        return;
    }
    let identity_slot = if slot == ContentSlot::Inline { ContentSlot::Body } else { slot };
    let identity = egui::Id::unique(("panel-content", &context.instance, &context.owner, identity_slot as u8));
    // Explicit UI identity survives a different section/column/native parent.
    // Each owner/instance mounts only once in its complete presentation cohort.
    let mut child = ui.new_child(egui::UiBuilder::new().scope_id(identity).max_rect(inset));
    child.set_clip_rect(clip);
    let mut local = SlotFocusServices {
        inner: &mut *services.local,
        scope: LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() },
        key: ControllerKey { instance: context.instance.clone(), owner: context.owner.clone() },
        layer: child.layer_id(),
        viewport: child.ctx().viewport_id(),
        clip,
        wanted_focus,
        reports,
    };
    let mut services = ContentServices { models: services.models, commands: &mut *services.commands, local: &mut local };
    let result = frame_content.render(slot, &mut child, context, content, &mut services);
    if let Err(reason) = result {
        *failure = Some(reason);
    }
}

/// The render call supplies provenance directly; no previous-pass widget lookup
/// can establish that a conditional content control exists in this pass.
struct SlotFocusServices<'a> {
    inner: &'a mut dyn PanelLocalServices,
    scope: LocalScope,
    key: ControllerKey,
    layer: egui::LayerId,
    viewport: egui::ViewportId,
    clip: egui::Rect,
    wanted_focus: Option<egui::Id>,
    reports: &'a mut egui::IdMap<ContentFocus>,
}
impl PanelLocalServices for SlotFocusServices<'_> {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        self.inner.request(scope, request)
    }
    fn request_overlay_layer(&mut self, scope: &LocalScope, instance: &InstanceId, region: InputRegion, layer: egui::LayerId) -> PanelResult<()> {
        self.inner.request_overlay_layer(scope, instance, region, layer)
    }
    fn register_focus_response(&mut self, scope: &LocalScope, instance: &InstanceId, response: &egui::Response) -> PanelResult<()> {
        scope.validate()?;
        valid_id(&instance.0)?;
        if scope != &self.scope || instance != &self.key.instance || response.layer_id != self.layer || response.ctx.viewport_id() != self.viewport {
            return Err(error(Rule::Placement, "focus response differs from current content render"));
        }
        let visible = response.interact_rect.intersect(self.clip);
        if !response.enabled()
            || !response.sense.is_focusable()
            || !response.interact_rect.is_finite()
            || !visible.is_finite()
            || !visible.is_positive()
        {
            return Ok(());
        }
        // At most one current and one remembered candidate per mounted owner.
        let candidate = response.has_focus() || self.wanted_focus == Some(response.id);
        if candidate {
            if self.reports.get(&response.id).is_some_and(|report| report.key != self.key || report.scope != self.scope) {
                return Err(error(Rule::DuplicateId, "content focus response belongs to another owner"));
            }
            if self.reports.len() >= MAX_INSTANCES * 2 && !self.reports.contains_key(&response.id) {
                return Err(error(Rule::CollectionLimit, "content focus response bound"));
            }
        }
        self.inner.register_focus_response(scope, instance, response)?;
        if candidate {
            let selection = if response.has_focus() && response.ctx.text_edit_focused() {
                egui::TextEdit::load_state(&response.ctx, response.id).and_then(|state| state.cursor.char_range())
            } else {
                None
            };
            self.reports.insert(response.id, ContentFocus { key: self.key.clone(), scope: self.scope.clone(), id: response.id, visible, selection });
        }
        Ok(())
    }
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId) {
        self.inner.release(scope, instance);
    }
}

/// Preview the tab headings without registering a second set of controls.
fn paint_drop_tabs(
    ui: &egui::Ui,
    section: &HostSection,
    dragged: &InstanceId,
    index: usize,
    order: &[InstanceId],
    metadata: &BTreeMap<InstanceId, PanelPresentation>,
    direction: Direction,
) {
    let Some(strip) = section.tab_strip else { return };
    let t = solidcraft_theme::Tokens::get();
    let rtl = direction == Direction::Rtl;
    let mut cells: Vec<_> = section.tabs.iter().map(|tab| (&tab.instance, egui_rect(tab.bounds))).collect();
    let mut overflow = section.tab_overflow.map(egui_rect);
    if !cells.iter().any(|(id, _)| *id == dragged) {
        // Give the incoming tab a visible cell during the preview even when
        // the committed tab strip will place it in overflow.
        let preceding: Vec<_> = order.iter().filter(|id| *id != dragged).take(index).collect();
        let at = cells.iter().take_while(|(id, _)| preceding.contains(id)).count();
        let mut row = egui_rect(strip);
        if let Some(button) = overflow.filter(|button| button.width() < row.width()) {
            if rtl {
                row.min.x = button.right();
            } else {
                row.max.x = button.left();
            }
        } else {
            overflow = None;
        }
        cells.insert(at, (dragged, row));
        let width = row.width() / cells.len() as f32;
        for (i, (_, bounds)) in cells.iter_mut().enumerate() {
            let x = if rtl { row.right() - width * (i + 1) as f32 } else { row.left() + width * i as f32 };
            *bounds = egui::Rect::from_min_size(egui::pos2(x, row.top()), egui::vec2(width, row.height()));
        }
    }
    if let Some(bounds) = overflow {
        let size = (bounds.width().min(bounds.height()).min(t.control_height) - 2.0 * t.form_icon_inset).max(0.0);
        ui_kit::icons::paint_ink(ui.painter(), egui::Rect::from_center_size(bounds.center(), egui::vec2(size, size)), "dots", t.form_ink);
    }
    for (id, bounds) in cells {
        let Some(presentation) = metadata.get(id) else { continue };
        ui_kit::paint_panel_tab(ui, bounds, &presentation.title, *id == section.instance, false, id == dragged, rtl, 1.0);
    }
}

fn content_header_bounds(section: &HostSection) -> Option<LogicalRect> {
    let body = section.body?;
    let y = super::layout::bottom(section.action_bar.unwrap_or(section.header));
    let height = body.origin.y - y;
    if height <= 0.0 {
        return None;
    }
    Some(LogicalRect { origin: Point { x: body.origin.x, y }, size: Size { width: body.size.width, height } })
}

fn resolve_frame_layout(
    runtime: &PanelRuntime,
    render_domain: LogicalRect,
    capacity: LogicalRect,
    chrome: &[LogicalRect],
    options: &HostOptions,
) -> PanelResult<HostLayout> {
    let mut metrics = options.metrics;
    metrics.content_header_height = options.content_header_height;
    let mut layout = resolve_host_layout_in_domain(
        runtime.layout(),
        render_domain,
        capacity,
        chrome,
        runtime.environment().direction,
        metrics,
        &options.geometry,
    )?;
    for (_, bounds) in scroll_tracks(&layout) {
        layout.painted.push(bounds);
        layout.input.push(bounds);
        layout.occlusion.push(bounds);
        layout.reservations.push(bounds);
    }
    let obstacles: Vec<_> = layout.chrome_obstacles.iter().chain(&layout.painted).copied().collect();
    layout.usable = resolve_usable_region(layout.render_domain, &obstacles)?;
    layout.validate()?;
    Ok(layout)
}

fn scroll_tracks(layout: &HostLayout) -> Vec<(bool, LogicalRect)> {
    let capacity = egui_rect(layout.capacity);
    if !capacity.is_positive() {
        return Vec::new();
    }
    let thickness = solidcraft_theme::Tokens::get().space_8.min(capacity.width()).min(capacity.height());
    let horizontal = layout.extent.width > layout.capacity.size.width;
    let vertical = layout.extent.height > layout.capacity.size.height;
    let mut tracks = Vec::new();
    if horizontal {
        let bounds = egui::Rect::from_min_max(
            egui::pos2(capacity.left(), capacity.bottom() - thickness),
            egui::pos2(capacity.right() - if vertical { thickness } else { 0.0 }, capacity.bottom()),
        );
        if bounds.is_positive() {
            tracks.push((true, logical_rect(bounds)));
        }
    }
    if vertical {
        let bounds = egui::Rect::from_min_max(
            egui::pos2(capacity.right() - thickness, capacity.top()),
            egui::pos2(capacity.right(), capacity.bottom() - if horizontal { thickness } else { 0.0 }),
        );
        if bounds.is_positive() {
            tracks.push((false, logical_rect(bounds)));
        }
    }
    tracks
}

fn apply_action(revision: u64, mutations: Vec<Mutation>) -> LayoutAction {
    LayoutAction::Apply { expected_revision: Some(revision), mutations }
}

impl WorkspacePanelHost {
    #[allow(clippy::too_many_arguments)]
    fn paint_menu(
        &mut self,
        ui: &mut egui::Ui,
        runtime: &PanelRuntime,
        native: HostCapabilities,
        metadata: &BTreeMap<InstanceId, PanelPresentation>,
        owners: &BTreeMap<InstanceId, ContentOwner>,
        frame: &mut HostFrame,
        commands: &mut Vec<HostCommand>,
        allow_actions: bool,
        locale: solidcraft_ui_text::Locale,
    ) {
        let (Some(menu), Some(anchor)) = (self.menu.clone(), self.menu_anchor) else {
            return;
        };
        let layout = runtime.layout().layout();
        let mut groups = Vec::new();
        let mut content_actions = Vec::new();
        let mut tabs = Vec::new();
        match &menu {
            HostMenu::Panel(instance) => {
                let Ok(section) = layout.containing(instance) else {
                    self.clear_menu();
                    return;
                };
                let mut presentation = Vec::new();
                if runtime.layout().interaction().focused.as_ref() != Some(instance) || section.collapsed {
                    presentation.push(HostSharedAction::Focus);
                }
                if section_collapsible(layout, runtime.layout().registry(), section).unwrap_or(false) {
                    presentation.push(HostSharedAction::Collapse);
                }
                let mut placement: Vec<_> = [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating]
                    .into_iter()
                    .filter(|zone| *zone != section.location.zone() || section.tabs.len() > 1)
                    .map(HostSharedAction::Move)
                    .collect();
                placement.extend(
                    frame
                        .layout
                        .sections
                        .iter()
                        .filter(|card| card.section != section.id && !card.peek && intersect_rect(card.outer, frame.layout.capacity).is_some())
                        .map(|card| HostSharedAction::Join(card.section.clone())),
                );
                if !cfg!(target_arch = "wasm32") {
                    placement.push(HostSharedAction::Popout);
                }
                for group in [presentation, placement, vec![HostSharedAction::Close]] {
                    let admitted: Vec<_> = group
                        .into_iter()
                        .filter_map(|shared| {
                            let action = shared_action(
                                runtime,
                                instance,
                                &shared,
                                &native,
                                frame.layout.capacity,
                                &frame.layout.geometry_policy.identity_reservations,
                            )
                            .ok()?;
                            prepare_host_layout(runtime.layout(), &runtime_scope(runtime), &frame.layout, action.clone()).ok()?;
                            Some((shared, action))
                        })
                        .collect();
                    if !admitted.is_empty() {
                        groups.push(admitted);
                    }
                }
                let actions = metadata.get(instance).into_iter().flat_map(|presentation| &presentation.actions);
                content_actions.extend(actions.filter(|entry| {
                    !frame.controls.iter().any(|control| {
                        matches!(
                            &control.target,
                            HostControlTarget::Pointer(HostPointerTarget::HeaderAction { instance: owner, action })
                                if owner == instance && *action == entry.id
                        )
                    })
                }));
                tabs.extend(section.tabs.iter().filter(|tab| *tab != instance));
            }
            HostMenu::Tabs(section) => {
                if let Ok(section) = layout.section(section) {
                    tabs.extend(&section.tabs);
                }
            }
        }
        let has_shared = !groups.is_empty();
        let has_content = !content_actions.is_empty();
        let rows = groups.iter().map(Vec::len).sum::<usize>() + content_actions.len() + 2 * tabs.len() + usize::from(self.last_rejection.is_some());
        let separators = (groups.len() + usize::from(has_content) + tabs.len()).saturating_sub(1);
        let Some(bounds) = menu_geometry(frame.layout.capacity, anchor, rows, separators) else {
            self.clear_menu();
            return;
        };
        let id = egui::Id::unique((
            "universal_panel_menu",
            &runtime.environment().workspace,
            &runtime.environment().document,
            &runtime.environment().window,
            &runtime.environment().viewport,
        ));
        // Distinct menus retain their own scroll position. A long overflow
        // list must not start its shorter nested panel menu at the old offset.
        let scroll_id = match &menu {
            HostMenu::Panel(instance) => egui::Id::unique(("panel", instance)),
            HostMenu::Tabs(section) => egui::Id::unique(("tabs", section)),
        };
        self.menu_items.clear();
        ui_kit::panel_host_popup(ui, id, bounds, |ui| {
            if !ui.is_visible() || ui.is_sizing_pass() {
                return;
            }
            frame.menu_bounds = Some(logical_rect(bounds));
            egui::ScrollArea::vertical().id_salt(("items", scroll_id)).min_scrolled_height(0.0).max_height(ui.available_height()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(reason) = &self.last_rejection {
                    ui.colored_label(solidcraft_theme::Tokens::get().form_error, locale.text(rule_key(reason.rule)));
                }
                if let HostMenu::Panel(instance) = &menu {
                    for (index, group) in groups.into_iter().enumerate() {
                        if index > 0 {
                            ui_kit::Menu::separator(ui);
                        }
                        for (shared, action) in group {
                            let target = HostControlTarget::Shared { instance: instance.clone(), action: shared.clone() };
                            let label = shared_label(&shared, instance, runtime, metadata, locale);
                            let collapsed = layout.containing(instance).is_ok_and(|section| section.collapsed);
                            let response = menu_row(
                                ui,
                                control_id(runtime, owners, &target),
                                &label,
                                shared_menu_icon(&shared, collapsed, frame.layout.direction),
                                true,
                                &mut self.menu_reveal,
                            );
                            self.menu_items.push(response.id);
                            if shared == HostSharedAction::Collapse {
                                ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(!collapsed));
                            }
                            record_control(frame, &response, target, &label, true, bounds.intersect(ui.clip_rect()));
                            if allow_actions && response.clicked() {
                                commands.push(HostCommand::WidgetLayout(action, response.id));
                            }
                        }
                    }
                    if has_content {
                        if has_shared {
                            ui_kit::Menu::separator(ui);
                        }
                        let spec = layout.instance(instance).and_then(|panel| runtime.layout().registry().get(&panel.kind)).ok();
                        for action in content_actions {
                            let target = HostControlTarget::Content { instance: instance.clone(), action: action.id.clone() };
                            let registered = spec.and_then(|spec| spec.header_actions.iter().find(|entry| entry.id == action.id));
                            let response = menu_row(
                                ui,
                                control_id(runtime, owners, &target),
                                &action.label,
                                registered.map_or("settings", |entry| entry.icon.as_str()).into(),
                                action.enabled,
                                &mut self.menu_reveal,
                            );
                            if response.enabled() {
                                self.menu_items.push(response.id);
                            }
                            if let Some(pressed) = action.pressed {
                                ui.ctx().accesskit_node_builder(response.id, |node| node.set_toggled(pressed.into()));
                            }
                            record_control(frame, &response, target, &action.label, action.enabled, bounds.intersect(ui.clip_rect()));
                            if allow_actions && response.clicked() {
                                if registered.is_some() {
                                    commands.push(HostCommand::WidgetLayout(
                                        apply_action(
                                            frame.layout.revision,
                                            vec![Mutation::HeaderAction { instance: instance.clone(), action: action.id.clone() }],
                                        ),
                                        response.id,
                                    ));
                                } else {
                                    commands.push(HostCommand::Content(instance.clone(), action.id.clone(), response.id));
                                }
                            }
                            if let Some(reason) = &action.disabled_reason {
                                response.backing_on_disabled_hover_text(reason);
                            }
                        }
                    }
                }
                for (index, instance) in tabs.into_iter().enumerate() {
                    if has_shared || has_content || index > 0 {
                        ui_kit::Menu::separator(ui);
                    }
                    menu_instance(
                        ui,
                        runtime,
                        owners,
                        metadata,
                        frame,
                        commands,
                        &mut self.menu_items,
                        &mut self.menu_reveal,
                        instance,
                        bounds,
                        allow_actions,
                        locale,
                    );
                }
            });
            self.menu_reveal = None;
        });
    }
}

fn menu_geometry(capacity: LogicalRect, anchor: LogicalRect, rows: usize, separators: usize) -> Option<egui::Rect> {
    let capacity = egui_rect(capacity);
    if rows == 0 || !capacity.is_positive() || !capacity.is_finite() {
        return None;
    }
    let t = solidcraft_theme::Tokens::get();
    if capacity.width() <= t.space_6 * 2.0 || capacity.height() <= t.space_6 * 2.0 {
        return None;
    }
    let width = ui_kit::LIST_W.min(capacity.width());
    let content_height = rows as f32 * (t.control_height + t.space_2) + separators as f32 * (t.space_10 + 1.0);
    let height = (content_height.min((t.control_height + t.space_2) * 12.0) + t.space_12).min(capacity.height());
    let left = anchor.origin.x.min(capacity.right() - width).max(capacity.left());
    let top = (anchor.origin.y + anchor.size.height).min(capacity.bottom() - height).max(capacity.top());
    Some(egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height)))
}

fn menu_row(ui: &mut egui::Ui, id: egui::Id, label: &str, icon: ui_kit::Icon<'_>, enabled: bool, reveal: &mut Option<egui::Id>) -> egui::Response {
    let response = ui
        .push_id(id.with("menu"), |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let registry_id = format!("panel:{id:?}");
                let mut row = ui_kit::MenuItem::new(&registry_id, label, icon);
                row.enabled = enabled;
                row.show(ui)
            })
            .inner
        })
        .inner;
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_role(egui::accesskit::Role::MenuItem));
    let requested = *reveal == Some(response.id);
    if requested {
        *reveal = None;
    }
    // Native Tab/AccessKit can focus during paint; our late keyboard requests
    // need the explicit one-shot token because gained_focus is false next pass.
    if response.enabled() && response.has_focus() && (requested || response.gained_focus()) {
        response.scroll_to_me(None);
    }
    response.backing_on_hover_text(label)
}

fn record_control(frame: &mut HostFrame, response: &egui::Response, target: HostControlTarget, name: &str, enabled: bool, clip: egui::Rect) {
    let bounds = response.rect.intersect(clip);
    if bounds.is_positive() && bounds.is_finite() {
        frame.controls.push(HostFrameControl {
            id: response.id,
            target,
            bounds: logical_rect(bounds),
            role: InputRole::Action,
            accessible_name: name.to_owned(),
            enabled: enabled && response.enabled(),
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn menu_instance(
    ui: &mut egui::Ui,
    runtime: &PanelRuntime,
    owners: &BTreeMap<InstanceId, ContentOwner>,
    metadata: &BTreeMap<InstanceId, PanelPresentation>,
    frame: &mut HostFrame,
    commands: &mut Vec<HostCommand>,
    items: &mut Vec<egui::Id>,
    reveal: &mut Option<egui::Id>,
    instance: &InstanceId,
    clip: egui::Rect,
    allow_actions: bool,
    locale: solidcraft_ui_text::Locale,
) {
    let presentation = metadata.get(instance);
    let label = presentation.map_or_else(|| locale.text("ui.panel_host.tab"), |value| value.title.as_str());
    let icon = presentation.map_or("list", |value| value.icon.as_str());
    let target = HostControlTarget::Shared { instance: instance.clone(), action: HostSharedAction::Focus };
    let action = apply_action(runtime.layout().inspect().revision, vec![Mutation::Focus { instance: instance.clone() }]);
    let check = runtime.layout().prepare(action.clone());
    let accessible = locale.format("ui.panel_host.focus_target", &[("title", label)]);
    let response = menu_row(ui, control_id(runtime, owners, &target), &accessible, icon.into(), check.is_ok(), reveal);
    if response.enabled() {
        items.push(response.id);
    }
    record_control(frame, &response, target, &accessible, check.is_ok(), clip.intersect(ui.clip_rect()));
    if allow_actions && response.clicked() {
        commands.push(HostCommand::WidgetLayout(action, response.id));
    }
    if let Err(reason) = check {
        response.backing_on_disabled_hover_text(locale.text(rule_key(reason.rule)));
    }
    let target = HostControlTarget::PanelMenu { instance: instance.clone() };
    let actions_label = format!("{label} — {}", locale.text("ui.panel_host.menu"));
    let response = menu_row(ui, control_id(runtime, owners, &target), &actions_label, "dots".into(), true, reveal);
    if response.enabled() {
        items.push(response.id);
    }
    record_control(frame, &response, target, &actions_label, true, clip.intersect(ui.clip_rect()));
    if allow_actions && response.clicked() {
        commands.push(HostCommand::OpenMenu {
            menu: HostMenu::Panel(instance.clone()),
            anchor: logical_rect(response.rect),
            source: response.id,
            secondary: false,
        });
    }
}

fn shared_menu_icon(action: &HostSharedAction, collapsed: bool, direction: Direction) -> ui_kit::Icon<'static> {
    match action {
        HostSharedAction::Focus => "fit",
        HostSharedAction::Close => "close",
        HostSharedAction::Collapse if collapsed => return ui_kit::Icon::Paint(&ui_kit::panel_expand_menu_icon),
        HostSharedAction::Collapse => return ui_kit::Icon::Paint(&ui_kit::panel_collapse_menu_icon),
        HostSharedAction::Move(zone @ (Zone::Left | Zone::Right)) if matches!(direction.edge(*zone), Ok(PhysicalEdge::Left)) => "panel_dock_left",
        HostSharedAction::Move(Zone::Left | Zone::Right) => "panel_dock_right",
        HostSharedAction::Move(Zone::Bottom) => "panel_dock_bottom",
        HostSharedAction::Move(Zone::Floating) => "panel_float",
        HostSharedAction::Move(Zone::Window) | HostSharedAction::Popout => "panel_popout",
        HostSharedAction::Join(_) => "list",
    }
    .into()
}

fn shared_label(
    action: &HostSharedAction,
    instance: &InstanceId,
    runtime: &PanelRuntime,
    metadata: &BTreeMap<InstanceId, PanelPresentation>,
    locale: solidcraft_ui_text::Locale,
) -> String {
    if *action == HostSharedAction::Collapse && runtime.layout().layout().containing(instance).is_ok_and(|section| section.collapsed) {
        return locale.text("ui.panel_host.expand").to_owned();
    }
    if let HostSharedAction::Join(section) = action {
        let layout = runtime.layout();
        let title = layout
            .layout()
            .section(section)
            .ok()
            .and_then(|target| {
                let display = metadata.get(&target.active)?.title.as_str();
                // The Render scene tree and Inspector can both show the title
                // "Scene". Use the registered panel name when titles collide.
                if metadata.iter().any(|(id, other)| id != &target.active && other.title == display) {
                    let panel = layout.layout().instance(&target.active).ok()?;
                    let spec = layout.registry().get(&panel.kind).ok()?;
                    Some(locale.text(&spec.title_key))
                } else {
                    Some(display)
                }
            })
            .unwrap_or_else(|| locale.text("ui.panel_host.tab"));
        return locale.format("ui.panel_host.join_target", &[("title", title)]);
    }
    locale
        .text(match action {
            HostSharedAction::Focus => "ui.panel_host.focus",
            HostSharedAction::Close => "ui.panel_host.close",
            HostSharedAction::Collapse => "ui.panel_host.collapse",
            HostSharedAction::Move(zone @ (Zone::Left | Zone::Right)) => {
                if matches!(runtime.environment().direction.edge(*zone), Ok(PhysicalEdge::Left)) {
                    "ui.panel_host.left"
                } else {
                    "ui.panel_host.right"
                }
            }
            HostSharedAction::Move(Zone::Bottom) => "ui.panel_host.bottom",
            HostSharedAction::Move(Zone::Floating) => "ui.panel_host.floating",
            HostSharedAction::Move(Zone::Window) | HostSharedAction::Popout => "ui.panel_host.popout",
            HostSharedAction::Join(_) => "ui.panel_host.join",
        })
        .to_owned()
}

fn shared_action(
    runtime: &PanelRuntime,
    instance: &InstanceId,
    action: &HostSharedAction,
    native: &HostCapabilities,
    capacity: LogicalRect,
    reservations: &HostIdentityReservations,
) -> PanelResult<LayoutAction> {
    let service = runtime.layout();
    let panel = service.layout().instance(instance)?;
    let section = service.layout().containing(instance)?;
    let spec = service.registry().get(&panel.kind)?;
    let mutations = match action {
        HostSharedAction::Focus => vec![Mutation::Focus { instance: instance.clone() }],
        HostSharedAction::Close => vec![Mutation::Close { instance: instance.clone() }],
        HostSharedAction::Collapse => vec![Mutation::Collapse { section: section.id.clone(), collapsed: !section.collapsed }],
        HostSharedAction::Join(target) => {
            let target_section = service.layout().section(target)?;
            vec![Mutation::Move {
                instance: instance.clone(),
                destination: Destination::JoinTab { section: target.clone(), index: target_section.tabs.len() },
            }]
        }
        HostSharedAction::Move(zone) => {
            let (id, mut index) = new_section_identity(service, instance, reservations)?;
            if zone.docked() && spec.edge_rule == EdgeRule::PinOuter {
                index = service
                    .layout()
                    .sections
                    .iter()
                    .filter(|candidate| !(candidate.id == section.id && section.tabs.len() == 1))
                    .position(|candidate| candidate.location.zone() == *zone)
                    .unwrap_or(index);
            }
            let location = if zone.docked() {
                SectionLocation::Dock { zone: *zone }
            } else if *zone == Zone::Floating {
                SectionLocation::Floating { origin: capacity.origin }
            } else {
                return Err(error(Rule::Capability, "window movement needs native pop-out"));
            };
            let size = section.size;
            vec![
                Mutation::Move { instance: instance.clone(), destination: Destination::NewSection { section: id, index, location, size } },
                Mutation::Focus { instance: instance.clone() },
            ]
        }
        HostSharedAction::Popout => {
            if !spec.features.popout {
                return Err(error(Rule::FeatureDisabled, "panel cannot pop out"));
            }
            if cfg!(target_arch = "wasm32") {
                return shared_action(runtime, instance, &HostSharedAction::Move(Zone::Floating), native, capacity, reservations);
            }
            if !native.native_windows || !service.capabilities().native_windows {
                return Err(error(Rule::Capability, "native panel windows unavailable"));
            }
            let (id, index) = new_section_identity(service, instance, reservations)?;
            let mut number = 0;
            let host = loop {
                let candidate = HostId::new(format!("host.window.{number}"))?;
                if !service
                    .layout()
                    .sections
                    .iter()
                    .any(|section| matches!(&section.location, SectionLocation::Window { host, .. } if *host == candidate))
                {
                    break candidate;
                }
                number += 1;
                if number > MAX_SECTIONS {
                    return Err(error(Rule::CollectionLimit, "native host identity capacity"));
                }
            };
            let zone = if section.location.zone().docked() { section.location.zone() } else { spec.default_zone };
            let zone = if zone.docked() { zone } else { Zone::Left };
            let return_index = service.layout().sections.iter().position(|entry| entry.id == section.id).unwrap_or(0);
            let join = if section.tabs.len() > 1 {
                Some(ReturnTab { section: section.id.clone(), index: section.tabs.iter().position(|entry| entry == instance).unwrap_or(0) })
            } else {
                None
            };
            let location = SectionLocation::Window {
                host,
                origin: capacity.origin,
                return_to: DockReturn {
                    zone,
                    index: return_index,
                    size: section.size,
                    collapsed: section.collapsed,
                    join,
                    section: Some(section.id.clone()),
                    column: section.column.clone(),
                    column_index: service
                        .layout()
                        .columns
                        .iter()
                        .filter(|column| column.zone == zone)
                        .position(|column| Some(&column.id) == section.column.as_ref())
                        .unwrap_or(0),
                    column_width: section
                        .column
                        .as_ref()
                        .and_then(|id| service.layout().columns.iter().find(|column| column.id == *id).map(|column| column.width)),
                    weight: section.weight,
                    preferred_extent: section.preferred_extent,
                    extent_priority: section.extent_priority,
                },
            };
            vec![Mutation::Move {
                instance: instance.clone(),
                destination: Destination::NewSection { section: id, index, location, size: section.size },
            }]
        }
    };
    let action = apply_action(service.inspect().revision, mutations);
    service.prepare(action.clone())?;
    Ok(action)
}

fn new_section_identity(service: &LayoutService, instance: &InstanceId, reservations: &HostIdentityReservations) -> PanelResult<(SectionId, usize)> {
    let source = service.layout().containing(instance)?;
    let index = service.layout().sections.len().saturating_sub(usize::from(source.tabs.len() == 1));
    if source.tabs.len() == 1 {
        return Ok((source.id.clone(), index));
    }
    Ok((next_host_section_id(service.layout(), reservations)?, index))
}
