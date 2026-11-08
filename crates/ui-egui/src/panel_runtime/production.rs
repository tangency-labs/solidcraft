//! Bind the shared host to existing application content, without owning that content.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::panels::*;

use super::*;

const VIEWPORT: &str = "solidcraft.panels";
const MAX_ACTIONS: usize = 16;
const WORKSPACES: [(&str, &str, &str); 5] = [
    ("solidcraft.classic", "Classic", "classic"),
    ("solidcraft.modelling", "Modelling", "modelling"),
    ("solidcraft.sketch", "Sketch", "sketch"),
    ("solidcraft.render", "Render", "render"),
    ("solidcraft.review", "Review", "review"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Product {
    Browser,
    History,
    Assistant,
    RenderBrowser,
}
impl Product {
    const ALL: [Self; 4] = [Self::Browser, Self::History, Self::Assistant, Self::RenderBrowser];

    fn index(self) -> usize {
        match self {
            Self::Browser => 0,
            Self::History => 1,
            Self::Assistant => 2,
            Self::RenderBrowser => 3,
        }
    }
    fn visible(self, app: &crate::SolveApp) -> bool {
        match self {
            Self::Browser | Self::RenderBrowser => app.ui.show_browser,
            Self::History => app.ui.show_timeline,
            Self::Assistant => app.agent_chat.open,
        }
    }
    fn set_visible(self, app: &mut crate::SolveApp, open: bool) {
        match self {
            Self::Browser | Self::RenderBrowser => app.ui.show_browser = open,
            Self::History => app.ui.show_timeline = open,
            Self::Assistant => app.agent_chat.open = open,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::History => "history",
            Self::Assistant => "assistant",
            Self::RenderBrowser => "render-browser",
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::Browser => "solidcraft.browser",
            Self::History => "solidcraft.history",
            Self::Assistant => "solidcraft.assistant",
            Self::RenderBrowser => "solidcraft.render_browser",
        }
    }
    fn title_key(self) -> &'static str {
        match self {
            Self::Browser => "ui.browser_title",
            Self::History => "ui.history_title",
            Self::Assistant => "ui.assistant",
            Self::RenderBrowser => "render.page_scene",
        }
    }
    fn from_instance(instance: &InstanceId) -> PanelResult<Self> {
        Self::ALL.into_iter().find(|product| product.id() == instance.0).ok_or_else(|| error(Rule::MissingTarget, "unknown application panel"))
    }
    fn available(self, app: &crate::SolveApp) -> bool {
        let render = crate::render_workspace::active(app);
        match self {
            Self::Browser => !render && !crate::render_ui::focused(app),
            Self::History => !render && !app.preferences.history_in_browser,
            Self::Assistant => crate::agent_chat::shown(app) && !app.preferences.agent_chat_popped,
            Self::RenderBrowser => render && !crate::render_ui::focused(app),
        }
    }
    fn header_actions(self) -> Vec<HeaderActionSpec> {
        let entries: Vec<(&str, &str, &str)> = match self {
            Self::History => vec![
                ("first", "ui.history_first", "timeline_first"),
                ("back", "ui.history_back", "timeline_back"),
                ("play", "cmd.timeline_play", "timeline_play"),
                ("forward", "ui.history_forward", "timeline_forward"),
                ("last", "ui.history_last", "timeline_last"),
            ],
            Self::Assistant => vec![("new-chat", "ui.agent_chat_new", "new")],
            Self::RenderBrowser => vec![
                ("by-object", "render.tree_by_object", "list"),
                ("by-look", "render.tree_by_look", "list"),
                ("search", "render.tree_search", "search"),
            ],
            Self::Browser => Vec::new(),
        };
        entries.into_iter().map(|(id, key, icon)| HeaderActionSpec { id: ControlId(id.into()), label_key: key.into(), icon: icon.into() }).collect()
    }
    fn presentation(self, app: &crate::SolveApp, play_button: bool) -> PanelPresentation {
        let mut presentation = self.empty_presentation();
        presentation.actions = match self {
            Self::History => crate::ui_kit::HistoryTransport::shown(play_button)
                .iter()
                .map(|control| {
                    let availability = crate::timeline::transport_availability(app, *control);
                    RuntimeAction {
                        id: ControlId(control.id().into()),
                        label: control.label(app.timeline_playback.is_some()).into(),
                        enabled: availability.is_ok(),
                        disabled_reason: availability.err(),
                        pressed: (*control == crate::ui_kit::HistoryTransport::Play).then_some(app.timeline_playback.is_some()),
                    }
                })
                .collect(),
            Self::Assistant => vec![RuntimeAction {
                id: ControlId("new-chat".into()),
                label: crate::i18n::tr("ui.agent_chat_new").into(),
                enabled: true,
                disabled_reason: None,
                pressed: None,
            }],
            Self::RenderBrowser => [
                ("by-object", "render.tree_by_object", !app.render_workspace.view.by_look),
                ("by-look", "render.tree_by_look", app.render_workspace.view.by_look),
                ("search", "render.tree_search", app.render_workspace.view.search_objects),
            ]
            .into_iter()
            .map(|(id, key, pressed)| RuntimeAction {
                id: ControlId(id.into()),
                label: crate::i18n::tr(key).into(),
                enabled: true,
                disabled_reason: None,
                pressed: Some(pressed),
            })
            .collect(),
            Self::Browser => Vec::new(),
        };
        presentation
    }
    fn empty_presentation(self) -> PanelPresentation {
        PanelPresentation {
            title: crate::i18n::tr(self.title_key()).into(),
            icon: "list".into(),
            status: None,
            attention: false,
            actions: Vec::new(),
        }
    }
}

struct PanelSnapshot {
    product: Product,
    available: bool,
    presentation: PanelPresentation,
    extent: Option<ContentExtent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ContentExtent {
    width: f32,
    height: f32,
    width_independent: bool,
    flow: Flow,
    composer: Option<f32>,
}
struct ProductionSnapshot {
    document: DocumentId,
    play_button: bool,
    visible: [bool; 4],
    visibility: [Option<VisibilityIntent>; 4],
    panels: [PanelSnapshot; 4],
    actions: VecDeque<QueuedAction>,
}
struct VisibilityIntent {
    document: DocumentId,
    open: bool,
}
struct QueuedAction {
    document: DocumentId,
    product: Product,
    action: ControlId,
}

/// Only bounded presentation metadata and explicit pending actions are shared.
/// Trees, drafts, chat services and selection remain on their existing owners.
#[derive(Clone)]
pub(super) struct ProductionState(Rc<RefCell<ProductionSnapshot>>);
impl ProductionState {
    fn new(app: &crate::SolveApp) -> PanelResult<Self> {
        Ok(Self(Rc::new(RefCell::new(ProductionSnapshot {
            document: DocumentId::new(app.session.document_id.clone())?,
            play_button: app.preferences.history_play_button,
            visible: Product::ALL.map(|product| product.visible(app)),
            visibility: [None, None, None, None],
            panels: Product::ALL.map(|product| PanelSnapshot {
                product,
                available: product.available(app),
                presentation: product.presentation(app, app.preferences.history_play_button),
                extent: None,
            }),
            actions: VecDeque::new(),
        }))))
    }
}

pub(crate) fn enable_production(app: &mut crate::SolveApp) -> PanelResult<()> {
    if app.panel_runtime.is_some() {
        return Err(error(Rule::Lifecycle, "application panel runtime is already enabled"));
    }
    let state = ProductionState::new(app)?;
    let registry = registry(app)?;
    let workspace = WorkspaceId::new(WORKSPACES[0].0)?;
    let layout = default_layout(app, workspace.clone(), &registry)?;
    let mut runtime = PanelRuntime::new(
        LayoutService::new(registry, layout, HostCapabilities { persistence: true, ..Default::default() })?,
        RuntimeEnvironment {
            workspace: workspace.clone(),
            document: Some(DocumentId::new(app.session.document_id.clone())?),
            window: HostId::new(VIEWPORT)?,
            viewport: ViewportId::new(VIEWPORT)?,
            direction: crate::i18n::panel_direction(&app.preferences.language),
            motion: Motion::Animated,
        },
    )?;
    for product in Product::ALL {
        let state = state.clone();
        runtime.register(
            PanelKind::new(product.kind())?,
            ControllerBinding {
                ownership: Ownership::Document,
                create: Box::new(move |_instance, owner| {
                    let ContentOwner::Document(document) = owner else {
                        return Err(error(Rule::Placement, "application panel requires its document owner"));
                    };
                    Ok(Box::new(ProductionController { product, document: document.clone(), state: state.clone() }))
                }),
            },
        )?;
    }
    let services = ProductionServices {
        models: ProductionModels,
        commands: ProductionCommands,
        local: ProductionLocal { workspace, document: DocumentId::new(app.session.document_id.clone())?, keyboard: None },
    };
    let mut integration = AppPanelRuntime::new(runtime, Box::new(services))?;
    integration.production = Some(state);
    integration.tick(&app.session)?;
    let mut options = HostOptions::default();
    options.content_header_height = 0.0;
    options.metrics.footer_height = 0.0;
    integration.enable_host(options)?;
    let mut catalogue = commands::InMemoryCatalogue::default();
    for (id, name, suffix) in WORKSPACES {
        let workspace = WorkspaceId::new(id)?;
        let layout = default_layout(app, workspace.clone(), integration.runtime().layout().registry())?;
        catalogue.register_builtin(name, &LayoutEnvelope::new(layout), integration.runtime().layout().registry())?;
        catalogue.register_workspace(commands::WorkspaceDescriptor {
            id: workspace,
            title_key: format!("ui.panel_layout.workspace.{suffix}"),
            default_preset: name.into(),
        })?;
    }
    let repaired = super::preferences::configure_preferences(&mut integration, &app.panel_layout_preferences, &mut catalogue)?;
    app.panel_runtime = Some(integration);
    if repaired {
        super::preferences::report_notice(app);
    }
    Ok(())
}

pub(crate) fn refresh_production(app: &crate::SolveApp) -> PanelResult<()> {
    let Some(state) = app.panel_runtime.as_ref().and_then(|integration| integration.production.as_ref()) else { return Ok(()) };
    let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel snapshot is borrowed"))?;
    let document = DocumentId::new(app.session.document_id.clone())?;
    if snapshot.document != document {
        for panel in &mut snapshot.panels {
            panel.extent = None;
        }
    }
    snapshot.document = document;
    // Keep an explicit view-toggle edge pending while its content context is
    // unavailable. Ordinary workspace changes do not create a new toggle edge.
    for product in Product::ALL {
        let visible = product.visible(app);
        if app.panel_runtime.as_ref().is_some_and(|integration| {
            integration
                .runtime()
                .layout()
                .layout()
                .instances
                .iter()
                .any(|instance| instance.id.0 == product.id() && instance.requested_open == visible)
        }) {
            snapshot.visible[product.index()] = visible;
        }
    }
    let play_button = snapshot.play_button;
    for panel in &mut snapshot.panels {
        panel.available = panel.product.available(app);
        panel.presentation = panel.product.presentation(app, play_button);
    }
    Ok(())
}

pub(crate) fn drain_production_actions(app: &mut crate::SolveApp) -> PanelResult<()> {
    let Some(state) = app.panel_runtime.as_ref().and_then(|integration| integration.production.clone()) else { return Ok(()) };
    let (actions, visibility, play_button) = {
        let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel actions are borrowed"))?;
        (std::mem::take(&mut snapshot.actions), std::mem::replace(&mut snapshot.visibility, [None, None, None, None]), snapshot.play_button)
    };
    for (product, intent) in Product::ALL.into_iter().zip(visibility) {
        if let Some(intent) = intent
            && intent.document.0 == app.session.document_id
        {
            product.set_visible(app, intent.open);
        }
    }
    for action in actions {
        if action.document.0 != app.session.document_id || !action.product.available(app) {
            continue;
        }
        let presentation = action.product.presentation(app, play_button);
        if !presentation.actions.iter().any(|binding| binding.id == action.action && binding.enabled) {
            continue;
        }
        match action.product {
            Product::History => {
                if let Some(control) = crate::ui_kit::HistoryTransport::WITH_PLAY.into_iter().find(|control| control.id() == action.action.0) {
                    crate::timeline::run_transport(app, control);
                }
            }
            Product::Assistant => crate::agent_chat::request_new_chat(app),
            Product::RenderBrowser => {
                let params = match action.action.0.as_str() {
                    "by-object" => json!({"by_look":false}),
                    "by-look" => json!({"by_look":true}),
                    "search" => json!({"search":!app.render_workspace.view.search_objects}),
                    _ => continue,
                };
                app.run("view.render_browser", params).map_err(|reason| error(Rule::Availability, reason))?;
            }
            Product::Browser => {}
        }
    }
    Ok(())
}

/// Existing view toggles update remembered placement only when their raw flag
/// changes. Context changes and hidden bodies never reopen closed panels.
pub(crate) fn sync_production_visibility(app: &mut crate::SolveApp) -> PanelResult<()> {
    let Some(integration) = app.panel_runtime.as_ref() else { return Ok(()) };
    let Some(state) = integration.production.as_ref() else { return Ok(()) };
    let snapshot = state.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "application panel visibility is borrowed"))?;
    if snapshot.document.0 != app.session.document_id {
        return Ok(());
    }
    let layout = integration.runtime().layout().layout();
    let mut mutations = Vec::new();
    for product in Product::ALL {
        let visible = product.visible(app);
        if snapshot.visible.get(product.index()).copied() != Some(visible) {
            let instance = layout.instance(&InstanceId::new(product.id())?)?;
            if instance.requested_open != visible && (!visible || product.available(app)) {
                mutations.push(if visible {
                    Mutation::Open { kind: instance.kind.clone(), instance: instance.id.clone(), destination: None }
                } else {
                    Mutation::Close { instance: instance.id.clone() }
                });
            }
        }
    }
    let revision = integration.runtime().layout().inspect().revision;
    drop(snapshot);
    if !mutations.is_empty() {
        refresh_production(app)?;
        app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: Some(revision), mutations })?;
    }
    Ok(())
}

