//! The optional panel world shares all command permissions and transaction handling.
use super::*;
use craft_plugin::ui as model;
use panel_ui as wit;
use wasmtime::component::HasSelf;

pub(super) fn enabled(runtime: &craft_plugin::RuntimeManifest) -> bool {
    runtime.ui.is_some()
}

pub(super) fn validate_manifest(manifest: &Manifest, runtime: &craft_plugin::RuntimeManifest) -> Result<()> {
    craft_plugin::check_capabilities(&manifest.capabilities, "the host")?;
    let requested = manifest.capabilities.iter().any(|capability| capability.split_once('@').is_some_and(|(name, _)| name == "ui"));
    if requested != enabled(runtime) || (requested && runtime.is_importer()) {
        return Err(HostError::InvalidManifest {
            field: "capabilities".into(),
            reason: "ui@0.1 and runtime.ui declarations must both be present on a command plugin".into(),
        });
    }
    Ok(())
}

pub(super) fn check_imports(engine: &Engine, component: &Component, runtime: &craft_plugin::RuntimeManifest) -> Result<()> {
    if !enabled(runtime) {
        let package = craft_plugin::API.split_once('@').map(|(package, _)| package).unwrap_or(craft_plugin::API);
        if component.component_type().imports(engine).any(|(name, _)| name.starts_with(&format!("{package}/ui@"))) {
            return Err(HostError::PermissionDenied { permission: "ui capability and panel declarations are not granted".into() });
        }
    }
    Ok(())
}

