//! Assistant content for the shared panel and the standalone desktop window.

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::type_face::RichTextExt as _;
use crate::ui_kit::{Container, Edge, Surface};
use egui::{Align, Align2, Color32, Id, Layout, Rect, RichText, Sense, Stroke, pos2, vec2};
use solidcraft_engine::panels::Flow;

use super::*;
use crate::ui_kit;
use solidcraft_theme::Tokens;

const MIN_WIDTH: f32 = 280.0;

/// Standalone window placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentPanelPrefs {
    pub popped: bool,
    pub window: Option<[f32; 4]>,
}

impl AgentPanelPrefs {
    pub fn read(p: &crate::prefs::Prefs) -> Self {
        Self { popped: p.agent_chat_popped, window: p.agent_chat_window }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn write(&self, p: &mut crate::prefs::Prefs) {
        p.agent_chat_popped = self.popped;
        p.agent_chat_window = self.window;
    }
}

/// What the person asked of the chat while the panel was drawn. The panel only records these;
/// the host runs them after the frame's panel ([`run`]).
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    /// New chat, asking first when something would be lost.
    RequestNewChat,
    /// New chat, confirmed.
    NewChat,
    /// Look for installed agents again (desktop only, as discovery and agents are).
    #[cfg(not(target_arch = "wasm32"))]
    Refresh,
    #[cfg(not(target_arch = "wasm32"))]
    Connect,
    Disconnect,
    /// Talk to another agent (the current one is disconnected).
    SwitchAgent(String),
    /// Send a message; `steer` stops the agent first and sends the queue with it.
    Send {
        text: String,
        steer: bool,
    },
    /// Stop the agent and send the queued messages now.
    SendNow,
    Cancel,
    RemovePending(usize),
    /// Undo the changes of the message at this index.
    UndoChanges(usize),
    SetMode(String),
    SetModel(String),
    SetCustomModel(String),
    SetConfigOption {
        id: String,
        value: String,
    },
    /// Open the picker for a pill in the message, which goes at this character of the message box
    /// (its end when not given).
    StartPick {
        at: Option<usize>,
    },
    AttachDropped(Vec<std::path::PathBuf>),
    /// Choose files to attach, one or several.
    ChooseAttachment,
    /// Choose a folder to attach: the agent is told where it is and what it holds.
    ChooseFolder,
    /// Attach the clipboard's picture (desktop only, as the clipboard reader is).
    #[cfg(not(target_arch = "wasm32"))]
    PastePicture,
    /// Answer the permission request at this index with an option (none: cancel).
    AnswerPermission(usize, Option<String>),
    /// Allow (or refuse) a held request once.
    Approve(u64, bool),
    /// Allow a held request and its kind from now on.
    ApproveAlways(u64),
    /// What the agent (none: every agent without its own choice) may do without asking.
    SetPermission {
        agent: Option<String>,
        mode: agents::Mode,
    },
    /// Attach a picture pasted into the text box (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    AttachPicture(super::paste::Picture),
    /// The assistant's page in Preferences.
    OpenSettings,
    /// Preferences, adding an agent.
    AddAgent,
}

/// Run one thing the panel recorded.
pub fn run(app: &mut SolveApp, ctx: &egui::Context, action: PanelAction) {
    match action {
        PanelAction::RequestNewChat => request_new_chat(app),
        PanelAction::NewChat => new_chat(app),
        #[cfg(not(target_arch = "wasm32"))]
        PanelAction::Refresh => refresh(app, ctx),
        #[cfg(not(target_arch = "wasm32"))]
        PanelAction::Connect => connect(app, ctx),
        PanelAction::Disconnect => disconnect(app),
        PanelAction::SwitchAgent(id) => {
            disconnect(app);
            app.agent_chat.agent_id = Some(id);
        }
        PanelAction::Send { text, steer } => {
            send(app, ctx, text);
            if steer {
                send_now(app, ctx);
            }
        }
        PanelAction::SendNow => send_now(app, ctx),
        PanelAction::Cancel => cancel(app),
        PanelAction::RemovePending(n) => remove_pending(app, n),
        PanelAction::UndoChanges(n) => undo_changes(app, n),
        PanelAction::SetMode(m) => set_mode(app, &m),
        PanelAction::SetModel(m) => set_model(app, &m),
        PanelAction::SetCustomModel(m) => {
            set_custom_model(app, &m);
        }
        PanelAction::SetConfigOption { id, value } => set_config_option(app, &id, &value),
        PanelAction::StartPick { at } => start_pick(app, at),
        PanelAction::AttachDropped(paths) => attach_paths(app, paths.into_iter()),
        PanelAction::ChooseAttachment => app.pick_files(tr("ui.agent_chat_attachable"), &attach::extensions(), |app, paths| {
            attach_paths(app, paths.into_iter().map(std::path::PathBuf::from));
        }),
        PanelAction::ChooseFolder => app.pick_folder(|app, path| attach_paths(app, path.into_iter().map(std::path::PathBuf::from))),
        #[cfg(not(target_arch = "wasm32"))]
        PanelAction::PastePicture => {
            if let Err(e) = paste_picture(app) {
                app.agent_chat.notice(e, true);
            }
        }
        PanelAction::AnswerPermission(n, option) => answer_permission(app, n, option.as_deref()),
        PanelAction::Approve(id, allow) => approve(app, id, allow),
        PanelAction::ApproveAlways(id) => approve_always(app, id),
        PanelAction::SetPermission { agent: Some(id), mode } => {
            app.preferences.agent_permissions.insert(id, mode);
        }
        PanelAction::SetPermission { agent: None, mode } => app.preferences.ai_permissions = mode,
        #[cfg(not(target_arch = "wasm32"))]
        PanelAction::AttachPicture(p) => {
            if let Err(e) = attach_rgba(app, p.width, p.height, &p.rgba) {
                app.agent_chat.notice(e, true);
            }
        }
        PanelAction::OpenSettings => crate::prefs::open_at(app, crate::prefs::PAGE_ASSISTANT),
        PanelAction::AddAgent => super::settings::open_add(app),
    }
}

/// What the pointer is on in the chat, to light its target in the model: the panel says which pill
/// or chip, the host looks up what it stands for after the draw ([`light_hovered`]). View state, not
/// something asked: it lasts the frame.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hovered {
    /// A pill in the message box, by its number.
    Pill(u32),
    /// A pill (a numbered group) in a sent (`sent`) or queued message at `at`.
    Group { sent: bool, at: usize, group: u32 },
    /// A chip of a sent or queued message: the item's place in that message's context.
    Item { sent: bool, at: usize, item: usize },
}

/// Run what the panel recorded, in the order it was asked.
fn run_actions(app: &mut SolveApp, ctx: &egui::Context) {
    for action in std::mem::take(&mut app.agent_chat.actions) {
        run(app, ctx, action);
    }
}

/// What the panel shows of the app that hosts it, read when the panel is about to be drawn.
/// Everything the panel changes in the app goes back as a [`PanelAction`].
#[derive(Clone)]
pub struct PanelView {
    /// AI features are on: otherwise nothing of the chat is drawn.
    pub shown: bool,
    /// The app's layout (floating panels, and the frame a feature panel gets).
    /// The history bar when it is on the right: the floating panel stops above it.
    /// The open design's name, for the chat window's title.
    pub document: String,
    /// The open design's revision: a message's changes can be undone from the chat while they
    /// left this very design.
    pub revision: super::Revision,
    /// Length units and decimals for the summaries of what the agent did.
    pub units: String,
    pub decimals: u8,
    /// The agents to choose from, and the one this conversation uses.
    pub agents: Vec<agents::Entry>,
    pub current: Option<agents::Entry>,
    /// The agent the permissions picker sets (the running one, else the chosen one; none: the
    /// default), and what it may do.
    pub permission_agent: Option<String>,
    pub permission: agents::Mode,
    /// How many things are selected in the model.
    pub selected: usize,
    /// A tool or dialog is open (picking context waits for it).
    pub dialog_open: bool,
    /// Changes allowed in one turn (0: no limit).
    pub turn_limit: u32,
    /// Why the chosen model may not work, if it may not.
    pub model_warning: Option<String>,
    /// The chat buttons' tooltip: name and shortcut.
    /// The keys that show or hide the chat, and whether the keymap editor is taking keys.
    pub toggle_keys: Vec<String>,
    pub capturing_keys: bool,
    /// The interface reads right to left: a wide shared panel puts the conversation on the right.
    pub rtl: bool,
    /// The shared panel host is drawing and measuring this content: the agent controls scroll with
    /// the agent list, above a composer of the measured height.
    pub shared: bool,
    /// The composer's height the shared host measured last time, if it has.
    pub composer_height: Option<f32>,
}

impl PanelView {
    /// The view of SolidCraft's state the panel draws from now.
    pub fn of(app: &mut SolveApp, ctx: &egui::Context) -> Self {
        let current = agents::current(app);
        let permission_agent =
            if app.agent_chat.state().running() { Some(app.agent_chat.running_id.clone()) } else { current.as_ref().map(|e| e.id.clone()) };
        let permission = permission_agent.as_deref().map_or(app.preferences.ai_permissions, |id| agents::mode_for(&app.preferences, id));
        PanelView {
            shown: shown(app),
            document: app.session.doc.name.clone(),
            revision: super::Revision::of(&app.session.doc),
            units: app.session.doc.units.clone(),
            decimals: app.preferences.length_decimals,
            agents: agents::entries(app),
            current,
            permission_agent,
            permission,
            selected: selected_count(app),
            dialog_open: app.dialog.is_some(),
            turn_limit: app.preferences.agent_chat_turn_limit,
            model_warning: settings::model_warning(app),
            toggle_keys: crate::keymap::keys(app, "ui.assistant"),
            capturing_keys: crate::keymap::capturing(app),
            rtl: crate::i18n::panel_direction(&app.preferences.language) == solidcraft_engine::panels::Direction::Rtl,
            shared: crate::panel_runtime::shared_content_measurement(ctx),
            composer_height: crate::panel_runtime::shared_content_composer_height(ctx),
        }
    }
}

/// What the content tells the shared panel host it is drawn in: kept while the panel draws and
/// handed to the host after ([`hand_over`]).
#[derive(Clone, Default)]
pub struct ContentReport {
    /// The composer's height this frame.
    pub composer_height: Option<f32>,
    /// The layer the content was drawn on, the top of its body and the height it needs.
    pub extent: Option<(egui::LayerId, f32, f32)>,
    /// The text fields that take the keyboard, in the order they were drawn.
    pub focus: Vec<egui::Response>,
}

/// Give the shared panel host what the content reported while it was drawn in `ui` (nothing
/// happens outside the shared host's body).
fn hand_over(app: &mut SolveApp, ui: &egui::Ui) {
    let report = std::mem::take(&mut app.agent_chat.report);
    for response in &report.focus {
        crate::panel_runtime::register_content_focus(ui.ctx(), response);
    }
    if let Some(height) = report.composer_height {
        crate::panel_runtime::report_content_composer_height(ui.ctx(), height);
    }
    if let Some((layer, top, height)) = report.extent
        && layer == ui.layer_id()
    {
        crate::panel_runtime::report_content_extent(ui, top, height, false);
    }
}

/// Consume the configured toggle before CAD shortcuts, including in the chat's own window.
/// Bare character keys remain text while a field owns the keyboard; chords still hide chat.
pub fn toggle_shortcut(app: &mut SolveApp, ctx: &egui::Context) {
    if !shown(app) || crate::keymap::capturing(app) {
        return;
    }
    let keys = crate::keymap::keys(app, "ui.assistant");
    if toggle_pressed(ctx, &keys) {
        toggle(app);
    }
}

/// Take a press of the toggle's keys out of this frame's input.
fn toggle_pressed(ctx: &egui::Context, keys: &[String]) -> bool {
    let typing = ctx.egui_wants_keyboard_input();
    ctx.input_mut(|input| {
        let mut pressed = false;
        input.events.retain(|event| {
            if let egui::Event::Key { key, modifiers, pressed: true, repeat, .. } = event {
                if typing && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
                    return true;
                }
                if keys.contains(&crate::keymap::key_name(*key, *modifiers)) {
                    pressed |= !repeat;
                    return false;
                }
            }
            true
        });
        pressed
    })
}

#[cfg(not(target_arch = "wasm32"))]
const THUMB_MAX: (f32, f32) = (150.0, 56.0);
#[cfg(not(target_arch = "wasm32"))]
const PREVIEW_MAX: (f32, f32) = (560.0, 380.0);

/// Show or hide the panel (a folded panel unfolds first).
pub fn toggle(app: &mut SolveApp) {
    use solidcraft_engine::panels::{LayoutAction, Mutation};

    let shared = app.panel_runtime().filter(|_| !app.preferences.agent_chat_popped).and_then(|runtime| {
        let layout = runtime.layout().layout();
        let panel = layout.instances.iter().find(|panel| panel.kind.0 == "solidcraft.assistant")?;
        let section = layout.containing(&panel.id).ok();
        let reveal = !app.agent_chat.open || !panel.requested_open || section.is_some_and(|section| section.collapsed || section.active != panel.id);
        let mutations = if reveal {
            let section = section.map_or_else(|| panel.memory.section.clone(), |section| section.id.clone());
            let mut mutations = Vec::new();
            if !panel.requested_open {
                mutations.push(Mutation::Open { kind: panel.kind.clone(), instance: panel.id.clone(), destination: None });
            }
            mutations.push(Mutation::Collapse { section, collapsed: false });
            mutations.push(Mutation::Activate { instance: panel.id.clone() });
            mutations
        } else {
            vec![Mutation::Close { instance: panel.id.clone() }]
        };
        Some((reveal, LayoutAction::Apply { expected_revision: None, mutations }))
    });
    if let Some((reveal, action)) = shared {
        let was_open = app.agent_chat.open;
        if reveal {
            app.agent_chat.open = true;
        }
        if let Err(reason) = app.dispatch_panel_layout(action) {
            app.agent_chat.open = was_open;
            let _ = crate::panel_runtime::refresh_production(app);
            app.set_status(solidcraft_engine::panels::commands::error_text(reason), true);
        }
        return;
    }
    app.agent_chat.open = !app.agent_chat.open;
}

/// The tooltip of the chat's buttons: its name and shortcut.
pub fn tip(app: &SolveApp) -> String {
    match crate::keymap::display_for(app, "ui.assistant") {
        Some(key) => format!("{} ({key})", tr("ui.agent_chat")),
        None => tr("ui.agent_chat").to_string(),
    }
}

/// Selectable transcript labels do not take egui keyboard focus. Keep their keys out of
/// CAD until a pointer press outside the chat, not merely a move of the pointer. Use the
/// panel in this window, never label selections belonging to another native viewport.
/// Called before layout so even a click and a shortcut in the same frame are routed here.
pub(crate) fn owns_keyboard_input(app: &mut SolveApp, ctx: &egui::Context) -> bool {
    let visible =
        shown(app) && app.agent_chat.open && (app.preferences.agent_chat_popped || shared_section(app).is_some_and(|section| !section.collapsed));
    let rect = app.agent_chat.panel_rect.filter(|_| visible);
    let Some(rect) = rect else {
        app.agent_chat.keyboard_owned = false;
        return false;
    };
    ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::PointerButton { pos, pressed: true, .. } = event {
                app.agent_chat.keyboard_owned = rect.contains(*pos);
            }
        }
    });
    app.agent_chat.keyboard_owned
}

/// Draw the Assistant's standalone desktop window.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    poll(app, &ctx);
    let visible = shown(app) && app.agent_chat.open && app.preferences.agent_chat_popped;
    app.agent_chat.drop_rect = None;
    app.agent_chat.hovered = None;
    app.agent_chat.hover.clear();
    if !visible {
        app.agent_chat.panel_rect = None;
        app.agent_chat.window_placed = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if visible {
        new_chat_shortcut(&mut app.agent_chat, &ctx);
        if app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none() {
            refresh(app, &ctx);
        }
        refresh_gone(app);
        let view = PanelView::of(app, &ctx);
        let mut prefs = AgentPanelPrefs::read(&app.preferences);
        window(&mut app.agent_chat, &view, &ctx, &mut prefs);
        prefs.write(&mut app.preferences);
        hand_over(app, ui);
        light_hovered(app);
        show_drop_hint(&app.agent_chat, &ctx);
    }
    run_actions(app, &ctx);
}

/// Prepare the shared docked presentation without polling its retained service.
/// Read the previous visible rectangle before clearing this frame's geometry.
pub(crate) fn prepare_shared(app: &mut SolveApp, ctx: &egui::Context) {
    // The pill or chip the pointer is on is found again when the content is drawn. What it lit
    // stays until then: the shared host draws the model before its panels, so the model shows
    // what the content found last frame (`finish_shared` puts it out when the content is not drawn).
    app.agent_chat.hovered = None;
    if shown(app) {
        new_chat_shortcut(&mut app.agent_chat, ctx);
        run_actions(app, ctx);
        #[cfg(not(target_arch = "wasm32"))]
        if app.agent_chat.open && app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none() {
            refresh(app, ctx);
        }
    }
    app.agent_chat.panel_rect = None;
    app.agent_chat.drop_rect = None;
    app.agent_chat.window_placed = false;
}

