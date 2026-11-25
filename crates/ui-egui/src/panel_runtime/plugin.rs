//! Declarative plugin content. The shared host owns placement, chrome and widgets.
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use serde_json::json;
use sha2::{Digest, Sha256};
use solidcraft_engine::panels::*;
use solidcraft_engine::plugin::{PanelSnapshot, ui as model};

use super::*;

pub(super) const PREFIX: &str = "plugin.";
const MAX_ACTIONS: usize = 16;

#[derive(Default)]
struct Snapshot {
    panels: BTreeMap<String, PanelSnapshot>,
    actions: VecDeque<QueuedAction>,
    visibility: BTreeMap<(u64, String, String), u64>,
    document: String,
}
struct QueuedAction {
    document: String,
    approval: String,
    plugin: String,
    entry: String,
    event: model::UiEvent,
}
#[derive(Clone, Default)]
pub(super) struct PluginState(Rc<RefCell<Snapshot>>);

impl PluginState {
    pub(super) fn initial(session: &solidcraft_engine::Session) -> PanelResult<Self> {
        let panels = session.plugin_panels().into_iter().map(|panel| Ok((instance(&panel)?.0, panel))).collect::<PanelResult<BTreeMap<_, _>>>()?;
        Ok(Self(Rc::new(RefCell::new(Snapshot { panels, document: session.document_id.clone(), ..Default::default() }))))
    }
    pub(super) fn register_specs(&self, registry: &mut PanelRegistry) -> PanelResult<()> {
        let snapshot = self.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "plugin panel declarations borrowed"))?;
        for panel in snapshot.panels.values() {
            registry.register(registration(panel)?)?;
        }
        Ok(())
    }
    pub(super) fn register_controllers(&self, runtime: &mut PanelRuntime) -> PanelResult<()> {
        let snapshot = self.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "plugin panel declarations borrowed"))?;
        for (id, panel) in &snapshot.panels {
            runtime.register(registration(panel)?.kind, self.binding(id))?;
        }
        Ok(())
    }
    /// Make one declared panel a registered kind, as a workspace layout needs it. Idempotent.
    pub(crate) fn ensure_registered(&self, runtime: &mut PanelRuntime, plugin: &str, panel: &model::PanelSpec) -> PanelResult<InstanceId> {
        let id = instance_for(plugin, panel)?;
        let spec = registration_for(plugin, panel)?;
        if runtime.layout().registry().get(&spec.kind).is_err() {
            runtime.register_panel(spec, self.binding(&id.0))?;
        }
        Ok(id)
    }
    fn binding(&self, id: &str) -> ControllerBinding {
        let state = self.clone();
        let id = id.to_owned();
        ControllerBinding {
            ownership: Ownership::Document,
            create: Box::new(move |_, _| Ok(Box::new(PluginController { id: id.clone(), state: state.clone() }))),
        }
    }
}

fn instance(panel: &PanelSnapshot) -> PanelResult<InstanceId> {
    instance_for(&panel.plugin, &panel.panel)
}

/// The panel identity a plugin's declared panel has in every workspace layout.
pub(crate) fn instance_for(plugin: &str, panel: &model::PanelSpec) -> PanelResult<InstanceId> {
    // Full SHA-256 in base32 keeps two 64-byte manifest ids inside the host's
    // 64-byte id bound. Delimit the two names, so their concatenations cannot alias.
    let mut hash = Sha256::new();
    hash.update(plugin.as_bytes());
    hash.update([0]);
    hash.update(panel.id.as_bytes());
    hash.update([0]);
    // A changed zone contract gets a new legal placement; ordinary updates keep
    // the same identity and the user's workspace placement memory.
    for zone in &panel.zones {
        hash.update([match zone {
            model::DockZone::Left => 0,
            model::DockZone::Right => 1,
            model::DockZone::Bottom => 2,
        }]);
    }
    let mut name = String::from(PREFIX);
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    let letter = |value: u8| char::from(if value < 26 { b'a' + value } else { b'2' + value - 26 });
    for byte in hash.finalize() {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            name.push(letter(((buffer >> bits) & 31) as u8));
        }
    }
    if bits > 0 {
        name.push(letter(((buffer << (5 - bits)) & 31) as u8));
    }
    InstanceId::new(name)
}