fn value(value: types::Value) -> model::UiValue {
    match value {
        types::Value::None => model::UiValue::None,
        types::Value::Flag(value) => model::UiValue::Flag(value),
        types::Value::Integer(value) => model::UiValue::Integer(value),
        types::Value::Number(value) => model::UiValue::Number(value),
        types::Value::Text(value) => model::UiValue::Text(value),
        types::Value::GeometryRef(value) => model::UiValue::GeometryRef(value),
    }
}
fn wit_value(value: &model::UiValue) -> types::Value {
    match value {
        model::UiValue::None => types::Value::None,
        model::UiValue::Flag(value) => types::Value::Flag(*value),
        model::UiValue::Integer(value) => types::Value::Integer(*value),
        model::UiValue::Number(value) => types::Value::Number(*value),
        model::UiValue::Text(value) => types::Value::Text(value.clone()),
        model::UiValue::GeometryRef(value) => types::Value::GeometryRef(value.clone()),
    }
}
impl<H: PluginHost> State<H> {
    fn panel(&mut self, panel: &str) -> std::result::Result<(), PluginError> {
        self.check()?;
        if self.panels.as_ref().and_then(|panels| panels.panel(panel)).is_none() {
            return Err(
                self.fail(HostError::InvalidInput { field: "ui.panel".into(), reason: "unknown panel or declarations are not loaded".into() })
            );
        }
        Ok(())
    }
}
impl<H: PluginHost> wit::Host for State<H> {
    fn set_data(&mut self, panel: String, updates: Vec<types::Datum>) -> std::result::Result<(), PluginError> {
        self.panel(&panel)?;
        let updates: Vec<_> = updates.into_iter().map(|datum| model::Datum { path: datum.path, value: value(datum.value) }).collect();
        // Validate independently of the app hook, including finite numbers and data size.
        model::PanelData::default().set_data(&updates).map_err(|error| self.fail(error))?;
        self.host.ui_set_data(&panel, &updates).map_err(|error| self.fail(error))
    }
    fn clear_data(&mut self, panel: String) -> std::result::Result<(), PluginError> {
        self.panel(&panel)?;
        self.host.ui_clear_data(&panel).map_err(|error| self.fail(error))
    }
    fn show_panel(&mut self, panel: String, visible: bool) -> std::result::Result<(), PluginError> {
        self.panel(&panel)?;
        self.host.ui_show_panel(&panel, visible).map_err(|error| self.fail(error))
    }
}
macro_rules! enum_from {
    ($value:expr, $source:ident, $target:ident, $($case:ident),+ $(,)?) => {
        match $value { $(wit::$source::$case => model::$target::$case),+ }
    };
}
fn zone(value: wit::DockZone) -> model::DockZone {
    enum_from!(value, DockZone, DockZone, Left, Right, Bottom)
}
fn button(value: wit::ButtonSpec) -> model::ButtonSpec {
    model::ButtonSpec { label: value.label, icon: value.icon, entry: value.entry }
}
fn widget(value: wit::Widget) -> model::Widget {
    match value {
        wit::Widget::Group(children) => model::Widget::Group(children),
        wit::Widget::Tabs(tabs) => model::Widget::Tabs(tabs.into_iter().map(|tab| model::Tab { label: tab.label, child: tab.child }).collect()),
        wit::Widget::Split(split) => model::Widget::Split(model::SplitSpec { vertical: split.vertical, first: split.first, second: split.second }),
        wit::Widget::Toolbar(buttons) => model::Widget::Toolbar(buttons.into_iter().map(button).collect()),
        wit::Widget::Button(spec) => model::Widget::Button(button(spec)),
        wit::Widget::Form(form) => model::Widget::Form(model::FormSpec {
            bind: form.bind,
            fields: form
                .fields
                .into_iter()
                .map(|field| model::Field {
                    key: field.key,
                    label: field.label,
                    kind: enum_from!(field.kind, FieldKind, FieldKind, Number, Length, Angle, Text, Toggle, Choice, Selection),
                    choices: field.choices,
                    on_change: field.on_change,
                })
                .collect(),
        }),
        wit::Widget::ItemList(list) => model::Widget::ItemList(model::ListSpec { bind: list.bind, on_select: list.on_select }),
        wit::Widget::ItemTree(tree) => model::Widget::ItemTree(model::TreeSpec { bind: tree.bind, on_select: tree.on_select }),
        wit::Widget::Table(table) => model::Widget::Table(model::TableSpec {
            bind: table.bind,
            row_id: table.row_id,
            columns: table.columns.into_iter().map(|column| model::Column { key: column.key, label: column.label, unit: column.unit }).collect(),
            select: enum_from!(table.select, SelectionMode, SelectionMode, None, Single, Multi),
            on_select: table.on_select,
            on_edit: table.on_edit,
        }),
        // The schema stays exact even while phase 3 widgets draw placeholders in this host.
        wit::Widget::Chart(chart) => model::Widget::Chart(model::ChartSpec {
            kind: enum_from!(chart.kind, ChartKind, ChartKind, Line, Bar, Scatter),
            series: chart
                .series
                .into_iter()
                .map(|series| model::ChartSeries {
                    label: series.label,
                    role: match series.role {
                        types::ColorRole::Accent => model::ColorRole::Accent,
                        types::ColorRole::Ok => model::ColorRole::Ok,
                        types::ColorRole::Warn => model::ColorRole::Warn,
                        types::ColorRole::Danger => model::ColorRole::Danger,
                        types::ColorRole::Dim => model::ColorRole::Dim,
                        types::ColorRole::Series1 => model::ColorRole::Series1,
                        types::ColorRole::Series2 => model::ColorRole::Series2,
                        types::ColorRole::Series3 => model::ColorRole::Series3,
                        types::ColorRole::Series4 => model::ColorRole::Series4,
                        types::ColorRole::Series5 => model::ColorRole::Series5,
                        types::ColorRole::Series6 => model::ColorRole::Series6,
                        types::ColorRole::Series7 => model::ColorRole::Series7,
                        types::ColorRole::Series8 => model::ColorRole::Series8,
                    },
                    x: series.x,
                    y: series.y,
                })
                .collect(),
        }),
        wit::Widget::Text(text) => model::Widget::Text(text),
        wit::Widget::Progress(path) => model::Widget::Progress(path),
        wit::Widget::EmptyState(text) => model::Widget::EmptyState(text),
    }
}
fn declarations(value: wit::Declarations) -> model::Declarations {
    model::Declarations {
        ui_version: value.ui_version,
        panels: value
            .panels
            .into_iter()
            .map(|panel| model::PanelSpec {
                id: panel.id,
                title: panel.title,
                zones: panel.zones.into_iter().map(zone).collect(),
                layout: model::Layout {
                    root: panel.layout.root,
                    nodes: panel.layout.nodes.into_iter().map(|node| model::Node { widget: widget(node.widget) }).collect(),
                },
            })
            .collect(),
        workspaces: value
            .workspaces
            .into_iter()
            .map(|workspace| model::WorkspaceSpec {
                id: workspace.id,
                name: workspace.name,
                icon: workspace.icon,
                applies_to: workspace
                    .applies_to
                    .into_iter()
                    .map(|kind| enum_from!(kind, DesignKind, DesignKind, Part, Assembly, Sketch, Drawing))
                    .collect(),
                ribbon: workspace
                    .ribbon
                    .into_iter()
                    .map(|tab| model::RibbonTab {
                        name: tab.name,
                        groups: tab.groups.into_iter().map(|group| model::RibbonGroup { name: group.name, entries: group.entries }).collect(),
                    })
                    .collect(),
                panels: workspace.panels.into_iter().map(|(dock, panels)| (zone(dock), panels)).collect(),
                builtin_panels: workspace.builtin_panels,
                on_enter: workspace.on_enter,
                on_exit: workspace.on_exit,
            })
            .collect(),
    }
}
fn event(value: &model::UiEvent) -> wit::UiEvent {
    wit::UiEvent {
        panel: value.panel.clone(),
        path: value.path.clone(),
        row: value.row.clone(),
        value: value.value.as_ref().map(wit_value),
        event: match value.event {
            model::EventKind::Press => wit::EventKind::Press,
            model::EventKind::Select => wit::EventKind::Select,
            model::EventKind::Edit => wit::EventKind::Edit,
            model::EventKind::Change => wit::EventKind::Change,
            model::EventKind::Enter => wit::EventKind::Enter,
            model::EventKind::Exit => wit::EventKind::Exit,
        },
    }
}
fn validate(value: model::Declarations, runtime: &craft_plugin::RuntimeManifest) -> Result<model::Declarations> {
    value.validate(runtime)?;
    if runtime.ui.as_ref().is_some_and(|approved| approved != &value) {
        return Err(HostError::InvalidManifest {
            field: "runtime.ui".into(),
            reason: "ui-provider.describe differs from the manifest's approved declarations".into(),
        });
    }
    Ok(value)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn call_entry<H: PluginHost + 'static>(
    engine: &Engine,
    store: &mut Store<State<H>>,
    component: &Component,
    runtime: &craft_plugin::RuntimeManifest,
    entry: &str,
    params: &str,
    action: Option<&model::UiEvent>,
) -> wasmtime::Result<Result<String>> {
    let mut linker = Linker::new(engine);
    panel_bindings::PanelPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
    // Instantiation and static approval declarations run without command, file or clock
    // authority, even when the eventual entry is an edit. A swallowed refusal fails the call.
    let commands = std::mem::take(&mut store.data_mut().commands);
    let folders = std::mem::take(&mut store.data_mut().folders);
    let clock = std::mem::replace(&mut store.data_mut().clock, false);
    let effect = std::mem::replace(&mut store.data_mut().effect, Effect::Read);
    let described = (|| -> wasmtime::Result<_> {
        let guest = panel_bindings::PanelPlugin::instantiate(&mut *store, component, &linker)?;
        let described = panel_provider!(guest).call_describe(&mut *store)?;
        Ok((guest, described))
    })();
    store.data_mut().commands = commands;
    store.data_mut().folders = folders;
    store.data_mut().clock = clock;
    store.data_mut().effect = effect;
    let (guest, described) = described?;
    let panels = match validate(declarations(described), runtime) {
        Ok(panels) => panels,
        Err(error) => return Ok(Err(error)),
    };
    if let Some(action) = action {
        if let Err(error) = action.validate() {
            return Ok(Err(error));
        }
        let declared = panels.panel(&action.panel).is_some_and(|panel| {
            panel.layout.nodes.iter().enumerate().any(|(node, _)| {
                u32::try_from(node)
                    .ok()
                    .and_then(|node| panels.action_command("panel", node, action).ok())
                    .is_some_and(|command| command == format!("panel.{entry}"))
            })
        });
        if !declared {
            return Ok(Err(HostError::InvalidInput {
                field: "ui.event".into(),
                reason: "event does not name this entry's declared panel action".into(),
            }));
        }
    }
    store.data_mut().panels = Some(panels);
    if let Some(action) = action
        && let Err(error) = panel_provider!(guest).call_on_event(&mut *store, &event(action))?
    {
        return Ok(Err(host_error(error)));
    }
    Ok(call_entry!(guest, &mut *store, entry, &params.to_owned())?.map_err(host_error))
}

