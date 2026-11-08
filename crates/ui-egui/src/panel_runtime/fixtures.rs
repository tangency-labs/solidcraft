//! Shared executable mount/editor/native fixtures. These are synthetic specimens,
//! not production panel migrations, UI commands or evidence of visual correctness.
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use solidcraft_engine::panels::*;

use super::*;

#[derive(Clone, Debug, Default)]
pub struct FixtureState {
    pub updates: u64,
    pub renders: u64,
    pub cancels: u64,
    pub reopens: u64,
    pub applies: u64,
    pub finishes: u64,
    pub pulses: u64,
    pub reparents: u64,
    pub destroys: u64,
    pub transaction_live: bool,
    pub reject_cancel: bool,
    pub reject_reopen: bool,
    pub fail_update: bool,
    pub invalid_availability: bool,
    pub last_state: Option<ControllerState>,
    pub draft: String,
    pub last_clip: Option<egui::Rect>,
}

#[derive(Clone, Default)]
pub struct FixtureProbe {
    states: Rc<RefCell<BTreeMap<ControllerKey, Rc<RefCell<FixtureState>>>>>,
}
impl FixtureProbe {
    pub fn state(&self, key: &ControllerKey) -> PanelResult<FixtureState> {
        let states = self.states.try_borrow().map_err(|_| error(Rule::Lifecycle, "fixture probe borrowed"))?;
        let state = states.get(key).ok_or_else(|| error(Rule::MissingTarget, "fixture controller missing"))?;
        let value = state.try_borrow().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?.clone();
        Ok(value)
    }
    pub fn edit(&self, key: &ControllerKey, edit: impl FnOnce(&mut FixtureState)) -> PanelResult<()> {
        let states = self.states.try_borrow().map_err(|_| error(Rule::Lifecycle, "fixture probe borrowed"))?;
        let state = states.get(key).ok_or_else(|| error(Rule::MissingTarget, "fixture controller missing"))?;
        let mut value = state.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?;
        edit(&mut value);
        if value.draft.len() > 4096 {
            value.draft = value.draft.chars().take(1024).collect();
        }
        Ok(())
    }
    fn publish(&self, key: ControllerKey, state: Rc<RefCell<FixtureState>>) -> PanelResult<()> {
        let mut states = self.states.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture probe borrowed"))?;
        if states.len() >= MAX_RETAINED_CONTROLLERS && !states.contains_key(&key) {
            return Err(error(Rule::CollectionLimit, "fixture state cap"));
        }
        states.insert(key, state);
        Ok(())
    }
}

struct FixtureController {
    state: Rc<RefCell<FixtureState>>,
    editor: bool,
    instance: InstanceId,
    probe: FixtureProbe,
    key: ControllerKey,
}