fn registry(app: &crate::SolveApp) -> PanelResult<PanelRegistry> {
    let mut registry = PanelRegistry::default();
    let tokens = crate::theme::Tokens::get();
    // The existing strip has a 28-point chip and six-point inset on each side;
    // the shared content slot adds its own symmetric two-point inset.
    let bottom_height = tokens.form_header_height.max(tokens.space_24 + tokens.space_4 + 2.0 * tokens.space_6 + 2.0 * tokens.space_2);
    for product in Product::ALL {
        let history = product == Product::History;
        let assistant = product == Product::Assistant;
        let mut allowed_zones = [Zone::Left, Zone::Right, Zone::Floating].into_iter().collect::<std::collections::BTreeSet<_>>();
        if history {
            allowed_zones.insert(Zone::Bottom);
        }
        let mut header_actions = product.header_actions();
        if history && !app.preferences.history_play_button {
            header_actions.retain(|action| action.id.0 != "play");
        }
        registry.register(PanelSpec {
            kind: PanelKind::new(product.kind())?,
            title_key: product.title_key().into(),
            icon: "list".into(),
            allowed_zones,
            default_zone: if history { Zone::Bottom } else { Zone::Left },
            min_size: Size { width: 240.0, height: if assistant { 120.0 } else { crate::theme::Tokens::get().control_height } },
            default_size: Size { width: crate::prefs::DEFAULT_BROWSER_WIDTH, height: if history { bottom_height } else { 320.0 } },
            max_size: Size { width: MAX_LOGICAL, height: MAX_LOGICAL },
            edge_rule: if assistant { EdgeRule::PinOuter } else { EdgeRule::None },
            surface: PanelSurface::FollowZone,
            features: PanelFeatures { popout: false, hot_closable: assistant, ..Default::default() },
            side_height_policy: if assistant { SideHeightPolicy::Fill } else { SideHeightPolicy::Content },
            fixed_bottom_height: history.then_some(bottom_height),
            header_actions,
            text_heavy: assistant,
        })?;
    }
    Ok(registry)
}