/// Independent viewers and drop feedback keep their existing window policy.
pub(crate) fn finish_shared(app: &mut SolveApp, ctx: &egui::Context) {
    if !shown(app) || !app.agent_chat.open {
        app.agent_chat.keyboard_owned = false;
        app.agent_chat.hover.clear();
        return;
    }
    if app.agent_chat.panel_rect.is_none() {
        app.agent_chat.keyboard_owned = false;
        // Not drawn this frame: nothing in it is under the pointer.
        app.agent_chat.hover.clear();
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !shared_section(app).is_some_and(|section| section.location.zone() == solidcraft_engine::panels::Zone::Window) {
        gallery_window(&mut app.agent_chat, ctx);
    }
    show_drop_hint(&app.agent_chat, ctx);
}

/// Existing Assistant controls and conversation, inside the shared host's body.
pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui, flow: Flow) {
    if !shown(app) || !app.agent_chat.open {
        return;
    }
    refresh_gone(app);
    let view = PanelView::of(app, ui.ctx());

    let prefs = AgentPanelPrefs::read(&app.preferences);
    draw_content(&mut app.agent_chat, &view, &prefs, ui, flow);
    hand_over(app, ui);
    let ctx = ui.ctx().clone();
    #[cfg(not(target_arch = "wasm32"))]
    if ctx.viewport_id() != egui::ViewportId::ROOT {
        if ui.is_enabled() {
            window_input(&mut app.agent_chat, &ctx, ui.max_rect());
            gallery_window(&mut app.agent_chat, &ctx);
        }
        app.agent_chat.panel_rect = None;
        app.agent_chat.drop_rect = None;
        app.agent_chat.hovered = None;
        run_actions(app, &ctx);
        return;
    }
    light_hovered(app);
    run_actions(app, &ctx);
}

/// The controls and conversation alone, filling `ui` (the shared host's body, which has its own
/// header); what was asked is left in `chat.actions`, what the host measures in `chat.report`.
pub fn draw_content(chat: &mut AgentChat, view: &PanelView, prefs: &AgentPanelPrefs, ui: &mut egui::Ui, flow: Flow) {
    let rect = ui.max_rect().intersect(ui.clip_rect());
    if !view.shown || !chat.open || !rect.is_finite() || !rect.is_positive() {
        return;
    }
    chat.panel_rect = Some(rect);
    chat.drop_rect = Some(rect);
    solidcraft_ui_registry::publish_rect("agent_chat:panel", rect);
    ui_kit::scope(ui, |ui| content(chat, view, ui, flow, prefs));
}

fn shared_section(app: &SolveApp) -> Option<&solidcraft_engine::panels::Section> {
    let layout = app.panel_runtime()?.layout().layout();
    let instance = layout.instances.iter().find(|instance| instance.kind.0 == "solidcraft.assistant")?;
    let section = layout.containing(&instance.id).ok()?;
    (section.active == instance.id).then_some(section)
}

/// Keep feature editors clear of the Assistant when it occupies an outer side.
pub(crate) fn tool_space(app: &SolveApp, mut available: Rect) -> Rect {
    use solidcraft_engine::panels::{SectionLocation, Zone};

    let Some(section) = shared_section(app) else { return available };
    if !shown(app) || !app.agent_chat.open || app.preferences.agent_chat_popped || section.collapsed {
        return available;
    }
    let Some(chat) = app.agent_chat.panel_rect.filter(|r| r.is_finite() && r.is_positive()) else { return available };
    let gap = crate::ui_kit::SHELL_GAP;
    match section.location {
        SectionLocation::Dock { zone: Zone::Left } => {
            available.min.x = crate::ui_kit::within(chat.right() + gap, available.left(), available.right());
        }
        SectionLocation::Dock { zone: Zone::Right } => {
            available.max.x = crate::ui_kit::within(chat.left() - gap, available.left(), available.right());
        }
        _ => {}
    }
    available
}

/// The most chips one group shows before "+N more".
const MAX_CHIPS: usize = 6;
/// A chip's longest label (characters), and what its frame adds to the text's width.
const CHIP_CHARS: usize = 26;
const CHIP_PAD: f32 = 13.0;

/// One thing in a group's rows: its heading, a chip for an item, or the "+N more" note.
#[cfg(not(target_arch = "wasm32"))]
enum ChipPart<'a> {
    Heading(String),
    /// An item and its place among all of the context's items.
    Chip(usize, &'a context::Entry),
    More(String),
}

/// What was selected when a message was sent, grouped (elements, features, bodies) with a count
/// each and a chip per item; hovering a chip tells what is known about it and lights it in the
/// model. `gone` flags the items that have left the design (struck through); each chip is
/// published as `{handle}:chip:{place}`. The rows are filled by hand: egui wraps plain widgets,
/// not framed ones, and one row wider than the chat would widen the whole panel.
#[cfg(not(target_arch = "wasm32"))]
fn context_chips(ui: &mut egui::Ui, context: &Context, gone: &[bool], handle: &str) -> ChipEvents {
    let mut events = ChipEvents::default();
    if context.is_empty() {
        return events;
    }
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_small);
    let (avail, gap) = (ui.available_width(), ui.spacing().item_spacing.x);
    let text_width = |ui: &egui::Ui, text: &str| ui.painter().layout_no_wrap(text.to_string(), font.clone(), t.text).size().x;
    let more = |n: usize| trf("ui.agent_chat_ctx_more", &[("n", &n.to_string())]);
    for kind in context::Kind::ALL {
        let items: Vec<(usize, &context::Entry)> = context.items.iter().enumerate().filter(|(_, i)| i.kind == kind).collect();
        if items.is_empty() {
            continue;
        }
        let mut parts = vec![ChipPart::Heading(format!("{} {}", tr(kind.heading()), items.len()))];
        parts.extend(items.iter().take(MAX_CHIPS).map(|(n, i)| ChipPart::Chip(*n, i)));
        if items.len() > MAX_CHIPS {
            parts.push(ChipPart::More(more(items.len() - MAX_CHIPS)));
        }
        let widths: Vec<f32> = parts
            .iter()
            .map(|p| match p {
                ChipPart::Heading(s) | ChipPart::More(s) => text_width(ui, s),
                ChipPart::Chip(_, i) => text_width(ui, &shorten(&i.label, CHIP_CHARS)) + CHIP_PAD,
            })
            .collect();
        let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
        let mut used = 0.0_f32;
        for (n, w) in widths.iter().enumerate() {
            let fits = used + if used > 0.0 { gap } else { 0.0 } + w <= avail;
            if !fits && used > 0.0 {
                rows.push(Vec::new());
                used = 0.0;
            }
            used += if used > 0.0 { gap } else { 0.0 } + w;
            if let Some(row) = rows.last_mut() {
                row.push(n);
            }
        }
        for row in rows {
            ui.horizontal(|ui| {
                for n in row {
                    match &parts[n] {
                        ChipPart::Heading(s) | ChipPart::More(s) => {
                            ui.label(RichText::new(s).color(t.text_dim).size(t.font_small));
                        }
                        ChipPart::Chip(place, item) => {
                            let is_gone = gone.get(*place).copied().unwrap_or(false);
                            let chip = egui::Frame::NONE
                                .stroke(Stroke::new(1.0, t.text_dim.gamma_multiply(0.5)))
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(5, 1))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.horizontal(|ui| {
                                        let mut text = RichText::new(shorten(&item.label, CHIP_CHARS)).color(t.text).size(t.font_small);
                                        if is_gone {
                                            text = text.color(t.text_dim).strikethrough();
                                        }
                                        ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend))
                                    })
                                    .inner
                                })
                                .response;
                            solidcraft_ui_registry::publish_handle(&format!("{handle}:chip:{place}"), chip.rect.center());
                            // Hovering a chip lights its target in the model.
                            if chip.hovered() {
                                events.hovered = Some(*place);
                            }
                            let tip = match (is_gone, item.detail.is_empty()) {
                                (true, true) => tr("ui.agent_chat_ctx_gone").to_string(),
                                (true, false) => format!("{}\n{}", tr("ui.agent_chat_ctx_gone"), item.detail),
                                (false, _) => item.detail.clone(),
                            };
                            if !tip.is_empty() {
                                chip.backing_on_hover_text(tip);
                            }
                        }
                    }
                }
            });
        }
    }
    if context.omitted > 0 {
        ui.label(RichText::new(more(context.omitted)).color(t.text_dim).size(t.font_small));
    }
    events
}

/// What happened to a row of chips this frame: the pointer is on one (its place in the context).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct ChipEvents {
    hovered: Option<usize>,
}

/// `s` cut to `max` characters, with an ellipsis.
#[cfg(not(target_arch = "wasm32"))]
fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

/// The pictures of a message as small thumbnails in a row. Hovering one shows it bigger for as long
/// as the pointer stays; a click returns its place in the row, to open the viewer there.
#[cfg(not(target_arch = "wasm32"))]
fn picture_strip(ui: &mut egui::Ui, n: usize, pictures: &[Picture]) -> Option<usize> {
    let t = Tokens::get();
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for (i, picture) in pictures.iter().enumerate() {
            let texture = match picture.texture(ui.ctx()) {
                Shown::Ready(texture) => texture,
                // Being decoded: a spinner the size of a thumbnail's corner, until it is in.
                Shown::Loading => {
                    ui.add(egui::Spinner::new().size(t.space_24));
                    continue;
                }
                // Not a readable picture: its name, or the plain caption.
                Shown::Failed => {
                    let caption = if picture.note.is_empty() { tr("ui.agent_chat_view_attached").to_string() } else { picture.note.clone() };
                    ui.label(RichText::new(caption).color(t.text_dim).size(t.font_small));
                    continue;
                }
            };
            let size = fit(texture.size_vec2(), vec2(THUMB_MAX.0, THUMB_MAX.1));
            let thumb = ui.add(egui::Image::new((texture.id(), size)).corner_radius(t.radius_sm).sense(Sense::click()));
            solidcraft_ui_registry::publish_handle(&format!("agent_chat:picture:{n}:{i}"), thumb.rect.center());
            let hover_max = (ui.ctx().content_rect().size() * 0.5).min(vec2(PREVIEW_MAX.0, PREVIEW_MAX.1));
            let thumb = thumb.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_ui(|ui| {
                ui.add(egui::Image::new((texture.id(), fit(texture.size_vec2(), hover_max))).corner_radius(t.radius_sm));
                if !picture.note.is_empty() {
                    ui.label(&picture.note);
                }
            });
            if thumb.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// `size` scaled to fit in `max`, keeping its shape (never below one point).
#[cfg(not(target_arch = "wasm32"))]
fn fit(size: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    let scale = (max.x / size.x.max(1.0)).min(max.y / size.y.max(1.0)).min(2.0);
    vec2((size.x * scale).max(1.0), (size.y * scale).max(1.0))
}

/// The viewer: a message's pictures over the app, one at a time with a counter, Previous and Next
/// (also the arrow keys), closed with the button, Esc or a click outside.
#[cfg(not(target_arch = "wasm32"))]
fn gallery_window(chat: &mut AgentChat, ctx: &egui::Context) {
    let Some(mut gallery) = chat.gallery.take() else { return };
    let count = gallery.pictures.len();
    gallery.at = gallery.at.min(count.saturating_sub(1));
    let (left, right) =
        ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)));
    let mut step: isize = if left { -1 } else { isize::from(right) };
    let t = Tokens::get();
    let max = ctx.content_rect().size() * vec2(0.8, 0.7);
    let modal = ui_kit::Container::new(ctx, ui_kit::Surface::Window).modal(Id::unique("sc_agent_chat_gallery")).show(ctx, |ui| {
        ui.set_max_width(max.x);
        ui.horizontal(|ui| {
            let prev = ui_kit::button(ui, tr("ui.agent_chat_previous_picture"), ui_kit::Button::Enabled(count > 1));
            solidcraft_ui_registry::publish_handle("agent_chat:gallery_prev", prev.rect.center());
            let next = ui_kit::button(ui, tr("ui.agent_chat_next_picture"), ui_kit::Button::Enabled(count > 1));
            solidcraft_ui_registry::publish_handle("agent_chat:gallery_next", next.rect.center());
            if prev.clicked() {
                step = -1;
            }
            if next.clicked() {
                step = 1;
            }
            ui.label(RichText::new(format!("{} / {}", gallery.at + 1, count)).color(t.text_dim));
        });
        if let Some(picture) = gallery.pictures.get(gallery.at) {
            match picture.texture(ui.ctx()) {
                Shown::Ready(texture) => {
                    let resp = ui.add(egui::Image::new((texture.id(), fit(texture.size_vec2(), max))).corner_radius(t.radius_sm));
                    solidcraft_ui_registry::publish_rect("agent_chat:gallery_picture", resp.rect);
                }
                Shown::Loading => {
                    ui.add(egui::Spinner::new().size(t.space_24));
                }
                Shown::Failed => {
                    ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim));
                }
            }
            if !picture.note.is_empty() {
                ui.add(egui::Label::new(&picture.note).wrap());
            }
        }
    });
    if count > 0 && step != 0 {
        gallery.at = (gallery.at as isize + step).rem_euclid(count as isize) as usize;
    }
    if !modal.should_close() {
        chat.gallery = Some(gallery);
    }
}

/// While files are dragged over the window: if they would drop on the chat, say so over it.
fn show_drop_hint(chat: &AgentChat, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(rect) = chat.drop_rect
        && ctx.input(|i| !i.raw.hovered_files.is_empty())
        && drop_is_ours(chat, ctx)
    {
        drop_hint(ctx, rect);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (chat, ctx);
}

/// Where dragged files will go: a tinted zone over the chat with a line of text.
#[cfg(not(target_arch = "wasm32"))]
fn drop_hint(ctx: &egui::Context, area: Rect) {
    let t = Tokens::get();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("sc_agent_chat_drop_hint")));
    let zone = area.shrink(6.0);
    painter.rect(zone, t.radius_xl, t.accent.gamma_multiply(0.16), Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    let font = egui::FontId::proportional(Tokens::get().font_heading);
    let text = painter.layout(tr("ui.agent_chat_drop_hint").to_string(), font, t.text, (zone.width() - 24.0).max(40.0));
    let at = zone.center() - text.size() / 2.0;
    painter.galley(at, text, t.text);
    solidcraft_ui_registry::publish_rect("agent_chat:drop_hint", zone);
}

/// New chat (when the chat has focus).
pub const NEW_CHAT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), egui::Key::L);

/// Clear the chat, asking first when something would be lost.
pub(crate) fn request_new_chat(app: &mut SolveApp) {
    if new_chat_needs_confirm(app) {
        app.agent_chat.confirm_new = true;
    } else {
        new_chat(app);
    }
}

fn new_chat_shortcut(chat: &mut AgentChat, ctx: &egui::Context) {
    let in_chat = ctx.memory(|m| m.has_focus(Id::unique("sc_agent_chat_input")))
        || ctx.pointer_hover_pos().is_some_and(|p| chat.panel_rect.is_some_and(|r| r.contains(p)));
    if chat.open && in_chat && ctx.input_mut(|i| i.consume_shortcut(&NEW_CHAT_SHORTCUT)) {
        chat.actions.push(PanelAction::RequestNewChat);
    }
}

/// A saved window place (`[x, y, width, height]`) that can be used: finite, a usable size, and
/// not absurdly far away. `x` and `y` may be negative: a screen left of or above the main one.
pub fn window_place(saved: Option<[f32; 4]>) -> Option<([f32; 2], [f32; 2])> {
    let [x, y, w, h] = saved?;
    let usable = [x, y, w, h].iter().all(|v| v.is_finite())
        && (MIN_WIDTH..=10_000.0).contains(&w)
        && (200.0..=10_000.0).contains(&h)
        && x.abs() <= 100_000.0
        && y.abs() <= 100_000.0;
    usable.then_some(([x, y], [w, h]))
}

/// The chat window's settings. The saved place and size go only to a window that is opening:
/// egui turns a changed place into a move, and the system rounds what it is given, so giving it
/// every frame made the window chase its own rounding (it shook a pixel up and down).
pub fn window_builder(document: &str, placed: bool, prefs: &AgentPanelPrefs) -> egui::ViewportBuilder {
    let title = format!("{document} — {}", tr("ui.agent_chat"));
    let builder = egui::ViewportBuilder::default().with_title(title).with_min_inner_size([MIN_WIDTH, 320.0]);
    if placed {
        return builder;
    }
    match window_place(prefs.window) {
        Some((pos, size)) => builder.with_position(pos).with_inner_size(size),
        None => builder.with_inner_size([380.0, 720.0]),
    }
}

/// The chat in its own window, named after the design: a native window on the desktop, which can
/// go to another screen, or a window floating inside the app where the system can't open one.
/// The system's close button closes the chat (it opens there again); Dock puts it back.
#[cfg(not(target_arch = "wasm32"))]
fn window(chat: &mut AgentChat, view: &PanelView, ctx: &egui::Context, prefs: &mut AgentPanelPrefs) {
    let builder = window_builder(&view.document, chat.window_placed, prefs);
    chat.window_placed = true;
    // The side panel isn't drawn: its place no longer counts as "in the chat".
    chat.panel_rect = None;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("sc_agent_chat_window"), builder, |ui, class| {
        // Root/embedded inputs are handled by SolveApp::logic. A native chat window owns
        // its input and must see the toggle before its text editor can consume the chord.
        // The window holds the panel, not the shared host, so the toggle works on the panel's
        // own state (the copy being drawn from).
        if class != egui::ViewportClass::EmbeddedWindow && view.shown && !view.capturing_keys && toggle_pressed(ui.ctx(), &view.toggle_keys) {
            chat.open = !chat.open;
        }
        let t = Tokens::get();
        let frame = Container::new(ui.ctx(), Surface::Panel).edge_frame(t.dialog_body);
        let panel = Container::new(ui.ctx(), Surface::Viewport).central();
        let r = Container::show_central(ui, panel, frame, |ui| ui_kit::scope(ui, |ui| body(chat, view, ui, prefs)));
        // The picture viewer opens over this window, where the click was, not over the main one.
        gallery_window(chat, ui.ctx());
        if class == egui::ViewportClass::EmbeddedWindow {
            // Floating inside the app: its handles are the main window's, as the panel's are. The
            // app's own drop handling sees the drops, so the window counts as the chat's place
            // (the hint is drawn by the caller).
            solidcraft_ui_registry::publish_rect("agent_chat:window", r.response.rect);
            chat.panel_rect = Some(r.response.rect);
            chat.drop_rect = Some(r.response.rect);
            return;
        }
        let ctx = ui.ctx().clone();
        window_input(chat, &ctx, r.response.rect);
        let (outer, inner, close) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
        if let (Some(o), Some(n)) = (outer, inner) {
            prefs.window = Some([o.min.x, o.min.y, n.width(), n.height()]);
        }
        if close {
            chat.open = false;
        }
    });
}