pub(super) fn registration(panel: &PanelSnapshot) -> PanelResult<PanelSpec> {
    registration_for(&panel.plugin, &panel.panel)
}

pub(crate) fn registration_for(plugin: &str, panel: &model::PanelSpec) -> PanelResult<PanelSpec> {
    let allowed_zones: BTreeSet<_> = panel
        .zones
        .iter()
        .map(|zone| match zone {
            model::DockZone::Left => Zone::Left,
            model::DockZone::Right => Zone::Right,
            model::DockZone::Bottom => Zone::Bottom,
        })
        .chain([Zone::Floating])
        .collect();
    let default_zone = panel
        .zones
        .first()
        .map(|zone| match zone {
            model::DockZone::Left => Zone::Left,
            model::DockZone::Right => Zone::Right,
            model::DockZone::Bottom => Zone::Bottom,
        })
        .ok_or_else(|| error(Rule::Placement, "plugin panel requires a dock zone"))?;
    Ok(PanelSpec {
        kind: PanelKind::new(instance_for(plugin, panel)?.0)?,
        title_key: "ui.plugin_panel_badge".into(),
        icon: crate::plugin_images::GENERIC.into(),
        allowed_zones,
        default_zone,
        min_size: Size { width: 240.0, height: solidcraft_theme::Tokens::get().control_height },
        default_size: Size { width: crate::prefs::DEFAULT_BROWSER_WIDTH, height: 320.0 },
        max_size: Size { width: MAX_LOGICAL, height: MAX_LOGICAL },
        edge_rule: EdgeRule::None,
        surface: PanelSurface::Translucent,
        features: PanelFeatures { popout: false, ..Default::default() },
        side_height_policy: SideHeightPolicy::Content,
        fixed_bottom_height: None,
        header_actions: Vec::new(),
        text_heavy: true,
    })
}

/// Merge writes and capture data once per egui frame. No plugin code runs here.
pub(super) fn sync(app: &mut crate::SolveApp) -> PanelResult<()> {
    // Restore the actual workspace before consuming a guest visibility request.
    if app.panel_runtime.as_ref().and_then(|integration| integration.preferences.as_ref()).is_some_and(|preferences| preferences.pending.is_some()) {
        return Ok(());
    }
    app.session.merge_plugin_panel_data();
    let panels = app.session.plugin_panels();
    let Some(integration) = app.panel_runtime.as_mut().filter(|integration| integration.production.is_some()) else { return Ok(()) };
    let state = integration.plugins.clone();
    let actions = {
        let mut snapshot = state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "plugin panel data borrowed"))?;
        snapshot.document = app.session.document_id.clone();
        snapshot.panels = panels.into_iter().map(|panel| Ok((instance(&panel)?.0, panel))).collect::<PanelResult<_>>()?;
        std::mem::take(&mut snapshot.actions)
    };
    for QueuedAction { document, approval, plugin, entry, event } in actions {
        if document != app.session.document_id {
            continue;
        }
        let current = state.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "plugin panel data borrowed"))?;
        let panel =
            current.panels.values().find(|panel| panel.plugin == plugin && panel.panel.id == event.panel && panel.approval == approval).cloned();
        drop(current);
        if let Some(panel) = panel {
            if event.event == model::EventKind::Change && event.value.is_none() {
                crate::plugin_menu::start_panel_picking(app, plugin, approval, entry, event);
                continue;
            }
            let params = json!({"approval":panel.approval,"params":event,"ui_event":event});
            if let Err(reason) = crate::plugin_menu::run_panel(app, format!("{plugin}.{entry}"), params) {
                app.set_status(reason, true);
            }
        }
    }
    let panels = state.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "plugin panel data borrowed"))?.panels.clone();
    let mut failure = None;
    for (id, panel) in panels {
        // A plugin workspace shows exactly the panels it places; the others wait for a built-in workspace.
        if !crate::plugin_workspaces::panel_allowed(app, &panel.plugin, &panel.panel.id) {
            continue;
        }
        let result = sync_visibility(app, &state, id, panel);
        if failure.is_none() {
            failure = result.err();
        }
    }
    failure.map_or(Ok(()), Err)
}