fn default_layout(app: &crate::SolveApp, workspace: WorkspaceId, registry: &PanelRegistry) -> PanelResult<Layout> {
    let mut layout = Layout::empty(workspace);
    let vertical = app.ui.show_timeline && app.preferences.timeline_left;
    let stacked = vertical && app.preferences.timeline_under_browser;
    let share = if app.preferences.browser_share.is_finite() { app.preferences.browser_share.clamp(0.05, 0.95) } else { 0.5 };
    for product in Product::ALL {
        let id = InstanceId::new(product.id())?;
        let requested_open = product.visible(app);
        let kind = PanelKind::new(product.kind())?;
        let spec = registry.get(&kind)?;
        let zone = match product {
            Product::History if !app.preferences.timeline_left => Zone::Bottom,
            Product::Assistant if app.preferences.agent_chat_side == "right" => Zone::Right,
            Product::RenderBrowser if app.preferences.render_browser_side == "right" => Zone::Right,
            _ => Zone::Left,
        };
        let width = match product {
            Product::Browser => app.preferences.browser_width,
            Product::History if stacked => app.preferences.browser_width,
            Product::History => app.preferences.timeline_left_width,
            Product::Assistant => app.preferences.agent_chat_width,
            Product::RenderBrowser => app.preferences.render_browser_width,
        };
        let width = if width.is_finite() { width.clamp(spec.min_size.width, 1200.0) } else { spec.default_size.width };
        let width = if product == Product::History && stacked {
            layout.columns.iter().find(|column| column.id.0 == "panel-column-browser").map_or(width, |column| column.width)
        } else {
            width
        };
        let size = Size { width, height: spec.default_size.height };
        let section = SectionId::new(format!("panel-{}", product.id()))?;
        let location = SectionLocation::Dock { zone };
        let column = zone
            .side()
            .then(|| ColumnId(format!("panel-column-{}", if product == Product::History && stacked { Product::Browser.id() } else { product.id() })));
        if requested_open
            && let Some(column) = &column
            && !layout.columns.iter().any(|existing| existing.id == *column)
        {
            layout.columns.push(Column { id: column.clone(), zone, width });
        }
        let collapsed = spec.collapsible_in(zone)
            && match product {
                Product::Browser if vertical => app.preferences.timeline_left_browser_collapsed,
                Product::Browser => app.preferences.browser_collapsed,
                Product::History => app.preferences.timeline_collapsed,
                Product::Assistant => app.preferences.agent_chat_minimized,
                Product::RenderBrowser => false,
            };
        let weight = match product {
            Product::Browser if stacked => share,
            Product::History if stacked => 1.0 - share,
            _ => 1.0,
        };
        let section_index = if product == Product::Assistant && !requested_open { 0 } else { layout.sections.len() };
        if requested_open {
            layout.sections.push(Section {
                id: section.clone(),
                location: location.clone(),
                column: column.clone(),
                weight,
                preferred_extent: None,
                extent_priority: 0,
                tabs: vec![id.clone()],
                active: id.clone(),
                size,
                collapsed,
            });
        }
        let column_index = if product == Product::Assistant && !requested_open {
            0
        } else {
            layout
                .columns
                .iter()
                .filter(|existing| existing.zone == zone)
                .position(|existing| Some(&existing.id) == column.as_ref())
                .unwrap_or_else(|| layout.columns.iter().filter(|existing| existing.zone == zone).count())
        };
        layout.instances.push(PanelInstance {
            id,
            kind,
            requested_open,
            memory: PlacementMemory {
                section,
                location,
                section_index,
                tab_index: 0,
                size,
                collapsed,
                column: column.clone(),
                column_index,
                column_width: column.map(|_| width),
                weight,
                preferred_extent: None,
                extent_priority: 0,
            },
        });
    }
    // A pinned Assistant precedes ordinary columns on its chosen logical side.
    layout.columns.sort_by_key(|column| (column.zone != Zone::Left, column.id.0 != "panel-column-assistant"));
    layout.sections.sort_by_key(|section| {
        section.column.as_ref().and_then(|id| layout.columns.iter().position(|column| column.id == *id)).unwrap_or(MAX_SECTIONS)
    });
    for (section_index, section) in layout.sections.iter().enumerate() {
        let column_index = layout
            .columns
            .iter()
            .filter(|column| column.zone == section.location.zone())
            .position(|column| Some(&column.id) == section.column.as_ref())
            .unwrap_or(0);
        if let Some(instance) = layout.instances.iter_mut().find(|instance| instance.id == section.active) {
            instance.memory.section_index = section_index;
            instance.memory.column_index = column_index;
        }
    }
    validate(&layout, registry)?;
    Ok(layout)
}

