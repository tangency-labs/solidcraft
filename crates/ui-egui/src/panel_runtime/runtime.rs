use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::panels::*;

use super::*;

/// A floating rectangle changes geometry, not the controller's presentation
/// host. Keep its keyboard/overlay ownership while retaining ordinary reparent
/// guards for changed groups, dock edges, native hosts and workspaces.
fn same_presentation_host(before: Option<(&SectionId, &SectionLocation)>, after: Option<(&SectionId, &SectionLocation)>) -> bool {
    match (before, after) {
        (Some((before_id, SectionLocation::Floating { .. })), Some((after_id, SectionLocation::Floating { .. }))) => before_id == after_id,
        _ => before == after,
    }
}

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
    /// Capture bounded read-only measurement data. The returned immutable
    /// provider can answer candidate widths without borrowing live controllers,
    /// rendering hidden bodies or calling mutable services. The default reports
    /// registration's preferred outer height honestly, not an intrinsic extent.
    fn measure(&self, _owner: &ContentOwner, _models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<std::sync::Arc<dyn HostContentMetrics>> {
        Ok(std::sync::Arc::new(PreferredPanelMetrics { height: spec.default_size.height, policy: spec.side_height_policy }))
    }
    /// Read the live measurement source through this same controller. Immutable
    /// providers cannot report later source changes through their old revision.
    /// A cheap override must include every model/style/policy input to measure;
    /// the safe default captures a new bounded provider for comparison only.
    /// Neither path may render, tick services or publish owner state.
    fn measurement_revision(&self, owner: &ContentOwner, models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<u64> {
        Ok(self.measure(owner, models, spec)?.revision())
    }
    /// MUST be side-effect-free. Failure rejects the whole layout transaction.
    /// Validate cancellation and reserve anything fallible before committing.
    /// State notifications are observational and cannot veto a layout change.
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()>;
    /// Infallible application of an already prepared lifecycle transition.
    fn lifecycle(&mut self, event: &LifecycleEvent);
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()>;
    /// Safe fallback for existing content; adapters can supply compact controls.
    fn render_inline(&mut self, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.render(ContentSlot::Body, ui, context, services)
    }
    /// Validate a binding without mutating its owner. Implementations may add
    /// transaction-specific refusals here, before any host state commits.
    fn prepare_action(&self, _owner: &ContentOwner, action: &ControlId, _models: &dyn PanelModels) -> PanelResult<()> {
        let presentation = self.presentation();
        presentation.validate()?;
        let binding = presentation
            .actions
            .iter()
            .find(|binding| binding.id == *action)
            .ok_or_else(|| error(Rule::Capability, "content action has no controller binding"))?;
        if !binding.enabled {
            return Err(error(Rule::Availability, binding.disabled_reason.as_deref().unwrap_or("content action unavailable")));
        }
        Ok(())
    }
    /// Apply/Finish/Cancel/Stop stay explicit content actions, never layout mutations.
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()>;
}

#[derive(Debug)]
struct PreferredPanelMetrics {
    height: f32,
    policy: SideHeightPolicy,
}
impl HostContentMetrics for PreferredPanelMetrics {
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        Ok(SideContentMeasure { request: request.clone(), needed_height: self.height.max(request.chrome_height), policy: self.policy })
    }
    fn revision(&self) -> u64 {
        u64::from(self.height.to_bits()) * 2 + u64::from(self.policy == SideHeightPolicy::Fill)
    }
}

#[derive(Debug)]
struct CapturedPanelMetrics {
    panels: BTreeMap<InstanceId, std::sync::Arc<dyn HostContentMetrics>>,
    revision: u64,
}
impl HostContentMetrics for CapturedPanelMetrics {
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        self.panels.get(&request.instance).ok_or_else(|| error(Rule::Availability, "content metrics unavailable"))?.measure(request)
    }
    fn revision(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut signature = std::collections::hash_map::DefaultHasher::new();
        self.revision.hash(&mut signature);
        for (id, provider) in &self.panels {
            id.hash(&mut signature);
            provider.revision().hash(&mut signature);
        }
        signature.finish()
    }
}