impl PanelController for FixtureController {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        if self.state.try_borrow().is_ok_and(|state| state.invalid_availability) {
            return Availability::Unavailable { reason: String::new() };
        }
        if self.editor {
            match models.read(owner, "fixture.editable") {
                Ok(serde_json::Value::Bool(true)) => Availability::Available,
                _ => Availability::Unavailable { reason: "synthetic editor requires editable document context".into() },
            }
        } else {
            Availability::Available
        }
    }
    fn update(&mut self, _owner: &ContentOwner, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.probe.publish(self.key.clone(), self.state.clone())?;
        let mut state = self.state.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?;
        state.updates = state.updates.saturating_add(1);
        if state.fail_update {
            return Err(error(Rule::Lifecycle, "synthetic service update failed"));
        }
        Ok(())
    }
    fn presentation(&self) -> PanelPresentation {
        let summary = self.state.try_borrow().ok().map(|s| (s.updates, s.transaction_live));
        PanelPresentation {
            title: format!("Specimen {}", self.instance.0),
            icon: "list".into(),
            status: summary.map(|(updates, live)| format!("updates={updates}; transaction={live}")),
            attention: self.editor,
            actions: if self.editor {
                ["apply", "cancel", "finish"]
                    .into_iter()
                    .map(|id| RuntimeAction { id: ControlId(id.into()), label: id.into(), enabled: true, disabled_reason: None, pressed: None })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        let state = self.state.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?;
        if self.editor && state.transaction_live && state.reject_cancel && matches!(event, LifecycleEvent::Close | LifecycleEvent::DestroyOwner) {
            return Err(error(Rule::Lifecycle, "synthetic editor cannot safely cancel"));
        }
        if state.reject_reopen && matches!(event, LifecycleEvent::Reopen) {
            return Err(error(Rule::Lifecycle, "synthetic controller cannot safely reopen"));
        }
        Ok(())
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        // Publishing diagnostics happens after commit, never in a staged factory.
        let _ = self.probe.publish(self.key.clone(), self.state.clone());
        // This fixture's preflight prohibits concurrent borrows. An adapter must
        // make commit infallible; it must not defer fallible work into this hook.
        if let Ok(mut state) = self.state.try_borrow_mut() {
            match event {
                LifecycleEvent::Close => {
                    if self.editor && state.transaction_live {
                        state.cancels = state.cancels.saturating_add(1);
                        state.transaction_live = false;
                    }
                }
                LifecycleEvent::Reopen => {
                    state.reopens = state.reopens.saturating_add(1);
                    if self.editor {
                        state.transaction_live = true;
                    }
                }
                LifecycleEvent::Reparent => state.reparents = state.reparents.saturating_add(1),
                LifecycleEvent::DestroyOwner => {
                    if self.editor && state.transaction_live {
                        state.cancels = state.cancels.saturating_add(1);
                        state.transaction_live = false;
                    }
                    state.destroys = state.destroys.saturating_add(1);
                    state.last_state = Some(ControllerState { destroyed: true, ..ControllerState::default() });
                }
                LifecycleEvent::State(value) => state.last_state = Some(value.clone()),
            }
        }
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, _context: &PanelContext, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        if slot == ContentSlot::Body {
            let mut state = self.state.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?;
            state.renders = state.renders.saturating_add(1);
            state.last_clip = Some(ui.clip_rect());
        }
        Ok(())
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        if !self.editor {
            return Err(error(Rule::MissingTarget, "log has no editor actions"));
        }
        let mut state = self.state.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "fixture state borrowed"))?;
        match action.0.as_str() {
            "apply" => {
                // Only this explicit content command reaches the model sink.
                services.commands.execute(owner, "fixture.apply", serde_json::json!({"draft":state.draft}))?;
                state.applies = state.applies.saturating_add(1);
            }
            "cancel" => {
                if state.reject_cancel {
                    return Err(error(Rule::Lifecycle, "synthetic cancel rejected"));
                }
                state.cancels = state.cancels.saturating_add(1);
                state.transaction_live = false;
            }
            "finish" => {
                services.commands.execute(owner, "fixture.finish", serde_json::json!({"draft":state.draft}))?;
                state.finishes = state.finishes.saturating_add(1);
                state.transaction_live = false;
            }
            _ => return Err(error(Rule::MissingTarget, "unknown synthetic action")),
        }
        Ok(())
    }
}

pub fn bind_synthetic_controllers(runtime: &mut PanelRuntime, probe: &FixtureProbe) -> PanelResult<()> {
    for (kind, editor, ownership) in [
        ("fixture.application", false, Ownership::Application),
        ("fixture.editor", true, Ownership::Document),
        ("fixture.log", false, Ownership::Workspace),
        ("fixture.pin", false, Ownership::Workspace),
    ] {
        runtime.register(PanelKind::new(kind)?, fixture_controller_binding(probe, editor, ownership))?;
    }
    Ok(())
}

/// Bind the same synthetic lifecycle to another registered kind/ownership policy.
/// This lets capacity fixtures exercise several document editors without adding
/// production registrations or changing the pure layout registry.
pub fn fixture_controller_binding(probe: &FixtureProbe, editor: bool, ownership: Ownership) -> ControllerBinding {
    let probe = probe.clone();
    ControllerBinding {
        ownership,
        create: Box::new(move |instance, owner| {
            let key = ControllerKey { instance: instance.id.clone(), owner: owner.clone() };
            Ok(Box::new(FixtureController {
                state: Rc::new(RefCell::new(FixtureState { transaction_live: editor && instance.requested_open, ..FixtureState::default() })),
                editor,
                instance: instance.id.clone(),
                probe: probe.clone(),
                key,
            }))
        }),
    }
}

