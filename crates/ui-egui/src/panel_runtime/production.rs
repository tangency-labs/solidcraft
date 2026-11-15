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
    RenderLibrary,
    RenderInspector,
    RenderShots,
    SketchOptions,
    Bom,
}
impl Product {
    const ALL: [Self; 9] = [
        Self::Browser,
        Self::History,
        Self::Assistant,
        Self::RenderBrowser,
        Self::RenderLibrary,
        Self::RenderInspector,
        Self::RenderShots,
        Self::SketchOptions,
        Self::Bom,
    ];
    const NEW_RENDER: [Self; 3] = [Self::RenderLibrary, Self::RenderInspector, Self::RenderShots];

    fn index(self) -> usize {
        match self {
            Self::Browser => 0,
            Self::History => 1,
            Self::Assistant => 2,
            Self::RenderBrowser => 3,
            Self::RenderLibrary => 4,
            Self::RenderInspector => 5,
            Self::RenderShots => 6,
            Self::SketchOptions => 7,
            Self::Bom => 8,
        }
    }
    fn visible(self, app: &crate::SolveApp) -> bool {
        match self {
            Self::Browser => app.ui.show_browser,
            Self::RenderBrowser => app.render_workspace.view.browser_open,
            Self::History => app.ui.show_timeline,
            Self::Assistant => app.agent_chat.open,
            Self::RenderLibrary => true,
            Self::RenderInspector => crate::render_workspace::inspector_visible(app),
            Self::RenderShots => app.preferences.render_shots_strip,
            Self::SketchOptions => true,
            Self::Bom => crate::bom_panel::available(app),
        }
    }
    fn set_visible(self, app: &mut crate::SolveApp, open: bool) {
        match self {
            Self::Browser => app.ui.show_browser = open,
            Self::RenderBrowser => app.render_workspace.view.browser_open = open,
            Self::History => app.ui.show_timeline = open,
            Self::Assistant => app.agent_chat.open = open,
            Self::RenderLibrary => {}
            Self::RenderInspector => crate::render_workspace::set_inspector_visible(app, open),
            Self::RenderShots => app.preferences.render_shots_strip = open,
            Self::Bom if !open => app.bom = None,
            Self::Bom | Self::SketchOptions => {}
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::History => "history",
            Self::Assistant => "assistant",
            Self::RenderBrowser => "render-browser",
            Self::RenderLibrary => "render-library",
            Self::RenderInspector => "render-inspector",
            Self::RenderShots => "render-shots",
            Self::SketchOptions => crate::sketch_palette::INSTANCE_ID,
            Self::Bom => "bom",
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::Browser => "solidcraft.browser",
            Self::History => "solidcraft.history",
            Self::Assistant => "solidcraft.assistant",
            Self::RenderBrowser => "solidcraft.render_browser",
            Self::RenderLibrary => "solidcraft.render_library",
            Self::RenderInspector => "solidcraft.render_inspector",
            Self::RenderShots => "solidcraft.render_shots",
            Self::SketchOptions => "solidcraft.sketch_options",
            Self::Bom => "solidcraft.bom",
        }
    }
    fn title_key(self) -> &'static str {
        match self {
            Self::Browser => "ui.browser_title",
            Self::History => "ui.history_title",
            Self::Assistant => "ui.assistant",
            Self::RenderBrowser => "render.page_scene",
            Self::RenderLibrary => "render.library_title",
            Self::RenderInspector => "render.inspector",
            Self::RenderShots => "render.ribbon_shots",
            Self::SketchOptions => "ui.palette_sketch_options",
            Self::Bom => crate::bom_panel::TITLE_KEY,
        }
    }
    fn icon(self) -> &'static str {
        match self {
            Self::SketchOptions => "sketch",
            Self::Bom => "bom",
            _ => "list",
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
            Self::RenderBrowser | Self::RenderLibrary | Self::RenderInspector | Self::RenderShots => render && !crate::render_ui::focused(app),
            Self::SketchOptions => !render && crate::sketch_palette::editing(app),
            Self::Bom => crate::bom_panel::available(app),
        }
    }
    fn default_zone(self, placement: &crate::prefs::Prefs) -> Zone {
        match self {
            Self::History if !placement.timeline_left => Zone::Bottom,
            Self::Assistant if placement.agent_chat_side == "right" => Zone::Right,
            Self::RenderBrowser | Self::RenderLibrary if placement.render_browser_side == "right" => Zone::Right,
            Self::RenderShots => Zone::Bottom,
            Self::RenderInspector if placement.render_inspector_side != "left" => Zone::Right,
            Self::SketchOptions | Self::Bom => Zone::Right,
            _ => Zone::Left,
        }
    }

    fn column_owner(self, stack_history: bool) -> Self {
        match self {
            Self::History if stack_history => Self::Browser,
            Self::RenderLibrary => Self::RenderBrowser,
            _ => self,
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
                ("search", "render.tree_search", "zoom"),
            ],
            Self::RenderLibrary => vec![
                ("grid", "render.library_grid", "grid"),
                ("list", "render.library_list", "list"),
                ("references", "render.stage_panels.surfaces_references", "library"),
            ],
            Self::RenderInspector => vec![("remove", "render.inspector_remove", "delete")],
            Self::Browser | Self::RenderShots | Self::SketchOptions | Self::Bom => Vec::new(),
        };
        entries.into_iter().map(|(id, key, icon)| HeaderActionSpec { id: ControlId(id.into()), label_key: key.into(), icon: icon.into() }).collect()
    }
    fn presentation(self, app: &crate::SolveApp) -> PanelPresentation {
        if self == Self::SketchOptions {
            return crate::sketch_palette::shared_presentation(app).unwrap_or_else(|| self.empty_presentation());
        }
        let mut presentation = self.empty_presentation();
        if self == Self::Assistant {
            let waiting = app.agent_chat.waiting();
            let state = app.agent_chat.state();
            presentation.status = Some(if waiting > 0 {
                solidcraft_ui_text::format("ui.assistant_waiting", &[("count", &waiting.to_string())])
            } else {
                state.label().into()
            });
            presentation.attention = waiting > 0
                || matches!(state, crate::agent_chat::State::NeedsAuth | crate::agent_chat::State::Exited | crate::agent_chat::State::Failed);
        }
        presentation.actions = match self {
            Self::History => crate::ui_kit::HistoryTransport::shown(app.preferences.history_play_button)
                .iter()
                .map(|control| {
                    let availability = crate::timeline::transport_availability(app, *control);
                    RuntimeAction {
                        id: ControlId(control.id().into()),
                        label: control.label(app.timeline_playback.is_some(), crate::timeline::play_labels()).into(),
                        enabled: availability.is_ok(),
                        disabled_reason: availability.err(),
                        pressed: (*control == crate::ui_kit::HistoryTransport::Play).then_some(app.timeline_playback.is_some()),
                    }
                })
                .collect(),
            Self::Assistant => vec![RuntimeAction {
                id: ControlId("new-chat".into()),
                label: solidcraft_ui_text::tr("ui.agent_chat_new").into(),
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
                label: solidcraft_ui_text::tr(key).into(),
                enabled: true,
                disabled_reason: None,
                pressed: Some(pressed),
            })
            .collect(),
            Self::RenderLibrary => {
                presentation.title = crate::render_workspace::library::library_title(app);
                crate::render_workspace::library::library_mode(app).map_or_else(Vec::new, |mode| {
                    self.header_actions()
                        .into_iter()
                        .enumerate()
                        .map(|(index, action)| RuntimeAction {
                            id: action.id,
                            label: solidcraft_ui_text::tr(&action.label_key).into(),
                            enabled: true,
                            disabled_reason: None,
                            pressed: Some(mode == index),
                        })
                        .collect()
                })
            }
            Self::RenderInspector => {
                use crate::render_workspace::inspector_panel as inspector;
                let (title, icon) = inspector::shared_title(app);
                presentation.title = title;
                presentation.icon = icon.into();
                presentation.status = inspector::shared_status(app);
                inspector::shared_remove_available(app)
                    .then(|| RuntimeAction {
                        id: ControlId("remove".into()),
                        label: solidcraft_ui_text::tr("render.inspector_remove").into(),
                        enabled: true,
                        disabled_reason: None,
                        pressed: None,
                    })
                    .into_iter()
                    .collect()
            }
            Self::Browser | Self::RenderShots | Self::SketchOptions | Self::Bom => Vec::new(),
        };
        presentation
    }
    fn empty_presentation(self) -> PanelPresentation {
        PanelPresentation {
            title: solidcraft_ui_text::tr(self.title_key()).into(),
            icon: self.icon().into(),
            status: None,
            attention: false,
            actions: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct PanelVisibilitySnapshot([bool; Product::ALL.len()]);

impl PanelVisibilitySnapshot {
    pub(crate) fn capture(app: &crate::SolveApp) -> Self {
        Self(Product::ALL.map(|product| product.visible(app)))
    }

    pub(crate) fn restore(self, app: &mut crate::SolveApp) {
        for (product, visible) in Product::ALL.into_iter().zip(self.0) {
            if !matches!(product, Product::Bom | Product::SketchOptions | Product::RenderLibrary) {
                product.set_visible(app, visible);
            }
        }
        app.render_workspace.view.inspector_visibility_request = None;
        if let Some(state) = app.panel_runtime.as_ref().and_then(|integration| integration.production.as_ref())
            && let Ok(mut state) = state.0.try_borrow_mut()
        {
            state.visibility = Default::default();
            state.visible = Product::ALL.map(|product| product.visible(app));
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
    sketch_owner: Option<(DocumentId, u64)>,
    sketch_placement_attempt: Option<(u64, LogicalRect, u64)>,
    bom_focus_attempt: Option<(u64, LogicalRect)>,
    render_addition_attempts: [Option<(u64, LogicalRect)>; Product::ALL.len()],
    visible: [bool; Product::ALL.len()],
    visibility: [Option<VisibilityIntent>; Product::ALL.len()],
    panels: [PanelSnapshot; Product::ALL.len()],
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
            render_addition_attempts: Default::default(),
            visible: Product::ALL.map(|product| product.visible(app)),
            sketch_owner: None,
            sketch_placement_attempt: None,
            bom_focus_attempt: None,
            visibility: Default::default(),
            panels: Product::ALL.map(|product| PanelSnapshot {
                product,
                available: product.available(app),
                presentation: product.presentation(app),
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
    let registry = registry()?;
    let workspace = WorkspaceId::new(WORKSPACES[0].0)?;
    let defaults = crate::prefs::Prefs::default();
    let placement = if app.panel_layout_preferences.record.is_none() { &app.preferences } else { &defaults };
    let layout = default_layout(app, placement, workspace.clone(), &registry)?;
    let mut runtime = PanelRuntime::new(
        LayoutService::new(
            registry,
            layout,
            HostCapabilities {
                persistence: true,
                native_windows: cfg!(not(target_arch = "wasm32")),
                max_native_hosts: if cfg!(target_arch = "wasm32") { 0 } else { MAX_SECTIONS },
            },
        )?,
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
                create: Box::new(move |_instance, owner| Ok(Box::new(ProductionController { product, owner: owner.clone(), state: state.clone() }))),
            },
        )?;
    }
    let services = ProductionServices {
        models: ProductionModels,
        commands: ProductionCommands,
        local: ProductionLocal {
            workspace,
            document: DocumentId::new(app.session.document_id.clone())?,
            viewport: ViewportId(VIEWPORT.into()),
            keyboard: None,
        },
    };
    let mut integration = AppPanelRuntime::new(runtime, Box::new(services))?;
    integration.production = Some(state);
    integration.tick(&app.session)?;
    let mut options = HostOptions { content_header_height: 0.0, ..Default::default() };
    options.geometry.default_opaque = false;
    options.metrics.footer_height = 0.0;
    integration.enable_host(options)?;
    let mut catalogue = commands::InMemoryCatalogue::default();
    for (id, name, suffix) in WORKSPACES {
        let workspace = WorkspaceId::new(id)?;
        let layout = default_layout(app, &defaults, workspace.clone(), integration.runtime().layout().registry())?;
        catalogue.register_builtin(name, &LayoutEnvelope::new(layout), integration.runtime().layout().registry())?;
        catalogue.register_workspace(commands::WorkspaceDescriptor {
            id: workspace,
            title_key: format!("ui.panel_layout.workspace.{suffix}"),
            default_preset: name.into(),
        })?;
    }
    let repaired = super::preferences::configure_preferences(&mut integration, &app.panel_layout_preferences, &mut catalogue)?;
    app.panel_runtime = Some(integration);
    crate::bom_panel::set_focus_pending(app, true);
    if repaired {
        super::preferences::report_notice(app);
    }
    Ok(())
}

pub(crate) fn refresh_production(app: &crate::SolveApp) -> PanelResult<()> {
    let Some(state) = app.panel_runtime.as_ref().and_then(|integration| integration.production.clone()) else { return Ok(()) };
    let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel snapshot is borrowed"))?;
    let document = DocumentId::new(app.session.document_id.clone())?;
    if snapshot.document != document {
        snapshot.sketch_placement_attempt = None;
        snapshot.bom_focus_attempt = None;
        for panel in &mut snapshot.panels {
            panel.extent = None;
        }
    }
    snapshot.document = document;
    // Keep an explicit view-toggle edge pending while its content context is
    // unavailable. Ordinary workspace changes do not create a new toggle edge.
    for product in Product::ALL {
        if product == Product::Bom {
            continue;
        }
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
    for panel in &mut snapshot.panels {
        panel.available = panel.product.available(app);
        panel.presentation = panel.product.presentation(app);
    }
    Ok(())
}

pub(crate) fn drain_production_actions(app: &mut crate::SolveApp) -> PanelResult<()> {
    let Some(state) = app.panel_runtime.as_ref().and_then(|integration| integration.production.clone()) else { return Ok(()) };
    let (actions, visibility) = {
        let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel actions are borrowed"))?;
        (std::mem::take(&mut snapshot.actions), std::mem::take(&mut snapshot.visibility))
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
        let presentation = action.product.presentation(app);
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
            Product::RenderLibrary => {
                let mode = match action.action.0.as_str() {
                    "grid" => 0,
                    "list" => 1,
                    "references" => 2,
                    _ => continue,
                };
                crate::render_workspace::library::set_library_mode(app, mode);
            }
            Product::RenderInspector => crate::render_workspace::inspector_panel::shared_remove(app),
            Product::Browser | Product::RenderShots | Product::SketchOptions | Product::Bom => {}
        }
    }
    Ok(())
}

pub(crate) fn apply_placement_command(app: &mut crate::SolveApp, command: &str, params: &serde_json::Value) -> PanelResult<()> {
    match command {
        "view.history_layout" => {
            let dock = params.get("dock").and_then(serde_json::Value::as_str);
            if dock == Some("browser") {
                app.preferences.history_in_browser = true;
                return Ok(());
            }
            let was_combined = app.preferences.history_in_browser;
            if dock.is_some() {
                app.preferences.history_in_browser = false;
            }
            let zone = match dock {
                Some("bottom") => Some(Zone::Bottom),
                Some("left" | "under_browser") => Some(Zone::Left),
                _ => None,
            };
            let width = params.get("width").and_then(serde_json::Value::as_f64).map(|width| width as f32);
            let collapsed = params.get("collapsed").and_then(serde_json::Value::as_bool).or(dock.map(|_| false));
            let result = place_compatibly(app, &[(Product::History, zone)], dock == Some("under_browser"), width, collapsed);
            if result.is_err() {
                app.preferences.history_in_browser = was_combined;
            }
            result
        }
        "view.render_dock" => {
            let side = params.get("side").and_then(serde_json::Value::as_str);
            let Some(side) = side else { return Ok(()) };
            let zone = if side == "left" { Zone::Left } else { Zone::Right };
            let opposite = if zone == Zone::Left { Zone::Right } else { Zone::Left };
            let (browser, inspector) =
                if params.get("panel").and_then(serde_json::Value::as_str) == Some("inspector") { (opposite, zone) } else { (zone, opposite) };
            place_compatibly(
                app,
                &[(Product::RenderBrowser, Some(browser)), (Product::RenderLibrary, Some(browser)), (Product::RenderInspector, Some(inspector))],
                false,
                None,
                None,
            )
        }
        _ => Ok(()),
    }
}

pub(crate) fn place_assistant(app: &mut crate::SolveApp, zone: Option<Zone>, width: Option<f32>, collapsed: Option<bool>) -> PanelResult<()> {
    super::app::ensure_production(app)?;
    sync_production_visibility(app)?;
    place_compatibly(app, &[(Product::Assistant, zone)], false, width, collapsed)
}

fn place_compatibly(
    app: &mut crate::SolveApp,
    targets: &[(Product, Option<Zone>)],
    under_browser: bool,
    width: Option<f32>,
    collapsed: Option<bool>,
) -> PanelResult<()> {
    super::app::ensure_production(app)?;
    let integration = app.panel_runtime.as_ref().ok_or_else(|| error(Rule::Lifecycle, "application panel runtime unavailable"))?;
    let current = integration.runtime().layout().inspect();
    let layout = &current.layout;
    let mut reserved = integration.placement_reservations()?;
    let browser = layout.containing(&InstanceId::new(Product::Browser.id())?).ok();
    let mut mutations = Vec::new();
    for (product, zone) in targets {
        let instance = InstanceId::new(product.id())?;
        let Ok(source) = layout.containing(&instance) else { continue };
        let separate_history = *product == Product::History && *zone == Some(Zone::Left) && !under_browser;
        let zone = zone.unwrap_or(source.location.zone());
        let side_anchor = layout
            .sections
            .iter()
            .filter(|section| section.location.zone() == zone && section.id != source.id)
            .min_by_key(|section| section.column != source.column);
        let needs_column = separate_history && side_anchor.is_some_and(|anchor| source.location.zone() != zone || source.column == anchor.column);
        let below = browser.filter(|browser| under_browser && browser.location.zone() == Zone::Left);
        let needs_stack = below.is_some_and(|browser| {
            source.id == browser.id
                || source.column != browser.column
                || layout.sections.iter().position(|section| section.id == source.id)
                    < layout.sections.iter().position(|section| section.id == browser.id)
        });
        let section = if source.location.zone() != zone || needs_stack || needs_column {
            let section = next_section_id_with_reservations(layout, &reserved.sections)?;
            reserved.sections.insert(section.clone());
            let size = Size { width: width.unwrap_or(source.size.width).clamp(240.0, 1200.0), height: source.size.height };
            let destination = if let Some(browser) = below {
                Destination::Relative {
                    section: browser.id.clone(),
                    placement: RelativePlacement::Below,
                    new_section: section.clone(),
                    new_column: None,
                    size,
                }
            } else if let Some(anchor) = side_anchor.filter(|_| needs_column) {
                Destination::Relative {
                    section: anchor.id.clone(),
                    placement: RelativePlacement::BeforeColumn,
                    new_section: section.clone(),
                    new_column: Some(next_column_id_with_reservations(layout, &reserved.columns)?),
                    size,
                }
            } else {
                Destination::NewSection { section: section.clone(), index: 0, location: SectionLocation::Dock { zone }, size }
            };
            mutations.push(Mutation::Move { instance, destination });
            if width.is_some() && zone.side() {
                mutations.push(Mutation::SetSize { section: section.clone(), size });
                // Compatibility placement supplies a width, so it must not pin an automatic height.
                if !source.location.zone().side() || source.preferred_extent.is_none() {
                    mutations.push(Mutation::ResetSideHeight { section: section.clone() });
                }
            }
            if zone == Zone::Bottom && source.collapsed {
                mutations.push(Mutation::Collapse { section: section.clone(), collapsed: false });
            }
            section
        } else {
            if let (Some(width), Some(column)) = (width, &source.column) {
                mutations.push(Mutation::SetColumnWidth { column: column.clone(), width: width.clamp(240.0, 1200.0) });
            }
            source.id.clone()
        };
        if let Some(collapsed) = collapsed.filter(|_| zone.side()) {
            mutations.push(Mutation::Collapse { section, collapsed });
        }
    }
    if !mutations.is_empty() {
        // The compatibility command reconciles the current view context before applying.
        app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: None, mutations })?;
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
    let reveal_library = app.render_workspace.view.library_focus_pending && Product::RenderLibrary.available(app);
    let inspector_visibility = app.render_workspace.view.inspector_visibility_request.filter(|_| Product::RenderInspector.available(app));
    let revision = integration.runtime().layout().inspect().revision;
    let addition = app.universal_panel_frame().map(|frame| (revision, frame.layout.capacity));
    let mut adding = None;
    for product in Product::ALL {
        if product == Product::Bom {
            continue;
        }
        let visible = product.visible(app);
        let id = InstanceId::new(product.id())?;
        let instance = layout.instance(&id).ok();
        if instance.is_none()
            && Product::NEW_RENDER.contains(&product)
            && visible
            && product.available(app)
            && adding.is_none()
            && addition.is_some()
            && addition != snapshot.render_addition_attempts[product.index()]
        {
            // Layouts saved before these registrations gain one panel at a time.
            // Reuse a compatible side column when possible.
            let zone = product.default_zone(&crate::prefs::Prefs::default());
            let kind = PanelKind::new(product.kind())?;
            let size = integration.runtime().layout().registry().get(&kind)?.default_size;
            let section = next_section_id_with_reservations(layout, &integration.preference_identity_reservations()?)?;
            let companion = product.column_owner(false);
            let anchor = layout
                .containing(&InstanceId::new(companion.id())?)
                .ok()
                .filter(|section| section.location.zone() == zone)
                .or_else(|| layout.sections.iter().find(|section| section.location.zone() == zone));
            let destination = if let Some(anchor) = anchor.filter(|_| zone.side()) {
                Destination::Relative {
                    section: anchor.id.clone(),
                    placement: RelativePlacement::Below,
                    new_section: section,
                    new_column: None,
                    size,
                }
            } else {
                Destination::NewSection { section, index: layout.sections.len(), location: SectionLocation::Dock { zone }, size }
            };
            mutations.push(Mutation::Open { kind, instance: id, destination: Some(destination) });
            adding = Some(product);
        } else if let Some(instance) = instance {
            let reveal =
                product == Product::RenderLibrary && reveal_library || product == Product::RenderInspector && inspector_visibility == Some(true);
            if reveal {
                if !layout.containing(&id).is_ok_and(|section| !section.collapsed && section.active == id) {
                    mutations.push(Mutation::Focus { instance: id });
                }
            } else if product == Product::RenderInspector {
                if inspector_visibility == Some(false) && instance.requested_open {
                    mutations.push(Mutation::Close { instance: id });
                }
            } else if snapshot.visible.get(product.index()).copied() != Some(visible)
                && instance.requested_open != visible
                && (!visible || product.available(app))
            {
                mutations.push(if visible {
                    Mutation::Open { kind: instance.kind.clone(), instance: id, destination: None }
                } else {
                    Mutation::Close { instance: id }
                });
            }
        }
    }
    let library_present = layout.instance(&InstanceId::new(Product::RenderLibrary.id())?).is_ok();
    let inspector_present = layout.instance(&InstanceId::new(Product::RenderInspector.id())?).is_ok();
    drop(snapshot);
    if let Some(product) = adding {
        state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel visibility is borrowed"))?.render_addition_attempts
            [product.index()] = addition;
    }
    if !mutations.is_empty() {
        refresh_production(app)?;
        app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: Some(revision), mutations })?;
    }
    if reveal_library && library_present {
        app.render_workspace.view.library_focus_pending = false;
    }
    if inspector_visibility == Some(false) || (inspector_visibility == Some(true) && inspector_present) {
        app.render_workspace.view.inspector_visibility_request = None;
    }
    sync_sketch_options(app)?;
    sync_bom(app)
}

fn sync_bom(app: &mut crate::SolveApp) -> PanelResult<()> {
    let Some(integration) = app.panel_runtime.as_ref() else { return Ok(()) };
    let Some(state) = integration.production.clone() else { return Ok(()) };
    let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "BOM focus is borrowed"))?;
    let layout = integration.runtime().layout().layout();
    let revision = integration.runtime().layout().inspect().revision;
    let mut mutations = Vec::new();
    let mut bom_focused = false;
    let mut bom_attempt = None;
    if crate::bom_panel::focus_pending(app) {
        let id = InstanceId::new(Product::Bom.id())?;
        if layout.containing(&id).is_ok_and(|section| !section.collapsed && section.active == id) {
            bom_focused = true;
        } else if let Some(attempt) = app.universal_panel_frame().map(|frame| (revision, frame.layout.capacity))
            && Some(attempt) != snapshot.bom_focus_attempt
        {
            let mutation = if layout.instance(&id).is_ok() { Mutation::Focus { instance: id } } else { open_bom(integration, layout, id)? };
            mutations.push(mutation);
            bom_attempt = Some(attempt);
            bom_focused = true;
        }
    }
    if let Some(attempt) = bom_attempt {
        snapshot.bom_focus_attempt = Some(attempt);
    }
    drop(snapshot);
    if !mutations.is_empty() {
        app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: Some(revision), mutations })?;
    }
    if bom_focused {
        crate::bom_panel::set_focus_pending(app, false);
    }
    Ok(())
}