/// A detached Assistant owns its file drops and keyboard input in that viewport.
#[cfg(not(target_arch = "wasm32"))]
fn window_input(chat: &mut AgentChat, ctx: &egui::Context, area: Rect) {
    let dropped = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
    if !dropped.is_empty() {
        chat.actions.push(PanelAction::AttachDropped(dropped.iter().map(|file| file.path().to_path_buf()).collect()));
    }
    if ctx.input(|input| !input.raw.hovered_files.is_empty()) {
        drop_hint(ctx, area);
    }
    if ctx.input_mut(|input| input.consume_shortcut(&NEW_CHAT_SHORTCUT)) {
        chat.actions.push(PanelAction::RequestNewChat);
    }
}

/// Whether the chat's own window should become active because the pointer is over its text box:
/// only when it is in a window of its own that isn't active and the app's main window is (never
/// taking the focus from another app).
pub fn take_focus_for_text(popped: bool, over_text: bool, chat_active: Option<bool>, main_active: Option<bool>) -> bool {
    popped && over_text && chat_active == Some(false) && main_active == Some(true)
}

/// A small drawn button for the standalone window.
#[cfg(not(target_arch = "wasm32"))]
fn glyph_button(ui: &mut egui::Ui, kind: &str, tip: &str) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let c = r.center();
    let s = Stroke::new(1.4, t.text);
    let p = ui.painter();
    match kind {
        "close" => {
            p.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], s);
            p.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
        }
        // A window with its right side filled: the panel docked.
        "dock" => {
            p.rect_stroke(Rect::from_center_size(c, vec2(11.0, 10.0)), 1.0, s, egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_min_max(c + vec2(1.5, -5.0), c + vec2(5.5, 5.0)), 0.0, t.text);
        }
        "newchat" => {
            // A speech bubble with a plus.
            ui.painter().rect_stroke(Rect::from_center_size(c, vec2(14.0, 11.0)), 2.5, s, egui::StrokeKind::Inside);
            ui.painter().line_segment([c + vec2(-2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.2, t.text));
            ui.painter().line_segment([c + vec2(0.0, -2.5), c + vec2(0.0, 2.5)], Stroke::new(1.2, t.text));
        }
        _ => {}
    }
    solidcraft_ui_registry::publish_rect(&format!("agent_chat:{kind}"), r);
    solidcraft_ui_registry::publish_handle(&format!("agent_chat:{kind}"), r.center());
    resp.backing_on_hover_text(tip)
}

/// The standalone Assistant window title and actions.
#[cfg(not(target_arch = "wasm32"))]
fn header(chat: &mut AgentChat, ui: &mut egui::Ui, prefs: &mut AgentPanelPrefs) {
    let t = Tokens::get();
    let state = chat.state();
    ui_kit::assistant_header(ui, |ui| {
        ui.horizontal(|ui| {
            ui.allocate_exact_size(vec2(0.0, t.space_16 + t.space_2), Sense::hover());
            ui.label(RichText::new(tr("ui.assistant")).semibold().color(t.text).size(t.font_heading));
            let waiting = chat.waiting();
            // Ready without SolidCraft's tools can't reach the design: say so, and how to fix it (#12283).
            let without_design = waiting == 0 && chat.without_design();
            let s = if waiting > 0 {
                tr("ui.agent_chat_waiting").to_string()
            } else if without_design {
                tr("ui.agent_chat_state_no_design").to_string()
            } else {
                state.label().to_string()
            };
            if ui.available_width() > t.space_24 * 9.0 {
                let badge = ui_kit::assistant_badge(
                    ui,
                    &s,
                    if waiting > 0 || without_design {
                        ui_kit::AssistantTone::Warning
                    } else {
                        match state {
                            State::Ready => ui_kit::AssistantTone::Success,
                            State::Busy => ui_kit::AssistantTone::Working,
                            State::NeedsAuth => ui_kit::AssistantTone::Warning,
                            State::Exited | State::Failed => ui_kit::AssistantTone::Error,
                            _ => ui_kit::AssistantTone::Neutral,
                        }
                    },
                );
                if without_design {
                    badge.backing_on_hover_text(chat.no_design_hint());
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if glyph_button(ui, "close", tr("ui.agent_chat_close")).clicked() {
                    chat.open = false;
                }
                if glyph_button(
                    ui,
                    "newchat",
                    &format!("{} ({})", tr("ui.agent_chat_new"), if cfg!(target_os = "macos") { "Cmd+Shift+L" } else { "Ctrl+Shift+L" }),
                )
                .clicked()
                {
                    chat.actions.push(PanelAction::RequestNewChat);
                }
                if glyph_button(ui, "dock", tr("ui.agent_chat_dock")).clicked() {
                    prefs.popped = false;
                }
            });
        });
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn body(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, prefs: &mut AgentPanelPrefs) {
    header(chat, ui, prefs);
    content(chat, view, ui, Flow::Tall, prefs);
}

fn content(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, flow: Flow, prefs: &AgentPanelPrefs) {
    #[cfg(target_arch = "wasm32")]
    let _ = (chat, view, flow, prefs);
    #[cfg(not(target_arch = "wasm32"))]
    if !view.shared {
        ui_kit::assistant_controls(ui, |ui| switcher(chat, view, ui));
    }
    #[cfg(target_arch = "wasm32")]
    {
        ui.label(tr("ui.agent_chat_desktop_only"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop_body(chat, view, ui, flow, prefs);
}

#[cfg(not(target_arch = "wasm32"))]
fn desktop_body(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, flow: Flow, prefs: &AgentPanelPrefs) {
    let top = ui.cursor().top();
    let shared = view.shared;

    let state = chat.state();
    let bounds = ui.available_rect_before_wrap();
    let gap = Tokens::get().space_8;
    // Compact wide cards keep the existing stack until both regions fit.
    let wide = shared && flow == Flow::Wide && bounds.width() >= MIN_WIDTH * 2.0 + gap;
    let (interaction_bounds, transcript_bounds) = if wide {
        let width = (bounds.width() - gap) / 2.0;
        let left = Rect::from_min_size(bounds.min, vec2(width, bounds.height()));
        let right = Rect::from_min_max(pos2(bounds.right() - width, bounds.top()), bounds.max);
        if view.rtl { (left, Some(right)) } else { (right, Some(left)) }
    } else {
        (bounds, None)
    };
    let root = ui.scope_id();
    // Orientation changes rectangles, never the retained region identities.
    let mut interaction = shared.then(|| {
        let mut child = ui.new_child(egui::UiBuilder::new().scope_id(root.with("assistant-interaction")).max_rect(interaction_bounds));
        child.set_clip_rect(ui.clip_rect().intersect(interaction_bounds));
        child
    });
    let (top_height, outcome, remaining) = {
        let column = interaction.as_mut().unwrap_or(&mut *ui);
        let top_height = top_controls(chat, view, column, state, shared);
        let outcome = composer(chat, view, column, state, prefs);
        (top_height, outcome, column.available_rect_before_wrap())
    };
    let mut transcript = shared.then(|| {
        let bounds = transcript_bounds.unwrap_or(remaining);
        let mut child = ui.new_child(egui::UiBuilder::new().scope_id(root.with("assistant-transcript")).max_rect(bounds));
        child.set_clip_rect(ui.clip_rect().intersect(bounds));
        child
    });
    let transcript_height = transcript_body(chat, view, transcript.as_mut().unwrap_or(&mut *ui));
    let interaction_height = top_height + outcome.height;
    let height = if wide { interaction_height.max(transcript_height) } else { interaction_height + transcript_height };
    chat.report.extent = Some((ui.layer_id(), top, height));
    finish_composer(chat, outcome);
}

#[cfg(not(target_arch = "wasm32"))]
fn new_chat_confirmation(chat: &mut AgentChat, ui: &mut egui::Ui) {
    if chat.confirm_new {
        ui_kit::scope(ui, |ui| {
            inset(ui, |ui| {
                ui.set_width(ui.available_width());
                ui_kit::text(ui, tr("ui.agent_chat_new_confirm"), ui_kit::Text::Report("new_chat", false));
                ui.horizontal(|ui| {
                    let yes = ui_kit::button(ui, tr("ui.agent_chat_new"), ui_kit::Button::Normal);
                    solidcraft_ui_registry::publish_target("agent_chat:new_confirm", yes.rect);
                    if yes.clicked() {
                        chat.actions.push(PanelAction::NewChat);
                    }
                    if ui_kit::button(ui, tr("ui.agent_chat_cancel"), ui_kit::Button::Normal).clicked() {
                        chat.confirm_new = false;
                    }
                });
            });
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn top_controls(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, state: State, shared: bool) -> f32 {
    let top = ui.cursor().top();
    let reveal_confirmation = shared && {
        let id = ui.scope_id().with("assistant-confirm-visible");
        let visible = chat.confirm_new;
        ui.data_mut(|data| {
            let previous = data.get_temp::<bool>(id).unwrap_or(false);
            data.insert_temp(id, visible);
            visible && !previous
        })
    };
    if !shared {
        new_chat_confirmation(chat, ui);
    }
    // Shared top controls must yield scroll space to the pinned composer, including
    // when discovery lists several agents or a long message grows the editor. The
    // sign-in box scrolls in the same place in either layout (#15087), and may use
    // everything above the composer: there is no conversation to show until it is done.
    let needs_auth = state == State::NeedsAuth;
    let picker_max = if shared || needs_auth {
        let t = Tokens::get();
        let composer = view.composer_height.unwrap_or(48.0 + t.control_height * 3.0 + t.space_8 * 2.0 + t.space_6 * 3.0);
        (ui.available_height() - composer - ui.spacing().item_spacing.y).max(0.0)
    } else {
        ui.available_height() / 3.0
    };
    let provider_scroll = egui::ScrollArea::vertical()
        .id_salt("sc_agent_chat_picker_scroll")
        .min_scrolled_height(0.0)
        .max_height(picker_max)
        .auto_shrink([false, true]);
    let provider_scroll = if reveal_confirmation { provider_scroll.vertical_scroll_offset(0.0) } else { provider_scroll };
    let picker = provider_scroll.show(ui, |ui| {
        solidcraft_ui_registry::publish_rect("agent_chat:providers", ui.clip_rect());
        if shared {
            new_chat_confirmation(chat, ui);
            ui_kit::assistant_controls(ui, |ui| switcher(chat, view, ui));
        }
        picker(chat, view, ui);
        if needs_auth {
            inset(ui, |ui| auth_box(chat, ui));
        }
    });
    ui.cursor().top() - top - picker.inner_rect.height().min(picker.content_size.y) + picker.content_size.y
}

#[cfg(not(target_arch = "wasm32"))]
struct ComposerOutcome {
    height: f32,
    send_clicked: bool,
    stop_clicked: bool,
    enter: bool,
    steer: bool,
    esc: bool,
}

#[cfg(not(target_arch = "wasm32"))]
fn composer(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, state: State, prefs: &AgentPanelPrefs) -> ComposerOutcome {
    // The input sits at the bottom of the panel and grows upwards: a long message makes its box
    // taller up to `edit_max`, then it scrolls, and the conversation takes what is left above. It
    // is a panel of its own at the bottom, as high as its content this frame, so the buttons under
    // the box are always in view.
    let edit_max = (ui.available_height() * 0.4).clamp(48.0, 160.0);
    let busy = state == State::Busy;
    // Enter sends (queues while the agent works); Ctrl/Cmd+Enter sends now; Esc stops. They are
    // read before the field sees them so no newline is typed.
    let focused = ui.memory(|m| m.has_focus(input_id()));
    let (steer, esc) = if focused {
        ui.input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter), busy && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
    } else {
        (false, false)
    };
    let (mut send_clicked, mut stop_clicked, mut enter, mut pick_clicked) = (false, false, false, false);
    let frame = Container::new(ui.ctx(), Surface::Viewport).edge_frame(egui::Color32::TRANSPARENT);
    let composer = Container::new(ui.ctx(), Surface::Viewport).edge("sc_agent_chat_composer", Edge::Bottom);
    let composer = composer.resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let footer = ui_kit::assistant_composer_footer(ui, |ui| {
            let boxed = ui_kit::assistant_composer(ui, focused, |ui| {
                // The files for the next message (#1744), as chips of their own.
                attachment_chips(chat, ui);
                // Ctrl/Cmd+V: files and pictures on the clipboard attach; text goes to the box below.
                #[cfg(not(target_arch = "wasm32"))]
                if focused && let Some(action) = super::paste::handle(chat, ui) {
                    chat.actions.push(action);
                }
                let r = egui::ScrollArea::vertical()
                    .id_salt("sc_agent_chat_input_scroll")
                    .max_height(edit_max)
                    // Grow the bottom panel to the editor's scroll limit instead of letting
                    // its previous short-message height clip the action rows below it.
                    .min_scrolled_height(edit_max)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        pill_input(chat, view, ui, focused, tr(if busy { "ui.agent_chat_placeholder_busy" } else { "ui.agent_chat_placeholder" }))
                    })
                    .inner;
                solidcraft_ui_registry::publish_rect("agent_chat:input", r.rect);
                chat.report.focus.push(r.clone());
                // macOS shows a window's cursors only while it is the active window: over the text box of a
                // chat window that isn't, the pointer stays an arrow. Coming from the main window, the chat
                // window becomes active, so the text cursor shows and the first click places the caret.
                let (chat_active, main_active) =
                    (ui.ctx().input(|i| i.viewport().focused), ui.ctx().input_for(egui::ViewportId::ROOT, |i| i.viewport().focused));
                if take_focus_for_text(prefs.popped, r.hovered(), chat_active, main_active) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), Tokens::get().control_height + Tokens::get().space_2),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_height(Tokens::get().control_height + Tokens::get().space_2);
                        add_menu(chat, ui);
                        let mode = view.permission;
                        let permissions = ui
                            .allocate_ui(vec2(Tokens::get().space_24 * 4.0, Tokens::get().control_height), |ui| {
                                ui_kit::dropdown_menu(
                                    ui,
                                    "sc_agent_permissions",
                                    tr(match mode {
                                        agents::Mode::Ask => "ui.agent_chat_mode_ask",
                                        agents::Mode::Edits => "ui.agent_chat_mode_edits",
                                        agents::Mode::All => "ui.agent_chat_mode_all",
                                    }),
                                    |ui| {
                                        for choice in agents::Mode::ALL {
                                            if ui_kit::list_picker(ui, choice.id(), choice.label(), mode == choice).clicked() {
                                                chat.actions.push(PanelAction::SetPermission { agent: view.permission_agent.clone(), mode: choice });
                                            }
                                        }
                                    },
                                )
                            })
                            .inner;
                        let permissions = permissions.backing_on_hover_text(mode.label());
                        solidcraft_ui_registry::publish_rect("agent_chat:permissions", permissions.rect);
                        solidcraft_ui_registry::publish_handle("agent_chat:permissions", permissions.rect.center());
                        // A message waits for files still being read.
                        let reading = !chat.loading.is_empty();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let action = ui_kit::assistant_send(
                                ui,
                                busy || !reading,
                                busy,
                                tr(if busy { "ui.agent_chat_stop_turn_tip" } else { "ui.agent_chat_send" }),
                            );
                            if busy {
                                solidcraft_ui_registry::publish_handle("agent_chat:stopturn", action.rect.center());
                                stop_clicked = action.clicked();
                                let queued = ui_kit::button(ui, tr("ui.agent_chat_queue"), ui_kit::Button::Enabled(!reading));
                                solidcraft_ui_registry::publish_rect("agent_chat:send", queued.rect);
                                solidcraft_ui_registry::publish_handle("agent_chat:send", queued.rect.center());
                                send_clicked = queued.clicked();
                            } else {
                                solidcraft_ui_registry::publish_rect("agent_chat:send", action.rect);
                                solidcraft_ui_registry::publish_handle("agent_chat:send", action.rect.center());
                                send_clicked = action.clicked();
                            }
                        });
                    },
                );
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), Tokens::get().control_height),
                    Layout::left_to_right(Align::Center).with_main_wrap(true),
                    |ui| {
                        ui.set_min_height(Tokens::get().control_height);
                        // A picture of the view goes with the message to agents that take pictures.
                        let images = chat.images || !state.running();
                        let attach = ui
                            .add_enabled_ui(images, |ui| ui_kit::checkbox(ui, tr("ui.agent_chat_attach_view"), &mut chat.attach_view))
                            .inner
                            .backing_on_hover_text(tr("ui.agent_chat_attach_view_tip"))
                            .backing_on_disabled_hover_text(tr("ui.agent_chat_no_images"));
                        solidcraft_ui_registry::publish_handle("agent_chat:attach", attach.rect.center());
                        // What is selected in the model goes with the message: text for the agent,
                        // chips in the conversation.
                        let selected = view.selected;
                        let label = if selected > 0 {
                            format!("{} ({selected})", tr("ui.agent_chat_attach_selection"))
                        } else {
                            tr("ui.agent_chat_attach_selection").to_string()
                        };
                        let pick =
                            ui_kit::checkbox(ui, &label, &mut chat.attach_selection).backing_on_hover_text(tr("ui.agent_chat_attach_selection_tip"));
                        solidcraft_ui_registry::publish_handle("agent_chat:attach_selection", pick.rect.center());
                        // Choose faces, edges, features and bodies on purpose, then OK.
                        let picker = ui_kit::button(ui, tr("ui.agent_chat_pick_context"), ui_kit::Button::Enabled(!view.dialog_open))
                            .backing_on_hover_text(tr("ui.agent_chat_pick_context_tip"));
                        solidcraft_ui_registry::publish_handle("agent_chat:pick_context", picker.rect.center());
                        pick_clicked = picker.clicked();
                        if let Some(turn) = &chat.turn {
                            let n = turn.changes.to_string();
                            let count = match view.turn_limit {
                                0 => trf("ui.agent_chat_changes_n", &[("n", &n)]),
                                limit => trf("ui.agent_chat_changes_of", &[("n", &n), ("limit", &limit.to_string())]),
                            };
                            ui.label(RichText::new(count).color(Tokens::get().text_dim).size(Tokens::get().font_small));
                        }
                    },
                );
            });
            solidcraft_ui_registry::publish_rect("agent_chat:composer_box", boxed.response.rect);
        });
        solidcraft_ui_registry::publish_rect("agent_chat:composer_footer", footer.response.rect);
        // The enclosing bottom panel can clamp its response to the remaining
        // viewport. Reserve the actual footer content on the next frame.
        footer.response.rect.height()
    });
    chat.report.composer_height = Some(composer.inner);
    if pick_clicked {
        // The pill goes where the caret is in the box.
        chat.actions.push(PanelAction::StartPick { at: caret(ui.ctx()) });
    }
    ComposerOutcome { height: composer.inner, send_clicked, stop_clicked, enter, steer, esc }
}