pub(crate) fn describe<H: PluginHost + 'static>(
    mut host: H,
    manifest: &Manifest,
    bytes: &[u8],
    options: RuntimeOptions,
) -> (H, Result<model::Declarations>) {
    let prepared = (|| {
        let runtime = manifest
            .runtime
            .as_ref()
            .ok_or_else(|| HostError::InvalidManifest { field: "runtime".into(), reason: "no executable runtime".into() })?;
        runtime.validate()?;
        validate_manifest(manifest, runtime)?;
        if !enabled(runtime) {
            return Err(HostError::PermissionDenied { permission: "ui capability is not declared".into() });
        }
        if bytes.len() > runtime.max_component_bytes() {
            return Err(limit("component size"));
        }
        if host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        let stack = stack_bytes(&runtime.limits, options, false)?;
        let (engine, component) = compile(bytes, stack)?;
        if super::check_imports(&engine, &component)? != Wit::Current {
            return Err(component_error("ui requires the current WIT version"));
        }
        Ok((runtime, engine, component, stack))
    })();
    let (runtime, engine, component, stack) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return (host, Err(error)),
    };
    let limits = &runtime.limits;
    // No command or filesystem authority while reading static approval declarations.
    let state = State {
        design_tokens: false,
        host,
        commands: BTreeMap::new(),
        groups: BTreeSet::new(),
        granted_capabilities: Vec::new(),
        effect: Effect::Read,
        user_storage: false,
        folders: BTreeMap::new(),
        input_folders: BTreeMap::new(),
        folder_bytes: 0,
        folder_files: BTreeSet::new(),
        staged: BTreeMap::new(),
        created_dirs: Vec::new(),
        clock: false,
        forms: false,
        limits: store_limits(limits, 10_000),
        failed: None,
        calls: 0,
        max_calls: MAX_CALLS,
        log_bytes: 0,
        deadline: Instant::now() + Duration::from_millis(limits.deadline_ms),
        import: None,
        panels: None,
    };
    let mut store = Store::new(&engine, state);
    let outcome = (|| {
        prepare_store(&mut store, limits)?;
        let response = with_interrupts(&engine, || -> wasmtime::Result<_> {
            let mut linker = Linker::new(&engine);
            panel_bindings::PanelPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;
            let guest = panel_bindings::PanelPlugin::instantiate(&mut store, &component, &linker)?;
            panel_provider!(guest).call_describe(&mut store)
        })?;
        if let Some(error) = store.data().failed.clone() {
            return Err(error);
        }
        let result = response.map_err(|error| execution_error(&error, store.get_fuel().ok(), limits, stack))?;
        if store.data().host.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if Instant::now() >= store.data().deadline {
            return Err(deadline());
        }
        validate(declarations(result), runtime)
    })();
    host = store.into_data().host;
    (host, outcome.map_err(|error| with_limits(error, limits.deadline_ms)))
}