fn sync_visibility(app: &mut crate::SolveApp, state: &PluginState, id: String, panel: PanelSnapshot) -> PanelResult<()> {
    let spec = registration(&panel)?;
    // Guest counters restart in each document execution context.
    let visibility_key = (app.session.uid, app.session.document_id.clone(), id.clone());
    let integration = app.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "plugin panel host missing"))?;
    if integration.runtime.layout().registry().get(&spec.kind).is_err() {
        integration.runtime.register_panel(spec.clone(), state.binding(&id))?;
    }
    let layout = integration.runtime.layout().layout();
    let existing = layout.instance(&InstanceId(id.clone())).ok();
    let prior =
        state.0.try_borrow().map_err(|_| error(Rule::Lifecycle, "plugin panel visibility borrowed"))?.visibility.get(&visibility_key).copied();
    let request = panel.visibility_revision > 0 && prior != Some(panel.visibility_revision);
    let open = existing.is_none() || (request && panel.visible && existing.is_some_and(|instance| !instance.requested_open));
    let close = request && !panel.visible && (existing.is_none() || existing.is_some_and(|instance| instance.requested_open));
    if !open && !close {
        state
            .0
            .try_borrow_mut()
            .map_err(|_| error(Rule::Lifecycle, "plugin panel visibility borrowed"))?
            .visibility
            .insert(visibility_key, panel.visibility_revision);
        return Ok(());
    }
    let mut mutations = Vec::new();
    if open {
        let destination = if existing.is_none() {
            let section = next_section_id_with_reservations(layout, &integration.preference_identity_reservations()?)?;
            Some(
                if let Some(anchor) = layout.sections.iter().find(|section| spec.default_zone.side() && section.location.zone() == spec.default_zone)
                {
                    Destination::Relative {
                        section: anchor.id.clone(),
                        placement: RelativePlacement::Below,
                        new_section: section,
                        new_column: None,
                        size: spec.default_size,
                    }
                } else {
                    Destination::NewSection {
                        section,
                        index: layout.sections.len(),
                        location: SectionLocation::Dock { zone: spec.default_zone },
                        size: spec.default_size,
                    }
                },
            )
        } else {
            None
        };
        mutations.push(Mutation::Open { kind: spec.kind, instance: InstanceId(id.clone()), destination });
    }
    if close {
        mutations.push(Mutation::Close { instance: InstanceId(id.clone()) });
    }
    let revision = integration.runtime.layout().inspect().revision;
    app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: Some(revision), mutations })?;
    state
        .0
        .try_borrow_mut()
        .map_err(|_| error(Rule::Lifecycle, "plugin panel visibility borrowed"))?
        .visibility
        .insert(visibility_key, panel.visibility_revision);
    Ok(())
}

pub(crate) fn queue_selection(app: &crate::SolveApp, plugin: String, approval: String, entry: String, event: model::UiEvent) {
    if let Some(integration) = app.panel_runtime.as_ref()
        && let Ok(mut state) = integration.plugins.0.try_borrow_mut()
        && state.actions.len() < MAX_ACTIONS
    {
        state.actions.push_back(QueuedAction { document: app.session.document_id.clone(), approval, plugin, entry, event });
    }
}