#[cfg(not(target_arch = "wasm32"))]
fn transcript_body(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui) -> f32 {
    // Select All and Copy of the whole conversation, read before its labels see the keys.
    super::select_all::handle(chat, ui.ctx());
    let conversation = egui::ScrollArea::vertical()
        .id_salt("sc_agent_chat_scroll")
        .min_scrolled_height(0.0)
        .max_height(ui.available_height())
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::same(Tokens::get().space_12 as i8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = Tokens::get().space_10;
                conversation(chat, view, ui);
            });
            // Last, so the scroll goes to this area and not to one nested in the conversation.
            scroll_while_selecting(ui);
        });
    solidcraft_ui_registry::publish_rect("agent_chat:conversation", conversation.inner_rect);
    conversation.content_size.y
}

#[cfg(not(target_arch = "wasm32"))]
fn finish_composer(chat: &mut AgentChat, outcome: ComposerOutcome) {
    let ComposerOutcome { send_clicked, stop_clicked, enter, steer, esc, .. } = outcome;
    if esc || stop_clicked {
        chat.actions.push(PanelAction::Cancel);
    }
    // A message waits for files still being read, and files alone make a message.
    let reading = !chat.loading.is_empty();
    let something = !chat.input.trim().is_empty() || !chat.attachments.is_empty();
    if reading {
        // Enter waits with the button.
    } else if (send_clicked || enter || steer) && something {
        let text = std::mem::take(&mut chat.input).trim_end_matches('\n').to_string();
        chat.actions.push(PanelAction::Send { text, steer });
    } else if steer {
        // Nothing typed: Ctrl+Enter sends what is already queued.
        chat.actions.push(PanelAction::SendNow);
    }
}

/// The header's second row: the agent this conversation uses (a compact switcher), the agent's
/// own mode when it has modes, and the state of its login.
#[cfg(not(target_arch = "wasm32"))]
fn switcher(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let all = &view.agents;
    let current = view.current.clone();
    // An advertised (even empty) config list replaces the older ACP model/mode pickers.
    let modern = chat.config_options.is_some();
    ui.horizontal_wrapped(|ui| {
        let label = current.as_ref().map_or_else(
            || tr(if chat.detecting.is_some() { "ui.agent_chat_detecting" } else { "ui.agent_chat_none_found_short" }).to_string(),
            |e| e.name.clone(),
        );
        let models = !modern && chat.models_shown();
        let providers = if models { chat.model_providers() } else { Vec::new() };
        let w =
            (ui.available_width() - if modern || chat.modes.is_empty() { 4.0 } else { 116.0 } - if models { 146.0 } else { 0.0 }).clamp(90.0, 260.0);
        let mut picked: Option<String> = None;
        let mut add = false;
        let combo = ui
            .allocate_ui(vec2(w, t.control_height), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui_kit::dropdown_menu(ui, "sc_agent_switch", &label, |ui| {
                    for e in all {
                        let sel = current.as_ref().is_some_and(|c| c.id == e.id);
                        let r = ui_kit::list_picker(ui, &e.id, &e.name, sel).backing_on_hover_text(format!("{} {}", e.detail(), e.login));
                        solidcraft_ui_registry::publish_handle(&format!("agent_chat:agent:{}", e.id), r.rect.center());
                        if r.clicked() {
                            picked = Some(e.id.clone());
                        }
                    }
                    ui.separator();
                    if ui_kit::list_picker(ui, "add", tr("ui.ai_add_agent"), false).clicked() {
                        add = true;
                    }
                    if ui_kit::list_picker(ui, "settings", tr("ui.ai_settings"), false).clicked() {
                        chat.actions.push(PanelAction::OpenSettings);
                    }
                })
            })
            .inner;
        solidcraft_ui_registry::publish_rect("agent_chat:switcher", combo.rect);
        solidcraft_ui_registry::publish_handle("agent_chat:switcher", combo.rect.center());
        combo.backing_on_hover_text(tr("ui.ai_switch_tip"));
        if add {
            chat.actions.push(PanelAction::AddAgent);
        }
        if let Some(id) = picked
            && current.as_ref().is_none_or(|c| c.id != id)
        {
            chat.actions.push(PanelAction::SwitchAgent(id));
        }
        // The agent's own modes (ACP `modes`): its own tools follow them.
        if !modern && !chat.modes.is_empty() {
            let modes = chat.modes.clone();
            let cur = modes.iter().find(|(id, _)| *id == chat.mode).map_or_else(|| chat.mode.clone(), |(_, n)| n.clone());
            let mut pick: Option<String> = None;
            let r = ui
                .allocate_ui(vec2(t.form_label_width, t.control_height), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui_kit::dropdown_menu(ui, "sc_agent_mode", &cur, |ui| {
                        for (id, name) in &modes {
                            if ui_kit::list_picker(ui, id, name, *id == chat.mode).clicked() {
                                pick = Some(id.clone());
                            }
                        }
                    })
                })
                .inner;
            solidcraft_ui_registry::publish_rect("agent_chat:mode", r.rect);
            solidcraft_ui_registry::publish_handle("agent_chat:mode", r.rect.center());
            r.backing_on_hover_text(tr("ui.ai_agent_mode_tip"));
            if let Some(m) = pick {
                chat.actions.push(PanelAction::SetMode(m));
            }
        }
        // The agent's models (ACP `models`, or an endpoint's list): shown only when it reports some.
        if models {
            // Models from several providers fold under their providers inside the picker, the
            // current model's provider open.
            let list = chat.grouped_models();
            let open = chat.shown_provider().filter(|_| !providers.is_empty());
            let cur = list.iter().find(|(m, _, _, _)| m.id == chat.model).map_or_else(|| chat.model.clone(), |(_, name, _, _)| name.clone());
            let width = if providers.is_empty() { t.form_label_width } else { picker_width(ui, &cur) };
            let choices: Vec<ui_kit::SearchChoice<'_>> =
                list.iter().map(|(m, name, group, tip)| ui_kit::SearchChoice { value: &m.id, label: name, tip, group: group.as_deref() }).collect();
            let custom = chat.custom_models_accepted();
            let picker = ui
                .allocate_ui(vec2(width, t.control_height), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui_kit::search_picker(
                        ui,
                        ui_kit::SearchPicker {
                            id: "sc_agent_model",
                            handles: "agent_chat:model",
                            current: &cur,
                            current_value: &chat.model,
                            hint: super::model_search_hint(custom),
                            choices: &choices,
                            custom,
                            note: chat.models_note.as_deref(),
                            open_group: open.as_deref(),
                            enabled: true,
                        },
                    )
                })
                .inner;
            picker.response.backing_on_hover_text(tr("ui.agent_chat_model_tip"));
            match picker.inner {
                Some(ui_kit::Picked::Listed(m)) => chat.actions.push(PanelAction::SetModel(m)),
                Some(ui_kit::Picked::Custom(m)) => chat.actions.push(PanelAction::SetCustomModel(m)),
                None => {}
            }
        }
    });
    if let Some(warning) = &view.model_warning {
        ui.add(egui::Label::new(RichText::new(warning).color(t.warning).size(t.font_small)).wrap());
    }
    if let Some(options) = chat.config_options.clone() {
        config_selects(chat, ui, &options);
    }
    ui.horizontal(|ui| {
        let b = if chat.state().running() {
            ui_kit::button(ui, tr("ui.agent_chat_stop"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.agent_chat_stop_tip"))
        } else {
            let label = if chat.state() == State::Failed { "ui.agent_chat_retry" } else { "ui.agent_chat_connect" };
            ui_kit::button(ui, tr(label), ui_kit::Button::Normal)
        };
        solidcraft_ui_registry::publish_handle("agent_chat:connect", b.rect.center());
        if b.clicked() {
            chat.actions.push(if chat.state().running() { PanelAction::Disconnect } else { PanelAction::Connect });
        }
    });
    if let Some(a) = &chat.auth_status {
        ui.label(RichText::new(a).color(t.text_dim).size(solidcraft_theme::Tokens::get().font_small));
    }
}

#[cfg(not(target_arch = "wasm32"))]
const FLATPAK_OVERRIDE: &str = "flatpak override --user io.tangencylabs.solidcraft --filesystem=~/.nvm:ro --filesystem=~/.local/bin:ro --filesystem=~/.local/share/claude:ro --filesystem=~/.claude --filesystem=~/.claude.json --share=network";

/// The strictly confined Snap can neither see nor start the agents installed on the computer, and
/// no permission the user can grant changes that (#1458).
#[cfg(not(target_arch = "wasm32"))]
fn snap() -> bool {
    crate::update::is_snap(&|k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
}

/// A sandbox hides the agents installed on the computer, so an empty list says why.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn sandboxed() -> bool {
    std::env::var_os("FLATPAK_ID").is_some() || snap()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn no_agents_message() -> String {
    if std::env::var_os("FLATPAK_ID").is_some() {
        tr("ui.agent_chat_flatpak_none_found")
    } else if snap() {
        tr("ui.agent_chat_snap_none_found")
    } else {
        tr("ui.agent_chat_none_found")
    }
    .to_string()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn no_agents(ui: &mut egui::Ui) {
    let flatpak = std::env::var_os("FLATPAK_ID").is_some();
    let style = if sandboxed() { ui_kit::Text::Report("none", false) } else { ui_kit::Text::Note("none") };
    ui_kit::text(ui, &no_agents_message(), style);
    if flatpak {
        // The Copy button comes before the long command so it shows without scrolling.
        let copy = ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small);
        solidcraft_ui_registry::publish_handle("agent_chat:flatpak_override", copy.rect.center());
        if copy.clicked() {
            ui.ctx().copy_text(FLATPAK_OVERRIDE.to_string());
        }
        ui_kit::text(ui, FLATPAK_OVERRIDE, ui_kit::Text::Report("flatpak_override", false));
    }
}

/// The width of a provider or model picker: its label's, from the usual picker width up to 200
/// points and never wider than the panel, so a short name is read whole and a long one truncates.
#[cfg(not(target_arch = "wasm32"))]
fn picker_width(ui: &egui::Ui, label: &str) -> f32 {
    let t = Tokens::get();
    let label: String = label.chars().take(64).collect();
    let text = ui.painter().layout_no_wrap(label, egui::FontId::proportional(t.font_label), t.text).size().x + 34.0;
    let most = ui.max_rect().width().clamp(t.form_label_width, 200.0);
    text.clamp(t.form_label_width, most)
}

/// An agent's select options are authoritative. Compact value pickers share a strip below the
/// agent switcher; only a narrow chat wraps them, rather than spending two full rows per option.
#[cfg(not(target_arch = "wasm32"))]
fn config_selects(chat: &mut AgentChat, ui: &mut egui::Ui, options: &[acp::ConfigOption]) {
    let t = Tokens::get();
    let pending = chat.config_pending;
    let strip_width = ui.available_width().min(ui.max_rect().width()).max(0.0);
    let mut row = Vec::new();
    let mut used = 0.0;
    for option in options {
        let current = option.options.iter().find(|choice| choice.value == option.current_value);
        let selected = current.map_or(option.current_value.as_str(), |choice| choice.name.as_str());
        let preview = compact_text(selected, 32);
        let preferred = ui.painter().layout_no_wrap(preview, egui::FontId::proportional(t.font_label), t.text).size().x + 34.0;
        let width = preferred.clamp(76.0, strip_width.clamp(76.0, 200.0));
        let spacing = if row.is_empty() { 0.0 } else { ui.spacing().item_spacing.x };
        if !row.is_empty() && used + spacing + width > strip_width {
            ui.horizontal(|ui| {
                for (option, width) in row.drain(..) {
                    config_select(chat, ui, option, width, pending);
                }
            });
            used = 0.0;
        }
        used += if used == 0.0 { width } else { spacing + width };
        row.push((option, width));
    }
    if !row.is_empty() {
        ui.horizontal(|ui| {
            for (option, width) in row {
                config_select(chat, ui, option, width, pending);
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn config_select(chat: &mut AgentChat, ui: &mut egui::Ui, option: &acp::ConfigOption, width: f32, pending: bool) {
    let t = Tokens::get();
    let current = option.options.iter().find(|choice| choice.value == option.current_value);
    let selected = current.map_or(option.current_value.as_str(), |choice| choice.name.as_str());
    // Bound both the measurement and the visible label: agent-provided text may be huge.
    // Width follows the current value, leaving short choices room to sit on one line.
    let font = egui::FontId::proportional(t.font_label);
    let mut chars = 32;
    let displayed = loop {
        let text = compact_text(selected, chars);
        if chars == 1 || ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text).size().x <= width - 36.0 {
            break text;
        }
        chars -= 1;
    };
    let name = if option.name.is_empty() { &option.id } else { &option.name };
    let tip = [name.as_str(), option.description.as_str(), current.map_or("", |choice| choice.description.as_str())]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut picked = None;
    let enabled = !pending && !option.options.is_empty();
    let handles = format!("agent_chat:config:{}", option.id);
    let response = if option.category == "model" {
        let choices: Vec<ui_kit::SearchChoice<'_>> = option
            .options
            .iter()
            .map(|choice| ui_kit::SearchChoice {
                value: &choice.value,
                label: &choice.name,
                tip: &choice.description,
                group: choice.group.as_deref(),
            })
            .collect();
        let id = format!("sc_agent_config:{}", option.id);
        let picker = ui
            .allocate_ui(vec2(width, t.control_height), |ui| {
                ui_kit::search_picker(
                    ui,
                    ui_kit::SearchPicker {
                        id: &id,
                        handles: &handles,
                        current: &displayed,
                        current_value: &option.current_value,
                        hint: super::model_search_hint(true),
                        choices: &choices,
                        custom: true,
                        note: None,
                        open_group: None,
                        enabled,
                    },
                )
            })
            .inner;
        picked = picker.inner;
        // Under the names every kind of agent's model picker goes by.
        let rows = format!("{handles}:");
        for (name, at) in solidcraft_ui_registry::handles(&rows) {
            if let Some(row) = name.strip_prefix(&rows) {
                solidcraft_ui_registry::publish_handle(&format!("agent_chat:model:{row}"), at);
            }
        }
        solidcraft_ui_registry::publish_target("agent_chat:model", picker.response.rect);
        picker.response
    } else {
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                ui.allocate_ui(vec2(width, t.control_height), |ui| {
                    ui_kit::dropdown_menu(ui, ("sc_agent_config", &option.id), &displayed, |ui| {
                        let mut previous_group: Option<&str> = None;
                        for choice in &option.options {
                            let group = choice.group.as_deref().filter(|g| !g.is_empty());
                            if let Some(group) = group
                                && previous_group != Some(group)
                            {
                                ui.label(RichText::new(compact_text(group, 32)).color(t.text_dim).size(t.font_small)).backing_on_hover_text(group);
                            }
                            previous_group = group;
                            let row = ui_kit::list_picker(ui, &choice.value, &compact_text(&choice.name, 32), choice.value == option.current_value);
                            let row = row.backing_on_hover_text(
                                [choice.name.as_str(), choice.description.as_str()]
                                    .into_iter()
                                    .filter(|part| !part.is_empty())
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            );
                            solidcraft_ui_registry::publish_handle(&format!("{handles}:{}", choice.value), row.rect.center());
                            if row.clicked() {
                                picked = Some(ui_kit::Picked::Listed(choice.value.clone()));
                            }
                        }
                    })
                })
                .inner
            })
            .inner;
        solidcraft_ui_registry::publish_target(&handles, response.rect);
        if option.category == "mode" {
            solidcraft_ui_registry::publish_handle("agent_chat:mode", response.rect.center());
        }
        response
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, format!("{name}: {selected}")));
    response.backing_on_hover_text(tip);
    match picked {
        Some(ui_kit::Picked::Listed(value)) => chat.actions.push(PanelAction::SetConfigOption { id: option.id.clone(), value }),
        Some(ui_kit::Picked::Custom(value)) => chat.actions.push(PanelAction::SetCustomModel(value)),
        None => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn compact_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() { format!("{short}…") } else { short }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod config_ui_tests {
    use super::*;

    fn option(id: &str, category: &str, choices: &[(&str, &str, Option<&str>)]) -> acp::ConfigOption {
        acp::ConfigOption {
            id: id.into(),
            name: id.into(),
            description: format!("Choose {id}"),
            category: category.into(),
            current_value: choices.first().map_or("", |choice| choice.0).into(),
            options: choices
                .iter()
                .map(|(value, name, group)| acp::ConfigChoice {
                    value: (*value).into(),
                    name: (*name).into(),
                    description: format!("Description for {name}"),
                    group: group.map(str::to_owned),
                })
                .collect(),
        }
    }

    fn draw_switcher(app: &mut SolveApp, ctx: &egui::Context, width: f32) {
        solidcraft_ui_registry::clear_handles();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 700.0));
        let view = PanelView::of(app, ctx);
        ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            ui.allocate_ui(vec2(width - 16.0, 600.0), |ui| {
                ui_kit::scope(ui, |ui| switcher(&mut app.agent_chat, &view, ui));
            });
        })
        .textures_delta
        .clear();
    }

    #[test]
    fn config_options_share_one_row_when_they_fit_and_wrap_without_legacy_duplicates() {
        assert_eq!(compact_text("Énergie très élevée", 7), "Énergie…");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        // Make legacy provider/model pickers eligible so their absence is meaningful.
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.modes = vec![("legacy".into(), "Legacy mode".into())];
        app.agent_chat.models = vec![
            acp::ModelInfo { id: "old".into(), name: "North · Old model".into(), description: String::new() },
            acp::ModelInfo { id: "other".into(), name: "South · Other model".into(), description: String::new() },
        ];
        assert!(models_shown(&app));
        assert_eq!(model_providers(&app).len(), 2);
        app.agent_chat.config_options = Some(vec![
            option(
                "engine",
                "model",
                &[("fast", "Énergie très élevée pour une conception détaillée", Some("Standard")), ("large", "Large", Some("Advanced"))],
            ),
            option("thought", "thought_level", &[("low", "Max", None), ("high", "High", None)]),
            option("workflow", "mode", &[("plan", "Build", None)]),
        ]);
        let ctx = crate::fonts::test_context();
        draw_switcher(&mut app, &ctx, 760.0);
        let at = |name| solidcraft_ui_registry::handle_at(name).expect("config picker handle");
        let agent = at("agent_chat:switcher");
        let model = at("agent_chat:config:engine");
        let effort = at("agent_chat:config:thought");
        let mode = at("agent_chat:config:workflow");
        assert!(agent.y < model.y && (model.y - effort.y).abs() < 2.0 && (effort.y - mode.y).abs() < 2.0);
        assert!(model.x < effort.x && effort.x < mode.x && mode.x < 760.0);
        assert_eq!(at("agent_chat:model"), model);
        assert_eq!(at("agent_chat:mode"), mode);
        assert_eq!(solidcraft_ui_registry::handles("agent_chat:mode").iter().filter(|(name, _)| name == "agent_chat:mode").count(), 1);
        assert_eq!(solidcraft_ui_registry::handles("agent_chat:model").iter().filter(|(name, _)| name == "agent_chat:model").count(), 1);
        assert!(solidcraft_ui_registry::handle_at("agent_chat:provider").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:model:old").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:model:other").is_none());

        draw_switcher(&mut app, &ctx, 280.0);
        let model = at("agent_chat:config:engine");
        let effort = at("agent_chat:config:thought");
        let mode = at("agent_chat:config:workflow");
        assert!(model.y < effort.y && (effort.y - mode.y).abs() < 2.0, "only the long model needs its own line: {model:?}, {effort:?}, {mode:?}");
        for id in ["engine", "thought", "workflow"] {
            let rect = solidcraft_ui_registry::rect_of(&format!("agent_chat:config:{id}")).expect("config control bounds");
            assert!(rect.left() >= 0.0 && rect.right() <= 280.0, "{id} exceeds the narrow panel: {rect:?}");
        }

        app.agent_chat.config_options = Some(vec![option("engine", "model", &[("fast", "Fast", None)])]);
        app.agent_chat.config_pending = true;
        draw_switcher(&mut app, &ctx, 280.0);
        assert!(solidcraft_ui_registry::handle_at("agent_chat:config:engine").is_some());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:config:thought").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:config:workflow").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:switcher").is_some());

        app.agent_chat.config_options = Some(vec![]);
        draw_switcher(&mut app, &ctx, 280.0);
        assert!(solidcraft_ui_registry::handle_at("agent_chat:switcher").is_some());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:config:engine").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:mode").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:provider").is_none());
        assert!(solidcraft_ui_registry::handle_at("agent_chat:model").is_none());
    }
}