struct ProductionController {
    product: Product,
    document: DocumentId,
    state: ProductionState,
}
impl PanelController for ProductionController {
    fn availability(&self, owner: &ContentOwner, _models: &dyn PanelModels) -> Availability {
        if self.state.0.try_borrow().is_ok_and(|snapshot| {
            *owner == ContentOwner::Document(snapshot.document.clone())
                && snapshot.panels.iter().any(|panel| panel.product == self.product && panel.available)
        }) {
            Availability::Available
        } else {
            Availability::Unavailable { reason: crate::i18n::tr("ui.panel_host.rule.availability").into() }
        }
    }
    fn update(&mut self, _owner: &ContentOwner, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Ok(())
    }
    fn presentation(&self) -> PanelPresentation {
        self.state
            .0
            .try_borrow()
            .ok()
            .and_then(|snapshot| snapshot.panels.iter().find(|panel| panel.product == self.product).map(|panel| panel.presentation.clone()))
            .unwrap_or_else(|| self.product.empty_presentation())
    }
    fn measure(&self, _owner: &ContentOwner, _models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<Arc<dyn HostContentMetrics>> {
        let snapshot = self.state.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "content measurement is borrowed"))?;
        Ok(Arc::new(ProductionMetrics {
            extent: snapshot.panels.iter().find(|panel| panel.product == self.product).and_then(|panel| panel.extent),
            inset: crate::theme::Tokens::get().space_2 * 2.0,
            policy: spec.side_height_policy,
        }))
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        if matches!(event, LifecycleEvent::Close | LifecycleEvent::Reopen) {
            let _snapshot = self.state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel visibility is borrowed"))?;
        }
        Ok(())
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        let open = match event {
            LifecycleEvent::Close => false,
            LifecycleEvent::Reopen => true,
            _ => return,
        };
        if let Ok(mut snapshot) = self.state.0.try_borrow_mut()
            && snapshot.document == self.document
            && let Some(slot) = snapshot.visibility.get_mut(self.product.index())
        {
            *slot = Some(VisibilityIntent { document: self.document.clone(), open });
        }
    }
    fn render(&mut self, _slot: ContentSlot, _ui: &mut egui::Ui, _context: &PanelContext, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Err(error(Rule::Lifecycle, "application panel content requires its current frame binding"))
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.availability(owner, services.models).require()?;
        self.prepare_action(owner, action, services.models)?;
        let mut snapshot = self.state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel actions are borrowed"))?;
        if snapshot.actions.len() >= MAX_ACTIONS {
            return Err(error(Rule::CollectionLimit, "application panel action queue is full"));
        }
        let document = snapshot.document.clone();
        snapshot.actions.push_back(QueuedAction { document, product: self.product, action: action.clone() });
        Ok(())
    }
}