struct PluginController {
    id: String,
    state: PluginState,
}
impl PluginController {
    fn panel(&self) -> PanelResult<PanelSnapshot> {
        self.state
            .0
            .try_borrow()
            .map_err(|_| error(Rule::Lifecycle, "plugin panel data borrowed"))?
            .panels
            .get(&self.id)
            .cloned()
            .ok_or_else(|| error(Rule::Availability, "plugin panel removed or disabled"))
    }
}
impl PanelController for PluginController {
    fn availability(&self, owner: &ContentOwner, _models: &dyn PanelModels) -> Availability {
        let current =
            self.state.0.try_borrow().is_ok_and(|snapshot| matches!(owner, ContentOwner::Document(document) if document.0 == snapshot.document));
        if current && self.panel().is_ok() {
            Availability::Available
        } else {
            Availability::Unavailable { reason: solidcraft_ui_text::tr("ui.plugin_dialog_disabled").into() }
        }
    }
    fn update(&mut self, _owner: &ContentOwner, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Ok(())
    }
    fn presentation(&self) -> PanelPresentation {
        let panel = self.panel().ok();
        PanelPresentation {
            title: panel
                .as_ref()
                .map(|panel| {
                    let title = format!("{} · {} · {}", solidcraft_ui_text::tr("ui.plugin_panel_badge"), panel.plugin_name, panel.panel.title);
                    if title.len() <= 256 {
                        title
                    } else {
                        let mut short = String::new();
                        for ch in title.chars() {
                            if short.len() + ch.len_utf8() > 253 {
                                break;
                            }
                            short.push(ch);
                        }
                        short.push('…');
                        short
                    }
                })
                .unwrap_or_else(|| solidcraft_ui_text::tr("ui.plugin_panel_badge").into()),
            icon: crate::plugin_images::GENERIC.into(),
            status: Some(solidcraft_ui_text::tr("ui.plugin_panel_badge").into()),
            attention: false,
            actions: Vec::new(),
        }
    }
    fn prepare_lifecycle(&self, _event: &LifecycleEvent) -> PanelResult<()> {
        Ok(())
    }
    fn lifecycle(&mut self, _event: &LifecycleEvent) {}
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        if !matches!(slot, ContentSlot::Body | ContentSlot::Inline) {
            return Ok(());
        }
        let panel = self.panel()?;
        if panel.ui_version != model::UI_VERSION {
            ui.label(solidcraft_ui_text::tr("ui.plugin_panel_newer"));
            return Ok(());
        }
        self.availability(&context.owner, services.models).require()?;
        let units = panel.units.as_str();
        let mut events = Vec::new();
        ui.push_id((&context.workspace.0, &self.id), |ui| {
            draw(ui, &panel, panel.panel.layout.root, &mut events, units, 0);
        });
        if !context.preview_geometry {
            let mut snapshot = self.state.0.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "plugin panel actions borrowed"))?;
            let document = snapshot.document.clone();
            for (entry, event) in events {
                if snapshot.actions.len() < MAX_ACTIONS {
                    snapshot.actions.push_back(QueuedAction {
                        document: document.clone(),
                        approval: panel.approval.clone(),
                        plugin: panel.plugin.clone(),
                        entry,
                        event,
                    });
                }
            }
        }
        Ok(())
    }
    fn action(&mut self, _owner: &ContentOwner, _action: &ControlId, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Err(error(Rule::Capability, "plugin panel has no header actions"))
    }
}

fn event(panel: &PanelSnapshot, kind: model::EventKind, path: &str, row: Option<String>, value: Option<model::UiValue>) -> model::UiEvent {
    model::UiEvent { panel: panel.panel.id.clone(), event: kind, path: path.into(), row, value }
}
pub(super) type Actions = Vec<(String, model::UiEvent)>;

#[derive(Clone)]
struct TextDraft {
    observed_source: String,
    value: String,
    submitted: Option<String>,
}

fn button(ui: &mut egui::Ui, panel: &PanelSnapshot, spec: &model::ButtonSpec, actions: &mut Actions) {
    let response = crate::ui_kit::icon_label_action(ui, &spec.label, spec.icon.as_deref(), true, false);
    crate::scenario::publish_rect(&format!("plugin-panel:{}:{}:button:{}", panel.plugin, panel.panel.id, spec.entry), response.rect);
    if response.clicked() {
        actions.push((spec.entry.clone(), event(panel, model::EventKind::Press, &spec.entry, None, None)));
    }
}

