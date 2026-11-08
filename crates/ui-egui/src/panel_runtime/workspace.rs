//! Shared opt-in workspace renderer. The only mutation path is the retained
//! runtime action sink; resolved geometry and drag previews are observational.
use std::collections::BTreeMap;

use solidcraft_engine::panels::*;

use super::runtime::{RuntimeHostProposal, standalone_header_action};
use super::*;
use crate::ui_kit::{self, HostChrome};

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
            metrics: HostMetrics { inline_max_height: crate::theme::PANEL_INLINE_MAX_HEIGHT, ..HostMetrics::default() },
            geometry: HostGeometryPolicy::default(),
            content_header_height: crate::theme::Tokens::get().control_height,
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
}

#[derive(Clone)]
struct RegisteredOverlay {
    region: InputRegion,
    layer: Option<egui::LayerId>,
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
    pending_escape: bool,
    last_rejection: Option<PanelError>,
    overlays: BTreeMap<InstanceId, Vec<RegisteredOverlay>>,
    observed_focus: Option<InstanceId>,
    focus_request: Option<InstanceId>,
    egui_context: Option<egui::Context>,
    owned_focus: Option<egui::Id>,
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
            pending_escape: false,
            last_rejection: None,
            overlays: BTreeMap::new(),
            observed_focus: None,
            focus_request: None,
            egui_context: None,
            owned_focus: None,
            pending: None,
            last_capacity: None,
            last_domain: None,
            last_chrome: Vec::new(),
        })
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
        self.menu = None;
        self.menu_anchor = None;
        self.overlays.clear();
        self.preview_started = None;
        self.pending_escape = false;
        self.focus_request = None;
        self.last_mount_frame = None;
        self.last_mount_revision = None;
    }
    /// Owner/scope loss and application hiding also release owned keyboard input.
    pub(crate) fn invalidate_context(&mut self) {
        self.invalidate_geometry();
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
                self.menu = None;
                self.menu_anchor = None;
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
        frame.layout =
            resolve_frame_layout(runtime, frame.layout.render_domain, frame.layout.capacity, &frame.layout.chrome_obstacles, &self.options)?;
        let scope = runtime_scope(runtime);
        let window = runtime.environment().window.clone();
        let locale = crate::i18n::Locale::for_language(crate::i18n::language());
        let _resolved_labels = crate::i18n::resolved_labels();
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
                            .is_ok_and(|spec| spec.features.collapsible)
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
        let mut renderer = WorkspaceContentHost {
            ui,
            layout: &frame.layout,
            move_source: self.interaction.move_source(),
            controls: &mut frame.controls,
            metadata: &metadata,
            header_icons: &header_icons,
            scope: &scope,
            window: &window,
            owned_focus: &mut self.owned_focus,
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
        // An Area's first sizing pass is invisible and non-interactable. Keep
        // the intended resolved geometry, but never publish it as actual paint
        // or an input/occlusion shield, even if no further egui pass is allowed.
        let mut unpainted_instances = Vec::new();
        for (layer, bounds) in &unpainted {
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
                    ui.memory_mut(|memory| memory.request_focus(control.id));
                    self.focus_request = None;
                }
            }
        }
        self.paint_menu(ui, runtime, native.capabilities(), &metadata, &owners, &mut frame, &mut commands, allow_actions, locale);
        self.pending = Some(PendingHostCommands { scope, window, frame_number, allow_actions, commands, pointer_events, mount: mount.clone() });
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
        let PendingHostCommands { scope, frame_number, allow_actions, mut commands, mut pointer_events, mount, .. } = pending;
        let Some(mut frame) = self.frame.take() else {
            ui.ctx().request_repaint();
            return Ok(());
        };
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
            self.menu = None;
            self.menu_anchor = None;
            self.pending_escape = false;
            self.preview_started = None;
            self.focus_request = None;
            self.last_input_frame = Some(frame_number);
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
            && preview.rejection.is_none()
            && preview.projected.is_some()
        {
            let now = ui.input(|input| input.time);
            let started = *self.preview_started.get_or_insert(now);
            let duration = if runtime.environment().motion == Motion::Reduced || ui.style().animation_time <= 0.0 {
                0.0
            } else {
                ui.style().animation_time.clamp(0.12, 0.16)
            };
            let progress = if duration <= 0.0 { 1.0 } else { ((now - started) as f32 / duration).clamp(0.0, 1.0) };
            let opacity = 1.0 - (1.0 - progress).powi(3);
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
                            self.menu = None;
                            self.menu_anchor = None;
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
                            self.menu = None;
                            self.menu_anchor = None;
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
                        self.menu = Some(menu);
                        self.menu_anchor = Some(anchor);
                        // This pass still reports only the menu actually painted.
                        ui.ctx().request_repaint();
                    }
                }
                HostCommand::Layout(_) | HostCommand::WidgetLayout(_, _) | HostCommand::Prepared(_) => {}
            }
        }
        if painted_move_source.as_ref() != self.interaction.move_source() {
            ui.ctx().request_repaint();
        }
        if let (Some(instance), Some(at)) = (self.interaction.move_source(), self.interaction.move_position())
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
            let label = crate::i18n::tr(if horizontal { "ui.panel_host.scroll_horizontal" } else { "ui.panel_host.scroll_vertical" });
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
        _locale: crate::i18n::Locale,
    ) {
        for control in &frame.controls {
            let HostControlTarget::Pointer(target @ HostPointerTarget::Sash { .. }) = &control.target else {
                continue;
            };
            let response = ui_kit::panel_host_sash(ui, control.id, egui_rect(control.bounds), &control.accessible_name);
            if allow_actions && response.double_clicked() {
                pointer_events.push((response.id, HostPointerEvent::DoubleClick { target: target.clone() }));
            }
            if allow_actions && response.has_focus() && !response.dragged() {
                let horizontal = control.bounds.size.height > control.bounds.size.width;
                let delta = ui.input_mut(|input| {
                    let back = if horizontal { egui::Key::ArrowLeft } else { egui::Key::ArrowUp };
                    let forward = if horizontal { egui::Key::ArrowRight } else { egui::Key::ArrowDown };
                    u8::from(input.consume_key(egui::Modifiers::NONE, forward)) as f32
                        - u8::from(input.consume_key(egui::Modifiers::NONE, back)) as f32
                });
                if delta != 0.0 {
                    let at = center(control.bounds);
                    let step = crate::theme::Tokens::get().space_8.max(HOST_DRAG_THRESHOLD);
                    let end = Point { x: at.x + if horizontal { delta * step } else { 0.0 }, y: at.y + if horizontal { 0.0 } else { delta * step } };
                    for event in [
                        HostPointerEvent::Press { at, target: Some(target.clone()) },
                        HostPointerEvent::Move { at: end },
                        HostPointerEvent::Release { at: end },
                    ] {
                        pointer_events.push((response.id, event));
                    }
                }
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
                self.menu = None;
                self.menu_anchor = None;
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
                        self.menu = None;
                        self.menu_anchor = None;
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
                                self.menu = None;
                                self.menu_anchor = None;
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
    locale: crate::i18n::Locale,
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

fn rule_key(rule: Rule) -> &'static str {
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
    layout: &'a HostLayout,
    move_source: Option<&'a InstanceId>,
    controls: &'a mut Vec<HostFrameControl>,
    metadata: &'a BTreeMap<InstanceId, PanelPresentation>,
    header_icons: &'a BTreeMap<(InstanceId, ControlId), String>,
    scope: &'a LocalScope,
    window: &'a HostId,
    owned_focus: &'a mut Option<egui::Id>,
    commands: &'a mut Vec<HostCommand>,
    allow_actions: bool,
    unpainted: &'a mut Vec<(egui::LayerId, LogicalRect)>,
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
        let fallback;
        let style = if let Some(style) = self.layout.geometry_policy.surfaces.as_ref() {
            style
        } else {
            fallback = HostSurfaceStyle::capture(self.ui.ctx(), false, self.layout.geometry_policy.requested_opacity, None);
            &fallback
        };
        let appearance = style.appearance(section.surface, section.peek || section.zone == Zone::Floating);
        ui_kit::panel_host_surface(
            self.ui,
            section_surface_id(self.scope, self.window, &section.section),
            egui_rect(section.outer),
            section.peek || section.zone == Zone::Floating,
            appearance.fill,
            matches!(appearance.contrast, ContrastOutcome::RetainReadableFallback { .. }).then(|| style.palette(appearance)),
            |ui, content_clip| {
                if (section.peek || section.zone == Zone::Floating) && (!ui.is_visible() || ui.is_sizing_pass()) {
                    self.unpainted.push((ui.layer_id(), section.outer));
                    return;
                }
                // A surface response provides semantic focus and owns only the actual
                // painted card; child controls retain their own accessible responses.
                let response = ui.interact(egui_rect(section.outer), surface_id, egui::Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), &presentation.title));
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
                    let data = pointer_instance(target).and_then(|instance| self.metadata.get(instance));
                    let label = &control.accessible_name;
                    let response = if matches!(target, HostPointerTarget::Collapse { .. }) {
                        ui_kit::panel_host_disclosure(ui, control.id, egui_rect(control.bounds), label, control.enabled, section.collapsed)
                    } else {
                        ui_kit::panel_host_control_label_opacity(
                            ui,
                            control.id,
                            egui_rect(control.bounds),
                            HostChrome {
                                label,
                                icon: if matches!(target, HostPointerTarget::Menu { .. }) {
                                    Some("dots")
                                } else if matches!(target, HostPointerTarget::TabOverflow { .. }) {
                                    Some("chevron")
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
                                grip_only: matches!(target, HostPointerTarget::Header { .. }) && section.tabs.len() + section.hidden_tabs.len() > 1,
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
                    let clicked = response.clicked();
                    let secondary = response.secondary_clicked();
                    if let Some(status) = data.and_then(|value| value.status.as_deref()) {
                        response.clone().on_hover_text(status);
                    }
                    if let HostPointerTarget::HeaderAction { action, .. } = target
                        && let Some(reason) = data
                            .and_then(|presentation| presentation.actions.iter().find(|binding| binding.id == *action))
                            .and_then(|binding| binding.disabled_reason.as_deref())
                    {
                        response.clone().on_disabled_hover_text(reason);
                    }
                    if self.allow_actions && (clicked || secondary) {
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
                let t = crate::theme::Tokens::get();
                // The title and its controls share the card background. One
                // kit separator spans the header, rather than per-control tiles.
                if !section.collapsed && !section.inline {
                    let header = egui_rect(section.header);
                    t.secondary_hline(ui.painter(), header.x_range(), header.bottom(), t.dialog_rule);
                }
                // Content scrolls only where it chooses. Each slot is an isolated
                // bounded child, so malicious/minimum-size content cannot enlarge
                // the outer host or cover the shared menu and tab strip. Keep
                // content fills inside the frame's rounded corners as well.
                let mut content_ui = ui.new_child(egui::UiBuilder::new().id_salt("content_clip").max_rect(egui_rect(section.outer)));
                content_ui.set_clip_rect(content_clip);
                let content_ui = &mut content_ui;
                if !section.collapsed && section.inline {
                    if let Some(body) = section.inline_content {
                        render_slot(content_ui, ContentSlot::Inline, body, context, content, services, &mut failure);
                    }
                } else if !section.collapsed {
                    if let Some(header) = content_header_bounds(section) {
                        render_slot(content_ui, ContentSlot::Header, header, context, content, services, &mut failure);
                    }
                    if let Some(body) = section.body {
                        render_slot(content_ui, ContentSlot::Body, body, context, content, services, &mut failure);
                    }
                    if let Some(footer) = section.footer {
                        let has_footer_actions = presentation.actions.iter().any(|action| !section.actions.iter().any(|(id, _)| *id == action.id));
                        let row_height = if has_footer_actions { self.layout.metrics.control_extent.min(footer.size.height) } else { 0.0 };
                        if row_height > 0.0 && egui_rect(footer).intersect(content_ui.clip_rect()).is_positive() {
                            let inset = crate::theme::Tokens::get().space_4.min(footer.size.width / 2.0);
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
                                        let visible = response.rect.intersect(ui.clip_rect()).intersect(egui_rect(row));
                                        if visible.is_positive() {
                                            self.controls.push(HostFrameControl {
                                                id: response.id,
                                                target,
                                                bounds: logical_rect(visible),
                                                role: InputRole::Action,
                                                accessible_name: action.label.clone(),
                                                enabled: action.enabled && response.enabled(),
                                            });
                                        }
                                        if self.allow_actions && response.clicked() {
                                            self.commands.push(HostCommand::Content(context.instance.clone(), action.id.clone(), response.id));
                                        }
                                        if let Some(reason) = &action.disabled_reason {
                                            response.on_disabled_hover_text(reason);
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
                            context,
                            content,
                            services,
                            &mut failure,
                        );
                    }
                }
                // Resize must be above content on the card's actual UI/layer. Its
                // published bounds and scoped ID remain the layout authority's.
                for control in self.controls.iter() {
                    if matches!(&control.target, HostControlTarget::Pointer(HostPointerTarget::Resize { section: id, .. }) if *id == section.section)
                    {
                        ui_kit::panel_host_sash(ui, control.id, egui_rect(control.bounds), &control.accessible_name);
                    }
                }
                if let Some(focused) = ui.memory(|memory| memory.focused())
                    && ui.ctx().read_response(focused).is_some_and(|response| {
                        response.layer_id == ui.layer_id()
                            && response.interact_rect.is_positive()
                            && egui_rect(section.outer).contains_rect(response.interact_rect)
                    })
                {
                    *self.owned_focus = Some(focused);
                }
            },
        );
        if let Some(reason) = failure { Err(reason) } else { Ok(()) }
    }
}

fn render_slot(
    ui: &mut egui::Ui,
    slot: ContentSlot,
    bounds: LogicalRect,
    context: &PanelContext,
    content: &mut dyn PanelController,
    services: &mut ContentServices<'_>,
    failure: &mut Option<PanelError>,
) {
    if failure.is_some() || bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return;
    }
    let bounds = egui_rect(bounds);
    let t = crate::theme::Tokens::get();
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
    let result = if slot == ContentSlot::Inline {
        content.render_inline(&mut child, context, services)
    } else {
        content.render(slot, &mut child, context, services)
    };
    if let Err(reason) = result {
        *failure = Some(reason);
    }
}

fn content_header_bounds(section: &HostSection) -> Option<LogicalRect> {
    let body = section.body?;
    let y = section.header.origin.y + section.header.size.height;
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
    let thickness = crate::theme::Tokens::get().space_8.min(capacity.width()).min(capacity.height());
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
        locale: crate::i18n::Locale,
    ) {
        let (Some(menu), Some(anchor)) = (self.menu.clone(), self.menu_anchor) else {
            return;
        };
        let Some(bounds) = menu_geometry(frame.layout.capacity, anchor) else {
            self.menu = None;
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
        ui_kit::panel_host_popup(ui, id, bounds, |ui| {
            if !ui.is_visible() || ui.is_sizing_pass() {
                return;
            }
            frame.menu_bounds = Some(logical_rect(bounds));
            egui::ScrollArea::vertical().id_salt(("items", scroll_id)).max_height(ui.available_height()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if let Some(reason) = &self.last_rejection {
                    ui.colored_label(crate::theme::Tokens::get().form_error, locale.text(rule_key(reason.rule)));
                }
                match &menu {
                    HostMenu::Panel(instance) => {
                        if let Some(presentation) = metadata.get(instance) {
                            ui.label(&presentation.title);
                            if let Some(status) = &presentation.status {
                                ui.label(status);
                            }
                        }
                        let mut actions = vec![HostSharedAction::Focus, HostSharedAction::Collapse];
                        actions.extend([Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating].into_iter().map(HostSharedAction::Move));
                        actions.extend(
                            runtime
                                .layout()
                                .layout()
                                .sections
                                .iter()
                                .filter(|section| !section.tabs.contains(instance))
                                .map(|section| HostSharedAction::Join(section.id.clone())),
                        );
                        actions.extend([HostSharedAction::Popout, HostSharedAction::Close]);
                        for shared in actions {
                            let target = HostControlTarget::Shared { instance: instance.clone(), action: shared.clone() };
                            let label = shared_label(&shared, instance, runtime, metadata, locale);
                            let action = shared_action(
                                runtime,
                                instance,
                                &shared,
                                &native,
                                frame.layout.capacity,
                                &frame.layout.geometry_policy.identity_reservations,
                            )
                            .and_then(|action| {
                                prepare_host_layout(runtime.layout(), &runtime_scope(runtime), &frame.layout, action.clone())?;
                                Ok(action)
                            });
                            let reason = action.as_ref().err().map(|reason| locale.text(rule_key(reason.rule)).to_owned());
                            let response = menu_row(ui, control_id(runtime, owners, &target), &label, action.is_ok());
                            record_control(frame, &response, target, &label, action.is_ok(), bounds.intersect(ui.clip_rect()));
                            if allow_actions
                                && response.clicked()
                                && let Ok(action) = action
                            {
                                commands.push(HostCommand::WidgetLayout(action, response.id));
                            }
                            if let Some(reason) = reason {
                                response.on_disabled_hover_text(reason);
                            }
                        }
                        if let Some(presentation) = metadata.get(instance) {
                            for action in &presentation.actions {
                                let target = HostControlTarget::Content { instance: instance.clone(), action: action.id.clone() };
                                let response = menu_row(ui, control_id(runtime, owners, &target), &action.label, action.enabled);
                                record_control(frame, &response, target, &action.label, action.enabled, bounds.intersect(ui.clip_rect()));
                                if allow_actions && response.clicked() {
                                    let registered = runtime
                                        .layout()
                                        .layout()
                                        .instance(instance)
                                        .and_then(|panel| runtime.layout().registry().get(&panel.kind))
                                        .is_ok_and(|spec| spec.header_actions.iter().any(|entry| entry.id == action.id));
                                    if registered {
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
                                    response.on_disabled_hover_text(reason);
                                }
                            }
                        }
                        // Collapsed and inline groups have one total header.
                        // Retain access to inactive identities through the same
                        // bounded shared menu, after the selected panel's actions.
                        if let Ok(section) = runtime.layout().layout().containing(instance) {
                            for tab in section.tabs.iter().filter(|tab| *tab != instance) {
                                menu_instance(ui, runtime, owners, metadata, frame, commands, tab, bounds, allow_actions, locale);
                            }
                        }
                    }
                    HostMenu::Tabs(section) => {
                        if let Ok(section) = runtime.layout().layout().section(section) {
                            for instance in &section.tabs {
                                menu_instance(ui, runtime, owners, metadata, frame, commands, instance, bounds, allow_actions, locale);
                            }
                        }
                    }
                }
            });
        });
    }
}

fn menu_geometry(capacity: LogicalRect, anchor: LogicalRect) -> Option<egui::Rect> {
    let capacity = egui_rect(capacity);
    if !capacity.is_positive() || !capacity.is_finite() {
        return None;
    }
    let t = crate::theme::Tokens::get();
    if capacity.width() <= t.space_6 * 2.0 || capacity.height() <= t.space_6 * 2.0 {
        return None;
    }
    let width = t.form_card_width.min(capacity.width());
    let height = (t.control_height * 12.0 + t.space_12).min(capacity.height());
    let left = anchor.origin.x.min(capacity.right() - width).max(capacity.left());
    let top = (anchor.origin.y + anchor.size.height).min(capacity.bottom() - height).max(capacity.top());
    Some(egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height)))
}

fn menu_row(ui: &mut egui::Ui, id: egui::Id, label: &str, enabled: bool) -> egui::Response {
    ui.push_id(id.with("menu"), |ui| ui.add_enabled_ui(enabled, |ui| ui_kit::button(ui, label, ui_kit::Button::Small)).inner)
        .inner
        .on_hover_text(label)
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
    instance: &InstanceId,
    clip: egui::Rect,
    allow_actions: bool,
    locale: crate::i18n::Locale,
) {
    let label = metadata.get(instance).map_or_else(|| locale.text("ui.panel_host.tab").to_owned(), |value| value.title.clone());
    ui.label(&label);
    if metadata.get(instance).is_some_and(|presentation| presentation.attention) {
        ui.label(locale.text("ui.panel_host.attention"));
    }
    ui.horizontal(|ui| {
        let target = HostControlTarget::Shared { instance: instance.clone(), action: HostSharedAction::Focus };
        let action = apply_action(runtime.layout().inspect().revision, vec![Mutation::Focus { instance: instance.clone() }]);
        let check = runtime.layout().prepare(action.clone());
        let response = menu_row(ui, control_id(runtime, owners, &target), locale.text("ui.panel_host.focus"), check.is_ok());
        let accessible = locale.format("ui.panel_host.focus_target", &[("title", &label)]);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, check.is_ok(), &accessible));
        record_control(frame, &response, target, &accessible, check.is_ok(), clip.intersect(ui.clip_rect()));
        if allow_actions && response.clicked() {
            commands.push(HostCommand::WidgetLayout(action, response.id));
        }
        if let Err(reason) = check {
            response.on_disabled_hover_text(locale.text(rule_key(reason.rule)));
        }
        let target = HostControlTarget::PanelMenu { instance: instance.clone() };
        let response = menu_row(ui, control_id(runtime, owners, &target), locale.text("ui.panel_host.menu"), true);
        record_control(frame, &response, target, locale.text("ui.panel_host.menu"), true, clip.intersect(ui.clip_rect()));
        if allow_actions && response.clicked() {
            commands.push(HostCommand::OpenMenu {
                menu: HostMenu::Panel(instance.clone()),
                anchor: logical_rect(response.rect),
                source: response.id,
                secondary: false,
            });
        }
    });
}

fn shared_label(
    action: &HostSharedAction,
    instance: &InstanceId,
    runtime: &PanelRuntime,
    metadata: &BTreeMap<InstanceId, PanelPresentation>,
    locale: crate::i18n::Locale,
) -> String {
    if *action == HostSharedAction::Collapse && runtime.layout().layout().containing(instance).is_ok_and(|section| section.collapsed) {
        return locale.text("ui.panel_host.expand").to_owned();
    }
    if let HostSharedAction::Join(section) = action {
        let title = runtime
            .layout()
            .layout()
            .section(section)
            .ok()
            .and_then(|section| metadata.get(&section.active))
            .map(|presentation| presentation.title.as_str())
            .unwrap_or_else(|| locale.text("ui.panel_host.tab"));
        return locale.format("ui.panel_host.join_target", &[("title", title)]);
    }
    locale
        .text(match action {
            HostSharedAction::Focus => "ui.panel_host.focus",
            HostSharedAction::Close => "ui.panel_host.close",
            HostSharedAction::Collapse => "ui.panel_host.collapse",
            HostSharedAction::Move(Zone::Left) => "ui.panel_host.left",
            HostSharedAction::Move(Zone::Right) => "ui.panel_host.right",
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