#[derive(Debug)]
struct ProductionMetrics {
    extent: Option<ContentExtent>,
    inset: f32,
    policy: SideHeightPolicy,
}
impl HostContentMetrics for ProductionMetrics {
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        // Bootstrap a newly visible width at capacity. Only actual content extent,
        // never the allocated viewport, can subsequently shorten the panel.
        let needed_height = self
            .extent
            .filter(|extent| extent.width_independent || (extent.flow == Flow::Tall && (extent.width - request.width).abs() < 0.5))
            .map_or(MAX_LOGICAL, |extent| (request.chrome_height + extent.height + self.inset).min(MAX_LOGICAL));
        Ok(SideContentMeasure { request: request.clone(), needed_height, policy: self.policy })
    }
    fn revision(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.extent
            .filter(|_| self.policy == SideHeightPolicy::Content)
            .map(|extent| {
                (
                    if extent.width_independent { 0 } else { extent.width.to_bits() },
                    extent.height.to_bits(),
                    extent.width_independent,
                    if extent.width_independent { 0 } else { extent.flow as u8 },
                )
            })
            .hash(&mut hash);
        self.inset.to_bits().hash(&mut hash);
        matches!(self.policy, SideHeightPolicy::Fill).hash(&mut hash);
        hash.finish()
    }
}