fn value_text(value: Option<&model::UiValue>, unit: Option<&str>, units: &str) -> String {
    match value {
        Some(model::UiValue::Text(text) | model::UiValue::GeometryRef(text)) => text.clone(),
        Some(model::UiValue::Flag(flag)) => flag.to_string(),
        Some(model::UiValue::Integer(number)) => match unit {
            Some("length" | "mm") => crate::prefs::format_length(*number as f64, units, 3),
            Some("angle" | "deg") => format!("{number}°"),
            _ => number.to_string(),
        },
        Some(model::UiValue::Number(number)) => match unit {
            Some("length" | "mm") => crate::prefs::format_length(*number, units, 3),
            Some("angle" | "deg") => format!("{number}°"),
            _ => number.to_string(),
        },
        _ => String::new(),
    }
}

pub(super) fn draw(ui: &mut egui::Ui, panel: &PanelSnapshot, index: u32, actions: &mut Actions, units: &str, depth: usize) {
    if depth == 0 && panel.ui_version != model::UI_VERSION {
        ui.label(solidcraft_ui_text::tr("ui.plugin_panel_newer"));
        return;
    }
    if depth >= model::MAX_WIDGETS {
        return;
    }
    let Some(node) = usize::try_from(index).ok().and_then(|index| panel.panel.layout.nodes.get(index)) else { return };
    ui.push_id(index, |ui| match &node.widget {
        model::Widget::Group(children) => {
            for child in children {
                draw(ui, panel, *child, actions, units, depth + 1);
            }
        }
        model::Widget::Tabs(tabs) => {
            let id = ui.scope_id().with("plugin-tabs");
            let mut active = ui.ctx().data_mut(|data| data.get_temp::<usize>(id).filter(|index| *index < tabs.len()).unwrap_or_default());
            let labels: Vec<&str> = tabs.iter().map(|tab| tab.label.as_str()).collect();
            crate::ui_kit::segmented(ui, &mut active, &labels, crate::ui_kit::Segments { compact: true, ..Default::default() });
            ui.ctx().data_mut(|data| data.insert_temp(id, active));
            if let Some(tab) = tabs.get(active) {
                draw(ui, panel, tab.child, actions, units, depth + 1);
            }
        }
        model::Widget::Split(split) => {
            if split.vertical {
                draw(ui, panel, split.first, actions, units, depth + 1);
                ui.separator();
                draw(ui, panel, split.second, actions, units, depth + 1);
            } else {
                ui.columns(2, |columns| {
                    if let Some(first) = columns.first_mut() {
                        draw(first, panel, split.first, actions, units, depth + 1);
                    }
                    if let Some(second) = columns.get_mut(1) {
                        draw(second, panel, split.second, actions, units, depth + 1);
                    }
                });
            }
        }
        model::Widget::Toolbar(buttons) => {
            ui.horizontal_wrapped(|ui| {
                for spec in buttons {
                    button(ui, panel, spec, actions);
                }
            });
        }
        model::Widget::Button(spec) => button(ui, panel, spec, actions),
        model::Widget::Table(spec) => table(ui, panel, spec, actions, units),
        model::Widget::ItemList(spec) => {
            let rows = panel.data.rows(&spec.bind);
            if rows.is_empty() {
                ui.label(solidcraft_ui_text::tr("ui.plugin_panel_empty"));
            }
            let selected_id = ui.scope_id().with("plugin-list-selection");
            let mut selected = ui.ctx().data_mut(|data| data.get_temp::<String>(selected_id));
            egui::ScrollArea::vertical().id_salt("plugin-list").show_rows(ui, crate::ui_kit::row_height(), rows.len(), |ui, range| {
                for index in range {
                    if let Some((id, fields)) = rows.get(index) {
                        let text = value_text(fields.get("label").or_else(|| fields.get("name")).or_else(|| fields.values().next()), None, "mm");
                        let response = crate::ui_kit::list_picker(ui, id, &text, selected.as_ref() == Some(id));
                        if response.clicked()
                            && let Some(entry) = &spec.on_select
                        {
                            selected = Some(id.clone());
                            actions.push((entry.clone(), event(panel, model::EventKind::Select, &spec.bind, Some(id.clone()), None)));
                        }
                    }
                }
            });
            ui.ctx().data_mut(|data| data.insert_temp(selected_id, selected.unwrap_or_default()));
        }
        model::Widget::Form(spec) => form(ui, panel, spec, actions, units),
        model::Widget::Text(text) | model::Widget::EmptyState(text) => {
            ui.label(text);
        }
        model::Widget::Progress(path) => {
            let amount = match panel.data.get(path) {
                Some(model::UiValue::Number(value)) => *value,
                Some(model::UiValue::Integer(value)) => *value as f64,
                _ => 0.0,
            };
            let fraction = amount.clamp(0.0, 1.0) as f32;
            crate::ui_kit::progress(ui, &format!("plugin_panel:{}:{}", panel.plugin, panel.panel.id), fraction);
            ui.label(format!("{:.0} %", fraction * 100.0));
        }
        model::Widget::ItemTree(_) | model::Widget::Chart(_) | model::Widget::Unknown => {
            ui.label(solidcraft_ui_text::tr("ui.plugin_panel_newer"));
        }
    });
}