pub struct ControllerBinding {
    pub ownership: Ownership,
    /// Staging may be discarded on failure; do not publish external side effects.
    pub create: Box<dyn Fn(&PanelInstance, &ContentOwner) -> PanelResult<Box<dyn PanelController>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Private, side-effect-free owner cohort. Providers and newly created
/// controllers survive projection and are moved into the successful commit.
struct RuntimeCohort {
    base_environment: RuntimeEnvironment,
    base_revision: u64,
    environment: RuntimeEnvironment,
    staged: BTreeMap<ControllerKey, RetainedController>,
    availability: AvailabilityMap,
    metrics: std::sync::Arc<dyn HostContentMetrics>,
    metric_revision: u64,
    providers: BTreeMap<ControllerKey, std::sync::Arc<dyn HostContentMetrics>>,
}

pub(crate) struct RuntimeHostProposal {
    candidate: PreparedHostLayout,
    reference: HostLayout,
    source_reference: HostLayout,
    cohort: RuntimeCohort,
}
impl RuntimeHostProposal {
    pub(crate) fn action(&self) -> &LayoutAction {
        self.candidate.action()
    }
    pub(crate) fn layout(&self) -> &Layout {
        self.candidate.prepared().layout()
    }
    pub(crate) fn validate_preparation_context(&self, context: &LayoutPreparationContext) -> PanelResult<()> {
        if self.reference.geometry_policy.identity_reservations.preparation_context()? != *context {
            return Err(error(Rule::Revision, "host proposal preparation context changed"));
        }
        Ok(())
    }
    pub(crate) fn validate_captured(&self, runtime: &PanelRuntime, models: &dyn PanelModels) -> PanelResult<()> {
        self.validate(runtime, &self.source_reference, models)
    }
    pub(crate) fn validate(&self, runtime: &PanelRuntime, reference: &HostLayout, models: &dyn PanelModels) -> PanelResult<()> {
        if &self.source_reference != reference {
            return Err(error(Rule::Revision, "host proposal source environment changed"));
        }
        self.candidate.validate_current(runtime.layout(), &runtime_scope(runtime), &self.reference)?;
        let snapshot = self.candidate.proposal_snapshot();
        if self.cohort.availability != snapshot.availability || !std::sync::Arc::ptr_eq(&self.cohort.metrics, &snapshot.metrics) {
            return Err(error(Rule::Revision, "runtime cohort differs from admitted host snapshot"));
        }
        runtime.validate_cohort(self.layout(), &self.cohort, models)
    }
}

pub(crate) fn standalone_header_action(action: &LayoutAction) -> bool {
    matches!(action, LayoutAction::Apply { mutations, .. } if matches!(mutations.as_slice(), [Mutation::HeaderAction { .. }]))
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

/// A mount supplies the complete presented set for exactly this cohort. Native
/// viewport adapters must never reconcile another viewport's presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PresentationCohort {
    All,
    InApp,
    Native(HostId),
}
impl PresentationCohort {
    fn includes(&self, current: Option<&HostId>, previous: Option<&HostId>) -> bool {
        match self {
            Self::All => true,
            Self::InApp => current.is_none() && previous.is_none(),
            Self::Native(host) => current == Some(host) || previous == Some(host),
        }
    }
}

/// A scoped live action sink for host and future command routing. It includes the
/// runtime lifecycle guard rather than exposing a mutable bare LayoutService.
pub struct RuntimeActionSink<'a, 'services> {
    pub runtime: &'a mut PanelRuntime,
    pub native: &'a mut dyn NativeHostAdapter,
    pub services: &'a mut ContentServices<'services>,
}

pub(crate) fn normalize_native_reply(mut reply: LayoutReply, native: &HostCapabilities) -> LayoutReply {
    reply.capabilities.native_windows &= native.native_windows;
    reply.capabilities.max_native_hosts = reply.capabilities.max_native_hosts.min(native.max_native_hosts);
    reply
}

impl ActionSink for RuntimeActionSink<'_, '_> {
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        self.runtime.layout.project_with_context(action, context)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        let reply = self.runtime.dispatch_in_context(action, context, self.native, self.services)?;
        Ok(normalize_native_reply(reply, &self.native.capabilities()))
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        self.runtime.layout.project(action)
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        let reply = self.runtime.dispatch(action, self.native, self.services)?;
        Ok(normalize_native_reply(reply, &self.native.capabilities()))
    }
    fn inspect(&self) -> LayoutReply {
        normalize_native_reply(self.runtime.layout.inspect(), &self.native.capabilities())
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
    /// Snapshot only current available owners. No controller render/update, list
    /// enumeration, command or local input service runs during metric capture.
    pub fn capture_metrics(&self, models: &dyn PanelModels) -> PanelResult<std::sync::Arc<dyn HostContentMetrics>> {
        // Current-frame collection never creates/discards a controller. The
        // explicit context reconciliation must retain owners before painting.
        Ok(self.capture_cohort_with(self.layout.layout(), self.environment.clone(), BTreeMap::new(), BTreeMap::new(), models)?.metrics)
    }
    fn capture_cohort(&self, layout: &Layout, models: &dyn PanelModels) -> PanelResult<RuntimeCohort> {
        self.require_live()?;
        let mut environment = self.environment.clone();
        environment.workspace = layout.workspace.clone();
        environment.validate()?;
        let staged = self.stage_controllers(layout, &environment)?;
        self.capture_cohort_with(layout, environment, staged, BTreeMap::new(), models)
    }
    fn capture_cohort_with(
        &self,
        layout: &Layout,
        environment: RuntimeEnvironment,
        staged: BTreeMap<ControllerKey, RetainedController>,
        mut providers: BTreeMap<ControllerKey, std::sync::Arc<dyn HostContentMetrics>>,
        models: &dyn PanelModels,
    ) -> PanelResult<RuntimeCohort> {
        use std::hash::{Hash, Hasher};
        self.require_live()?;
        let availability = self.cohort_availability(layout, &environment, &staged, models)?;
        let mut panels = BTreeMap::new();
        let mut signature = std::collections::hash_map::DefaultHasher::new();
        for instance in &layout.instances {
            instance.id.hash(&mut signature);
            if !instance.requested_open || availability.get(&instance.id) != Some(&Availability::Available) {
                continue;
            }
            let key = self.key_in(instance, &environment)?;
            let retained = self
                .controllers
                .get(&key)
                .or_else(|| staged.get(&key))
                .ok_or_else(|| error(Rule::Availability, "content metrics controller unavailable"))?;
            let metrics = match providers.get(&key) {
                Some(metrics) => metrics.clone(),
                None => {
                    let metrics = retained.controller.measure(&key.owner, models, self.layout.registry().get(&instance.kind)?)?;
                    providers.insert(key.clone(), metrics.clone());
                    metrics
                }
            };
            key.hash(&mut signature);
            panels.insert(instance.id.clone(), metrics);
        }
        let metrics = std::sync::Arc::new(CapturedPanelMetrics { panels, revision: signature.finish() });
        let metric_revision = metrics.revision();
        Ok(RuntimeCohort {
            base_environment: self.environment.clone(),
            base_revision: self.layout.inspect().revision,
            environment,
            staged,
            availability,
            metrics,
            metric_revision,
            providers,
        })
    }
    fn cohort_availability(
        &self,
        layout: &Layout,
        environment: &RuntimeEnvironment,
        staged: &BTreeMap<ControllerKey, RetainedController>,
        models: &dyn PanelModels,
    ) -> PanelResult<AvailabilityMap> {
        let mut availability = AvailabilityMap::new();
        for instance in &layout.instances {
            let state = match self.key_in(instance, environment) {
                Ok(key) => match self.controllers.get(&key).or_else(|| staged.get(&key)) {
                    Some(retained) => {
                        let state = retained.controller.availability(&key.owner, models);
                        if matches!(&state, Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256) {
                            return Err(error(Rule::Availability, "controller availability reason bound"));
                        }
                        if self.update_errors.iter().any(|e| e.controller.as_ref() == Some(&key)) { retained.availability.clone() } else { state }
                    }
                    None => Availability::Unavailable { reason: "content controller unavailable".into() },
                },
                Err(reason) => Availability::Unavailable { reason: reason.detail },
            };
            availability.insert(instance.id.clone(), state);
        }
        Ok(availability)
    }
    fn validate_cohort(&self, layout: &Layout, cohort: &RuntimeCohort, models: &dyn PanelModels) -> PanelResult<()> {
        self.require_live()?;
        if self.environment != cohort.base_environment
            || self.layout.inspect().revision != cohort.base_revision
            || self.cohort_availability(layout, &cohort.environment, &cohort.staged, models)? != cohort.availability
            || cohort.metrics.revision() != cohort.metric_revision
        {
            return Err(error(Rule::Revision, "runtime proposal cohort changed"));
        }
        for instance in &layout.instances {
            if !instance.requested_open || cohort.availability.get(&instance.id) != Some(&Availability::Available) {
                continue;
            }
            let key = self.key_in(instance, &cohort.environment)?;
            let retained = self
                .controllers
                .get(&key)
                .or_else(|| cohort.staged.get(&key))
                .ok_or_else(|| error(Rule::Revision, "measured controller is no longer retained"))?;
            let original = cohort.providers.get(&key).ok_or_else(|| error(Rule::Revision, "controller metric source snapshot missing"))?;
            let live = retained.controller.measurement_revision(&key.owner, models, self.layout.registry().get(&instance.kind)?)?;
            if live != original.revision() {
                return Err(error(Rule::Revision, "controller measurement source changed after capture"));
            }
        }
        Ok(())
    }
    pub(crate) fn prepare_host_proposal(
        &self,
        reference: HostLayout,
        action: LayoutAction,
        models: &dyn PanelModels,
    ) -> PanelResult<RuntimeHostProposal> {
        self.prepare_host_proposal_with(reference, action, models, None, None)
    }
    pub(crate) fn prepare_host_proposal_in_context(
        &self,
        reference: HostLayout,
        action: LayoutAction,
        context: &LayoutPreparationContext,
        models: &dyn PanelModels,
    ) -> PanelResult<RuntimeHostProposal> {
        self.prepare_host_proposal_with(reference, action, models, None, Some(context))
    }
    pub(crate) fn revise_host_proposal(
        &self,
        previous: RuntimeHostProposal,
        reference: HostLayout,
        action: LayoutAction,
        context: Option<&LayoutPreparationContext>,
        models: &dyn PanelModels,
    ) -> PanelResult<RuntimeHostProposal> {
        previous.validate(self, &reference, models)?;
        let context = match context {
            Some(context) => context.clone(),
            None => previous.reference.geometry_policy.identity_reservations.preparation_context()?,
        };
        self.prepare_host_proposal_with(reference, action, models, Some(previous.cohort), Some(&context))
    }
    fn prepare_host_proposal_with(
        &self,
        source_reference: HostLayout,
        action: LayoutAction,
        models: &dyn PanelModels,
        previous: Option<RuntimeCohort>,
        context: Option<&LayoutPreparationContext>,
    ) -> PanelResult<RuntimeHostProposal> {
        // Workspace/preset staging can supply an incoming envelope context. Keep
        // the outgoing painted environment independently for freshness, while B
        // owns the exact supplied preparation environment throughout admission.
        let mut reference = source_reference.clone();
        if let Some(context) = context {
            let reservations = HostIdentityReservations {
                sections: context.sections.clone(),
                columns: context.columns.clone(),
                extent_priorities: context.extent_priorities.clone(),
                replacement_ownership: context.replacement_ownership.clone(),
            };
            reservations.validate()?;
            reference.geometry_policy.identity_reservations = reservations;
        }
        let mut cohort = None;
        let candidate = prepare_host_layout_with_snapshot(&self.layout, &runtime_scope(self), &reference, action, |layout| {
            let captured = if let Some(previous) = previous {
                let mut environment = self.environment.clone();
                environment.workspace = layout.workspace.clone();
                environment.validate()?;
                let staged = self.stage_controllers_with(layout, &environment, previous.staged)?;
                self.capture_cohort_with(layout, environment, staged, previous.providers, models)?
            } else {
                self.capture_cohort(layout, models)?
            };
            let snapshot = HostProposalSnapshot { availability: captured.availability.clone(), metrics: captured.metrics.clone() };
            cohort = Some(captured);
            Ok(snapshot)
        })?;
        Ok(RuntimeHostProposal {
            candidate,
            reference,
            source_reference,
            cohort: cohort.ok_or_else(|| error(Rule::Lifecycle, "host capture callback was not called"))?,
        })
    }
    /// Pointer candidates already own their geometry. Capture current retained
    /// owners without replaying their action, and reject a mismatching snapshot.
    pub(crate) fn retain_host_candidate(
        &self,
        candidate: PreparedHostLayout,
        reference: HostLayout,
        models: &dyn PanelModels,
    ) -> PanelResult<RuntimeHostProposal> {
        candidate.validate_current(&self.layout, &runtime_scope(self), &reference)?;
        let layout = candidate.prepared().layout();
        let environment = self.environment.clone();
        if layout.workspace != environment.workspace {
            return Err(error(Rule::Revision, "pointer candidate changed workspace"));
        }
        // Fresh comparison providers come from the same retained/staged objects.
        // Keep B's original aggregate for geometry/commit when revisions agree.
        let mut cohort = self.capture_cohort(layout, models)?;
        let snapshot = candidate.proposal_snapshot();
        if cohort.availability != snapshot.availability || cohort.metrics.revision() != snapshot.metrics.revision() {
            return Err(error(Rule::Revision, "pointer proposed measurement sources changed"));
        }
        cohort.metrics = snapshot.metrics.clone();
        cohort.metric_revision = snapshot.metrics.revision();
        Ok(RuntimeHostProposal { candidate, source_reference: reference.clone(), reference, cohort })
    }
    pub(crate) fn commit_host_proposal(
        &mut self,
        proposal: RuntimeHostProposal,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<LayoutReply> {
        proposal.validate_captured(self, services.models)?;
        let RuntimeHostProposal { candidate, reference, cohort, .. } = proposal;
        let action = candidate.action().clone();
        let prepared = candidate.into_prepared(&self.layout, &runtime_scope(self), &reference)?;
        self.commit_layout(Some(action), prepared, native, services, Some(cohort))
    }
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }
    pub fn update_errors(&self) -> &[RuntimeUpdateError] {
        &self.update_errors
    }
    pub(crate) fn require_live(&self) -> PanelResult<()> {
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
    /// Read inactive tabs/rails through the registered ownership policy. This
    /// never creates, updates or renders a controller.
    pub fn instance_presentation(&self, instance: &InstanceId) -> PanelResult<PanelPresentation> {
        self.require_live()?;
        let key = self.key(self.layout.layout().instance(instance)?)?;
        self.presentation(&key)
    }
    pub fn instance_owner(&self, instance: &InstanceId) -> PanelResult<ContentOwner> {
        self.key(self.layout.layout().instance(instance)?).map(|key| key.owner)
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

    /// Re-read owner/context availability without ticking background work or
    /// evaluating a model. App boundaries use this after Session changes later
    /// in the same egui frame; incoming controllers are staged and retained.
    pub fn reconcile_context(&mut self, environment: RuntimeEnvironment, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.require_live()?;
        self.set_environment(environment, services.local)?;
        let staged = self.stage_controllers(self.layout.layout(), &self.environment)?;
        self.controllers.extend(staged);
        let mut availability = AvailabilityMap::new();
        let mut retained_availability = Vec::new();
        for instance in &self.layout.layout().instances {
            let state = match self.key(instance) {
                Ok(key) => {
                    let retained = self.controllers.get(&key).ok_or_else(|| error(Rule::Availability, "content controller unavailable"))?;
                    let available = retained.controller.availability(&key.owner, services.models);
                    if matches!(&available, Availability::Unavailable { reason } if reason.is_empty() || reason.len() > 256) {
                        return Err(error(Rule::Availability, "controller availability reason bound"));
                    }
                    // A boundary refresh is not a successful background tick.
                    let state = if self.update_errors.iter().any(|failure| failure.controller.as_ref() == Some(&key)) {
                        retained.availability.clone()
                    } else {
                        available
                    };
                    retained_availability.push((key, state.clone()));
                    state
                }
                Err(reason) => Availability::Unavailable { reason: reason.detail },
            };
            availability.insert(instance.id.clone(), state);
        }
        self.layout.set_availability(availability)?;
        for (key, state) in retained_availability {
            if let Some(retained) = self.controllers.get_mut(&key) {
                retained.availability = state;
            }
        }
        self.sync_states(None, services.local);
        Ok(())
    }

    /// Fail closed when a current-Session provider/context cannot be reconciled.
    /// Hiding releases local input but never closes or destroys retained drafts.
    pub fn suspend_presentations(&mut self, local: &mut dyn PanelLocalServices) {
        for (key, retained) in &mut self.controllers {
            if retained.state.presented || retained.state.activated {
                local.release(&retained.scope, &key.instance);
                retained.state.presented = false;
                retained.state.activated = false;
                retained.controller.lifecycle(&LifecycleEvent::State(retained.state.clone()));
            }
        }
    }

    fn stage_controllers(&self, layout: &Layout, environment: &RuntimeEnvironment) -> PanelResult<BTreeMap<ControllerKey, RetainedController>> {
        self.stage_controllers_with(layout, environment, BTreeMap::new())
    }
    fn stage_controllers_with(
        &self,
        layout: &Layout,
        environment: &RuntimeEnvironment,
        mut staged: BTreeMap<ControllerKey, RetainedController>,
    ) -> PanelResult<BTreeMap<ControllerKey, RetainedController>> {
        let mut owner_counts = BTreeMap::<ContentOwner, usize>::new();
        for key in self.controllers.keys().chain(staged.keys()) {
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
        let availability = self.cohort_availability(self.layout.layout(), &self.environment, &staged, models)?;
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
        if let LayoutAction::Apply { mutations, .. } = &action
            && mutations.iter().any(|m| matches!(m, Mutation::HeaderAction { .. }))
        {
            if !standalone_header_action(&action) {
                return Err(error(Rule::Capability, "header actions cannot be mixed with layout mutations"));
            }
            let prepared = self.layout.prepare(action.clone())?;
            if let [Mutation::HeaderAction { instance, action }] = mutations.as_slice() {
                let key = self.key(self.layout.layout().instance(instance)?)?;
                let retained = self.controllers.get(&key).ok_or_else(|| error(Rule::Availability, "header action controller unavailable"))?;
                retained.controller.availability(&key.owner, services.models).require()?;
                retained.availability.require()?;
                retained.controller.prepare_action(&key.owner, action, services.models)?;
                self.content_action(instance, action, services)?;
            }
            return self.layout.commit(prepared);
        }
        let prepared = self.layout.prepare(action.clone())?;
        self.commit_layout(Some(action), prepared, native, services, None)
    }

    /// Host-disabled/native adapters still prepare against the full envelope
    /// context and the same once-created proposed owners used for lifecycle.
    fn dispatch_in_context(
        &mut self,
        action: LayoutAction,
        context: &LayoutPreparationContext,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<LayoutReply> {
        if action == LayoutAction::Inspect || standalone_header_action(&action) {
            return self.dispatch(action, native, services);
        }
        self.require_live()?;
        let owners = self.layout.project_with_context(action.clone(), context)?;
        let cohort = self.capture_cohort(&owners, services.models)?;
        let prepared = self.layout.prepare_with_context(action.clone(), &cohort.availability, context)?;
        self.commit_layout(Some(action), prepared, native, services, Some(cohort))
    }

    fn commit_layout(
        &mut self,
        action: Option<LayoutAction>,
        mut prepared: PreparedLayout,
        native: &mut dyn NativeHostAdapter,
        services: &mut ContentServices<'_>,
        cohort: Option<RuntimeCohort>,
    ) -> PanelResult<LayoutReply> {
        self.require_live()?;
        let requests: Vec<_> = match &action {
            Some(LayoutAction::Apply { mutations, .. }) => mutations
                .iter()
                .filter_map(|m| match m {
                    Mutation::Open { instance, .. }
                    | Mutation::Focus { instance }
                    | Mutation::Activate { instance }
                    | Mutation::Peek { instance } => Some(instance.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let replacement = matches!(&action, Some(LayoutAction::Replace { .. }));
        let mut explicit_close = BTreeSet::new();
        let mut explicit_destroy = BTreeSet::new();
        let mut explicit_open = BTreeSet::new();
        if let Some(LayoutAction::Apply { mutations, .. }) = &action {
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
        let cohort = match cohort {
            Some(cohort) => cohort,
            None => self.capture_cohort(prepared.layout(), services.models)?,
        };
        self.validate_cohort(prepared.layout(), &cohort, services.models)?;
        let RuntimeCohort { environment, mut staged, availability, .. } = cohort;
        // A batch may replace an earlier private snapshot. Only controllers in
        // its final owner cohort are published; discarded stages never ticked.
        let proposed_keys: BTreeSet<_> = prepared.layout().instances.iter().filter_map(|p| self.key_in(p, &environment).ok()).collect();
        staged.retain(|key, _| proposed_keys.contains(key));
        let capabilities = native.capabilities();
        if capabilities.max_native_hosts > MAX_SECTIONS {
            return Err(error(Rule::Capability, "native adapter reports invalid capacity"));
        }
        if prepared.effects().iter().any(|e| matches!(e, HostEffect::CreateNative { .. })) && !capabilities.native_windows {
            return Err(error(Rule::Capability, "native adapter is unavailable"));
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
                && (!same_presentation_host(old, new) || self.environment.workspace != environment.workspace)
                && before.is_some_and(|p| p.requested_open)
                && after.is_some_and(|p| p.requested_open)
            {
                retained.controller.prepare_lifecycle(&LifecycleEvent::Reparent)?;
                events.push((key.clone(), LifecycleEvent::Reparent));
            }
        }
        let header_actions: Vec<_> = prepared
            .effects()
            .iter()
            .filter_map(|effect| match effect {
                HostEffect::HeaderAction { instance, action } => Some((instance.clone(), action.clone())),
                _ => None,
            })
            .collect();
        // A fallible owner action is its own command. Mixing it with structural
        // changes could otherwise commit half a content/layout transaction.
        if !header_actions.is_empty() && !matches!(&action, Some(LayoutAction::Apply { mutations, .. }) if mutations.len() == 1) {
            return Err(error(Rule::Capability, "header actions cannot be mixed with layout mutations"));
        }
        for (instance, action) in &header_actions {
            let key = self.key_in(prepared.layout().instance(instance)?, &environment)?;
            let retained = self
                .controllers
                .get(&key)
                .or_else(|| staged.get(&key))
                .ok_or_else(|| error(Rule::Availability, "header action controller unavailable"))?;
            retained.availability.require()?;
            retained.controller.prepare_action(&key.owner, action, services.models)?;
        }
        // Effects are available from the same candidate, before irreversible work.
        native.prepare(prepared.effects())?;
        for (instance, action) in &header_actions {
            self.content_action(instance, action, services)?;
        }
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
        self.sync_presented_states(resolved, &PresentationCohort::All, local);
    }

    fn sync_presented_states(&mut self, resolved: Option<&ResolvedLayout>, cohort: &PresentationCohort, local: &mut dyn PanelLocalServices) {
        let active_keys: BTreeSet<_> = self.layout.layout().instances.iter().filter_map(|p| self.key(p).ok()).collect();
        let scope = self.scope();
        let availability: AvailabilityMap = self.layout.layout().instances.iter().map(|p| (p.id.clone(), self.layout.availability(&p.id))).collect();
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
            if !cohort.includes(host.as_ref(), retained.native_host.as_ref()) {
                continue;
            }
            let selected = section.is_some_and(|s| !s.collapsed && presented_tab(s, &availability) == Some(&key.instance));
            let geometry_presented = match resolved {
                Some(r) => r.panels.iter().any(|p| p.instance == key.instance && !p.painted.is_empty()),
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
        self.mount_frame(resolved, host, services, &PresentationCohort::All, true)
    }

    /// Mount exactly once with every currently presented in-app section,
    /// including an empty set when hidden. Native peers are not deactivated.
    pub fn mount_in_app(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.mount_frame(resolved, host, services, &PresentationCohort::InApp, true)
    }

    /// Includes every currently presented section in one native host, or an
    /// empty set to unmount only that host. Root and sibling windows survive.
    pub fn mount_native(
        &mut self,
        native_host: &HostId,
        resolved: &ResolvedLayout,
        host: &mut dyn PanelHost,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        valid_id(&native_host.0)?;
        if !resolved.rails.is_empty() {
            return Err(error(Rule::Placement, "native cohort cannot mount root rails"));
        }
        self.mount_frame(resolved, host, services, &PresentationCohort::Native(native_host.clone()), true)
    }

    pub(crate) fn repaint_in_app(
        &mut self,
        resolved: &ResolvedLayout,
        host: &mut dyn PanelHost,
        services: &mut ContentServices<'_>,
    ) -> PanelResult<()> {
        self.mount_frame(resolved, host, services, &PresentationCohort::InApp, false)
    }

    /// After a kit Area sizing pass, reconcile actual visibility without
    /// invoking any content renderer twice or deactivating native peers.
    pub(crate) fn reconcile_in_app_presentation(&mut self, resolved: &ResolvedLayout, local: &mut dyn PanelLocalServices) {
        self.sync_presented_states(Some(resolved), &PresentationCohort::InApp, local);
    }

    fn mount_frame(
        &mut self,
        resolved: &ResolvedLayout,
        host: &mut dyn PanelHost,
        services: &mut ContentServices<'_>,
        cohort: &PresentationCohort,
        reconcile: bool,
    ) -> PanelResult<()> {
        self.require_live()?;
        resolved.validate()?;
        for record in &resolved.unpresented {
            let section = self.layout.layout().section(&record.section)?;
            if let PresentationCohort::Native(expected) = cohort
                && !matches!(&section.location, SectionLocation::Window { host, .. } if host == expected)
            {
                return Err(error(Rule::Placement, "unpresented section belongs to another host"));
            }
        }
        let mut ids = BTreeSet::new();
        for geometry in &resolved.panels {
            geometry.validate()?;
            if !ids.insert(&geometry.instance) {
                return Err(error(Rule::ExactlyOnce, "panel mounted twice"));
            }
            let section = self.layout.layout().containing(&geometry.instance)?;
            let native_host = match &section.location {
                SectionLocation::Window { host, .. } => Some(host),
                _ => None,
            };
            if !cohort.includes(native_host, native_host) {
                return Err(error(Rule::Placement, "content belongs to another presentation cohort"));
            }
            if section.id != geometry.section
                || section.location.zone() != geometry.zone
                || !self.layout.layout().instance(&geometry.instance)?.requested_open
            {
                return Err(error(Rule::Placement, "resolved host differs from live layout"));
            }
            let availability: AvailabilityMap = section.tabs.iter().map(|id| (id.clone(), self.layout.availability(id))).collect();
            if presented_tab(section, &availability) != Some(&geometry.instance) || section.collapsed && geometry.body.is_some() {
                return Err(error(Rule::Placement, "cannot mount inactive or collapsed tab content"));
            }
            let (min, max) = tab_bounds(self.layout.layout(), self.layout.registry(), section)?;
            if !section.collapsed && !geometry.outer.size.within(min, max) {
                return Err(error(Rule::Bounds, "host violates tab outer bounds"));
            }
            if self.layout.availability(&geometry.instance) != Availability::Available {
                return Err(error(Rule::Availability, "cannot mount unavailable content"));
            }
            let key = self.key(self.layout.layout().instance(&geometry.instance)?)?;
            self.controllers.get(&key).ok_or_else(|| error(Rule::Availability, "content controller unavailable"))?.availability.require()?;
        }
        if reconcile {
            self.sync_presented_states(Some(resolved), cohort, services.local);
        }
        let interaction = self.layout.interaction().clone();
        for geometry in &resolved.panels {
            if geometry.painted.is_empty() {
                continue;
            }
            let instance = self.layout.layout().instance(&geometry.instance)?;
            let key = self.key(instance)?;
            let retained = self.controllers.get_mut(&key).ok_or_else(|| error(Rule::MissingTarget, "call update before mount"))?;
            let (flow, size_class) = responsive(geometry.zone, geometry.outer.size, retained.flow)?;
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
                preview_geometry: false,
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

    /// Release the exact native resources belonging to this runtime only after
    /// every controller and native adapter has accepted teardown.
    pub fn shutdown_with_native(&mut self, native: &mut dyn NativeHostAdapter, local: &mut dyn PanelLocalServices) -> PanelResult<()> {
        if self.destroyed {
            return Ok(());
        }
        let effects: Vec<_> = if native.capabilities().native_windows {
            self.layout
                .layout()
                .sections
                .iter()
                .filter_map(|section| match &section.location {
                    SectionLocation::Window { host, .. } => Some(HostEffect::ReleaseNative { host: host.clone() }),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        for retained in self.controllers.values() {
            retained.controller.prepare_lifecycle(&LifecycleEvent::DestroyOwner)?;
        }
        native.prepare(&effects)?;
        for (key, retained) in &mut self.controllers {
            local.release(&retained.scope, &key.instance);
            retained.controller.lifecycle(&LifecycleEvent::DestroyOwner);
        }
        native.commit(&effects);
        self.controllers.clear();
        self.destroyed = true;
        Ok(())
    }
}