/// What is drawn under the agent switcher keeps the switcher's side margin (#10172): it starts
/// where the switcher's fields start, not at the panel's edge.
#[cfg(not(target_arch = "wasm32"))]
fn inset<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(Tokens::get().space_12 as i8, 0)).show(ui, add).inner
}

/// Before the first connection: what's installed, how to install the rest, and Add agent.
#[cfg(not(target_arch = "wasm32"))]
fn picker(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui) {
    let open = chat.picking.unwrap_or(!chat.state().running() && chat.items.is_empty());
    if !open {
        return;
    }
    inset(ui, |ui| agent_list(chat, view, ui));
}

/// [`picker`]'s content: the agents found, Refresh and Add agent, and what isn't installed.
#[cfg(not(target_arch = "wasm32"))]
fn agent_list(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui) {
    let all = &view.agents;
    let current = view.current.clone();
    ui.horizontal(|ui| {
        ui_kit::text(ui, tr("ui.agent_chat_pick"), ui_kit::Text::Inline);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let refreshing = chat.detecting.is_some();
            if ui_kit::button(ui, tr("ui.agent_chat_refresh"), ui_kit::Button::Enabled(!refreshing)).clicked() {
                chat.actions.push(PanelAction::Refresh);
            }
            if ui_kit::button(ui, tr("ui.ai_add_agent"), ui_kit::Button::Enabled(true)).clicked() {
                chat.actions.push(PanelAction::AddAgent);
            }
        });
    });
    if chat.detecting.is_some() {
        ui_kit::text(ui, tr("ui.agent_chat_detecting"), ui_kit::Text::Note("detecting"));
    }
    for e in all {
        let selected = current.as_ref().is_some_and(|c| c.id == e.id);
        let resp = ui_kit::list_picker(ui, &e.id, &e.name, selected).backing_on_hover_text(format!("{} {}", e.detail(), e.login));
        solidcraft_ui_registry::publish_handle(&format!("agent_chat:pick:{}", e.id), resp.rect.center());
        if resp.clicked() {
            chat.agent_id = Some(e.id.clone());
        }
    }
    // In a sandbox the hint already says nothing was found, so the install list below is
    // headed "Not installed" instead of repeating it. The Snap can't start what the list
    // installs, so it has no list.
    let sandbox_hint = all.is_empty() && chat.detecting.is_none() && sandboxed();
    if sandbox_hint {
        no_agents(ui);
    }
    let missing: Vec<discover::Missing> = if snap() { Vec::new() } else { chat.detection.as_ref().map(|d| d.missing.clone()).unwrap_or_default() };
    if !missing.is_empty() {
        let heading = if all.is_empty() && !sandbox_hint { tr("ui.agent_chat_none_found") } else { tr("ui.agent_chat_not_installed") };
        ui_kit::section(ui, "sc_agent_missing", heading, all.is_empty(), |ui| {
            for m in missing {
                ui.horizontal(|ui| {
                    ui_kit::text(ui, m.name, ui_kit::Text::Inline);
                    if ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small).backing_on_hover_text(m.install).clicked() {
                        ui.ctx().copy_text(m.install.to_string());
                    }
                });
                ui_kit::text(ui, m.install, ui_kit::Text::Report(m.name, false));
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn auth_box(chat: &mut AgentChat, ui: &mut egui::Ui) {
    ui_kit::section(ui, "authentication", tr("ui.agent_chat_auth_title"), true, |ui| {
        ui.set_width(ui.available_width());
        ui_kit::text(ui, tr("ui.agent_chat_auth_needed"), ui_kit::Text::Note("auth_needed"));
        let methods = chat.auth.clone();
        let base = chat.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        if methods.is_empty() {
            ui_kit::text(ui, tr("ui.agent_chat_auth_none"), ui_kit::Text::Note("auth_none"));
        }
        for m in methods {
            ui.add_space(solidcraft_theme::Tokens::get().space_4);
            let desc = m.description.trim();
            ui_kit::text(
                ui,
                &if desc.is_empty() || desc == m.name { m.name.clone() } else { format!("{} — {desc}", m.name) },
                ui_kit::Text::Report(&m.id, false),
            );
            match &m.kind {
                acp::AuthKind::Terminal { args, .. } => {
                    ui_kit::text(ui, tr("ui.agent_chat_auth_terminal"), ui_kit::Text::Note("terminal"));
                    let cmd = format!("{base} {}", args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
                    ui.horizontal_wrapped(|ui| {
                        ui_kit::text(ui, &cmd, ui_kit::Text::Report("auth_command", false));
                        if ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small).clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    });
                }
                acp::AuthKind::Agent => {
                    let b = ui_kit::button(ui, trf("ui.agent_chat_auth_agent", &[("name", &m.name)]), ui_kit::Button::Normal);
                    solidcraft_ui_registry::publish_handle(&format!("agent_chat:auth:{}", m.id), b.rect.center());
                    if b.clicked() {
                        chat.authenticate(&m.id);
                    }
                }
                acp::AuthKind::EnvVar { vars } => {
                    ui_kit::text(ui, &trf("ui.agent_chat_auth_env", &[("vars", &vars.join(", "))]), ui_kit::Text::Report("auth_environment", false));
                }
            }
        }
        ui.add_space(solidcraft_theme::Tokens::get().space_4);
        let retry = ui_kit::button(ui, tr("ui.agent_chat_auth_retry"), ui_kit::Button::Normal);
        solidcraft_ui_registry::publish_handle("agent_chat:auth_retry", retry.rect.center());
        if retry.clicked() {
            chat.actions.push(PanelAction::Connect);
        }
    });
}

/// The visible **Copy message** button of one transcript message (#10675): copies the whole
/// message — the prompt as entered, or the assistant's original Markdown — and nothing else.
/// A streaming reply copies the stable text received so far, which is what `text` holds.
#[cfg(not(target_arch = "wasm32"))]
fn copy_message_button(ui: &mut egui::Ui, n: usize, text: &str) {
    let b = ui_kit::button(ui, tr("ui.agent_chat_copy_message"), ui_kit::Button::Small);
    solidcraft_ui_registry::publish_handle(&format!("agent_chat:copy:{n}"), b.rect.center());
    if b.clicked() {
        ui.ctx().copy_text(text.to_owned());
    }
}

/// When a message or tool call came in, small and dim ("3 min ago"); its exact time on hover.
#[cfg(not(target_arch = "wasm32"))]
fn time_label(ui: &mut egui::Ui, n: usize, stamp: &times::Stamp, now: f64) {
    let (Some(ago), Some(exact)) = (times::ago(stamp, now), times::exact(stamp)) else { return };
    let t = Tokens::get();
    let r = ui.add(egui::Label::new(RichText::new(ago).size(t.font_small).color(t.text_dim)).selectable(false)).backing_on_hover_text(exact);
    crate::scenario::publish_rect(&format!("agent_chat:time:{n}"), r.rect);
}

/// After the last item of an answer: how long it took ("Answered in 23 min").
#[cfg(not(target_arch = "wasm32"))]
fn took_line(ui: &mut egui::Ui, stamp: &times::Stamp) {
    if let Some(text) = times::took(stamp) {
        let t = Tokens::get();
        ui.label(RichText::new(text).size(t.font_small).color(t.text_dim));
    }
}

/// The right-click menu of one transcript message (#10675): the same single action as the
/// visible button, on the whole message area, beside the composer's own text menu.
/// The area stays hover-only so partial text selection keeps working, so the menu opens on a
/// right-click released over it rather than on a click the response itself would report.
#[cfg(not(target_arch = "wasm32"))]
fn copy_message_menu(ui: &egui::Ui, response: &egui::Response, n: usize, text: &str) {
    solidcraft_ui_registry::publish_rect(&format!("agent_chat:msg:{n}"), response.rect);
    let opened = ui
        .input(|i| i.pointer.button_released(egui::PointerButton::Secondary) && i.pointer.interact_pos().is_some_and(|p| response.rect.contains(p)));
    let mut copy = false;
    Container::new(ui.ctx(), Surface::Popup)
        .menu(response)
        .at_pointer_fixed()
        .frame(ui_kit::Menu::frame())
        .open_memory(if opened { Some(egui::SetOpenCommand::Bool(true)) } else { None })
        .show(|ui| {
            ui_kit::Menu::show(ui, crate::ui_kit::LIST_W, |ui| {
                let item = ui_kit::MenuItem::new("copy_message", tr("ui.agent_chat_copy_message"), "copy");
                let row = item.show(ui);
                solidcraft_ui_registry::publish_handle(&format!("agent_chat:copy_menu:{n}"), row.rect.center());
                if row.clicked() {
                    copy = true;
                    ui.close();
                }
            });
        });
    if copy {
        ui.ctx().copy_text(text.to_owned());
    }
}

/// A text selection dragged near or past the top or bottom of the conversation scrolls it, faster
/// the further out, so a long answer can be selected in one drag (#11527). Only a drag towards
/// that edge does: a click, a double click or a held press on a line by the edge, or a drag from
/// there into the conversation, scrolls nothing. `ui`: the conversation's scroll area, once its
/// content is drawn.
#[cfg(not(target_arch = "wasm32"))]
fn scroll_while_selecting(ui: &egui::Ui) {
    // Scrolling starts this far inside the edge, and goes this many points a second for each
    // point further out, up to four times as far out; once the pointer has moved more than
    // `TOWARDS` that way since the press.
    const EDGE: f32 = 32.0;
    const SPEED: f32 = 16.0;
    const TOWARDS: f32 = 8.0;
    let view = ui.clip_rect();
    let pointer = ui.input(|i| i.pointer.press_origin().filter(|p| i.pointer.primary_down() && view.contains(*p)).zip(i.pointer.latest_pos()));
    let Some((from, at)) = pointer else {
        return;
    };
    if !ui.ctx().plugin::<egui::text_selection::LabelSelectionState>().lock().has_selection() {
        return;
    }
    let content = ui.min_rect();
    let out = if at.y < view.top() + EDGE && from.y - at.y > TOWARDS && content.top() < view.top() - 0.5 {
        at.y - (view.top() + EDGE)
    } else if at.y > view.bottom() - EDGE && at.y - from.y > TOWARDS && content.bottom() > view.bottom() + 0.5 {
        at.y - (view.bottom() - EDGE)
    } else {
        return;
    };
    let step = out.clamp(-4.0 * EDGE, 4.0 * EDGE) * SPEED * ui.input(|i| i.stable_dt).min(0.1);
    // A positive delta moves the content down, showing what is above.
    ui.scroll_with_delta_animation(vec2(0.0, -step), egui::style::ScrollAnimation::none());
}

#[cfg(not(target_arch = "wasm32"))]
fn conversation(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut answer: Option<(usize, Option<String>)> = None;
    // Allow (true), Deny (false), Always allow (None).
    let mut approval: Option<(u64, Option<bool>)> = None;
    let mut undo: Option<usize> = None;
    let mut open_gallery: Option<Gallery> = None;
    // The pill or chip under the pointer, for the host to light.
    let mut hovered: Option<Hovered> = None;
    let current = &view.revision;
    let units = summary::Units { units: &view.units, decimals: view.decimals };
    let all_selected = chat.all_selected;
    let now = crate::now_ms();
    let busy = chat.state() == State::Busy;
    for (n, item) in chat.items.iter().enumerate() {
        let stamp = chat.stamp(n);
        match item {
            Item::User { text, image, pictures, context, files } => {
                // A picture-only prompt has no text to copy: no action rather than an empty copy (#10675).
                let copyable = !text.is_empty();
                let before = ui.cursor();
                ui_kit::assistant_bubble(ui, |ui| {
                    let gone = chat.gone.sent.get(&n).map_or(&[][..], Vec::as_slice);
                    if !text.is_empty()
                        && let Some(group) = user_text(ui, text, context, gone, &format!("agent_chat:sent:{n}"), all_selected)
                    {
                        hovered = Some(Hovered::Group { sent: true, at: n, group });
                    }
                    if !pictures.is_empty() {
                        if let Some(at) = picture_strip(ui, n, pictures) {
                            open_gallery = Some(Gallery { pictures: pictures.clone(), at });
                        }
                    } else if *image {
                        // Taken when the agent starts on the message: nothing to show yet.
                        ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim).size(t.font_small));
                    }
                    // The picks that are pills in the text light up from there; chips are for the rest.
                    let (rest, rest_flags, places) = unplaced(context, gone);
                    if let Some(place) = context_chips(ui, &rest, &rest_flags, &format!("agent_chat:sent:{n}")).hovered
                        && let Some(item) = places.get(place)
                    {
                        hovered = Some(Hovered::Item { sent: true, at: n, item: *item });
                    }
                    files_line(ui, files.iter().map(String::as_str));
                    if copyable || stamp.known() {
                        // Top-aligned: a row centred across its height takes all the height left in
                        // the conversation's view, which stretched a prompt near the top.
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if copyable {
                                copy_message_button(ui, n, text);
                            }
                            time_label(ui, n, &stamp, now);
                        });
                    }
                });
                if copyable {
                    let drawn = egui::Rect::from_x_y_ranges(before.x_range(), before.min.y..=ui.cursor().min.y);
                    let resp = ui.interact(drawn, Id::unique(("agent_chat_msg", n)), Sense::hover());
                    copy_message_menu(ui, &resp, n, text);
                }
            }
            Item::Agent { thought, text } => {
                if *thought {
                    super::markdown_view::selectable(ui, egui::Label::new(RichText::new(text).italics().color(t.text_dim)).wrap(), false);
                } else {
                    let before = ui.cursor();
                    ui_kit::assistant_prose(ui, |ui| super::markdown_view::show(ui, n, text, all_selected));
                    if !text.is_empty() {
                        ui.horizontal(|ui| {
                            copy_message_button(ui, n, text);
                            time_label(ui, n, &stamp, now);
                        });
                        let drawn = egui::Rect::from_x_y_ranges(before.x_range(), before.min.y..=ui.cursor().min.y);
                        let resp = ui.interact(drawn, Id::unique(("agent_chat_msg", n)), Sense::hover());
                        copy_message_menu(ui, &resp, n, text);
                    }
                }
            }
            Item::Tool { .. } => {
                if n > 0 && matches!(chat.items.get(n - 1), Some(Item::Tool { .. })) {
                    continue;
                }
                let cards = chat.items.iter().skip(n).take_while(|next| matches!(next, Item::Tool { .. })).count();
                ui_kit::assistant_tool_frame(ui, false, |ui| {
                    ui.spacing_mut().item_spacing.y = Tokens::get().space_2;
                    for (offset, next) in chat.items.iter().skip(n).take(cards).enumerate() {
                        if let Item::Tool { title, kind, status, input, output, .. } = next {
                            let words = summary::summary(title, input.as_ref(), output.as_deref(), *status == Some(ToolStatus::Completed), units);
                            let call = ToolCall {
                                title,
                                summary: words.as_deref(),
                                kind,
                                status: *status,
                                input: input.as_ref(),
                                output: output.as_deref(),
                                stamp: chat.stamp(n + offset),
                                now,
                                live: busy,
                            };
                            tool_card(ui, n + offset, &call);
                        }
                    }
                });
                // The answer's time goes after the cards when they end it.
                for k in n..n + cards {
                    took_line(ui, &chat.stamp(k));
                }
                continue;
            }
            Item::Plan(entries) => {
                for (c, s) in entries {
                    let (color, filled) = match s.as_str() {
                        "completed" => (t.accent, true),
                        "in_progress" => (t.warning, true),
                        _ => (t.text_dim, false),
                    };
                    ui.horizontal(|ui| {
                        status_dot(ui, color, filled);
                        ui.label(RichText::new(c).color(t.text_dim));
                    });
                }
            }
            Item::Permission { title, options, answer: given, .. } => {
                ui_kit::assistant_approval(ui, &trf("ui.agent_chat_permission", &[("title", title)]), |ui| match given {
                    Some(a) => {
                        ui.label(RichText::new(trf("ui.agent_chat_answered", &[("option", a)])).color(t.text_dim));
                    }
                    None => {
                        ui_kit::assistant_actions(ui, |ui| {
                            for o in options.iter().filter(|o| o.kind == "allow_once").chain(options.iter().filter(|o| o.kind == "allow_always")) {
                                let r = if o.kind == "allow_once" {
                                    ui_kit::primary_button(ui, &o.name)
                                } else {
                                    ui_kit::button(ui, &o.name, ui_kit::Button::Normal)
                                };
                                solidcraft_ui_registry::publish_handle(&format!("agent_chat:permission:{}", o.id), r.rect.center());
                                if r.clicked() {
                                    answer = Some((n, Some(o.id.clone())));
                                }
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
                                for o in options.iter().filter(|o| !matches!(o.kind.as_str(), "allow_once" | "allow_always")) {
                                    let r = ui_kit::button(ui, &o.name, ui_kit::Button::Normal);
                                    solidcraft_ui_registry::publish_handle(&format!("agent_chat:permission:{}", o.id), r.rect.center());
                                    if r.clicked() {
                                        answer = Some((n, Some(o.id.clone())));
                                    }
                                }
                            });
                        });
                    }
                });
            }
            Item::Approval { id, title, tool, answer: given } => {
                ui_kit::assistant_approval(ui, tr("ui.agent_chat_approval"), |ui| {
                    ui_kit::assistant_tool_detail(ui, |ui| {
                        super::markdown_view::selectable(ui, egui::Label::new(RichText::new(title).monospace().size(t.font_caption)).wrap(), false);
                    });
                    match given {
                        Some(true) => {
                            ui.label(RichText::new(tr("ui.agent_chat_allowed")).color(t.text_dim));
                        }
                        Some(false) => {
                            ui.label(RichText::new(tr("ui.agent_chat_denied")).color(t.text_dim));
                        }
                        None => {
                            ui_kit::assistant_actions(ui, |ui| {
                                let allow = ui_kit::primary_button(ui, tr("ui.agent_chat_allow_once"));
                                let always = ui_kit::button(ui, tr("ui.ai_always_allow"), ui_kit::Button::Normal)
                                    .backing_on_hover_text(trf("ui.ai_always_allow_tip", &[("tool", tool)]));
                                let deny = ui
                                    .with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui_kit::button(ui, tr("ui.agent_chat_deny"), ui_kit::Button::Normal)
                                    })
                                    .inner;
                                solidcraft_ui_registry::publish_handle("agent_chat:allow", allow.rect.center());
                                solidcraft_ui_registry::publish_handle("agent_chat:always", always.rect.center());
                                solidcraft_ui_registry::publish_handle("agent_chat:deny", deny.rect.center());
                                if allow.clicked() {
                                    approval = Some((*id, Some(true)));
                                }
                                if always.clicked() {
                                    approval = Some((*id, None));
                                }
                                if deny.clicked() {
                                    approval = Some((*id, Some(false)));
                                }
                            });
                        }
                    }
                });
            }
            Item::Changes { steps, revision, undone } => {
                ui.horizontal(|ui| {
                    let text = trf("ui.agent_chat_changes", &[("n", &steps.to_string())]);
                    ui.label(RichText::new(text).color(t.text_dim).size(solidcraft_theme::Tokens::get().font_caption));
                    if *undone {
                        ui.label(RichText::new(tr("ui.agent_chat_undone")).color(t.text_dim).size(solidcraft_theme::Tokens::get().font_caption));
                    } else if revision.same(current) {
                        let b =
                            ui_kit::button(ui, tr("ui.agent_chat_undo_changes"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.ai_undo_tip"));
                        solidcraft_ui_registry::publish_handle("agent_chat:undo", b.rect.center());
                        if b.clicked() {
                            undo = Some(n);
                        }
                    }
                });
            }
            Item::Notice { text, error } => {
                ui_kit::assistant_banner(ui, if *error { ui_kit::AssistantTone::Error } else { ui_kit::AssistantTone::Neutral }, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).color(if *error { t.error } else { t.text_dim })).wrap());
                });
            }
        }
        took_line(ui, &stamp);
    }
    // While the agent works with no tool running, how long since it last said or did something
    // (a running tool's card counts for itself).
    if let Some(heard) = chat.heard_at()
        && chat.waiting() == 0
        && !matches!(chat.items.last(), Some(Item::Tool { status: Some(ToolStatus::InProgress), .. }))
    {
        let line = ui.label(RichText::new(times::thinking(heard, now)).color(t.text_dim).size(t.font_small));
        crate::scenario::publish_rect("agent_chat:thinking", line.rect);
    }
    // "3 min ago" moves on: by the second for the first minute, then by the minute.
    if !busy && let Some(last) = chat.times.iter().rev().find(|s| s.known()) {
        let young = now - last.at < 60_000.0;
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(if young { 1 } else { 30 }));
    }
    let (mut send_now_clicked, mut send_queue_clicked) = (false, false);
    let mut remove: Option<usize> = None;
    let mut edit_done: Option<(usize, Option<String>)> = None;
    let stopped = chat.stopped;
    let editing = chat.pending_edit.clone();
    let mut start_edit: Option<(usize, String)> = None;
    let mut edit_buf = editing.as_ref().map(|(_, b)| b.clone());
    for (n, p) in chat.pending.iter().enumerate() {
        ui.add_space(6.0);
        let frame = Container::new(ui.ctx(), Surface::Panel)
            .edge_frame(t.panel)
            .stroke(Stroke::new(1.0, t.accent))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::same(6));
        Container::show_backing(ui, frame, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let tag = if stopped && !busy { "ui.agent_chat_queued_stopped" } else { "ui.agent_chat_queued" };
                ui.label(RichText::new(tr(tag)).color(t.accent).semibold().size(solidcraft_theme::Tokens::get().font_small));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let x = ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("ui.agent_chat_remove"));
                    solidcraft_ui_registry::publish_handle(&format!("agent_chat:pending_remove:{n}"), x.rect.center());
                    if x.clicked() {
                        remove = Some(n);
                    }
                    if editing.as_ref().is_none_or(|(i, _)| *i != n) {
                        let e = ui_kit::button(ui, tr("ui.agent_chat_edit"), ui_kit::Button::Small);
                        solidcraft_ui_registry::publish_handle(&format!("agent_chat:pending_edit:{n}"), e.rect.center());
                        if e.clicked() {
                            start_edit = Some((n, p.text.clone()));
                        }
                    }
                });
            });
            match (&editing, edit_buf.as_mut()) {
                (Some((i, _)), Some(buf)) if *i == n => {
                    let te = ui_kit::message_field(ui, Id::unique(("sc_agent_pending_input", n)), buf, "");
                    solidcraft_ui_registry::publish_rect("agent_chat:pending_input", te.rect);
                    chat.report.focus.push(te.clone());
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, tr("ui.agent_chat_save"), ui_kit::Button::Normal).clicked() {
                            edit_done = Some((n, Some(buf.clone())));
                        }
                        if ui_kit::button(ui, tr("ui.agent_chat_cancel"), ui_kit::Button::Normal).clicked() {
                            edit_done = Some((n, None));
                        }
                    });
                }
                _ => {
                    if !p.text.is_empty()
                        && let Some(group) = user_text(ui, &p.text, &p.context, &[], &format!("agent_chat:queued:{n}"), false)
                    {
                        hovered = Some(Hovered::Group { sent: false, at: n, group });
                    }
                    if p.image {
                        ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim).size(solidcraft_theme::Tokens::get().font_small));
                    }
                    let (rest, rest_flags, places) = unplaced(&p.context, &[]);
                    if let Some(place) = context_chips(ui, &rest, &rest_flags, &format!("agent_chat:queued:{n}")).hovered
                        && let Some(item) = places.get(place)
                    {
                        hovered = Some(Hovered::Item { sent: false, at: n, item: *item });
                    }
                    files_line(ui, p.files.iter().map(|f| f.name.as_str()));
                }
            }
        });
    }
    if !chat.pending.is_empty() && editing.is_none() {
        ui.horizontal(|ui| {
            if busy {
                let b =
                    ui_kit::button(ui, tr("ui.agent_chat_send_now"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.agent_chat_send_now_tip"));
                solidcraft_ui_registry::publish_handle("agent_chat:send_now", b.rect.center());
                send_now_clicked = b.clicked();
            } else {
                let b = ui_kit::primary_button(ui, tr("ui.agent_chat_send"));
                solidcraft_ui_registry::publish_handle("agent_chat:send_queue", b.rect.center());
                send_queue_clicked = b.clicked();
            }
        });
    }
    if let Some(start) = start_edit {
        chat.pending_edit = Some(start);
    } else if let (Some(buf), Some(slot)) = (edit_buf, chat.pending_edit.as_mut()) {
        slot.1 = buf;
    }
    if chat.items.is_empty() && chat.pending.is_empty() {
        if view.agents.is_empty() && chat.detecting.is_none() {
            ui_kit::assistant_empty(ui, tr("ui.agent_chat_setup_title"), tr("ui.agent_chat_setup_body"), |ui| {
                if ui_kit::primary_button(ui, tr("ui.agent_chat_open_settings")).clicked() {
                    chat.actions.push(PanelAction::OpenSettings);
                }
            });
        } else {
            ui_kit::assistant_prose(ui, |ui| {
                ui.label(RichText::new(tr("ui.agent_chat_intro")).color(t.text_dim));
                ui.label(RichText::new(tr("ui.agent_chat_privacy")).color(t.text_dim).size(t.font_small));
            });
        }
    }
    if let Some((n, text)) = edit_done {
        if let (Some(text), Some(p)) = (text.filter(|t| !t.trim().is_empty()), chat.pending.get_mut(n)) {
            p.text = text;
            p.regroup();
        }
        chat.pending_edit = None;
    }
    if let Some(n) = remove {
        chat.actions.push(PanelAction::RemovePending(n));
    }
    if send_now_clicked {
        chat.actions.push(PanelAction::SendNow);
    }
    if send_queue_clicked {
        chat.actions.push(PanelAction::SendNow);
    }
    if open_gallery.is_some() {
        chat.gallery = open_gallery;
    }
    if hovered.is_some() {
        chat.hovered = hovered;
    }
    if let Some((n, o)) = answer {
        chat.actions.push(PanelAction::AnswerPermission(n, o));
    }
    match approval {
        Some((id, Some(allow))) => chat.actions.push(PanelAction::Approve(id, allow)),
        Some((id, None)) => chat.actions.push(PanelAction::ApproveAlways(id)),
        None => {}
    }
    if let Some(n) = undo {
        chat.actions.push(PanelAction::UndoChanges(n));
    }
}

/// The files that went (or will go) with a message, by name.
#[cfg(not(target_arch = "wasm32"))]
fn files_line<'a>(ui: &mut egui::Ui, names: impl Iterator<Item = &'a str>) {
    let list = names.collect::<Vec<_>>().join(", ");
    if !list.is_empty() {
        let t = Tokens::get();
        ui.add(egui::Label::new(RichText::new(trf("ui.agent_chat_files", &[("files", &list)])).color(t.text_dim).size(t.font_small)).wrap());
    }
}