pub(super) const TABLE_ROW_INDENT: f32 = 14.0;

fn table(ui: &mut egui::Ui, panel: &PanelSnapshot, spec: &model::TableSpec, actions: &mut Actions, units: &str) {
    let rows = panel.data.rows(&spec.bind);
    let columns: Vec<_> = spec.columns.iter().map(|column| crate::ui_kit::Column::new(&column.label, 1.0).flex()).collect();
    let selected_id = ui.scope_id().with("plugin-selection");
    let mut selected = ui.ctx().data_mut(|data| data.get_temp::<BTreeSet<String>>(selected_id).unwrap_or_default());
    crate::ui_kit::table_custom(
        ui,
        &format!("plugin:{}:{}:{}", panel.plugin, panel.panel.id, spec.bind),
        &columns,
        solidcraft_ui_text::tr("ui.plugin_panel_empty"),
        |ui, table| {
            table.headers(ui, |_, _| false);
            egui::ScrollArea::vertical().id_salt("plugin-table").show_rows(ui, crate::ui_kit::table_row_height(), rows.len(), |ui, range| {
                for index in range {
                    if let Some((key, fields)) = rows.get(index) {
                        let row_id = value_text(fields.get(&spec.row_id), None, "mm");
                        let row_id = if row_id.is_empty() { key.clone() } else { row_id };
                        let indent = match fields.get("indent") {
                            Some(model::UiValue::Integer(level)) => (*level).clamp(0, model::MAX_TABLE_ROW_INDENT as i64) as usize,
                            Some(model::UiValue::Number(level)) => (*level as i64).clamp(0, model::MAX_TABLE_ROW_INDENT as i64) as usize,
                            Some(model::UiValue::Text(text)) => {
                                text.parse::<i64>().unwrap_or(0).clamp(0, model::MAX_TABLE_ROW_INDENT as i64) as usize
                            }
                            _ => 0,
                        };
                        let response = table.row(ui, key, selected.contains(&row_id), |row| {
                            for (column_index, column) in spec.columns.iter().enumerate() {
                                row.cell(|ui| {
                                    if column_index == 0 && indent > 0 {
                                        ui.add_space(indent as f32 * TABLE_ROW_INDENT);
                                    }
                                    ui.label(value_text(fields.get(&column.key), column.unit.as_deref(), units));
                                });
                            }
                        });
                        crate::scenario::publish_rect(&format!("plugin-panel:{}:{}:row:{row_id}", panel.plugin, panel.panel.id), response.rect);
                        if response.double_clicked()
                            && let Some(entry) = &spec.on_edit
                        {
                            actions.push((entry.clone(), event(panel, model::EventKind::Edit, &spec.bind, Some(row_id.clone()), None)));
                        } else if response.clicked() && spec.select != model::SelectionMode::None {
                            if spec.select == model::SelectionMode::Single {
                                selected.clear();
                            }
                            if !selected.insert(row_id.clone()) {
                                selected.remove(&row_id);
                            }
                            if let Some(entry) = &spec.on_select {
                                actions.push((entry.clone(), event(panel, model::EventKind::Select, &spec.bind, Some(row_id), None)));
                            }
                        }
                    }
                }
            });
        },
    );
    ui.ctx().data_mut(|data| data.insert_temp(selected_id, selected));
}