#[derive(Default)]
pub struct FakeModels {
    pub editable: bool,
}

/// Synthetic app provider for command/control fixtures, with the same retained
/// model/command/local-service bindings as PanelMountHarness.
#[derive(Default)]
pub struct FakeAppPanelServices {
    pub models: FakeModels,
    pub commands: FakeContentCommands,
    pub local: FakeLocalServices,
}
impl AppPanelServices for FakeAppPanelServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}
impl PanelModels for FakeModels {
    fn read(&self, _owner: &ContentOwner, key: &str) -> PanelResult<serde_json::Value> {
        match key {
            "fixture.editable" => Ok(serde_json::Value::Bool(self.editable)),
            _ => Err(error(Rule::MissingTarget, "unknown synthetic model read")),
        }
    }
}

#[derive(Default)]
pub struct FakeContentCommands {
    pub calls: u64,
    pub fail: bool,
}
impl ContentCommandSink for FakeContentCommands {
    fn execute(&mut self, _owner: &ContentOwner, command: &str, input: serde_json::Value) -> PanelResult<serde_json::Value> {
        valid_id(command)?;
        bounded_json(&input)?;
        if serde_json::to_vec(&input).map_err(|_| error(Rule::Schema, "invalid synthetic command"))?.len() > MAX_ENVELOPE_BYTES {
            return Err(error(Rule::CollectionLimit, "synthetic command cap"));
        }
        if self.fail {
            return Err(error(Rule::Lifecycle, "synthetic content command failed"));
        }
        self.calls = self.calls.saturating_add(1);
        Ok(serde_json::Value::Null)
    }
}

#[derive(Default)]
pub struct FakeLocalServices {
    pub scopes: BTreeMap<InstanceId, LocalScope>,
    pub keyboard: BTreeMap<LocalScope, (InstanceId, ControlId)>,
    pub requests: VecDeque<(LocalScope, LocalRequest)>,
    pub releases: u64,
}
impl FakeLocalServices {
    pub fn bind(&mut self, instance: InstanceId, scope: LocalScope) -> PanelResult<()> {
        valid_id(&instance.0)?;
        scope.validate()?;
        if self.scopes.len() >= MAX_INSTANCES && !self.scopes.contains_key(&instance) {
            return Err(error(Rule::CollectionLimit, "local scope cap"));
        }
        if let Some(old) = self.scopes.get(&instance).cloned() {
            if old == scope {
                return Ok(());
            }
            self.release(&old, &instance);
        }
        self.scopes.insert(instance, scope);
        Ok(())
    }
}
impl PanelLocalServices for FakeLocalServices {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        scope.validate()?;
        request.validate()?;
        if self.scopes.get(request.instance()) != Some(scope) {
            return Err(error(Rule::Placement, "cross-document or cross-viewport service request"));
        }
        match &request {
            LocalRequest::KeyboardOwner { instance, control } => {
                if self.keyboard.get(scope).is_some_and(|(owner, _)| owner != instance) {
                    return Err(error(Rule::Lifecycle, "keyboard owned by another panel"));
                }
                self.keyboard.insert(scope.clone(), (instance.clone(), control.clone()));
            }
            LocalRequest::ReleaseKeyboard { instance } => {
                if self.keyboard.get(scope).is_some_and(|(owner, _)| owner == instance) {
                    self.keyboard.remove(scope);
                }
            }
            LocalRequest::ClearOverlays { instance } => {
                self.requests.retain(|(at, req)| at != scope || req.instance() != instance || !matches!(req, LocalRequest::Overlay { .. }))
            }
            _ => {}
        }
        if self.requests.len() >= MAX_INSTANCES {
            self.requests.pop_front();
        }
        self.requests.push_back((scope.clone(), request));
        Ok(())
    }
    fn release(&mut self, scope: &LocalScope, instance: &InstanceId) {
        if self.keyboard.get(scope).is_some_and(|(owner, _)| owner == instance) {
            self.keyboard.remove(scope);
        }
        self.requests.retain(|(at, request)| at != scope || request.instance() != instance);
        if self.scopes.get(instance) == Some(scope) {
            self.scopes.remove(instance);
        }
        self.releases = self.releases.saturating_add(1);
    }
}