/// The widest a file chip gets; a narrower chat makes them narrower.
const CHIP_W: f32 = 170.0;

/// The items of a message that are not pills in its text (those of Attach selection, or all of them
/// when the message has no pills), with their "gone" flags and each one's place among all of the
/// message's items: the chips below the message.
#[cfg(not(target_arch = "wasm32"))]
fn unplaced<'a>(context: &'a Context, gone: &'a [bool]) -> (std::borrow::Cow<'a, Context>, std::borrow::Cow<'a, [bool]>, Vec<usize>) {
    use std::borrow::Cow;
    if context.groups.is_empty() {
        return (Cow::Borrowed(context), Cow::Borrowed(gone), (0..context.items.len()).collect());
    }
    let mut rest = Context { omitted: context.omitted, ..Context::default() };
    let mut flags = Vec::new();
    let mut places = Vec::new();
    for (at, item) in context.items.iter().enumerate().filter(|(_, i)| i.group == 0) {
        rest.items.push(item.clone());
        flags.push(gone.get(at).copied().unwrap_or(false));
        places.push(at);
    }
    (Cow::Owned(rest), Cow::Owned(flags), places)
}

/// A sent message's text, its pills drawn as pills: `gone` flags the context's items that have left
/// the design, which strikes through the pills they are in; `selected`: the whole transcript is
/// selected (#10676). Returns the number of the pill under the pointer (its group), for the host to
/// light what it stands for.
#[cfg(not(target_arch = "wasm32"))]
fn user_text(ui: &mut egui::Ui, text: &str, context: &Context, gone: &[bool], handle: &str, selected: bool) -> Option<u32> {
    let t = Tokens::get();
    if context.groups.is_empty() {
        super::markdown_view::selectable(ui, egui::Label::new(RichText::new(text).color(t.text)).wrap(), selected);
        return None;
    }
    let look = |id: u32| {
        context.groups.iter().any(|(n, _)| *n == id).then(|| {
            let gone = context.items.iter().enumerate().any(|(at, item)| item.group == id && gone.get(at).copied().unwrap_or(false));
            // The bubble is the accent colour itself: a pill in it takes the filled chip's colour
            // so it stands apart from the bubble.
            pills::Look { background: t.chip, text: if gone { t.text_dim } else { t.text }, gone }
        })
    };
    let font = egui::FontId::proportional(ui.style().text_styles.get(&egui::TextStyle::Body).map_or(t.font_label, |f| f.size));
    let galley = ui.fonts_mut(|f| f.layout_job(pills::job(text, &font, t.text, ui.available_width(), &look)));
    // Through the conversation's selectable label, so it stays in a selection scrolled out of view.
    let response = super::markdown_view::selectable(ui, egui::Label::new(egui::WidgetText::Galley(Arc::clone(&galley))), selected);
    // Where each pill is, for tests and scripts.
    for span in pills::spans(text) {
        let row = galley.pos_from_cursor(egui::text::CCursor::new(span.start + 2));
        solidcraft_ui_registry::publish_handle(&format!("{handle}:pill:{}", span.id), response.rect.min + row.center().to_vec2());
    }
    // The pointer on a pill: which one is read from the text layout, which starts at the label's corner.
    let pos = response.hover_pos()?;
    let at = galley.cursor_from_pos(pos - response.rect.min).index.0;
    pills::spans(text).into_iter().find(|s| s.start <= at && at < s.end).map(|s| s.id)
}

/// The id of the message box.
#[cfg(not(target_arch = "wasm32"))]
fn input_id() -> Id {
    Id::unique("sc_agent_chat_input")
}

/// Where the caret is in the message box (a character), if it has been in it.
#[cfg(not(target_arch = "wasm32"))]
fn caret(ctx: &egui::Context) -> Option<usize> {
    egui::TextEdit::load_state(ctx, input_id()).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0)
}

/// Put the caret of the message box at a character, and focus the box.
#[cfg(not(target_arch = "wasm32"))]
fn place_caret(ctx: &egui::Context, at: usize) {
    let mut state = egui::TextEdit::load_state(ctx, input_id()).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
    egui::TextEdit::store_state(ctx, input_id(), state);
    ctx.memory_mut(|m| m.request_focus(input_id()));
}