fn open_bom(integration: &AppPanelRuntime, layout: &Layout, instance: InstanceId) -> PanelResult<Mutation> {
    let kind = PanelKind::new(Product::Bom.kind())?;
    let spec = integration.runtime().layout().registry().get(&kind)?;
    let zone = spec.default_zone;
    let size = spec.default_size;
    let section = next_section_id_with_reservations(layout, &integration.preference_identity_reservations()?)?;
    let destination = if let Some(anchor) = layout.sections.iter().find(|section| section.location.zone() == zone) {
        Destination::Relative { section: anchor.id.clone(), placement: RelativePlacement::Below, new_section: section, new_column: None, size }
    } else {
        Destination::NewSection { section, index: layout.sections.len(), location: SectionLocation::Dock { zone }, size }
    };
    Ok(Mutation::Open { kind, instance, destination: Some(destination) })
}

fn sync_sketch_options(app: &mut crate::SolveApp) -> PanelResult<()> {
    let Some(integration) = app.panel_runtime.as_ref() else { return Ok(()) };
    let Some(state) = integration.production.clone() else { return Ok(()) };
    let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "application panel visibility is borrowed"))?;
    let Some(sketch) = app.session.active_sketch else {
        snapshot.sketch_owner = None;
        snapshot.sketch_placement_attempt = None;
        return Ok(());
    };
    let owner = (snapshot.document.clone(), sketch);
    let product = Product::SketchOptions;
    if !product.available(app) {
        return Ok(());
    }
    let layout = integration.runtime().layout().layout();
    let instance = InstanceId::new(product.id())?;
    let existing = layout.instance(&instance).ok();
    if existing.is_some_and(|panel| panel.requested_open) {
        snapshot.sketch_owner = Some(owner);
        snapshot.sketch_placement_attempt = None;
        return Ok(());
    }
    if existing.is_some() && snapshot.sketch_owner.as_ref() == Some(&owner) {
        return Ok(());
    }
    let revision = integration.runtime().layout().inspect().revision;
    let attempt = app.universal_panel_frame().map(|frame| (revision, frame.layout.capacity, sketch));
    if attempt.is_none() || snapshot.sketch_placement_attempt == attempt {
        return Ok(());
    }
    snapshot.sketch_placement_attempt = attempt;
    drop(snapshot);

    let kind = PanelKind::new(product.kind())?;
    let destination = if existing.is_none() {
        // Old presets lack the Sketch panel. Preserve their existing section identities and placements.
        let zone = Zone::Right;
        let spec = integration.runtime().layout().registry().get(&kind)?;
        let section = next_section_id_with_reservations(layout, &integration.preference_identity_reservations()?)?;
        let anchor = layout.sections.iter().find(|anchor| {
            anchor.location.zone() == zone
                && anchor.size.width >= spec.min_size.width
                && !anchor.tabs.iter().any(|id| id.0 == Product::Assistant.id())
        });
        Some(match anchor {
            Some(anchor) => Destination::Relative {
                section: anchor.id.clone(),
                placement: RelativePlacement::Below,
                new_section: section,
                new_column: None,
                size: spec.default_size,
            },
            None => {
                Destination::NewSection { section, index: layout.sections.len(), location: SectionLocation::Dock { zone }, size: spec.default_size }
            }
        })
    } else {
        None
    };
    match app.dispatch_panel_layout(LayoutAction::Apply {
        expected_revision: Some(revision),
        mutations: vec![Mutation::Open { kind, instance, destination }],
    }) {
        Ok(_) => Ok(()),
        Err(reason) if reason.rule == Rule::NoSpace => Ok(()),
        Err(reason) => Err(reason),
    }
}