struct ProductionModels;
impl PanelModels for ProductionModels {
    fn read(&self, _owner: &ContentOwner, _key: &str) -> PanelResult<Value> {
        Err(error(Rule::MissingTarget, "application content reads its existing owner directly"))
    }
}
struct ProductionCommands;
impl ContentCommandSink for ProductionCommands {
    fn execute(&mut self, _owner: &ContentOwner, _command: &str, _input: Value) -> PanelResult<Value> {
        Err(error(Rule::Capability, "application content uses its existing command path"))
    }
}
struct ProductionServices {
    models: ProductionModels,
    commands: ProductionCommands,
    local: ProductionLocal,
}
impl AppPanelServices for ProductionServices {
    fn refresh_workspace(&mut self, workspace: &WorkspaceId) -> PanelResult<()> {
        if !WORKSPACES.iter().any(|(id, _, _)| *id == workspace.0) {
            return Err(error(Rule::Placement, "unregistered application layout workspace"));
        }
        self.local.workspace = workspace.clone();
        Ok(())
    }
    fn refresh_read_only(&mut self, session: &Session) -> PanelResult<()> {
        self.local.document = DocumentId::new(session.document_id.clone())?;
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}
struct ProductionLocal {
    workspace: WorkspaceId,
    document: DocumentId,
    keyboard: Option<(LocalScope, InstanceId)>,
}
impl ProductionLocal {
    fn validate(&self, scope: &LocalScope, instance: &InstanceId) -> PanelResult<()> {
        scope.validate()?;
        Product::from_instance(instance)?;
        if scope.workspace != self.workspace || scope.document.as_ref() != Some(&self.document) || scope.viewport.0 != VIEWPORT {
            return Err(error(Rule::Placement, "application panel service has a different owner scope"));
        }
        Ok(())
    }
}
impl PanelLocalServices for ProductionLocal {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        request.validate()?;
        self.validate(scope, request.instance())?;
        match request {
            LocalRequest::KeyboardOwner { instance, .. } => {
                if self.keyboard.as_ref().is_some_and(|(owner_scope, owner)| owner_scope == scope && *owner != instance) {
                    return Err(error(Rule::Lifecycle, "another application panel owns the keyboard"));
                }
                self.keyboard = Some((scope.clone(), instance));
            }
            LocalRequest::ReleaseKeyboard { instance } => self.release(scope, &instance),
            LocalRequest::Overlay { .. } | LocalRequest::ClearOverlays { .. } => {}
            LocalRequest::Focus { .. }
            | LocalRequest::FileDrop { .. }
            | LocalRequest::PointerDrop { .. }
            | LocalRequest::Clipboard { .. }
            | LocalRequest::Pick { .. } => {
                return Err(error(Rule::Capability, "application content uses its current viewport input service"));
            }
        }
        Ok(())
    }
    fn register_focus_response(&mut self, scope: &LocalScope, instance: &InstanceId, _response: &egui::Response) -> PanelResult<()> {
        self.validate(scope, instance)
    }
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId) {
        if self.keyboard.as_ref().is_some_and(|(owner_scope, owner)| owner_scope == scope && owner == instance) {
            self.keyboard = None;
        }
    }
}