pub struct FakeNativeAdapter {
    pub capabilities: HostCapabilities,
    pub hosts: BTreeMap<HostId, SectionId>,
    pub fail_prepare: bool,
    pub commits: u64,
}
impl FakeNativeAdapter {
    pub fn new(native: bool) -> Self {
        Self {
            capabilities: HostCapabilities { native_windows: native, persistence: true, max_native_hosts: if native { 8 } else { 0 } },
            hosts: BTreeMap::new(),
            fail_prepare: false,
            commits: 0,
        }
    }
}
impl NativeHostAdapter for FakeNativeAdapter {
    fn capabilities(&self) -> HostCapabilities {
        self.capabilities.clone()
    }
    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        if self.fail_prepare {
            return Err(error(Rule::Capability, "synthetic native preparation failure"));
        }
        if effects.len() > MAX_MUTATIONS * 3 {
            return Err(error(Rule::CollectionLimit, "native effect cap"));
        }
        let mut hosts = self.hosts.clone();
        for effect in effects {
            match effect {
                HostEffect::CreateNative { host, section } => {
                    if !self.capabilities.native_windows {
                        return Err(error(Rule::Capability, "native windows unsupported"));
                    }
                    if hosts.insert(host.clone(), section.clone()).is_some() {
                        return Err(error(Rule::DuplicateId, "native host already exists"));
                    }
                }
                HostEffect::RebindNative { host, section } => {
                    if !self.capabilities.native_windows {
                        return Err(error(Rule::Capability, "native rebind unsupported"));
                    }
                    if !hosts.contains_key(host) {
                        return Err(error(Rule::MissingTarget, "native rebind host absent"));
                    }
                    hosts.insert(host.clone(), section.clone());
                }
                HostEffect::ReleaseNative { host } => {
                    hosts.remove(host);
                }
                _ => {}
            }
        }
        if hosts.len() > self.capabilities.max_native_hosts {
            return Err(error(Rule::Capability, "native host capacity"));
        }
        Ok(())
    }
    fn commit(&mut self, effects: &[HostEffect]) {
        for effect in effects {
            match effect {
                HostEffect::CreateNative { host, section } | HostEffect::RebindNative { host, section } => {
                    self.hosts.insert(host.clone(), section.clone());
                }
                HostEffect::ReleaseNative { host } => {
                    self.hosts.remove(host);
                }
                _ => {}
            }
        }
        self.commits = self.commits.saturating_add(1);
    }
}

#[derive(Clone, Debug)]
pub struct MountRecord {
    pub geometry: ResolvedPanel,
    pub presentation: PanelPresentation,
    pub context: PanelContext,
}

#[derive(Default)]
pub struct FakePanelHost {
    pub mounts: VecDeque<MountRecord>,
    pub slot_calls: u64,
    pub context: egui::Context,
}
impl PanelHost for FakePanelHost {
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
        let mut failure = None;
        let ctx = self.context.clone();
        let mut output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
            crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
                for slot in [ContentSlot::Header, ContentSlot::Body, ContentSlot::Footer] {
                    if failure.is_some() || geometry.body.is_none() {
                        break;
                    }
                    if let Err(reason) = content.render(slot, ui, context, services) {
                        failure = Some(reason);
                    }
                    self.slot_calls = self.slot_calls.saturating_add(1);
                }
            });
        });
        output.textures_delta.clear();
        if let Some(reason) = failure {
            return Err(reason);
        }
        if self.mounts.len() >= MAX_INSTANCES {
            self.mounts.pop_front();
        }
        self.mounts.push_back(MountRecord { geometry: geometry.clone(), presentation: presentation.clone(), context: context.clone() });
        Ok(())
    }
}