fn registry() -> PanelResult<PanelRegistry> {
    let mut registry = PanelRegistry::default();
    let tokens = solidcraft_theme::Tokens::get();
    // The existing strip has a 28-point chip and six-point inset on each side;
    // the shared content slot adds its own symmetric two-point inset.
    let bottom_height = tokens.form_header_height.max(tokens.space_24 + tokens.space_4 + 2.0 * tokens.space_6 + 2.0 * tokens.space_2);
    for product in Product::ALL {
        let history = product == Product::History;
        let assistant = product == Product::Assistant;
        let sketch = product == Product::SketchOptions;
        let bom = product == Product::Bom;
        let mut allowed_zones = [Zone::Left, Zone::Right, Zone::Floating].into_iter().collect::<std::collections::BTreeSet<_>>();
        let shots = product == Product::RenderShots;
        if history || shots {
            allowed_zones.insert(Zone::Bottom);
        }
        allowed_zones.insert(Zone::Window);
        registry.register(PanelSpec {
            kind: PanelKind::new(product.kind())?,
            title_key: product.title_key().into(),
            icon: product.icon().into(),
            allowed_zones,
            default_zone: match product {
                Product::History | Product::RenderShots => Zone::Bottom,
                Product::RenderInspector | Product::SketchOptions | Product::Bom => Zone::Right,
                _ => Zone::Left,
            },
            min_size: Size {
                width: 240.0,
                height: match product {
                    Product::RenderLibrary => 180.0,
                    Product::Bom => 200.0,
                    Product::SketchOptions => 150.0,
                    Product::RenderInspector => 160.0,
                    Product::Assistant => 120.0,
                    Product::RenderShots => bottom_height,
                    _ => tokens.control_height,
                },
            },
            default_size: Size { width: crate::prefs::DEFAULT_BROWSER_WIDTH, height: if history || shots { bottom_height } else { 320.0 } },
            max_size: Size { width: MAX_LOGICAL, height: MAX_LOGICAL },
            edge_rule: if assistant { EdgeRule::PinOuter } else { EdgeRule::None },
            surface: PanelSurface::Translucent,
            features: PanelFeatures { popout: true, hot_closable: assistant || bom, closable: !sketch, ..Default::default() },
            side_height_policy: if assistant { SideHeightPolicy::Fill } else { SideHeightPolicy::Content },
            fixed_bottom_height: (history || shots).then_some(bottom_height),
            header_actions: product.header_actions(),
            text_heavy: assistant || bom || sketch,
        })?;
    }
    Ok(registry)
}