#[derive(Clone)]
struct FocusCapture {
    layer: egui::LayerId,
    responses: Vec<egui::Response>,
    top: f32,
    extent: Option<ContentExtent>,
    composer: Option<f32>,
}
fn focus_capture_id() -> egui::Id {
    egui::Id::unique("shared-panel-current-content-focus")
}

pub(crate) fn shared_content_measurement(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<Option<FocusCapture>>(focus_capture_id()).flatten().is_some())
}

pub(crate) fn shared_content_composer_height(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|data| data.get_temp::<Option<FocusCapture>>(focus_capture_id()).flatten().and_then(|capture| capture.composer))
}

pub(crate) fn report_content_composer_height(ctx: &egui::Context, height: f32) {
    if !height.is_finite() || height < 0.0 {
        return;
    }
    ctx.data_mut(|data| {
        if let Some(Some(mut capture)) = data.remove_temp::<Option<FocusCapture>>(focus_capture_id()) {
            capture.composer = Some(height.min(MAX_LOGICAL));
            data.insert_temp(focus_capture_id(), Some(capture));
        }
    });
}

/// Natural scroll content and fixed bands from the current visible body only.
pub(crate) fn report_content_extent(ui: &egui::Ui, top: f32, height: f32, width_independent: bool) {
    if !top.is_finite() || !height.is_finite() || height < 0.0 {
        return;
    }
    ui.ctx().data_mut(|data| {
        if let Some(Some(mut capture)) = data.remove_temp::<Option<FocusCapture>>(focus_capture_id()) {
            if capture.layer == ui.layer_id() {
                capture.extent = Some(ContentExtent {
                    width: 0.0,
                    flow: Flow::Tall,
                    composer: None,
                    height: ((top - capture.top).max(0.0) + height).min(MAX_LOGICAL),
                    width_independent,
                });
            }
            data.insert_temp(focus_capture_id(), Some(capture));
        }
    });
}

