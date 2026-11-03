use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::panels::*;

use super::*;

/// Retention spans every open document, independently of the active layout.
pub const MAX_RETAINED_DOCUMENT_OWNERS: usize = crate::documents::MAX_OPEN;
/// Application, workspace and window owners share a separate bounded budget.
pub const MAX_RETAINED_OTHER_OWNERS: usize = MAX_INSTANCES;
/// Total retained owners; also bounds the separate destroyed-owner tombstone set.
pub const MAX_RETAINED_OWNERS: usize = MAX_RETAINED_DOCUMENT_OWNERS + MAX_RETAINED_OTHER_OWNERS;
/// Each owner can retain a full layout's controllers, including hidden ones.
pub const MAX_RETAINED_CONTROLLERS_PER_OWNER: usize = MAX_INSTANCES;
/// Layout instance limits remain unchanged; retention never evicts live content.
pub const MAX_RETAINED_CONTROLLERS: usize = MAX_RETAINED_OWNERS * MAX_RETAINED_CONTROLLERS_PER_OWNER;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ownership {
    Application,
    Workspace,
    Document,
    Window,
}

/// Window ownership is explicit content ownership, independent of native placement.
/// Redocking does not turn a document editor into a new window-owned controller.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentOwner {
    Application,
    Workspace(WorkspaceId),
    Document(DocumentId),
    Window(HostId),
}
impl ContentOwner {
    pub fn validate(&self) -> PanelResult<()> {
        match self {
            Self::Application => Ok(()),
            Self::Workspace(id) => valid_id(&id.0),
            Self::Document(id) => valid_id(&id.0),
            Self::Window(id) => valid_id(&id.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeEnvironment {
    pub workspace: WorkspaceId,
    pub document: Option<DocumentId>,
    pub window: HostId,
    pub viewport: ViewportId,
    pub direction: Direction,
    pub motion: Motion,
}
impl RuntimeEnvironment {
    pub fn validate(&self) -> PanelResult<()> {
        valid_id(&self.workspace.0)?;
        valid_id(&self.window.0)?;
        valid_id(&self.viewport.0)?;
        if let Some(doc) = &self.document {
            valid_id(&doc.0)?;
        }
        Ok(())
    }
    fn owner(&self, policy: Ownership) -> PanelResult<ContentOwner> {
        Ok(match policy {
            Ownership::Application => ContentOwner::Application,
            Ownership::Workspace => ContentOwner::Workspace(self.workspace.clone()),
            Ownership::Document => {
                ContentOwner::Document(self.document.clone().ok_or_else(|| error(Rule::Availability, "document-bound panel needs a document"))?)
            }
            Ownership::Window => ContentOwner::Window(self.window.clone()),
        })
    }
}

/// Content model access is owner-scoped and read-only. Layout code never receives
/// a Session. Consumers may implement richer views behind the same trait.
pub trait PanelModels {
    fn read(&self, owner: &ContentOwner, key: &str) -> PanelResult<serde_json::Value>;
}

/// Explicit content commands have their own typed owner. They are separate from
/// layout actions and are never implicitly sent when a panel is hidden/reparented.
pub trait ContentCommandSink {
    fn execute(&mut self, owner: &ContentOwner, command: &str, input: serde_json::Value) -> PanelResult<serde_json::Value>;
}

pub struct ContentServices<'a> {
    pub models: &'a dyn PanelModels,
    pub commands: &'a mut dyn ContentCommandSink,
    pub local: &'a mut dyn PanelLocalServices,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerState {
    pub requested_open: bool,
    pub available: Availability,
    pub presented: bool,
    pub activated: bool,
    pub destroyed: bool,
}
impl Default for ControllerState {
    fn default() -> Self {
        Self { requested_open: false, available: Availability::Available, presented: false, activated: false, destroyed: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleEvent {
    State(ControllerState),
    /// Only explicit close calls this; deactivation never cancels a transaction.
    Close,
    Reopen,
    Reparent,
    DestroyOwner,
}

pub trait PanelController {
    /// Availability may depend on document/tool context, independent of visibility.
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability;
    /// Runs for retained controllers, including hidden, closed and inactive documents.
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()>;
    fn presentation(&self) -> PanelPresentation;
    /// MUST be side-effect-free. Failure rejects the whole layout transaction.
    /// Validate cancellation and reserve anything fallible before committing.
    /// State notifications are observational and cannot veto a layout change.
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()>;
    /// Infallible application of an already prepared lifecycle transition.
    fn lifecycle(&mut self, event: &LifecycleEvent);
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()>;
    /// Apply/Finish/Cancel/Stop stay explicit content actions, never layout mutations.
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()>;
}

pub struct ControllerBinding {
    pub ownership: Ownership,
    /// Staging may be discarded on failure; do not publish external side effects.
    pub create: Box<dyn Fn(&PanelInstance, &ContentOwner) -> PanelResult<Box<dyn PanelController>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ControllerKey {
    pub instance: InstanceId,
    pub owner: ContentOwner,
}

struct RetainedController {
    controller: Box<dyn PanelController>,
    /// A controller created closed, or explicitly closed after creation, needs
    /// Reopen before reuse. Snapshot hiding never sets this lifecycle flag.
    needs_reopen: bool,
    state: ControllerState,
    scope: LocalScope,
    section: Option<SectionId>,
    native_host: Option<HostId>,
    flow: Option<Flow>,
    availability: Availability,
}

/// Layout stays on the app, controllers on their content owners, host services on
/// their local scopes. None of these values enter document serialization or undo.
pub struct PanelRuntime {
    layout: LayoutService,
    bindings: BTreeMap<PanelKind, ControllerBinding>,
    controllers: BTreeMap<ControllerKey, RetainedController>,
    environment: RuntimeEnvironment,
    destroyed_owners: BTreeSet<ContentOwner>,
    destroyed: bool,
    update_errors: Vec<RuntimeUpdateError>,
}

#[derive(Clone, Debug)]
pub struct RuntimeUpdateError {
    pub controller: Option<ControllerKey>,
    pub error: PanelError,
}

/// A scoped live action sink for host and future command routing. It includes the
/// runtime lifecycle guard rather than exposing a mutable bare LayoutService.
pub struct RuntimeActionSink<'a, 'services> {
    pub runtime: &'a mut PanelRuntime,
    pub native: &'a mut dyn NativeHostAdapter,
    pub services: &'a mut ContentServices<'services>,
}
impl ActionSink for RuntimeActionSink<'_, '_> {
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        self.runtime.layout.project(action)
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        let mut reply = self.runtime.dispatch(action, self.native, self.services)?;
        let native = self.native.capabilities();
        reply.capabilities.native_windows &= native.native_windows;
        reply.capabilities.max_native_hosts = reply.capabilities.max_native_hosts.min(native.max_native_hosts);
        Ok(reply)
    }
    fn inspect(&self) -> LayoutReply {
        let mut reply = self.runtime.layout.inspect();
        let native = self.native.capabilities();
        reply.capabilities.native_windows &= native.native_windows;
        reply.capabilities.max_native_hosts = reply.capabilities.max_native_hosts.min(native.max_native_hosts);
        reply
    }
}

impl PanelRuntime {
    pub fn new(layout: LayoutService, environment: RuntimeEnvironment) -> PanelResult<Self> {
        environment.validate()?;
        if layout.layout().workspace != environment.workspace {
            return Err(error(Rule::Placement, "runtime workspace differs from layout service"));
        }
        Ok(Self {
            layout,
            bindings: BTreeMap::new(),
            controllers: BTreeMap::new(),
            environment,
            destroyed_owners: BTreeSet::new(),
            destroyed: false,
            update_errors: Vec::new(),
        })
    }
    pub fn register(&mut self, kind: PanelKind, binding: ControllerBinding) -> PanelResult<()> {
        self.require_live()?;
        self.layout.registry().get(&kind)?;
        if self.bindings.contains_key(&kind) {
            return Err(error(Rule::DuplicateId, "controller binding already registered"));
        }
        if self.bindings.len() >= MAX_INSTANCES {
            return Err(error(Rule::CollectionLimit, "binding cap"));
        }
        self.bindings.insert(kind, binding);
        Ok(())
    }
    pub fn layout(&self) -> &LayoutService {
        &self.layout
    }
    pub fn environment(&self) -> &RuntimeEnvironment {
        &self.environment
    }
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }
    pub fn update_errors(&self) -> &[RuntimeUpdateError] {
        &self.update_errors
    }
    fn require_live(&self) -> PanelResult<()> {
        if self.destroyed {
            return Err(error(Rule::Lifecycle, "runtime destroyed"));
        }
        Ok(())
    }
    pub fn state(&self, key: &ControllerKey) -> Option<&ControllerState> {
        self.controllers.get(key).map(|c| &c.state)
    }
    pub fn presentation(&self, key: &ControllerKey) -> PanelResult<PanelPresentation> {
        let value = self.controllers.get(key).ok_or_else(|| error(Rule::MissingTarget, "controller not retained"))?.controller.presentation();
        value.validate()?;
        Ok(value)
    }
    fn key(&self, instance: &PanelInstance) -> PanelResult<ControllerKey> {
        self.key_in(instance, &self.environment)
    }
    fn key_in(&self, instance: &PanelInstance, environment: &RuntimeEnvironment) -> PanelResult<ControllerKey> {
        let binding = self.bindings.get(&instance.kind).ok_or_else(|| error(Rule::Availability, "no content binding available"))?;
        let owner = environment.owner(binding.ownership)?;
        if self.destroyed_owners.contains(&owner) {
            return Err(error(Rule::Availability, "content owner destroyed"));
        }
        Ok(ControllerKey { instance: instance.id.clone(), owner })
    }
    fn scope(&self) -> LocalScope {
        LocalScope {
            workspace: self.environment.workspace.clone(),
            document: self.environment.document.clone(),
            viewport: self.environment.viewport.clone(),
        }
    }

    /// Changes active context without dropping editors/conversations of inactive
    /// documents. Their independent service updates keep running under their owner.
    pub fn set_environment(&mut self, environment: RuntimeEnvironment, local: &mut dyn PanelLocalServices) -> PanelResult<()> {
        self.require_live()?;
        environment.validate()?;
        if environment.workspace != self.layout.layout().workspace {
            return Err(error(Rule::Placement, "apply the saved workspace layout through LayoutAction::Replace"));
        }
        if self.environment == environment {
            return Ok(());
        }
        // Capacity/factory refusal must leave the active owner and local input
        // untouched, just like a refused layout replacement.
        let staged = self.stage_controllers(self.layout.layout(), &environment)?;
        let mut events = Vec::new();
        for (key, retained) in &self.controllers {
            let mut state = retained.state.clone();
            state.presented = false;
            state.activated = false;
            events.push((key.clone(), state));
        }
        for (key, state) in events {
            if let Some(retained) = self.controllers.get_mut(&key) {
                local.release(&retained.scope, &key.instance);
                retained.controller.lifecycle(&LifecycleEvent::State(state.clone()));
                retained.state = state;
            }
        }
        self.environment = environment;
        self.controllers.extend(staged);
        Ok(())
    }

    fn stage_controllers(&self, layout: &Layout, environment: &RuntimeEnvironment) -> PanelResult<BTreeMap<ControllerKey, RetainedController>> {
        let mut staged = BTreeMap::new();
        let mut owner_counts = BTreeMap::<ContentOwner, usize>::new();
        for key in self.controllers.keys() {
            *owner_counts.entry(key.owner.clone()).or_default() += 1;
        }
        let mut document_owners = owner_counts.keys().filter(|owner| matches!(owner, ContentOwner::Document(_))).count();
        let mut other_owners = owner_counts.len() - document_owners;
        for p in &layout.instances {
            let key = match self.key_in(p, environment) {
                Ok(key) => key,
                Err(_) => continue,
            };
            if self.controllers.contains_key(&key) || staged.contains_key(&key) {
                continue;
            }
            if self.controllers.len() + staged.len() >= MAX_RETAINED_CONTROLLERS {
                return Err(error(Rule::CollectionLimit, "retained controller cap"));
            }
            if !owner_counts.contains_key(&key.owner) {
                if matches!(&key.owner, ContentOwner::Document(_)) {
                    if document_owners >= MAX_RETAINED_DOCUMENT_OWNERS {
                        return Err(error(Rule::CollectionLimit, "retained document owner cap"));
                    }
                    document_owners += 1;
                } else {
                    if other_owners >= MAX_RETAINED_OTHER_OWNERS {
                        return Err(error(Rule::CollectionLimit, "retained application/workspace/window owner cap"));
                    }
                    other_owners += 1;
                }
            }
            let count = owner_counts.entry(key.owner.clone()).or_default();
            if *count >= MAX_RETAINED_CONTROLLERS_PER_OWNER {
                return Err(error(Rule::CollectionLimit, "retained controller cap for content owner"));
            }
            *count += 1;
            let binding = self.bindings.get(&p.kind).ok_or_else(|| error(Rule::Availability, "binding disappeared"))?;
            let controller = (binding.create)(p, &key.owner)?;
            staged.insert(
                key,
                RetainedController {
                    controller,
                    needs_reopen: !p.requested_open,
                    state: ControllerState::default(),
                    scope: LocalScope {
                        workspace: environment.workspace.clone(),
                        document: environment.document.clone(),
                        viewport: environment.viewport.clone(),
                    },
                    section: None,
                    native_host: None,
                    flow: None,
                    availability: Availability::Available,
                },
            );
        }
        Ok(staged)
    }

    /// Refresh command/query availability from the current read-only provider.
    /// This does not tick content, evaluate models or invent presentation geometry.
    /// A failed retained service stays failed until a real update recovers it.
    pub fn refresh_context(&mut self, models: &dyn PanelModels, local: &mut dyn PanelLocalServices) -> PanelResult<()> {
        self.require_live()?;
        let staged = self.stage_controllers(self.layout.layout(), &self.environment)?;
        let mut availability = AvailabilityMap::new();
        for instance in &self.layout.layout().instances {
            let state = match self.key(instance) {
                Ok(key) => match self.controllers.get(&key).or_else(|| staged.get(&key)) {
                    Some(retained) => {
                        let state = retained.controller.availability(&key.owner, models);
                        if matches!(&state,Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256) {
                            return Err(error(Rule::Availability, "controller availability reason bound"));
                        }
                        if self.update_errors.iter().any(|failure| failure.controller.as_ref() == Some(&key)) {
                            retained.availability.clone()
                        } else {
                            state
                        }
                    }
                    None => Availability::Unavailable { reason: "content controller unavailable".into() },
                },
                Err(reason) => Availability::Unavailable { reason: reason.detail },
            };
            availability.insert(instance.id.clone(), state);
        }
        self.layout.set_availability(availability.clone())?;
        self.controllers.extend(staged);
        for instance in &self.layout.layout().instances {
            if let Ok(key) = self.key(instance)
                && let Some(retained) = self.controllers.get_mut(&key)
                && let Some(state) = availability.get(&instance.id)
            {
                retained.availability = state.clone();
            }
        }
        self.sync_states(None, local);
        Ok(())
    }

    /// Independent tick: all retained services, then current-owner availability.
    /// No geometry or painting is required for this method.
    pub fn update(&mut self, services: &mut ContentServices<'_>) -> PanelResult<()> {
        if self.destroyed {
            return Err(error(Rule::Lifecycle, "runtime destroyed"));
        }
        self.update_errors.clear();
        match self.stage_controllers(self.layout.layout(), &self.environment) {
            Ok(staged) => self.controllers.extend(staged),
            Err(reason) => self.update_errors.push(RuntimeUpdateError { controller: None, error: error(reason.rule, reason.detail) }),
        }
        for (key, retained) in &mut self.controllers {
            let update = retained.controller.update(&key.owner, services);
            let availability = retained.controller.availability(&key.owner, services.models);
            let invalid = matches!(&availability, Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256);
            let failed = update.is_err() || invalid;
            if let Err(reason) = update {
                self.update_errors.push(RuntimeUpdateError { controller: Some(key.clone()), error: error(reason.rule, reason.detail) });
            }
            if invalid {
                self.update_errors.push(RuntimeUpdateError {
                    controller: Some(key.clone()),
                    error: error(Rule::Availability, "controller availability reason bound"),
                });
            }
            if failed {
                retained.availability = Availability::Unavailable {
                    reason: if invalid { "invalid availability report".into() } else { "content service update failed".into() },
                };
            } else {
                retained.availability = availability;
            }
        }
        let mut availability = AvailabilityMap::new();
        for p in &self.layout.layout().instances {
            let state = match self.key(p) {
                Ok(key) => self
                    .controllers
                    .get(&key)
                    .map(|c| c.availability.clone())
                    .unwrap_or(Availability::Unavailable { reason: "content controller unavailable".into() }),
                Err(reason) => Availability::Unavailable { reason: reason.detail },
            };
            availability.insert(p.id.clone(), state);
        }
        if let Err(reason) = self.layout.set_availability(availability) {
            self.update_errors.push(RuntimeUpdateError { controller: None, error: reason });
        }
        self.sync_states(None, services.local);
        // At most two errors per retained controller, plus staging/reconciliation.
        if self.update_errors.is_empty() {
            Ok(())
        } else {
            Err(error(Rule::Lifecycle, format!("{} panel service failures; inspect update_errors", self.update_errors.len())))
        }
    }

    /// Lifecycle preflight and native preflight happen before layout commit. A
    /// failed close/cancel leaves layout, interaction, controllers and host intact.
    pub fn dispatch(
        &mut self,
        action: LayoutAction,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<LayoutReply> {
        if action == LayoutAction::Inspect {
            return Ok(self.layout.inspect());
        }
        if self.destroyed {
            return Err(error(Rule::Lifecycle, "runtime destroyed"));
        }
        let requests: Vec<_> = match &action {
            LayoutAction::Apply { mutations, .. } => mutations
                .iter()
                .filter_map(|m| match m {
                    Mutation::Open { instance, .. }
                    | Mutation::Focus { instance }
                    | Mutation::Activate { instance }
                    | Mutation::Peek { instance } => Some(instance.clone()),
                    _ => None,
                })
                .collect(),
            LayoutAction::Inspect | LayoutAction::Replace { .. } => Vec::new(),
        };
        let replacement = matches!(&action, LayoutAction::Replace { .. });
        let mut explicit_close = BTreeSet::new();
        let mut explicit_destroy = BTreeSet::new();
        let mut explicit_open = BTreeSet::new();
        if let LayoutAction::Apply { mutations, .. } = &action {
            for mutation in mutations {
                match mutation {
                    Mutation::Close { instance } => {
                        explicit_close.insert(instance.clone());
                    }
                    Mutation::Destroy { instance } => {
                        explicit_destroy.insert(instance.clone());
                    }
                    Mutation::Open { instance, .. } | Mutation::Focus { instance } => {
                        explicit_open.insert(instance.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut prepared = self.layout.prepare(action)?;
        let mut environment = self.environment.clone();
        environment.workspace = prepared.layout().workspace.clone();
        let capabilities = native.capabilities();
        if capabilities.max_native_hosts > MAX_SECTIONS {
            return Err(error(Rule::Capability, "native adapter reports invalid capacity"));
        }
        if prepared.effects().iter().any(|e| matches!(e, HostEffect::CreateNative { .. })) && !capabilities.native_windows {
            return Err(error(Rule::Capability, "native adapter is unavailable"));
        }
        let staged = self.stage_controllers(prepared.layout(), &environment)?;
        let mut availability = AvailabilityMap::new();
        for instance in &prepared.layout().instances {
            let state = match self.key_in(instance, &environment) {
                Ok(key) => match self.controllers.get(&key).or_else(|| staged.get(&key)) {
                    Some(retained) => {
                        let state = retained.controller.availability(&key.owner, services.models);
                        if matches!(&state, Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256) {
                            return Err(error(Rule::Availability, "controller availability reason bound"));
                        }
                        // A failed retained service cannot become healthy solely by moving it.
                        if self.update_errors.iter().any(|e| e.controller.as_ref() == Some(&key)) { retained.availability.clone() } else { state }
                    }
                    None => Availability::Unavailable { reason: "content controller unavailable".into() },
                },
                Err(reason) => Availability::Unavailable { reason: reason.detail },
            };
            availability.insert(instance.id.clone(), state);
        }
        prepared.set_availability(availability.clone())?;
        for id in requests {
            if let Ok(instance) = prepared.layout().instance(&id)
                && let Some(state) = availability.get(&instance.id)
            {
                state.require()?;
            }
        }
        let mut events: Vec<(ControllerKey, LifecycleEvent)> = Vec::new();
        for (key, retained) in self.controllers.iter().chain(staged.iter()) {
            let before = self.layout.layout().instances.iter().find(|p| p.id == key.instance);
            let after = prepared.layout().instances.iter().find(|p| p.id == key.instance);
            let affected = !matches!(&key.owner, ContentOwner::Workspace(workspace) if *workspace != self.environment.workspace);
            if explicit_destroy.contains(&key.instance) {
                retained.controller.prepare_lifecycle(&LifecycleEvent::DestroyOwner)?;
                events.push((key.clone(), LifecycleEvent::DestroyOwner));
            } else if affected && explicit_close.contains(&key.instance) {
                retained.controller.prepare_lifecycle(&LifecycleEvent::Close)?;
                events.push((key.clone(), LifecycleEvent::Close));
            } else if !matches!(&key.owner, ContentOwner::Workspace(workspace) if *workspace != environment.workspace)
                && (replacement || explicit_open.contains(&key.instance))
                && retained.needs_reopen
                && after.is_some_and(|p| p.requested_open)
            {
                retained.controller.prepare_lifecycle(&LifecycleEvent::Reopen)?;
                events.push((key.clone(), LifecycleEvent::Reopen));
            }
            let old = self.layout.layout().containing(&key.instance).ok().map(|s| (&s.id, &s.location));
            let new = prepared.layout().containing(&key.instance).ok().map(|s| (&s.id, &s.location));
            let same_controller = after.is_some_and(|p| self.key_in(p, &environment).is_ok_and(|k| k == *key))
                && before.is_some_and(|p| self.key(p).is_ok_and(|k| k == *key));
            if same_controller
                && (old != new || self.environment.workspace != environment.workspace)
                && before.is_some_and(|p| p.requested_open)
                && after.is_some_and(|p| p.requested_open)
            {
                retained.controller.prepare_lifecycle(&LifecycleEvent::Reparent)?;
                events.push((key.clone(), LifecycleEvent::Reparent));
            }
        }
        // Effects are available from the same candidate, before irreversible work.
        native.prepare(prepared.effects())?;
        let reply = self.layout.commit(prepared)?;
        self.controllers.extend(staged);
        if replacement {
            for (key, retained) in &self.controllers {
                services.local.release(&retained.scope, &key.instance);
            }
        }
        self.environment = environment;
        let current_keys: BTreeSet<_> = self.layout.layout().instances.iter().filter_map(|p| self.key(p).ok()).collect();
        for (key, retained) in &mut self.controllers {
            if current_keys.contains(key)
                && let Some(state) = availability.get(&key.instance)
            {
                retained.availability = state.clone();
            }
        }
        for (key, event) in events {
            if let Some(retained) = self.controllers.get_mut(&key) {
                if matches!(event, LifecycleEvent::Close | LifecycleEvent::Reparent | LifecycleEvent::DestroyOwner) {
                    services.local.release(&retained.scope, &key.instance);
                }
                retained.controller.lifecycle(&event);
                match event {
                    LifecycleEvent::Close => retained.needs_reopen = true,
                    LifecycleEvent::Reopen => retained.needs_reopen = false,
                    _ => {}
                }
            }
            if event == LifecycleEvent::DestroyOwner {
                self.controllers.remove(&key);
            }
        }
        native.commit(&reply.effects);
        // Presentation will be reconciled by mount/update; layout changes do not
        // force content drawing or touch model revisions/undo/tessellation.
        self.sync_states(None, services.local);
        Ok(reply)
    }

    fn sync_states(&mut self, resolved: Option<&ResolvedLayout>, local: &mut dyn PanelLocalServices) {
        let active_keys: BTreeSet<_> = self.layout.layout().instances.iter().filter_map(|p| self.key(p).ok()).collect();
        let scope = self.scope();
        let mut pending = Vec::new();
        for (key, retained) in &self.controllers {
            let instance = self.layout.layout().instance(&key.instance).ok();
            let current = active_keys.contains(key);
            let available = retained.availability.clone();
            let requested_open = current && instance.is_some_and(|p| p.requested_open);
            let section = if current { self.layout.layout().containing(&key.instance).ok() } else { None };
            let section_id = section.map(|s| s.id.clone());
            let host = section.and_then(|s| match &s.location {
                SectionLocation::Window { host, .. } => Some(host.clone()),
                _ => None,
            });
            let peek = &self.layout.interaction().peek;
            let selected = section.is_some_and(|s| (!s.collapsed && s.active == key.instance) || peek.as_ref() == Some(&key.instance));
            let geometry_presented = match resolved {
                Some(r) => r.panels.iter().any(|p| p.instance == key.instance),
                None => retained.state.presented && retained.section == section_id && retained.native_host == host && selected,
            };
            let presented = current && requested_open && available == Availability::Available && geometry_presented;
            let activated = presented && selected;
            let state = ControllerState { requested_open, available, presented, activated, destroyed: false };
            pending.push((key.clone(), state, section_id, host, current));
        }
        for (key, state, section, host, current) in pending {
            if let Some(retained) = self.controllers.get_mut(&key) {
                let mut target_scope = scope.clone();
                if let Some(host) = &host {
                    target_scope.viewport = ViewportId(host.0.clone());
                }
                if retained.state.presented && !state.presented || current && (retained.scope != target_scope || retained.native_host != host) {
                    local.release(&retained.scope, &key.instance);
                }
                if state != retained.state {
                    retained.controller.lifecycle(&LifecycleEvent::State(state.clone()));
                }
                retained.state = state;
                retained.section = section;
                retained.native_host = host;
                if current {
                    retained.scope = target_scope;
                }
            }
        }
    }

    pub fn mount(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.require_live()?;
        resolved.validate()?;
        for record in &resolved.unpresented {
            self.layout.layout().section(&record.section)?;
        }
        let mut ids = BTreeSet::new();
        for geometry in &resolved.panels {
            geometry.validate()?;
            if !ids.insert(&geometry.instance) {
                return Err(error(Rule::ExactlyOnce, "panel mounted twice"));
            }
            let section = self.layout.layout().containing(&geometry.instance)?;
            if section.id != geometry.section
                || section.location.zone() != geometry.zone
                || !self.layout.layout().instance(&geometry.instance)?.requested_open
            {
                return Err(error(Rule::Placement, "resolved host differs from live layout"));
            }
            let peek = self.layout.interaction().peek.as_ref() == Some(&geometry.instance);
            if section.active != geometry.instance && !peek || section.collapsed && !peek {
                return Err(error(Rule::Placement, "cannot mount inactive or collapsed tab content"));
            }
            let (min, max) = tab_bounds(self.layout.layout(), self.layout.registry(), section)?;
            if !geometry.outer.size.within(min, max) {
                return Err(error(Rule::Bounds, "host violates tab outer bounds"));
            }
            if self.layout.availability(&geometry.instance) != Availability::Available {
                return Err(error(Rule::Availability, "cannot mount unavailable content"));
            }
            let key = self.key(self.layout.layout().instance(&geometry.instance)?)?;
            self.controllers.get(&key).ok_or_else(|| error(Rule::Availability, "content controller unavailable"))?.availability.require()?;
        }
        self.sync_states(Some(resolved), services.local);
        let interaction = self.layout.interaction().clone();
        for geometry in &resolved.panels {
            let instance = self.layout.layout().instance(&geometry.instance)?;
            let key = self.key(instance)?;
            let retained = self.controllers.get_mut(&key).ok_or_else(|| error(Rule::MissingTarget, "call update before mount"))?;
            let (flow, size_class) = responsive(geometry.zone, geometry.body.size, retained.flow)?;
            let context = PanelContext {
                instance: geometry.instance.clone(),
                owner: key.owner.clone(),
                workspace: self.environment.workspace.clone(),
                document: self.environment.document.clone(),
                viewport: retained.scope.viewport.clone(),
                native_host: retained.native_host.clone(),
                outer: geometry.outer,
                usable_body: geometry.body,
                flow,
                size_class,
                focused: interaction.focused.as_ref() == Some(&geometry.instance),
                direction: self.environment.direction,
                motion: self.environment.motion,
            };
            let presentation = retained.controller.presentation();
            presentation.validate()?;
            host.mount(geometry, &presentation, &context, retained.controller.as_mut(), services)?;
            retained.flow = Some(flow);
        }
        Ok(())
    }

    pub fn content_action(&mut self, instance: &InstanceId, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.require_live()?;
        valid_id(&action.0)?;
        self.layout.availability(instance).require()?;
        let key = self.key(self.layout.layout().instance(instance)?)?;
        let retained = self.controllers.get_mut(&key).ok_or_else(|| error(Rule::MissingTarget, "controller not retained"))?;
        retained.availability.require()?;
        let presentation = retained.controller.presentation();
        presentation.validate()?;
        let command =
            presentation.actions.iter().find(|a| a.id == *action).ok_or_else(|| error(Rule::MissingTarget, "content action not advertised"))?;
        if !command.enabled {
            return Err(error(Rule::Availability, command.disabled_reason.as_deref().unwrap_or("content action unavailable")));
        }
        retained.controller.action(&key.owner, action, services)
    }

    pub fn destroy_owner(&mut self, owner: &ContentOwner, local: &mut dyn PanelLocalServices) -> PanelResult<()> {
        self.require_live()?;
        owner.validate()?;
        if self.destroyed_owners.len() >= MAX_RETAINED_OWNERS && !self.destroyed_owners.contains(owner) {
            return Err(error(Rule::CollectionLimit, "destroyed owner cap"));
        }
        for (key, retained) in &self.controllers {
            if key.owner == *owner {
                retained.controller.prepare_lifecycle(&LifecycleEvent::DestroyOwner)?;
            }
        }
        self.controllers.retain(|key, retained| {
            if key.owner != *owner {
                return true;
            }
            local.release(&retained.scope, &key.instance);
            retained.controller.lifecycle(&LifecycleEvent::DestroyOwner);
            false
        });
        self.destroyed_owners.insert(owner.clone());
        Ok(())
    }

    /// Explicitly rebind a previously destroyed owner; it receives fresh content
    /// controllers on update, never a silent resurrection during hidden ticks.
    pub fn revive_owner(&mut self, owner: &ContentOwner) -> PanelResult<()> {
        self.require_live()?;
        owner.validate()?;
        self.destroyed_owners.remove(owner);
        Ok(())
    }

    pub fn shutdown(&mut self, local: &mut dyn PanelLocalServices) -> PanelResult<()> {
        for retained in self.controllers.values() {
            retained.controller.prepare_lifecycle(&LifecycleEvent::DestroyOwner)?;
        }
        for (key, retained) in &mut self.controllers {
            local.release(&retained.scope, &key.instance);
            retained.controller.lifecycle(&LifecycleEvent::DestroyOwner);
        }
        self.controllers.clear();
        self.destroyed = true;
        Ok(())
    }
}