/// The message box, with the pills of its picks drawn as pills. A pill is text (see [`pills`]), so
/// the box edits, selects and pastes as always; what is added here: the caret goes after a pill just
/// made, Backspace and Delete take a pill whole, typing `@` asks the host to open the picker where
/// the `@` was, and the pointer on a pill says what it holds and has the host light it.
#[cfg(not(target_arch = "wasm32"))]
fn pill_input(chat: &mut AgentChat, view: &PanelView, ui: &mut egui::Ui, focused: bool, hint: &str) -> egui::Response {
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    if let Some(at) = chat.caret_to.take() {
        place_caret(&ctx, at);
    }
    // Backspace right after a pill, Delete right before one: the whole pill goes.
    if focused
        && let Some(r) = egui::TextEdit::load_state(&ctx, input_id()).and_then(|s| s.cursor.char_range())
        && r.primary == r.secondary
    {
        let at = r.primary.index.0;
        let known = |id: u32| chat.pills.iter().any(|p| p.id == id);
        let spans = pills::spans(&chat.input);
        let back = spans.iter().find(|s| s.end == at && known(s.id)).cloned();
        let forward = spans.iter().find(|s| s.start == at && known(s.id)).cloned();
        // Plain, and with the word-delete modifiers (Ctrl, Alt): a word would take only part of a mark.
        let modifiers = |k: egui::Key, ui: &egui::Ui| {
            [egui::Modifiers::NONE, egui::Modifiers::CTRL, egui::Modifiers::ALT]
                .into_iter()
                .find(|m| ui.input(|i| i.key_pressed(k) && i.modifiers.matches_exact(*m)))
        };
        let hit = match (back, forward) {
            (Some(span), _) if let Some(m) = modifiers(egui::Key::Backspace, ui) => Some((span, egui::Key::Backspace, m)),
            (_, Some(span)) if let Some(m) = modifiers(egui::Key::Delete, ui) => Some((span, egui::Key::Delete, m)),
            _ => None,
        };
        if let Some((span, key, m)) = hit {
            ui.input_mut(|i| i.consume_key(m, key));
            chat.input = pills::remove(&chat.input, span.start, span.end);
            place_caret(&ctx, span.start);
        }
    }
    let before = chat.input_chars;
    let font = egui::FontId::proportional(t.font_label);
    let look = {
        let ids: Vec<(u32, bool)> = chat.pills.iter().enumerate().map(|(n, p)| (p.id, chat.gone.pills.get(n).copied().unwrap_or(false))).collect();
        move |id: u32| {
            ids.iter().find(|(pill, _)| *pill == id).map(|(_, gone)| pills::Look {
                background: t.accent_soft,
                text: if *gone { t.text_dim } else { t.text },
                gone: *gone,
            })
        }
    };
    let mut layouter =
        |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| ui.fonts_mut(|f| f.layout_job(pills::job(text.as_str(), &font, t.text, wrap, &look)));
    let out = ui_kit::assistant_input_laid_out(ui, input_id(), &mut chat.input, hint, &mut layouter);
    let chars = chat.input.chars().count();
    chat.input_chars = chars;
    // `@` typed as a new word opens the picker (the host opens it after the frame), and is not
    // left in the text.
    if out.response.changed()
        && chars == before + 1
        && let Some(at) = out.cursor_range.map(|r| r.primary.index.0).filter(|at| *at > 0)
    {
        let typed: Vec<char> = chat.input.chars().skip(at.saturating_sub(2)).take(2).collect();
        let (prev, last) = if at >= 2 { (typed.first().copied(), typed.get(1).copied()) } else { (None, typed.first().copied()) };
        // Not while another tool or dialog is open (the picker would not open and the @ would be lost),
        // and not in the middle of an input method's composition.
        let composing = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Ime(_))));
        if last == Some('@') && prev.is_none_or(char::is_whitespace) && !view.dialog_open && !composing {
            chat.input = pills::remove(&chat.input, at - 1, at);
            chat.input_chars = chars - 1;
            place_caret(&ctx, at - 1);
            chat.actions.push(PanelAction::StartPick { at: Some(at - 1) });
        }
    }
    // The pointer on a pill lights what it stands for in the model, and says what that is.
    if out.response.hovered()
        && let Some(pos) = ui.input(|i| i.pointer.hover_pos())
    {
        let at = out.galley.cursor_from_pos(pos - out.galley_pos).index.0;
        let over = pills::spans(&chat.input).into_iter().find(|s| s.start <= at && at < s.end);
        if let Some(pill) = over.and_then(|s| chat.pills.iter().find(|p| p.id == s.id)) {
            chat.hovered = Some(Hovered::Pill(pill.id));
            let tip = pill.context.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>().join("\n");
            return out.response.response.on_hover_text(tip);
        }
    }
    out.response.response
}

/// The files for the next message, as chips in rows that fit the chat: a picture's thumbnail (or
/// a file mark), the name cut to fit, and a × that takes the file off. Laid out by hand: egui
/// wraps plain widgets, not framed groups, and one row wider than the chat widens all below it.
#[cfg(not(target_arch = "wasm32"))]
fn attachment_chips(chat: &mut AgentChat, ui: &mut egui::Ui) {
    // Thumbnails of files no longer on the message are let go.
    let keep: Vec<u64> = chat.attachments.iter().filter(|f| f.thumb.is_some()).map(|f| f.id).collect();
    chat.thumbs.retain(|k, _| keep.contains(k));
    if chat.files() == 0 {
        return;
    }
    let gap = ui.spacing().item_spacing.x;
    let avail = ui.available_width().max(1.0);
    let w = CHIP_W.min(avail);
    let per_row = (((avail + gap) / (w + gap)).floor() as usize).max(1);
    let (mut remove, mut stop) = (None, None);
    // The files read, then those still being read (each takes its place once it is done).
    let read = chat.attachments.len();
    let indices: Vec<usize> = (0..chat.files()).collect();
    for row in indices.chunks(per_row) {
        ui.horizontal(|ui| {
            for &i in row {
                if let Some(f) = chat.attachments.get(i) {
                    if chip(ui, &mut chat.thumbs, f, i, w) {
                        remove = Some(i);
                    }
                } else if let Some(l) = chat.loading.get(i - read)
                    && loading_chip(ui, &l.name, l.importing(), i - read, w)
                {
                    stop = Some(i - read);
                }
            }
        });
    }
    if let Some(i) = remove
        && i < chat.attachments.len()
    {
        chat.attachments.remove(i);
    }
    // Taken off while it is read: its reader's answer is dropped with it.
    if let Some(i) = stop
        && i < chat.loading.len()
    {
        chat.loading.remove(i);
    }
}

/// What a chip says when the pointer rests on it: the file's name; for one that goes by its place,
/// that place and what the agent does with it.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn chip_tip(f: &attach::Attachment) -> String {
    match &f.content {
        attach::Content::Reference { path, .. } => format!("{}\n{}", path.display(), tr("ui.agent_chat_file_by_place_tip")),
        attach::Content::Folder { path, .. } => format!("{}\n{}", path.display(), tr("ui.agent_chat_folder_tip")),
        _ => f.name.clone(),
    }
}

/// What a chip's × says: an imported part stays in the design when it comes off the message.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn remove_tip(f: &attach::Attachment) -> &'static str {
    tr(if matches!(f.content, attach::Content::Imported) { "ui.agent_chat_remove_imported" } else { "ui.agent_chat_remove_file" })
}

/// The chip of a file still being read or imported: a spinner, the name, and a × that takes it
/// off (an import goes on: its part lands in the design).
#[cfg(not(target_arch = "wasm32"))]
fn loading_chip(ui: &mut egui::Ui, name: &str, importing: bool, i: usize, w: f32) -> bool {
    let t = Tokens::get();
    let mut clicked = false;
    let r = crate::ui_kit::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(t.toolbar)
            .stroke(t.control_border_stroke(t.border))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(4, 3)),
        |ui| {
            let inner = (w - 10.0).max(40.0);
            ui.set_width(inner);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let doing = tr(if importing { "ui.agent_chat_file_importing" } else { "ui.agent_chat_file_loading" });
                let mark = ui.add(egui::Spinner::new().size(t.form_icon_size)).backing_on_hover_text(doing).rect.width();
                let name_w = (inner - mark - 18.0 - 8.0).max(8.0);
                ui.allocate_ui(vec2(name_w, 20.0), |ui| {
                    ui.set_width(name_w);
                    ui.add(egui::Label::new(RichText::new(name).size(t.font_small).color(t.text_dim)).truncate())
                })
                .inner
                .backing_on_hover_text(name);
                let (r, x) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = r.center();
                let s = Stroke::new(1.3, if x.hovered() { t.text } else { t.text_dim });
                ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                solidcraft_ui_registry::publish_handle(&format!("agent_chat:file_loading_remove:{i}"), c);
                clicked =
                    x.backing_on_hover_text(tr(if importing { "ui.agent_chat_remove_imported" } else { "ui.agent_chat_remove_file" })).clicked();
            });
        },
    );
    solidcraft_ui_registry::publish_handle(&format!("agent_chat:file_loading:{i}"), r.response.rect.center());
    clicked
}

/// One file chip, `w` wide in all; true when its × was clicked.
#[cfg(not(target_arch = "wasm32"))]
fn chip(ui: &mut egui::Ui, thumbs: &mut std::collections::HashMap<u64, egui::TextureHandle>, f: &attach::Attachment, i: usize, w: f32) -> bool {
    let t = Tokens::get();
    let mut clicked = false;
    let r = crate::ui_kit::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(t.toolbar)
            .stroke(t.control_border_stroke(t.border))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(4, 3)),
        |ui| {
            let inner = (w - 10.0).max(40.0);
            ui.set_width(inner);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mark = match &f.thumb {
                    Some(img) => {
                        let tex = thumbs
                            .entry(f.id)
                            .or_insert_with(|| ui.ctx().load_texture(format!("sc_attach_{}", f.id), (**img).clone(), egui::TextureOptions::LINEAR));
                        let size = tex.size_vec2();
                        ui.image((tex.id(), size * (22.0 / size.x.max(size.y).max(1.0)))).rect.width()
                    }
                    None if matches!(f.content, attach::Content::Folder { .. }) => {
                        // A folder: a body with a tab on its top left.
                        let (r, _) = ui.allocate_exact_size(vec2(28.0, 20.0), Sense::hover());
                        let s = Stroke::new(1.2, t.text_dim);
                        let body = Rect::from_min_max(pos2(r.left() + 3.0, r.top() + 5.0), pos2(r.right() - 3.0, r.bottom() - 2.0));
                        let tab = Rect::from_min_max(pos2(body.left(), r.top() + 2.0), pos2(body.left() + 9.0, body.top()));
                        ui.painter().rect_stroke(body, 1.5, s, egui::StrokeKind::Inside);
                        ui.painter().rect_filled(tab, 1.0, t.text_dim);
                        r.width()
                    }
                    None => {
                        // A page marked with the file's type.
                        let (r, _) = ui.allocate_exact_size(vec2(28.0, 20.0), Sense::hover());
                        let color = if matches!(f.content, attach::Content::Pdf { .. }) { t.error } else { t.text_dim };
                        ui.painter().rect_stroke(r.shrink2(vec2(1.0, 1.0)), 2.0, Stroke::new(1.2, color), egui::StrokeKind::Inside);
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            f.mark(),
                            egui::FontId::proportional(solidcraft_theme::Tokens::get().font_micro),
                            color,
                        );
                        r.width()
                    }
                };
                let name_w = (inner - mark - 18.0 - 8.0).max(8.0);
                ui.allocate_ui(vec2(name_w, 20.0), |ui| {
                    ui.set_width(name_w);
                    ui.add(egui::Label::new(RichText::new(&f.name).size(t.font_small)).truncate())
                })
                .inner
                .backing_on_hover_text(chip_tip(f));
                let (r, x) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = r.center();
                let s = Stroke::new(1.3, if x.hovered() { t.text } else { t.text_dim });
                ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                solidcraft_ui_registry::publish_handle(&format!("agent_chat:file_remove:{i}"), c);
                clicked = x.backing_on_hover_text(remove_tip(f)).clicked();
            });
        },
    );
    solidcraft_ui_registry::publish_target(&format!("agent_chat:file:{i}"), r.response.rect);
    clicked
}

/// The + button: choose files with the app's picker (pictures, text and CAD files, several at
/// once), choose a folder, or paste the clipboard's picture.
#[cfg(not(target_arch = "wasm32"))]
fn add_menu(chat: &mut AgentChat, ui: &mut egui::Ui) {
    let (mut choose, mut folder, mut paste) = (false, false, false);
    let r = ui_kit::action_menu(ui, "+", |ui| {
        let a = ui_kit::button(ui, tr("ui.agent_chat_add_file"), ui_kit::Button::Normal);
        solidcraft_ui_registry::publish_handle("agent_chat:add_file", a.rect.center());
        let f = ui_kit::button(ui, tr("ui.agent_chat_add_folder"), ui_kit::Button::Normal);
        solidcraft_ui_registry::publish_handle("agent_chat:add_folder", f.rect.center());
        let f = f.backing_on_hover_text(tr("ui.agent_chat_add_folder_tip"));
        let b = ui_kit::button(ui, tr("ui.agent_chat_paste_picture"), ui_kit::Button::Normal);
        solidcraft_ui_registry::publish_handle("agent_chat:paste_picture", b.rect.center());
        if a.clicked() {
            choose = true;
            ui.close();
        }
        if f.clicked() {
            folder = true;
            ui.close();
        }
        if b.clicked() {
            paste = true;
            ui.close();
        }
    });
    solidcraft_ui_registry::publish_handle("agent_chat:add", r.response.rect.center());
    r.response.backing_on_hover_text(tr("ui.agent_chat_add"));
    if choose {
        chat.actions.push(PanelAction::ChooseAttachment);
    }
    if folder {
        chat.actions.push(PanelAction::ChooseFolder);
    }
    if paste {
        chat.actions.push(PanelAction::PastePicture);
    }
}

/// A tool call as its card shows it.
#[cfg(not(target_arch = "wasm32"))]
struct ToolCall<'a> {
    /// The agent's title, shown when there are no words for the call.
    title: &'a str,
    /// What it did in words (#1457).
    summary: Option<&'a str>,
    kind: &'a str,
    status: Option<ToolStatus>,
    input: Option<&'a Value>,
    output: Option<&'a str>,
    /// When the call came in, the time now (ms), and whether the agent is still at it.
    stamp: times::Stamp,
    now: f64,
    live: bool,
}

/// Longest raw text shown under a card's Details (characters).
#[cfg(not(target_arch = "wasm32"))]
const MAX_DETAIL: usize = 8000;

/// A tool call as a card: its state and what it did in words on one line (the agent's own title
/// when there are none); a click on the line unfolds Details, the raw arguments and what it
/// returned.
#[cfg(not(target_arch = "wasm32"))]
fn tool_card(ui: &mut egui::Ui, n: usize, call: &ToolCall) {
    let t = Tokens::get();
    let status = call.status;
    let (color, mark, state) = match status {
        Some(ToolStatus::Completed) => (t.form_success, ui_kit::AssistantToolState::Done, "ui.ai_tool_done"),
        Some(ToolStatus::Failed) => (t.error, ui_kit::AssistantToolState::Failed, "ui.ai_tool_failed"),
        Some(ToolStatus::InProgress) => (t.accent, ui_kit::AssistantToolState::Running, "ui.ai_tool_running"),
        _ => (t.text_dim, ui_kit::AssistantToolState::Pending, "ui.ai_tool_waiting"),
    };
    let details = call.input.is_some() || call.output.is_some();
    ui_kit::assistant_tool_row(ui, status == Some(ToolStatus::Failed), |ui| {
        ui.set_width(ui.available_width());
        let id = ui.make_persistent_id(("sc_agent_tool", n));
        let mut state_ = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        let head = ui.allocate_ui_with_layout(vec2(ui.available_width(), t.control_height + t.space_2), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            ui_kit::assistant_tool_status(ui, mark);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if details {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.space_14, t.space_14), Sense::hover());
                    ui_kit::disclosure(ui.painter(), rect.center(), state_.is_open(), t.text_dim);
                    ui.label(RichText::new(tr("ui.ai_tool_details")).size(t.font_small).color(t.text_dim));
                }
                // A running call counts how long it has run ("running for 30 s").
                let running = (call.live && status == Some(ToolStatus::InProgress)).then(|| times::running(&call.stamp, call.now)).flatten();
                ui.label(RichText::new(running.as_deref().unwrap_or(tr(state))).size(t.font_small).color(color));
                if !call.kind.is_empty() && call.kind != "other" {
                    ui.label(RichText::new(call.kind).size(t.font_small).color(t.text_dim));
                }
                if running.is_none() {
                    time_label(ui, n, &call.stamp, call.now);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let text = match call.summary {
                        Some(words) => RichText::new(words).size(t.font_caption).color(t.text),
                        None => RichText::new(summary::plain_title(call.title)).monospace().size(t.font_caption).color(t.text),
                    };
                    ui.add(egui::Label::new(text).truncate().selectable(false)).backing_on_hover_text(call.title);
                });
            });
        });
        let click = ui.interact(head.response.rect, id.with("head"), Sense::click());
        solidcraft_ui_registry::publish_handle(&format!("agent_chat:tool:{n}"), head.response.rect.center());
        if details && click.clicked() {
            state_.toggle(ui);
        }
        if details {
            click.backing_on_hover_text(tr("ui.ai_tool_unfold"));
        }
        state_.show_body_unindented(ui, |ui| {
            ui.indent(id.with("detail"), |ui| {
                if let Some(input) = call.input {
                    let text = serde_json::to_string_pretty(input).unwrap_or_default();
                    tool_detail(ui, tr("ui.ai_tool_arguments"), &text, id.with("in"));
                }
                if let Some(o) = call.output {
                    tool_detail(ui, tr("ui.ai_tool_result"), o, id.with("out"));
                }
            });
        });
    });
}

/// One part of a card's Details: its heading and the raw text, cut short and scrolling.
#[cfg(not(target_arch = "wasm32"))]
fn tool_detail(ui: &mut egui::Ui, heading: &str, text: &str, id: Id) {
    let t = Tokens::get();
    ui.label(RichText::new(heading).size(t.font_small).color(t.text_dim));
    let shown: String =
        if text.chars().count() > MAX_DETAIL { text.chars().take(MAX_DETAIL).chain(std::iter::once('…')).collect() } else { text.to_string() };
    ui_kit::assistant_tool_detail(ui, |ui| {
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt(id).max_height(180.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(shown).monospace().size(t.font_small)).selectable(true).wrap());
        });
    });
}