fn default_layout(app: &crate::SolveApp, placement: &crate::prefs::Prefs, workspace: WorkspaceId, registry: &PanelRegistry) -> PanelResult<Layout> {
    let mut layout = Layout::empty(workspace);
    let vertical = app.ui.show_timeline && placement.timeline_left;
    let stacked = vertical && placement.timeline_under_browser;
    let share = placement.browser_share;
    let share = if (0.05..=0.95).contains(&share) { share } else { 0.5 };
    for product in Product::ALL {
        let id = InstanceId::new(product.id())?;
        let requested_open = product != Product::Bom && product.visible(app);
        let kind = PanelKind::new(product.kind())?;
        let spec = registry.get(&kind)?;
        let zone = product.default_zone(placement);
        let width = match product {
            Product::Browser => placement.browser_width,
            Product::History if stacked => placement.browser_width,
            Product::History => placement.timeline_left_width,
            Product::Assistant => placement.agent_chat_width,
            Product::RenderBrowser | Product::RenderLibrary => placement.render_browser_width,
            Product::RenderInspector => placement.render_inspector_width,
            Product::RenderShots => spec.default_size.width,
            _ => spec.default_size.width,
        };
        let width = if (1.0..=1200.0).contains(&width) { width.max(spec.min_size.width) } else { spec.default_size.width };
        let width = if product == Product::History && stacked {
            layout.columns.iter().find(|column| column.id.0 == "panel-column-browser").map_or(width, |column| column.width)
        } else {
            width
        };
        let size = Size { width, height: spec.default_size.height };
        let section = SectionId::new(format!("panel-{}", product.id()))?;
        let location = SectionLocation::Dock { zone };
        let column = zone.side().then(|| ColumnId(format!("panel-column-{}", product.column_owner(stacked).id())));
        if requested_open
            && let Some(column) = &column
            && !layout.columns.iter().any(|existing| existing.id == *column)
        {
            layout.columns.push(Column { id: column.clone(), zone, width });
        }
        let collapsed = spec.collapsible_in(zone)
            && match product {
                Product::Browser if vertical => placement.timeline_left_browser_collapsed,
                Product::Browser => placement.browser_collapsed,
                Product::History => placement.timeline_collapsed,
                Product::Assistant => placement.agent_chat_minimized,
                _ => false,
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
    owner: ContentOwner,
    state: ProductionState,
}
impl PanelController for ProductionController {
    fn availability(&self, owner: &ContentOwner, _models: &dyn PanelModels) -> Availability {
        if self.state.0.try_borrow().is_ok_and(|snapshot| {
            *owner == self.owner
                && *owner == ContentOwner::Document(snapshot.document.clone())
                && snapshot.panels.iter().any(|panel| panel.product == self.product && panel.available)
        }) {
            Availability::Available
        } else {
            Availability::Unavailable { reason: solidcraft_ui_text::tr("ui.panel_host.rule.availability").into() }
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
            inset: solidcraft_theme::Tokens::get().space_2 * 2.0,
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
            && self.owner == ContentOwner::Document(snapshot.document.clone())
        {
            let document = snapshot.document.clone();
            snapshot.visibility[self.product.index()] = Some(VisibilityIntent { document, open });
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
    #[cfg(not(target_arch = "wasm32"))]
    fn set_presentation_viewport(&mut self, native: Option<&HostId>) {
        self.local.viewport = ViewportId(native.map_or(VIEWPORT, |host| host.0.as_str()).into());
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}
struct ProductionLocal {
    workspace: WorkspaceId,
    document: DocumentId,
    viewport: ViewportId,
    keyboard: Option<(LocalScope, InstanceId)>,
}
impl ProductionLocal {
    fn validate(&self, scope: &LocalScope, instance: &InstanceId) -> PanelResult<()> {
        scope.validate()?;
        Product::from_instance(instance)?;
        if scope.workspace != self.workspace || scope.document.as_ref() != Some(&self.document) || scope.viewport != self.viewport {
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
    fn header_action_context_menu(&mut self, instance: &InstanceId, at: egui::Pos2) {
        if instance.0 == Product::History.id() {
            crate::context_menu::open_for(self.app, at, crate::context_menu::Target::HistoryControls);
        }
    }

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
        let _catalog_labels = solidcraft_ui_text::catalog_labels();
        context.validate()?;
        let product = Product::from_instance(&context.instance)?;
        let owner = ContentOwner::Document(DocumentId::new(self.app.session.document_id.clone())?);
        if context.owner != owner || !product.available(self.app) {
            return Err(error(Rule::Availability, "application panel content owner changed"));
        }
        match product {
            Product::RenderLibrary | Product::RenderInspector | Product::RenderShots if !context.preview_geometry => {
                let rect = super::workspace::egui_rect(context.outer);
                let id = match product {
                    Product::RenderLibrary => "render:appearances",
                    Product::RenderInspector => "render:inspector",
                    _ => "render:shots_strip",
                };
                crate::scenario::publish_rect(id, rect);
            }
            _ => {}
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
            Product::Assistant => crate::agent_chat::shared_content(self.app, ui, context.flow),
            Product::SketchOptions => crate::sketch_palette::shared_body(self.app, ui),
            Product::Bom => crate::bom_panel::shared_content(self.app, ui),
            Product::RenderLibrary => crate::render_workspace::library::shared_library(self.app, ui),
            Product::RenderInspector => crate::render_workspace::inspector_panel::shared_body(self.app, ui),
            Product::RenderShots => crate::render_workspace::shots::shared_shots(self.app, ui, context.flow, slot == ContentSlot::Inline),
        }
        let capture = ui.ctx().data_mut(|data| data.remove_temp::<Option<FocusCapture>>(focus_capture_id()).flatten());
        if context.document.as_ref().map(|document| document.0.as_str()) != Some(self.app.session.document_id.as_str()) {
            return Err(error(Rule::Availability, "application panel document changed while painting"));
        }
        let scope = LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() };
        if let Some(capture) = capture.filter(|_| !context.preview_geometry) {
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