/// Reusable harness uses actual reducer, controller, host and services interfaces.
pub struct PanelMountHarness {
    pub runtime: PanelRuntime,
    pub probe: FixtureProbe,
    pub models: FakeModels,
    pub commands: FakeContentCommands,
    pub local: FakeLocalServices,
    pub native: FakeNativeAdapter,
    pub host: FakePanelHost,
    pub persistence: FakePersistence,
    pub catalogue: commands::InMemoryCatalogue,
}
impl PanelMountHarness {
    pub fn command(&mut self, id: &str, params: &serde_json::Value) -> PanelResult<serde_json::Value> {
        let registry = self.runtime.layout().registry().clone();
        let mut services = ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local };
        let mut sink = RuntimeActionSink { runtime: &mut self.runtime, native: &mut self.native, services: &mut services };
        commands::execute(
            id,
            params,
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut self.catalogue, web: false },
        )
    }
    pub fn command_batch(&mut self, steps: &[serde_json::Value]) -> PanelResult<serde_json::Value> {
        let registry = self.runtime.layout().registry().clone();
        let mut services = ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local };
        let mut sink = RuntimeActionSink { runtime: &mut self.runtime, native: &mut self.native, services: &mut services };
        commands::execute_batch(
            steps,
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut self.catalogue, web: false },
        )
    }
    pub fn apply_layout(&mut self, layout: Layout, expected_revision: Option<u64>) -> PanelResult<LayoutReply> {
        self.runtime.dispatch(
            LayoutAction::Replace { expected_revision, layout },
            &mut self.native,
            &mut ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local },
        )
    }
    pub fn new(native: bool) -> PanelResult<Self> {
        Self::configured(native, true)
    }
    pub fn without_content(native: bool) -> PanelResult<Self> {
        Self::configured(native, false)
    }
    fn configured(native: bool, bind: bool) -> PanelResult<Self> {
        let layout = fixture_service(native)?;
        let catalogue = fixture_command_catalogue(layout.registry())?;
        let environment = RuntimeEnvironment {
            workspace: layout.layout().workspace.clone(),
            document: Some(DocumentId::new("fixture.document")?),
            window: HostId::new("fixture.window")?,
            viewport: ViewportId::new("fixture.viewport")?,
            direction: Direction::Ltr,
            motion: Motion::Reduced,
        };
        let mut runtime = PanelRuntime::new(layout, environment)?;
        let probe = FixtureProbe::default();
        if bind {
            bind_synthetic_controllers(&mut runtime, &probe)?;
        }
        Ok(Self {
            runtime,
            probe,
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            native: FakeNativeAdapter::new(native),
            host: FakePanelHost::default(),
            persistence: FakePersistence::default(),
            catalogue,
        })
    }
    pub fn dispatch(&mut self, mutations: Vec<Mutation>) -> PanelResult<LayoutReply> {
        self.runtime.dispatch(
            transaction(mutations),
            &mut self.native,
            &mut ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local },
        )
    }
    pub fn tick(&mut self) -> PanelResult<()> {
        self.runtime.update(&mut ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local })
    }
    pub fn mount(&mut self, capacity: LogicalRect) -> PanelResult<ResolvedLayout> {
        self.tick()?;
        let environment = self.runtime.environment().clone();
        let resolved = resolve_geometry(self.runtime.layout(), capacity, environment.direction, 48.0)?;
        for panel in &resolved.panels {
            let viewport = match &self.runtime.layout().layout().section(&panel.section)?.location {
                SectionLocation::Window { host, .. } => ViewportId::new(host.0.clone())?,
                _ => environment.viewport.clone(),
            };
            self.local.bind(
                panel.instance.clone(),
                LocalScope { workspace: environment.workspace.clone(), document: environment.document.clone(), viewport },
            )?;
        }
        self.runtime.mount(
            &resolved,
            &mut self.host,
            &mut ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local },
        )?;
        Ok(resolved)
    }
    pub fn save(&mut self) -> PanelResult<()> {
        let layout = self.runtime.layout().layout();
        let bytes = encode(&LayoutEnvelope::new(layout.clone()), self.runtime.layout().registry())?;
        self.persistence.save(&layout.workspace, &bytes)
    }
}