fn form(ui: &mut egui::Ui, panel: &PanelSnapshot, spec: &model::FormSpec, actions: &mut Actions, units: &str) {
    for field in &spec.fields {
        let path = format!("{}/{}", spec.bind, field.key);
        let current = panel.data.get(&path);
        let mut changed = None;
        ui.add_enabled_ui(field.on_change.is_some(), |ui| match field.kind {
            model::FieldKind::Toggle => {
                let mut value = matches!(current, Some(model::UiValue::Flag(true)));
                if crate::ui_kit::row(ui, &field.label, |ui| crate::ui_kit::checkbox(ui, "", &mut value)).changed() {
                    changed = Some(model::UiValue::Flag(value));
                }
            }
            model::FieldKind::Choice => {
                let mut selected =
                    field.choices.iter().position(|choice| matches!(current, Some(model::UiValue::Text(text)) if text == choice)).unwrap_or_default();
                let options: Vec<_> = field.choices.iter().map(String::as_str).collect();
                if crate::ui_kit::choice_row(ui, &field.label, &path, &mut selected, &options).changed()
                    && let Some(value) = field.choices.get(selected)
                {
                    changed = Some(model::UiValue::Text(value.clone()));
                }
            }
            model::FieldKind::Number | model::FieldKind::Length | model::FieldKind::Angle => {
                let mut value = match current {
                    Some(model::UiValue::Number(value)) => *value,
                    Some(model::UiValue::Integer(value)) => *value as f64,
                    _ => 0.0,
                };
                let (scale, unit) = crate::prefs::unit_scale(units);
                let scale = if field.kind == model::FieldKind::Length { scale } else { 1.0 };
                value *= scale;
                let suffix = match field.kind {
                    model::FieldKind::Length => format!(" {unit}"),
                    model::FieldKind::Angle => "°".into(),
                    _ => String::new(),
                };
                if crate::ui_kit::row(ui, &field.label, |ui| {
                    crate::ui_kit::number_field(ui, &mut value, f64::NEG_INFINITY..=f64::INFINITY, 0.1, &suffix)
                })
                .changed()
                {
                    changed = Some(model::UiValue::Number(value / scale));
                }
            }
            model::FieldKind::Selection => {
                let text = value_text(current, None, units);
                let response = crate::ui_kit::row(ui, &field.label, |ui| {
                    crate::ui_kit::selection_field(ui, &text, crate::ui_kit::Selection { active: false, filled: !text.is_empty() })
                });
                if response.clicked()
                    && let Some(entry) = &field.on_change
                {
                    actions.push((entry.clone(), event(panel, model::EventKind::Change, &path, None, None)));
                }
            }
            model::FieldKind::Text => {
                let id = ui.scope_id().with(&path);
                let source = value_text(current, None, units);
                let mut draft = ui.ctx().data_mut(|data| data.get_temp::<TextDraft>(id)).unwrap_or_else(|| TextDraft {
                    observed_source: source.clone(),
                    value: source.clone(),
                    submitted: None,
                });
                if draft.observed_source != source {
                    // Only an observed acknowledgement of the latest edit ends its pending state.
                    if draft.submitted.as_ref() == Some(&source) {
                        draft.submitted = None;
                    }
                    draft.observed_source.clone_from(&source);
                }
                if draft.submitted.is_none() {
                    draft.value.clone_from(&source);
                }
                let (_, response) = crate::ui_kit::row(ui, &field.label, |ui| crate::ui_kit::text_field_reserving(ui, &mut draft.value, "", 0.0));
                if response.changed() {
                    draft.submitted = Some(draft.value.clone());
                    changed = Some(model::UiValue::Text(draft.value.clone()));
                }
                if response.lost_focus() {
                    draft.submitted = None;
                }
                ui.ctx().data_mut(|data| data.insert_temp(id, draft));
            }
        });
        if let (Some(value), Some(entry)) = (changed, &field.on_change) {
            actions.push((entry.clone(), event(panel, model::EventKind::Change, &path, None, Some(value))));
        }
    }
}