/// Report the actual editor response while its existing body is painting.
pub(crate) fn register_content_focus(ctx: &egui::Context, response: &egui::Response) {
    if !response.enabled() || !response.sense.is_focusable() {
        return;
    }
    ctx.data_mut(|data| {
        if let Some(Some(mut capture)) = data.remove_temp::<Option<FocusCapture>>(focus_capture_id()) {
            if response.layer_id == capture.layer && capture.responses.len() < 64 {
                capture.responses.push(response.clone());
            }
            data.insert_temp(focus_capture_id(), Some(capture));
        }
    });
}

pub(crate) struct ProductionFrame<'a> {
    pub app: &'a mut crate::SolveApp,
    pub(super) state: ProductionState,
}
impl FramePanelContent for ProductionFrame<'_> {
    fn render(
        &mut self,
        slot: ContentSlot,
        ui: &mut egui::Ui,
        context: &PanelContext,
        _content: &mut dyn PanelController,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        if !matches!(slot, ContentSlot::Body | ContentSlot::Inline) {
            return Ok(());
        }
        context.validate()?;
        let product = Product::from_instance(&context.instance)?;
        if context.owner != ContentOwner::Document(DocumentId::new(self.app.session.document_id.clone())?) || !product.available(self.app) {
            return Err(error(Rule::Availability, "application panel content owner changed"));
        }
        match product {
            Product::Browser | Product::RenderBrowser => self.app.shell.browser = Some(super::workspace::egui_rect(context.outer)),
            Product::History => self.app.shell.history = Some(super::workspace::egui_rect(context.outer)),
            Product::Assistant => {}
        }
        let previous = self
            .state
            .0
            .try_borrow()
            .map_err(|_| error(Rule::Lifecycle, "content measurement is borrowed"))?
            .panels
            .iter()
            .find(|panel| panel.product == product)
            .and_then(|panel| panel.extent)
            .filter(|extent| extent.flow == context.flow && (extent.width - context.outer.size.width).abs() < 0.5);
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                focus_capture_id(),
                Some(FocusCapture {
                    layer: ui.layer_id(),
                    responses: Vec::new(),
                    top: ui.cursor().top(),
                    extent: None,
                    composer: previous.and_then(|extent| extent.composer),
                }),
            )
        });
        match product {
            Product::Browser | Product::RenderBrowser => crate::browser::panel_contents(self.app, ui),
            Product::History if context.flow == Flow::Wide => crate::timeline::panel_inline(self.app, ui),
            Product::History => crate::timeline_left::panel_contents(self.app, ui),
            Product::Assistant => crate::agent_chat::shared_content(self.app, ui),
        }
        let capture = ui.ctx().data_mut(|data| data.remove_temp::<Option<FocusCapture>>(focus_capture_id()).flatten());
        if context.document.as_ref().map(|document| document.0.as_str()) != Some(self.app.session.document_id.as_str()) {
            return Err(error(Rule::Availability, "application panel document changed while painting"));
        }
        let scope = LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() };
        if let Some(capture) = capture {
            if let Some(mut extent) = capture.extent {
                extent.width = context.outer.size.width;
                extent.flow = context.flow;
                extent.composer = capture.composer;
                let mut snapshot = self.state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "content measurement is borrowed"))?;
                if let Some(panel) = snapshot.panels.iter_mut().find(|panel| panel.product == product)
                    && panel.extent != Some(extent)
                {
                    panel.extent = Some(extent);
                    ui.ctx().request_repaint();
                }
            }
            for response in capture.responses {
                services.local.register_focus_response(&scope, &context.instance, &response)?;
            }
        }
        Ok(())
    }
}