/// A small status mark drawn as a shape (no glyph a system font might lack): filled when the
/// step is running or done, hollow while it waits.
#[cfg(not(target_arch = "wasm32"))]
fn status_dot(ui: &mut egui::Ui, color: Color32, filled: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    if filled {
        ui.painter().circle_filled(r.center(), 4.0, color);
    } else {
        ui.painter().circle_stroke(r.center(), 3.5, Stroke::new(1.2, color));
    }
}

/// A custom command as the picker shows it: the program's file name and its arguments, cut short.
pub(super) fn short_command(line: &str) -> String {
    let argv = acp::split_command(line);
    let mut parts: Vec<String> = argv.iter().map(|a| a.rsplit(['/', '\\']).next().unwrap_or(a).to_string()).collect();
    if parts.is_empty() {
        parts.push(line.to_string());
    }
    let s = parts.join(" ");
    if s.chars().count() > 40 { format!("{}…", s.chars().take(40).collect::<String>()) } else { s }
}

/// Quote an argument for the command field when it holds spaces.
pub(super) fn quote(a: &str) -> String {
    if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_explains_the_sandbox() {
        const CHILD: &str = "SOLIDCRAFT_TEST_SANDBOX_MESSAGE";
        if let Ok(case) = std::env::var(CHILD) {
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.agent_chat.detection = Some(discover::Detection::default());
            assert!(agents::entries(&app).is_empty());
            let message = no_agents_message();
            if case == "flatpak" {
                assert!(message.contains("sandbox"), "{message}");
                assert!(message.contains("restart"), "{message}");
                let ctx = crate::fonts::test_context();
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(300.0, 900.0))), ..Default::default() };
                let view = PanelView::of(&mut app, &ctx);
                for _ in 0..2 {
                    let mut output = ctx.run_ui(input.clone(), |ui| picker(&mut app.agent_chat, &view, ui));
                    assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == message));
                    assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == FLATPAK_OVERRIDE));
                    output.textures_delta.clear();
                }
                let at = solidcraft_ui_registry::handle_at("agent_chat:flatpak_override").expect("copy override");
                let mut copied = false;
                for pressed in [true, false] {
                    let mut output = ctx.run_ui(egui::RawInput { events: crate::test_support::pointer(at, pressed), ..input.clone() }, |ui| {
                        picker(&mut app.agent_chat, &view, ui)
                    });
                    copied |= output
                        .platform_output
                        .commands
                        .iter()
                        .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == FLATPAK_OVERRIDE));
                    output.textures_delta.clear();
                }
                assert!(copied, "Copy preserves the complete override command");
            } else {
                // #1458: the Snap can't start what the install list installs, so it says why instead.
                let snap = case == "snap";
                assert_eq!(message, tr(if snap { "ui.agent_chat_snap_none_found" } else { "ui.agent_chat_none_found" }));
                const INSTALL: &str = "npm install -g @example/agent";
                let missing = discover::Missing { id: "example", name: "Example agent", install: INSTALL };
                app.agent_chat.detection = Some(discover::Detection { missing: vec![missing], ..Default::default() });
                let ctx = crate::fonts::test_context();
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(300.0, 900.0))), ..Default::default() };
                let view = PanelView::of(&mut app, &ctx);
                for _ in 0..2 {
                    let mut output = ctx.run_ui(input.clone(), |ui| picker(&mut app.agent_chat, &view, ui));
                    if snap {
                        assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == message));
                    }
                    assert_eq!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == INSTALL), !snap, "{case}");
                    output.textures_delta.clear();
                }
            }
            return;
        }
        // Another snap's variables (SolidCraft started from a confined editor's terminal) are not the Snap.
        let cases: [(&str, &[(&str, &str)]); 4] = [
            ("flatpak", &[("FLATPAK_ID", "io.tangencylabs.solidcraft")]),
            ("snap", &[("SNAP", "/snap/solidcraft/12"), ("SNAP_NAME", "solidcraft")]),
            ("another snap", &[("SNAP", "/snap/code/200"), ("SNAP_NAME", "code")]),
            ("native", &[]),
        ];
        for (case, env) in cases {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "agent_chat::panel::tests::empty_discovery_explains_the_sandbox", "--nocapture"]);
            child.env(CHILD, case);
            for name in ["FLATPAK_ID", "SNAP", "SNAP_NAME"] {
                child.env_remove(name);
            }
            child.envs(env.iter().copied());
            assert!(child.status().unwrap().success(), "{case}");
        }
    }

    #[test]
    fn custom_commands_show_short_in_the_picker() {
        assert_eq!(short_command("python3 -I /very/long/path/to/mock_agent.py"), "python3 -I mock_agent.py");
        assert!(short_command(&"x".repeat(100)).ends_with('…'));
    }

    /// The panel records what was asked and sends nothing itself: the host runs it after the frame.
    #[test]
    fn the_send_button_records_the_message_for_the_host_to_send() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.detection = Some(discover::Detection::default());
        app.agent_chat.picking = Some(false);
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.input = "Make a plate".to_string();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(440.0, 720.0));
        let frame = |app: &mut SolveApp, events: Vec<egui::Event>| {
            solidcraft_ui_registry::clear_handles();
            let input = egui::RawInput { screen_rect: Some(screen), focused: true, events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                let prefs = AgentPanelPrefs::read(&app.preferences);
                let view = PanelView::of(app, ui.ctx());
                ui_kit::scope(ui, |ui| desktop_body(&mut app.agent_chat, &view, ui, Flow::Tall, &prefs));
            });
            output.textures_delta.clear();
        };
        // The composer settles its height over the first frames.
        for _ in 0..3 {
            frame(&mut app, Vec::new());
        }
        let at = solidcraft_ui_registry::handle_at("agent_chat:send").expect("the send button is drawn");
        for pressed in [true, false] {
            frame(&mut app, crate::test_support::pointer(at, pressed));
        }
        assert_eq!(app.agent_chat.actions, [PanelAction::Send { text: "Make a plate".into(), steer: false }]);
        assert!(app.agent_chat.items.is_empty(), "the panel itself sends nothing");
    }

    /// The composer of `app` drawn once with `events`, as the host draws it, its actions left unrun.
    fn composer_frame(ctx: &egui::Context, app: &mut SolveApp, events: Vec<egui::Event>) {
        solidcraft_ui_registry::clear_handles();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(440.0, 720.0));
        let input = egui::RawInput { screen_rect: Some(screen), focused: true, events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            let prefs = AgentPanelPrefs::read(&app.preferences);
            let view = PanelView::of(app, ui.ctx());
            ui_kit::scope(ui, |ui| desktop_body(&mut app.agent_chat, &view, ui, Flow::Tall, &prefs));
        });
        output.textures_delta.clear();
    }

    /// A chat ready to take a message, with "Make a plate" in the box.
    fn ready_app() -> SolveApp {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.detection = Some(discover::Detection::default());
        app.agent_chat.picking = Some(false);
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.input = "Make a plate".to_string();
        app
    }

    /// Pick context records where its pill goes (the caret in the box) for the host to open the
    /// picker; the panel opens no dialog itself.
    #[test]
    fn the_pick_context_button_records_the_pick_for_the_host() {
        let ctx = crate::fonts::test_context();
        let mut app = ready_app();
        // The caret is after "Make".
        app.agent_chat.caret_to = Some(4);
        for _ in 0..3 {
            composer_frame(&ctx, &mut app, Vec::new());
        }
        let at = solidcraft_ui_registry::handle_at("agent_chat:pick_context").expect("the button is drawn");
        for pressed in [true, false] {
            composer_frame(&ctx, &mut app, crate::test_support::pointer(at, pressed));
        }
        assert_eq!(app.agent_chat.actions, [PanelAction::StartPick { at: Some(4) }]);
        assert!(app.dialog.is_none(), "the panel opens no picker itself");
    }

    /// `@` typed where a word starts leaves the text and records a pick at its place for the host;
    /// the panel opens no dialog itself.
    #[test]
    fn an_at_sign_records_a_pick_where_it_was_typed() {
        let ctx = crate::fonts::test_context();
        let mut app = ready_app();
        app.agent_chat.input = "match ".into();
        app.agent_chat.caret_to = Some(6);
        for _ in 0..3 {
            composer_frame(&ctx, &mut app, Vec::new());
        }
        composer_frame(&ctx, &mut app, vec![egui::Event::Text("@".into())]);
        assert_eq!(app.agent_chat.input, "match ", "the @ is not left in the text");
        assert_eq!(app.agent_chat.actions, [PanelAction::StartPick { at: Some(6) }]);
        assert!(app.dialog.is_none(), "the panel opens no picker itself");
    }

    /// What a host with no agents, no selection and nothing open shows the panel.
    fn plain_view(shared: bool) -> PanelView {
        PanelView {
            shown: true,
            document: "Plate".into(),
            revision: Revision::of(&solidcraft_engine::Session::default().doc),
            units: "mm".into(),
            decimals: 2,
            agents: Vec::new(),
            current: None,
            permission_agent: None,
            permission: agents::Mode::default(),
            selected: 0,
            dialog_open: false,
            turn_limit: 0,
            model_warning: None,
            toggle_keys: Vec::new(),
            capturing_keys: false,
            rtl: false,
            shared,
            composer_height: None,
        }
    }

    /// A conversation ready to send "Make a plate", with no app.
    fn plain_chat() -> AgentChat {
        let mut chat = AgentChat::default();
        chat.open = true;
        chat.input = "Make a plate".into();
        chat.detection = Some(discover::Detection::default());
        chat.picking = Some(false);
        chat.state = Some(State::Ready);
        chat
    }

    /// The panel draws from the chat's state and plain host data, with no app, and returns what
    /// was asked.
    #[test]
    fn the_panel_draws_from_plain_state_and_returns_the_message_to_send() {
        let ctx = crate::fonts::test_context();
        let mut chat = plain_chat();
        let view = plain_view(false);
        let mut prefs = AgentPanelPrefs { popped: false, window: None };
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0));
        let frame = |chat: &mut AgentChat, prefs: &mut AgentPanelPrefs, events: Vec<egui::Event>| {
            solidcraft_ui_registry::clear_handles();
            let input = egui::RawInput { screen_rect: Some(screen), focused: true, events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| draw_content(chat, &view, prefs, ui, Flow::Tall));
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut chat, &mut prefs, Vec::new());
        }
        let at = solidcraft_ui_registry::handle_at("agent_chat:send").expect("the send button is drawn");
        for pressed in [true, false] {
            frame(&mut chat, &mut prefs, crate::test_support::pointer(at, pressed));
        }
        assert_eq!(chat.actions, [PanelAction::Send { text: "Make a plate".into(), steer: false }]);
        assert!(chat.items.is_empty(), "the panel itself sends nothing");
    }

    /// In the shared host's body the content keeps what the host measures (the composer's
    /// height, the height it needs, the text box that takes the keyboard) for the host to take
    /// after drawing, rather than handing it over itself.
    #[test]
    fn shared_content_reports_its_measures_for_the_host_to_take() {
        let ctx = crate::fonts::test_context();
        let mut chat = plain_chat();
        let view = plain_view(true);
        let prefs = AgentPanelPrefs { popped: false, window: None };
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(440.0, 720.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let mut layer = None;
        let mut output = ctx.run_ui(input, |ui| {
            layer = Some(ui.layer_id());
            draw_content(&mut chat, &view, &prefs, ui, Flow::Tall);
        });
        output.textures_delta.clear();
        let report = std::mem::take(&mut chat.report);
        assert!(report.composer_height.is_some_and(|h| h > 0.0), "the composer's height is kept");
        let (drawn_on, _, height) = report.extent.expect("the height the content needs is kept");
        assert_eq!(Some(drawn_on), layer);
        assert!(height > 0.0);
        let input = solidcraft_ui_registry::rect_of("agent_chat:input").expect("the text box is drawn");
        assert_eq!(report.focus.iter().map(|r| r.rect).collect::<Vec<_>>(), [input], "the text box takes the keyboard");
        assert!(chat.actions.is_empty());
    }

    /// The first frame after the chat opens already looks for agents, so it says it is looking
    /// rather than that none were found.
    #[test]
    fn the_first_frame_after_opening_shows_the_agents_being_detected() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        toggle(&mut app);
        assert!(app.agent_chat.open && app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none());
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200.0, 800.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            prepare_shared(&mut app, ui.ctx());
            shared_content(&mut app, ui, Flow::Tall);
        });
        let detecting = tr("ui.agent_chat_detecting");
        let shown = crate::test_support::text_shapes(&output).any(|text| text.galley.text() == detecting);
        output.textures_delta.clear();
        assert!(shown, "the first frame says it is detecting");
    }

    #[test]
    fn assistant_header_and_composer_keep_actions_inside_each_panel_width() {
        for dark in [false, true] {
            solidcraft_theme::set_dark(dark);
            for width in [280.0, 440.0, 760.0] {
                let ctx = crate::fonts::test_context();
                solidcraft_theme::apply(&ctx);
                let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
                app.agent_chat.detection = Some(discover::Detection::default());
                app.agent_chat.picking = Some(false);
                app.agent_chat.state = Some(State::Ready);
                app.agent_chat.input = "Make a plate".to_string();
                let input =
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width + 16.0, 720.0))), ..Default::default() };
                for _ in 0..2 {
                    solidcraft_ui_registry::clear_handles();
                    let mut output = ctx.run_ui(input.clone(), |ui| {
                        ui.set_width(width);
                        ui_kit::scope(ui, |ui| {
                            let mut prefs = AgentPanelPrefs::read(&app.preferences);
                            let view = PanelView::of(&mut app, ui.ctx());
                            header(&mut app.agent_chat, ui, &mut prefs);
                            desktop_body(&mut app.agent_chat, &view, ui, Flow::Tall, &prefs);
                        });
                    });
                    output.textures_delta.clear();
                }
                let controls: Vec<_> = ["close", "newchat", "dock", "permissions", "send"]
                    .into_iter()
                    .map(|name| {
                        let key = format!("agent_chat:{name}");
                        let rect = solidcraft_ui_registry::rect_of(&key).unwrap_or_else(|| panic!("missing {key}"));
                        assert!(rect.left() >= 0.0 && rect.right() <= width + 16.0, "{key} outside {width}: {rect:?}");
                        rect
                    })
                    .collect();
                for (i, a) in controls.iter().enumerate() {
                    for b in controls.iter().skip(i + 1) {
                        assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "overlapping Assistant controls at {width}: {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn assistant_permissions_keep_provider_choices_and_responsive_approval_actions() {
        for width in [280.0, 440.0, 760.0] {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.agent_chat.items.push(Item::Permission {
                key: 4,
                title: "Run solid.fillet".into(),
                answer: None,
                options: vec![
                    PermissionOption { id: "reject".into(), name: "Deny".into(), kind: "reject_once".into() },
                    PermissionOption { id: "remember".into(), name: "Always allow".into(), kind: "allow_always".into() },
                    PermissionOption { id: "once".into(), name: "Allow once".into(), kind: "allow_once".into() },
                    PermissionOption { id: "custom".into(), name: "Review details".into(), kind: "provider_specific".into() },
                ],
            });
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width + 16.0, 720.0))), ..Default::default() };
            solidcraft_ui_registry::clear_handles();
            let view = PanelView::of(&mut app, &ctx);
            let mut output = ctx.run_ui(input, |ui| {
                ui.set_width(width);
                ui_kit::scope(ui, |ui| conversation(&mut app.agent_chat, &view, ui));
            });
            output.textures_delta.clear();
            for id in ["reject", "remember", "once", "custom"] {
                let at = solidcraft_ui_registry::handle_at(&format!("agent_chat:permission:{id}")).expect("provider option retained");
                assert!(at.x > 0.0 && at.x < width + 16.0, "choice outside {width}: {id} {at:?}");
            }
            let once = solidcraft_ui_registry::handle_at("agent_chat:permission:once").expect("once");
            let always = solidcraft_ui_registry::handle_at("agent_chat:permission:remember").expect("always");
            assert!(once.x > always.x, "primary once choice remains on the right");
            assert_eq!(app.agent_chat.waiting(), 1, "drawing never answers an approval");
        }
    }

    #[test]
    fn assistant_agent_mode_and_model_wrap_without_escaping_narrow_width() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.detection = Some(discover::Detection::default());
        app.agent_chat.agent_id = Some("fixture".into());
        app.preferences.agents.push(agents::AgentConfig {
            id: "fixture".into(),
            name: "A coding agent with a long descriptive name".into(),
            command: "fixture".into(),
            ..Default::default()
        });
        app.agent_chat.modes = vec![("edit".into(), "A provider mode with a long descriptive label".into())];
        app.agent_chat.mode = "edit".into();
        app.agent_chat.models = vec![
            acp::ModelInfo {
                id: "model".into(),
                name: "A provider with a long name · A model with a long descriptive label".into(),
                description: String::new(),
            },
            acp::ModelInfo { id: "other".into(), name: "Another provider · model".into(), description: String::new() },
        ];
        app.agent_chat.model = "model".into();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(296.0, 720.0))), ..Default::default() };
        solidcraft_ui_registry::clear_handles();
        let view = PanelView::of(&mut app, &ctx);
        let mut output = ctx.run_ui(input, |ui| {
            ui.set_width(280.0);
            ui_kit::scope(ui, |ui| switcher(&mut app.agent_chat, &view, ui));
        });
        output.textures_delta.clear();
        let rects: Vec<_> = ["switcher", "mode", "model"]
            .into_iter()
            .map(|name| {
                let rect = solidcraft_ui_registry::rect_of(&format!("agent_chat:{name}")).expect("picker retained");
                assert!(rect.left() >= 0.0 && rect.right() <= 296.0, "{name} outside narrow panel: {rect:?}");
                rect
            })
            .collect();
        for (n, a) in rects.iter().enumerate() {
            for b in rects.iter().skip(n + 1) {
                assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "picker overlap: {a:?} {b:?}");
            }
        }
    }
}
