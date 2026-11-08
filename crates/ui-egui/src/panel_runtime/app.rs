use solidcraft_engine::Session;
use solidcraft_engine::panels::*;

use super::runtime::standalone_header_action;
use super::*;

/// Later content adapters supply their own services. The app integration exposes
/// only an immutable Session during refresh; a layout tick cannot run a document
/// command. Explicit content commands stay on the separate supplied sink.
pub trait AppPanelServices {
    fn refresh_workspace(&mut self, _workspace: &WorkspaceId) -> PanelResult<()> {
        Ok(())
    }
    fn refresh_read_only(&mut self, session: &Session) -> PanelResult<()>;
    fn services(&mut self) -> ContentServices<'_>;
}

pub struct AppPanelRuntime {
    pub(super) production: Option<super::production::ProductionState>,
    runtime: PanelRuntime,
    services: Box<dyn AppPanelServices>,
    native: Box<dyn NativeHostAdapter>,
    host: Option<WorkspacePanelHost>,
    last_tick_frame: Option<u64>,
    last_error: Option<PanelError>,
    catalogue: Box<dyn commands::LayoutCatalogue>,
    adapters_set: bool,
    pub(super) preferences: Option<super::preferences::PreferenceSession>,
}
impl AppPanelRuntime {
    pub fn new(runtime: PanelRuntime, services: Box<dyn AppPanelServices>) -> PanelResult<Self> {
        runtime.require_live()?;
        if runtime.layout().layout().sections.iter().any(|section| section.location.zone() == Zone::Window) {
            return Err(error(Rule::Lifecycle, "install the app adapter before creating or restoring native placement"));
        }
        Ok(Self {
            production: None,
            runtime,
            services,
            native: Box::<InAppNativeAdapter>::default(),
            host: None,
            last_tick_frame: None,
            last_error: None,
            catalogue: Box::<commands::InMemoryCatalogue>::default(),
            adapters_set: false,
            preferences: None,
        })
    }
    /// Native composition supplies preferences; standalone runtimes keep their explicit direction.
    fn sync_native_direction(&mut self, direction: Direction) -> PanelResult<()> {
        if self.runtime.environment().direction == direction {
            return Ok(());
        }
        let mut environment = self.runtime.environment().clone();
        environment.direction = direction;
        self.with_adapters(|runtime, _native, services| runtime.set_environment(environment, services.local))?;
        if let Some(host) = &mut self.host {
            host.invalidate_geometry();
            host.reconcile_scope(&self.runtime);
        }
        Ok(())
    }

    pub(super) fn set_motion(&mut self, motion: Motion) -> PanelResult<()> {
        let mut environment = self.runtime.environment().clone();
        environment.motion = motion;
        self.with_adapters(|runtime, _native, services| runtime.set_environment(environment, services.local))
    }

    pub(crate) fn set_keyboard_shortcuts(&mut self, shortcuts: Vec<(ContextualPanelAction, egui::KeyboardShortcut)>) {
        if let Some(host) = &mut self.host {
            host.set_keyboard_shortcuts(shortcuts);
        }
    }

    fn contextual_availability(&self, session: &Session, action: ContextualPanelAction) -> PanelResult<()> {
        self.runtime.require_live()?;
        if self.preferences.as_ref().is_some_and(|state| state.pending.is_some())
            || self.runtime.environment().document.as_ref().map(|document| document.0.as_str()) != Some(session.document_id.as_str())
        {
            return Err(error(Rule::Availability, "panel command context is not reconciled"));
        }
        self.host
            .as_ref()
            .ok_or_else(|| error(Rule::Availability, "shared panel host unavailable"))?
            .contextual_action(&self.runtime, action)
            .map(|_| ())
    }

    pub fn last_error(&self) -> Option<&PanelError> {
        self.last_error.as_ref()
    }
    pub fn runtime(&self) -> &PanelRuntime {
        &self.runtime
    }
    pub(super) fn install_preference_catalogue(
        &mut self,
        catalogue: super::preferences::PreferenceCatalogue,
        state: super::preferences::PreferenceSession,
    ) -> PanelResult<()> {
        self.runtime.require_live()?;
        if self.preferences.is_some() || self.adapters_set {
            return Err(error(Rule::Lifecycle, "application panel catalogue is already installed"));
        }
        self.catalogue = Box::new(catalogue);
        self.preferences = Some(state);
        Ok(())
    }
    pub(super) fn preference_record(&self) -> PanelResult<super::preferences::PreferenceRecord> {
        let state = self.preferences.as_ref().ok_or_else(|| error(Rule::Lifecycle, "panel preferences are not enabled"))?;
        if let Some(record) = &state.pending {
            return Ok(record.clone());
        }
        Ok(super::preferences::PreferenceRecord {
            version: 1,
            selected: self.runtime.layout().layout().workspace.clone(),
            catalogue: self.catalogue.state().export()?,
            rejected: state.rejected.clone(),
            legacy: state.legacy.clone(),
        })
    }
    pub(super) fn preference_preset_names(&self) -> Vec<String> {
        self.catalogue.state().builtin_presets().chain(self.catalogue.state().user_presets()).map(|(name, _)| name.to_string()).collect()
    }
    pub(super) fn preference_catalogue_state(&self) -> &commands::CatalogueState {
        self.catalogue.state()
    }
    pub(super) fn current_preference_envelope(&self) -> PanelResult<LayoutEnvelope> {
        commands::current_envelope_snapshot(self.catalogue.as_ref(), &self.runtime.layout().inspect(), self.runtime.layout().registry())
    }
    pub(super) fn preference_user_preset_names(&self) -> Vec<String> {
        self.catalogue.state().user_presets().map(|(name, _)| name.to_string()).collect()
    }
    pub(super) fn restore_preference_record(&mut self, session: &Session, record: super::preferences::PreferenceRecord) -> PanelResult<()> {
        let next = record.catalogue(self.runtime.layout().registry())?;
        let stored = next.remembered(&record.selected).ok_or_else(|| error(Rule::MissingTarget, "saved selected workspace has no layout"))?;
        let mut envelope = stored.project_envelope(self.runtime.layout().registry(), &record.selected)?;
        restore_quarantine(&mut envelope, self.runtime.layout().registry())?;
        self.restore_preference_catalogue(session, next, envelope)?;
        if let Some(state) = &mut self.preferences {
            state.pending = None;
            state.failed = false;
            state.failed_environment = None;
            state.rejected = record.rejected;
            state.legacy = record.legacy;
        }
        Ok(())
    }
    pub(super) fn restore_profile_layouts(&mut self, session: &Session, profile: super::preferences::ProfileLayouts) -> PanelResult<()> {
        if self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            return Err(error(Rule::Availability, "restore the saved panel layout before applying a profile"));
        }
        let registry = self.runtime.layout().registry();
        let mut next = commands::InMemoryCatalogue::from_snapshot(&self.catalogue.state().export()?, registry)?;
        use commands::LayoutCatalogue as _;
        let current_workspace = self.runtime.layout().layout().workspace.clone();
        let selected = profile.selected.clone().unwrap_or_else(|| current_workspace.clone());
        let descriptors = next.state().summaries();
        let mut incoming = None;
        for (workspace, record) in profile.workspaces {
            if record.layout.instances.iter().any(|instance| registry.get(&instance.kind).is_err()) {
                return Err(error(Rule::Availability, "profile panel kind is not registered; keep the saved profile for retry"));
            }
            let target = if profile.selected.is_none() { current_workspace.clone() } else { workspace };
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.id == target)
                .ok_or_else(|| error(Rule::MissingTarget, "profile layout workspace is not registered"))?;
            let current = if target == current_workspace {
                commands::current_envelope_snapshot(self.catalogue.as_ref(), &self.runtime.layout().inspect(), registry)?
            } else {
                let stored = next.state().remembered(&target).map(Ok).unwrap_or_else(|| next.state().preset(&descriptor.default_preset))?;
                let raw = decode_retained_record(stored.bytes(), registry)?;
                let mut envelope = stored.project_envelope(registry, &raw.layout.workspace)?;
                envelope.layout.workspace = target.clone();
                envelope
            };
            let mut projected = record.layout;
            projected.workspace = target.clone();
            let envelope = with_live_layout(&current, projected, registry)?;
            let rejected = self.catalogue.state().remembered(&target).and_then(|stored| stored.rejected());
            next.remember(&envelope, rejected, registry)?;
            if target == selected {
                incoming = Some(envelope);
            }
        }
        self.restore_preference_catalogue(
            session,
            next.state().clone(),
            incoming.ok_or_else(|| error(Rule::MissingTarget, "selected profile workspace has no arrangement"))?,
        )
    }
    fn reset_pending_preferences(&mut self, session: &Session, record: super::preferences::PreferenceRecord) -> PanelResult<serde_json::Value> {
        let registry = self.runtime.layout().registry();
        let mut catalogue = commands::InMemoryCatalogue::from_snapshot(&record.catalogue, registry)?;
        use commands::LayoutCatalogue as _;
        let descriptor = catalogue
            .state()
            .summaries()
            .into_iter()
            .find(|item| item.id == record.selected)
            .ok_or_else(|| error(Rule::MissingTarget, "saved workspace has no default"))?;
        let stored = catalogue.state().preset(&descriptor.default_preset)?;
        let raw = decode_retained_record(stored.bytes(), registry)?;
        let mut envelope = stored.project_envelope(registry, &raw.layout.workspace)?;
        envelope.layout.workspace = record.selected.clone();
        catalogue.remember(&envelope, None, registry)?;
        self.restore_preference_catalogue(session, catalogue.state().clone(), envelope)?;
        if let Some(state) = &mut self.preferences {
            let original = record.value();
            state.rejected = record.rejected.or(Some(original));
            state.legacy = record.legacy;
            state.pending = None;
            state.failed = false;
            state.failed_environment = None;
        }
        serde_json::to_value(self.runtime.layout().inspect()).map_err(|_| error(Rule::Schema, "cannot encode layout reset reply"))
    }
    pub(super) fn preference_identity_reservations(&self) -> PanelResult<std::collections::BTreeSet<SectionId>> {
        let envelope =
            commands::current_envelope_snapshot(self.catalogue.as_ref(), &self.runtime.layout().inspect(), self.runtime.layout().registry())?;
        Ok(preparation_context(&envelope)?.sections)
    }
    pub(super) fn apply_legacy_preferences(
        &mut self,
        session: &Session,
        mut mutations: Vec<Mutation>,
        weights: Vec<(InstanceId, f32)>,
        widths: Vec<(InstanceId, f32)>,
        legacy: super::preferences::LegacyPanelPlacements,
    ) -> PanelResult<()> {
        self.reconcile(session)?;
        self.refresh_metrics()?;
        self.refresh_reservations()?;
        self.with_command_context(|context| {
            let current = context.sink.inspect();
            let source_catalogue = context.catalogue.state().clone();
            let source = commands::current_envelope_snapshot(context.catalogue, &current, context.registry)?;
            let preparation = preparation_context(&source)?;
            let action = LayoutAction::Apply { expected_revision: Some(current.revision), mutations: mutations.clone() };
            let mut projected = context.sink.project_in_context(action, &preparation)?;
            if !widths.is_empty() {
                // Resolve reused/new columns after moves, then let the shared
                // reducer apply their widths and update every member together.
                for (instance, width) in widths {
                    let column =
                        projected.containing(&instance)?.column.clone().ok_or_else(|| error(Rule::Placement, "legacy side column disappeared"))?;
                    mutations.push(Mutation::SetColumnWidth { column, width });
                }
                let action = LayoutAction::Apply { expected_revision: Some(current.revision), mutations };
                projected = context.sink.project_in_context(action, &preparation)?;
            }
            for (instance, weight) in weights {
                let id = projected.containing(&instance)?.id.clone();
                let section = projected
                    .sections
                    .iter_mut()
                    .find(|section| section.id == id)
                    .ok_or_else(|| error(Rule::MissingTarget, "legacy section disappeared"))?;
                section.weight = weight;
            }
            let envelope = with_live_layout(&source, projected, context.registry)?;
            let incoming = preparation_context(&envelope)?;
            let action = LayoutAction::Replace { expected_revision: Some(current.revision), layout: envelope.layout.clone() };
            let mut catalogue = commands::InMemoryCatalogue::from_snapshot(&context.catalogue.state().export()?, context.registry)?;
            let rejected = context.catalogue.state().remembered(&envelope.layout.workspace).and_then(|record| record.rejected());
            catalogue.remember(&envelope, rejected, context.registry)?;
            use commands::LayoutCatalogue as _;
            let prepared = context.catalogue.prepare(catalogue.state().clone())?;
            check_preference_source(context, &source_catalogue, current.revision, &preparation)?;
            context.sink.project_in_context(action.clone(), &incoming)?;
            context.sink.dispatch_in_context(action, &incoming)?;
            context.catalogue.publish(prepared);
            Ok(())
        })?;
        if let Some(state) = &mut self.preferences {
            state.legacy = Some(legacy);
        }
        Ok(())
    }
    fn restore_preference_catalogue(&mut self, session: &Session, next: commands::CatalogueState, envelope: LayoutEnvelope) -> PanelResult<()> {
        if let Some(host) = &mut self.host {
            host.cancel_gesture();
        }
        self.reconcile(session)?;
        self.refresh_metrics()?;
        self.refresh_reservations()?;
        let preparation = preparation_context(&envelope)?;
        self.with_command_context(|context| {
            let current = context.sink.inspect();
            let revision = current.revision;
            let source_catalogue = context.catalogue.state().clone();
            let source = commands::current_envelope_snapshot(context.catalogue, &current, context.registry)?;
            let source_context = preparation_context(&source)?;
            let action = LayoutAction::Replace { expected_revision: Some(revision), layout: envelope.layout };
            // Both catalogue staging and the exact incoming-context host proposal
            // complete before either committed owner changes.
            let prepared = context.catalogue.prepare(next)?;
            check_preference_source(context, &source_catalogue, revision, &source_context)?;
            context.sink.project_in_context(action.clone(), &preparation)?;
            context.sink.dispatch_in_context(action, &preparation)?;
            context.catalogue.publish(prepared);
            Ok(())
        })
    }
    /// Internal capability injection. Production preferences/native realization
    /// remain peer work. Installation is once-only and precedes native hosts.
    pub fn set_command_adapters(&mut self, catalogue: Box<dyn commands::LayoutCatalogue>, native: Box<dyn NativeHostAdapter>) -> PanelResult<()> {
        if self.preferences.is_some() {
            return Err(error(Rule::Lifecycle, "application panel preferences already own the catalogue"));
        }
        self.set_native_adapter(native)?;
        self.catalogue = catalogue;
        self.adapters_set = true;
        Ok(())
    }
    pub fn command(&mut self, session: &Session, id: &str, params: &serde_json::Value) -> PanelResult<serde_json::Value> {
        if !matches!(id, "panel.list" | "layout.get")
            && let Some(host) = &mut self.host
        {
            host.cancel_gesture();
        }
        let command = commands::parse_command(id, params)?;
        if !matches!(id, "panel.list" | "layout.get" | "layout.reset") && self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            return Err(error(Rule::Availability, "saved panel layout restoration is pending; reset explicitly to recover"));
        }
        self.reconcile(session)?;
        if !matches!(&command, commands::PanelCommand::HeaderAction(_)) {
            self.refresh_metrics()?;
            self.refresh_reservations()?;
        }
        if id == "layout.reset"
            && let Some(record) = self.preferences.as_ref().and_then(|state| state.pending.clone())
        {
            if let commands::PanelCommand::Reset(params) = &command
                && params.expected_revision.is_some_and(|revision| revision != self.runtime.layout().inspect().revision)
            {
                return Err(error(Rule::Revision, "layout changed before reset"));
            }
            return self.reset_pending_preferences(session, record);
        }
        self.with_command_context(|context| commands::execute_command(command, context))
    }
    pub fn command_batch(&mut self, session: &Session, steps: &[serde_json::Value]) -> PanelResult<serde_json::Value> {
        if self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            return Err(error(Rule::Availability, "saved panel layout restoration is pending"));
        }
        if let Some(host) = &mut self.host {
            host.cancel_gesture();
        }
        // Unsupported intent mixtures refuse before even context lifecycle or
        // availability publication; supported calls then read current context.
        commands::preflight_batch(steps, &self.runtime.layout().layout().workspace)?;
        self.reconcile(session)?;
        if !matches!(steps, [step] if step.get("command").or_else(|| step.get("id")).and_then(serde_json::Value::as_str) == Some("panel.action")) {
            self.refresh_metrics()?;
            self.refresh_reservations()?;
        }
        self.with_command_context(|context| commands::execute_batch(steps, context))
    }
    /// Commands borrow the same adapter, provider and local overlay ledger as
    /// host gestures. Forward committed focus effects without parsing JSON.
    fn with_command_context<R>(&mut self, execute: impl FnOnce(&mut commands::PanelCommandContext<'_>) -> PanelResult<R>) -> PanelResult<R> {
        let Self { runtime, services, native, catalogue, host, .. } = self;
        let registry = runtime.layout().registry().clone();
        let scope = runtime_scope(runtime);
        let mut services = services.services();
        let mut committed = None;
        let run = |host: Option<&mut WorkspacePanelHost>, services: &mut ContentServices<'_>| {
            let mut live = RuntimeActionSink { runtime: &mut *runtime, native: native.as_mut(), services };
            let mut admitted = AdmittedActionSink { live: &mut live, host, proposal: std::cell::RefCell::new(None) };
            let mut sink = CommandActionSink { live: &mut admitted, committed: &mut committed };
            execute(&mut commands::PanelCommandContext {
                sink: &mut sink,
                registry: &registry,
                catalogue: catalogue.as_mut(),
                web: cfg!(target_arch = "wasm32"),
            })
        };
        let result = if let Some(host) = host.as_mut() {
            host.with_command_services(&scope, &mut services, |host, services| run(Some(host), services))
        } else {
            run(None, &mut services)
        };
        if let Some(host) = host {
            host.reconcile_scope(runtime);
            if let Some(reply) = committed {
                host.note_reply(&reply);
            }
        }
        result
    }
    /// Inspection borrows the exact injected provider. A read-only runtime keeps
    /// callers from bypassing the app's single stored native resource owner.
    pub fn with_services<R>(&mut self, use_services: impl FnOnce(&PanelRuntime, &mut ContentServices<'_>) -> PanelResult<R>) -> PanelResult<R> {
        self.with_adapters(|runtime, _native, services| use_services(runtime, services))
    }
    fn with_adapters<R>(
        &mut self,
        use_services: impl FnOnce(&mut PanelRuntime, &mut dyn NativeHostAdapter, &mut ContentServices<'_>) -> PanelResult<R>,
    ) -> PanelResult<R> {
        let Self { runtime, services, native, host, .. } = self;
        let scope = runtime_scope(runtime);
        let mut services = services.services();
        if let Some(host) = host {
            host.with_local_services(&scope, &mut services, |services| use_services(runtime, native.as_mut(), services))
        } else {
            use_services(runtime, native.as_mut(), &mut services)
        }
    }
    pub fn dispatch(&mut self, session: &Session, action: LayoutAction) -> PanelResult<LayoutReply> {
        if action != LayoutAction::Inspect && self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            return Err(error(Rule::Availability, "saved panel layout restoration is pending"));
        }
        if let Some(host) = &mut self.host {
            host.cancel_gesture();
        }
        self.reconcile(session)?;
        if !standalone_header_action(&action) {
            self.refresh_metrics()?;
            self.refresh_reservations()?;
        }
        self.with_command_context(|context| commands::execute_layout_action(action, context))
    }
    /// All host actions use one retained native adapter and the injected provider.
    pub fn dispatch_host(&mut self, session: &Session, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.dispatch(session, action)
    }
    /// Install before native placement exists. Replacing an adapter while it owns
    /// resources would orphan those resources and is rejected atomically.
    pub fn set_native_adapter(&mut self, native: Box<dyn NativeHostAdapter>) -> PanelResult<()> {
        self.runtime.require_live()?;
        if self.adapters_set {
            return Err(error(Rule::Lifecycle, "application panel adapters are already installed"));
        }
        if self.runtime.layout().layout().sections.iter().any(|section| section.location.zone() == Zone::Window) {
            return Err(error(Rule::Lifecycle, "redock native hosts before replacing their adapter"));
        }
        let capabilities = native.capabilities();
        if capabilities.max_native_hosts > MAX_SECTIONS || !capabilities.native_windows && capabilities.max_native_hosts != 0 {
            return Err(error(Rule::Capability, "invalid native adapter capacity"));
        }
        self.native = native;
        Ok(())
    }
    pub fn native_capabilities(&self) -> HostCapabilities {
        self.native.capabilities()
    }
    pub fn host(&self) -> Option<&WorkspacePanelHost> {
        self.host.as_ref()
    }
    pub fn enable_host(&mut self, options: HostOptions) -> PanelResult<()> {
        self.runtime.require_live()?;
        if self.host.is_some() {
            return Err(error(Rule::Lifecycle, "universal panel host already enabled"));
        }
        if !self.native.capabilities().native_windows
            && self.runtime.layout().layout().sections.iter().any(|section| section.location.zone() == Zone::Window)
        {
            return Err(error(Rule::Capability, "redock existing native placements through their adapter before enabling the in-app host"));
        }
        self.host = Some(WorkspacePanelHost::new(options)?);
        Ok(())
    }
    pub fn set_requested_opacity(&mut self, opacity: Option<f32>) -> PanelResult<()> {
        self.host.as_mut().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?.set_requested_opacity(opacity)
    }
    pub fn disable_host(&mut self) -> PanelResult<()> {
        if self.host.is_some() {
            self.with_adapters(|runtime, _native, services| runtime.mount_in_app(&ResolvedLayout::default(), &mut EmptyPanelHost, services))?;
        }
        self.host = None;
        Ok(())
    }
    /// Hiding the application workspace is an input/presentation transition,
    /// independent of layout revision. Retained owners keep their live services.
    fn hide_in_app(&mut self) -> PanelResult<()> {
        if let Some(host) = &mut self.host {
            host.invalidate_context();
        }
        self.with_adapters(|runtime, _native, services| runtime.mount_in_app(&ResolvedLayout::default(), &mut EmptyPanelHost, services))
    }
    pub fn prepare_host(&mut self, session: &Session, capacity: LogicalRect) -> PanelResult<()> {
        self.prepare_host_in_domain(session, capacity, capacity, &[])
    }

    pub fn prepare_host_in_domain(
        &mut self,
        session: &Session,
        render_domain: LogicalRect,
        capacity: LogicalRect,
        chrome: &[LogicalRect],
    ) -> PanelResult<()> {
        self.reconcile(session)?;
        self.refresh_metrics()?;
        if let Some(host) = &mut self.host {
            host.capture_reservations(&self.runtime, self.catalogue.as_ref())?;
            host.note_native_capabilities(self.native.capabilities());
        }
        let result = self.host.as_mut().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?.prepare_in_domain(
            &self.runtime,
            render_domain,
            capacity,
            chrome,
        );
        let environment = if result.is_ok() && self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            let reference =
                self.host.as_ref().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?.command_reference(&self.runtime)?;
            Some((
                reference.render_domain,
                reference.allocation_environment,
                reference.geometry_policy.content_revision,
                self.runtime.environment().clone(),
                reference.chrome_obstacles,
                self.native.capabilities(),
            ))
        } else {
            None
        };
        if let Some(state) = &mut self.preferences
            && state.failed
            && state.failed_environment != environment
        {
            state.failed = false;
        }
        if result.is_ok()
            && let Some(record) = self.preferences.as_ref().filter(|state| !state.failed).and_then(|state| state.pending.clone())
        {
            match self.restore_preference_record(session, record) {
                Ok(()) => {
                    self.refresh_metrics()?;
                    self.refresh_reservations()?;
                    self.host.as_mut().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?.prepare_in_domain(
                        &self.runtime,
                        render_domain,
                        capacity,
                        chrome,
                    )?;
                }
                Err(reason) => {
                    if let Some(state) = &mut self.preferences {
                        state.failed = true;
                        state.failed_environment = environment;
                    }
                    self.last_error = Some(reason);
                }
            }
        }
        if result.is_err() {
            self.with_adapters(|runtime, _native, services| runtime.mount_in_app(&ResolvedLayout::default(), &mut EmptyPanelHost, services))?;
        }
        result
    }
    pub fn show_host(&mut self, session: &Session, ui: &mut egui::Ui) -> PanelResult<()> {
        self.show_host_impl(session, ui, false)
    }
    pub fn show_host_deferred(&mut self, session: &Session, ui: &mut egui::Ui) -> PanelResult<()> {
        self.show_host_impl(session, ui, true)
    }
    fn show_production_host(&mut self, app: &mut crate::SolveApp, ui: &mut egui::Ui) -> PanelResult<()> {
        self.prepare_host_paint(&app.session, ui)?;
        app.panel_content_frame = self.host.as_ref().and_then(WorkspacePanelHost::frame).cloned();
        let mut services = self.services.services();
        let host = self.host.as_mut().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?;
        let pending = self.preferences.as_ref().is_some_and(|state| state.pending.is_some());
        if pending {
            host.cancel_gesture();
        }
        let state = self.production.clone().ok_or_else(|| error(Rule::Lifecycle, "production content binding unavailable"))?;
        let mut content = super::production::ProductionFrame { app, state };
        let result = ui
            .add_enabled_ui(!pending, |ui| host.show_deferred_with_content(ui, &mut self.runtime, self.native.as_mut(), &mut services, &mut content))
            .inner;
        if pending {
            host.cancel_gesture();
        }
        result
    }
    fn show_host_impl(&mut self, session: &Session, ui: &mut egui::Ui, deferred: bool) -> PanelResult<()> {
        self.prepare_host_paint(session, ui)?;
        let mut services = self.services.services();
        let host = self.host.as_mut().ok_or_else(|| error(Rule::Lifecycle, "universal panel host disabled"))?;
        if self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            host.cancel_gesture();
            // A fallback frame is observable while recovery is pending, but no
            // body/header gesture may commit into an unpublished catalogue.
            let result = ui.add_enabled_ui(false, |ui| host.show_deferred(ui, &mut self.runtime, self.native.as_mut(), &mut services)).inner;
            host.cancel_gesture();
            return result;
        }
        if deferred {
            host.show_deferred(ui, &mut self.runtime, self.native.as_mut(), &mut services)
        } else {
            host.show(ui, &mut self.runtime, self.native.as_mut(), &mut services, self.catalogue.as_mut())
        }
    }
    fn prepare_host_paint(&mut self, session: &Session, ui: &egui::Ui) -> PanelResult<()> {
        let geometry = self.host.as_ref().and_then(WorkspacePanelHost::prepared_geometry);
        self.reconcile(session)?;
        self.refresh_metrics()?;
        if let Some(host) = &mut self.host
            && host.frame().is_none()
            && let Some((domain, capacity, chrome)) = geometry
        {
            host.prepare_in_domain(&self.runtime, domain, capacity, &chrome)?;
            ui.ctx().request_repaint();
        }
        Ok(())
    }
    /// Finish after every window/modal has registered its current layer. Refresh
    /// current Session again before dispatch, without a background update.
    pub fn finish_host(&mut self, session: &Session, ui: &mut egui::Ui) -> PanelResult<()> {
        if self.preferences.as_ref().is_some_and(|state| state.pending.is_some()) {
            if let Some(host) = &mut self.host {
                host.cancel_gesture();
            }
            return Ok(());
        }
        if !self.host.as_ref().is_some_and(WorkspacePanelHost::has_pending) {
            return Ok(());
        }
        self.reconcile(session)?;
        self.refresh_metrics()?;
        let mut services = self.services.services();
        if let Some(host) = &mut self.host {
            host.finish(ui, &mut self.runtime, self.native.as_mut(), &mut services, self.catalogue.as_mut())
        } else {
            Ok(())
        }
    }
    /// Complete root/in-app cohort only. Native peers always use mount_native,
    /// even while the shared root renderer is disabled.
    pub fn mount(&mut self, session: &Session, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.reconcile(session)?;
        if self.host.is_some() {
            return Err(error(Rule::Lifecycle, "universal host owns the complete in-app presentation frame"));
        }
        self.with_adapters(|runtime, _native, services| runtime.mount_in_app(resolved, host, services))
    }
    pub fn mount_native(&mut self, session: &Session, native_host: &HostId, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.reconcile(session)?;
        self.with_adapters(|runtime, _native, services| runtime.mount_native(native_host, resolved, host, services))
    }
    pub fn content_action(&mut self, session: &Session, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
        self.reconcile(session)?;
        self.with_adapters(|runtime, _native, services| runtime.content_action(instance, action, services))
    }
    fn refresh_reservations(&mut self) -> PanelResult<()> {
        if let Some(host) = &mut self.host {
            host.capture_reservations(&self.runtime, self.catalogue.as_ref())?;
        }
        Ok(())
    }
    fn refresh_metrics(&mut self) -> PanelResult<()> {
        if let Some(host) = &mut self.host {
            let services = self.services.services();
            if let Err(reason) = host.capture_metrics(&self.runtime, services.models) {
                host.invalidate_context();
                self.runtime.suspend_presentations(services.local);
                return Err(reason);
            }
        }
        Ok(())
    }
    /// Current Session is refreshed at every presentation/action boundary, even
    /// after this egui frame's one background tick. No controller update runs.
    pub fn reconcile(&mut self, session: &Session) -> PanelResult<()> {
        let before = self.runtime.environment().clone();
        let revision = self.runtime.layout().inspect().revision;
        let result = (|| {
            self.services.refresh_workspace(&self.runtime.layout().layout().workspace)?;
            self.services.refresh_read_only(session)?;
            let mut environment = self.runtime.environment().clone();
            environment.document = Some(DocumentId::new(session.document_id.clone())?);
            self.with_adapters(|runtime, _native, services| runtime.reconcile_context(environment, services))
        })();
        match &result {
            Ok(()) => {
                if self.runtime.update_errors().is_empty() && !self.preferences.as_ref().is_some_and(|state| state.failed) {
                    self.last_error = None;
                }
                if (self.runtime.environment() != &before || self.runtime.layout().inspect().revision != revision)
                    && let Some(host) = &mut self.host
                {
                    host.invalidate_context();
                    host.reconcile_scope(&self.runtime);
                }
            }
            Err(reason) => {
                self.last_error = Some(reason.clone());
                if let Some(host) = &mut self.host {
                    host.invalidate_context();
                }
                let services = self.services.services();
                self.runtime.suspend_presentations(services.local);
            }
        }
        result
    }
    pub fn tick(&mut self, session: &Session) -> PanelResult<()> {
        let refreshed =
            self.services.refresh_workspace(&self.runtime.layout().layout().workspace).and_then(|()| self.services.refresh_read_only(session));
        let result = match refreshed {
            Ok(()) => (|| {
                let mut environment = self.runtime.environment().clone();
                environment.document = Some(DocumentId::new(session.document_id.clone())?);
                self.with_adapters(|runtime, _native, services| {
                    runtime.set_environment(environment, services.local)?;
                    runtime.update(services)
                })
            })(),
            Err(reason) => {
                // The provider still represents the previous context. Never
                // admit the incoming Session owner against those stale models.
                if let Some(host) = &mut self.host {
                    host.invalidate_context();
                }
                // Retained background owners still receive this tick's one
                // service attempt. Bounded failures remain in update_errors;
                // the caller receives the original refresh failure.
                let _update = self.with_adapters(|runtime, _native, services| runtime.update(services));
                let services = self.services.services();
                self.runtime.suspend_presentations(services.local);
                Err(reason)
            }
        };
        self.last_error = result.as_ref().err().cloned();
        result
    }
    /// Explicit teardown preflights all owners before any controller is destroyed.
    pub fn shutdown(&mut self) -> PanelResult<()> {
        self.with_adapters(|runtime, native, services| runtime.shutdown_with_native(native, services.local))
    }
}

/// Observe typed replies from the authoritative command sink, including repeated
/// Focus requests whose persistent focused instance has not changed.
struct CommandActionSink<'a> {
    live: &'a mut dyn ActionSink,
    committed: &'a mut Option<LayoutReply>,
}

/// The app has a real captured placement environment. Structural commands may
/// not bypass its allocator merely because they arrived over a control channel.
pub(super) struct AdmittedActionSink<'a, 'runtime, 'services> {
    pub(super) live: &'a mut RuntimeActionSink<'runtime, 'services>,
    pub(super) host: Option<&'a mut WorkspacePanelHost>,
    pub(super) proposal: std::cell::RefCell<Option<super::runtime::RuntimeHostProposal>>,
}
impl AdmittedActionSink<'_, '_, '_> {
    fn project_owned(&self, action: LayoutAction, context: Option<&LayoutPreparationContext>) -> PanelResult<Layout> {
        if standalone_header_action(&action) {
            return match context {
                Some(context) => self.live.project_in_context(action, context),
                None => self.live.project(action),
            };
        }
        let host = self.host.as_ref().ok_or_else(|| error(Rule::Availability, "panel host environment is unavailable"))?;
        let reference = host.command_reference(self.live.runtime)?;
        let mut cached = self.proposal.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "panel proposal already borrowed"))?;
        if let Some(proposal) = cached.as_ref().filter(|proposal| proposal.action() == &action) {
            let freshness = (|| {
                if let Some(context) = context {
                    proposal.validate_preparation_context(context)?;
                }
                proposal.validate(self.live.runtime, &reference, self.live.services.models)
            })();
            if let Err(reason) = freshness {
                cached.take();
                return Err(reason);
            }
            return Ok(proposal.layout().clone());
        }
        let proposal = match cached.take() {
            Some(previous) => self.live.runtime.revise_host_proposal(previous, reference, action, context, self.live.services.models)?,
            None => match context {
                Some(context) => self.live.runtime.prepare_host_proposal_in_context(reference, action, context, self.live.services.models)?,
                None => self.live.runtime.prepare_host_proposal(reference, action, self.live.services.models)?,
            },
        };
        let layout = proposal.layout().clone();
        *cached = Some(proposal);
        Ok(layout)
    }
    fn dispatch_owned(&mut self, action: LayoutAction, context: Option<&LayoutPreparationContext>) -> PanelResult<LayoutReply> {
        if action == LayoutAction::Inspect {
            return Ok(self.inspect());
        }
        if standalone_header_action(&action) {
            return match context {
                Some(context) => self.live.dispatch_in_context(action, context),
                None => self.live.dispatch(action),
            };
        }
        self.project_owned(action.clone(), context)?;
        let proposal = self.proposal.get_mut().take().ok_or_else(|| error(Rule::Lifecycle, "panel proposal missing"))?;
        if proposal.action() != &action {
            return Err(error(Rule::Revision, "panel dispatch intent changed"));
        }
        let reply = self.live.runtime.commit_host_proposal(proposal, self.live.native, self.live.services)?;
        Ok(super::runtime::normalize_native_reply(reply, &self.live.native.capabilities()))
    }
}
impl ActionSink for AdmittedActionSink<'_, '_, '_> {
    fn resolve_contextual(&self, action: ContextualPanelAction) -> PanelResult<LayoutAction> {
        self.host.as_ref().ok_or_else(|| error(Rule::Availability, "shared panel host unavailable"))?.contextual_action(self.live.runtime, action)
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        self.project_owned(action, None)
    }
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        self.project_owned(action, Some(context))
    }
    fn inspect(&self) -> LayoutReply {
        self.live.inspect()
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.dispatch_owned(action, None)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        self.dispatch_owned(action, Some(context))
    }
}
impl ActionSink for CommandActionSink<'_> {
    fn resolve_contextual(&self, action: ContextualPanelAction) -> PanelResult<LayoutAction> {
        self.live.resolve_contextual(action)
    }
    fn project_in_context(&self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<Layout> {
        self.live.project_in_context(action, context)
    }
    fn dispatch_in_context(&mut self, action: LayoutAction, context: &LayoutPreparationContext) -> PanelResult<LayoutReply> {
        let reply = self.live.dispatch_in_context(action, context)?;
        *self.committed = Some(reply.clone());
        Ok(reply)
    }
    fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
        self.live.project(action)
    }
    fn inspect(&self) -> LayoutReply {
        self.live.inspect()
    }
    fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        let reply = self.live.dispatch(action)?;
        *self.committed = Some(reply.clone());
        Ok(reply)
    }
}

impl crate::SolveApp {
    /// Read-only command metadata uses the exact host eligibility used at execution.
    pub(crate) fn panel_contextual_disabled_reason(&self, action: ContextualPanelAction) -> Option<String> {
        let eligibility = self
            .panel_runtime
            .as_ref()
            .ok_or_else(|| error(Rule::Availability, "shared panel host unavailable"))
            .and_then(|integration| integration.contextual_availability(&self.session, action));
        eligibility.err().map(|reason| {
            solidcraft_ui_text::Locale::for_language(&self.preferences.language).text(super::workspace::rule_key(reason.rule)).to_owned()
        })
    }

    pub fn run_panel_command(&mut self, id: &str, params: &serde_json::Value) -> PanelResult<serde_json::Value> {
        super::production::refresh_production(self)?;
        // Parse even without a runtime: hostile/unknown fields never become a
        // misleading capability success or reach the document command path.
        match self.panel_runtime.as_mut() {
            Some(integration) => integration.command(&self.session, id, params),
            None => {
                commands::parse_command(id, params)?;
                Err(error(Rule::Capability, "application panel runtime is not enabled"))
            }
        }
    }
    pub fn run_panel_batch(&mut self, steps: &[serde_json::Value]) -> PanelResult<serde_json::Value> {
        self.panel_runtime
            .as_mut()
            .ok_or_else(|| error(Rule::Capability, "application panel runtime is not enabled"))?
            .command_batch(&self.session, steps)
    }
    pub fn enable_universal_panel_host(&mut self, options: HostOptions) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "enable a panel runtime before its host"))?.enable_host(options)
    }
    pub fn disable_universal_panel_host(&mut self) -> PanelResult<()> {
        if let Some(integration) = &mut self.panel_runtime {
            integration.disable_host()?;
        }
        Ok(())
    }
    pub fn universal_panel_host_enabled(&self) -> bool {
        self.panel_content_frame.is_some() || self.panel_runtime.as_ref().is_some_and(|integration| integration.host.is_some())
    }
    pub fn universal_panel_frame(&self) -> Option<&HostFrame> {
        self.panel_runtime
            .as_ref()
            .and_then(|integration| integration.host.as_ref())
            .and_then(WorkspacePanelHost::frame)
            .or(self.panel_content_frame.as_ref())
    }
    /// Internal opt-in only: no preference, control method, command or default change.
    pub fn enable_panel_runtime(&mut self, runtime: PanelRuntime, services: Box<dyn AppPanelServices>) -> PanelResult<()> {
        if self.panel_runtime.is_some() {
            return Err(error(Rule::Lifecycle, "panel runtime already enabled"));
        }
        let mut integration = AppPanelRuntime::new(runtime, services)?;
        integration.tick(&self.session)?;
        self.panel_runtime = Some(integration);
        Ok(())
    }
    pub fn panel_runtime(&self) -> Option<&PanelRuntime> {
        self.panel_runtime.as_ref().map(|integration| &integration.runtime)
    }
    pub fn panel_runtime_mut(&mut self) -> Option<&mut AppPanelRuntime> {
        self.panel_runtime.as_mut()
    }
    pub fn set_panel_native_adapter(&mut self, native: Box<dyn NativeHostAdapter>) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.set_native_adapter(native)
    }
    pub fn dispatch_panel_layout(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        super::production::refresh_production(self)?;
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.dispatch(&self.session, action)
    }
    pub fn mount_panels(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.mount(&self.session, resolved, host)
    }
    /// A complete set for one native host; empty unmounts just that viewport.
    pub fn mount_native_panels(&mut self, native_host: &HostId, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.mount_native(
            &self.session,
            native_host,
            resolved,
            host,
        )
    }
    pub fn panel_content_action(&mut self, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
        super::production::refresh_production(self)?;
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.content_action(&self.session, instance, action)
    }
    pub fn tick_panel_runtime(&mut self) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.tick(&self.session)
    }
    pub fn disable_panel_runtime(&mut self) -> PanelResult<()> {
        let saved = super::preferences::preferences_value(self);
        if let Some(integration) = &mut self.panel_runtime {
            integration.shutdown()?;
        }
        self.panel_runtime = None;
        self.panel_layout_preferences.record = saved;
        Ok(())
    }
}

pub(crate) fn sync_production_setting(app: &mut crate::SolveApp) -> PanelResult<()> {
    let production = app.panel_runtime.as_ref().is_some_and(|integration| integration.production.is_some());
    if app.preferences.shared_panel_layout && app.panel_runtime.is_none() {
        super::production::enable_production(app)
    } else if !app.preferences.shared_panel_layout && production {
        app.disable_panel_runtime()
    } else {
        Ok(())
    }
}

pub(crate) fn tick_app(app: &mut crate::SolveApp, ctx: &egui::Context) {
    // Switching placement systems waits until Preferences Apply/Cancel settles
    // its snapshot. Content state itself always remains owned by the app.
    if !app.prefs_window.open
        && let Err(reason) = sync_production_setting(app)
    {
        app.set_status(solidcraft_ui_text::tr(super::workspace::rule_key(reason.rule)).to_owned(), true);
        if let Some(integration) = &mut app.panel_runtime {
            integration.last_error = Some(reason);
        }
    }
    if let Err(reason) = super::production::drain_production_actions(app)
        .and_then(|()| super::production::sync_production_visibility(app))
        .and_then(|()| super::production::refresh_production(app))
        && let Some(integration) = &mut app.panel_runtime
    {
        integration.last_error = Some(reason);
    }
    let direction = crate::i18n::panel_direction(&app.preferences.language);
    if let Some(integration) = &mut app.panel_runtime {
        let frame = ctx.cumulative_frame_nr();
        if integration.last_tick_frame == Some(frame) {
            return;
        }
        integration.last_tick_frame = Some(frame);
        // Errors remain inspectable without inventing untranslated production UI.
        let result = integration.sync_native_direction(direction).and_then(|()| integration.tick(&app.session));
        if result.is_err() || !integration.preferences.as_ref().is_some_and(|state| state.failed) {
            integration.last_error = result.err();
        }
    }
    if app
        .panel_runtime
        .as_ref()
        .and_then(|integration| integration.preferences.as_ref())
        .is_some_and(|state| state.failed || state.rejected.is_some())
    {
        super::preferences::report_notice(app);
    }
}

fn check_preference_source(
    context: &commands::PanelCommandContext<'_>,
    catalogue: &commands::CatalogueState,
    revision: u64,
    preparation: &LayoutPreparationContext,
) -> PanelResult<()> {
    let current = context.sink.inspect();
    let envelope = commands::current_envelope_snapshot(context.catalogue, &current, context.registry)?;
    if context.catalogue.state() != catalogue || current.revision != revision || preparation_context(&envelope)? != *preparation {
        return Err(error(Rule::Revision, "panel preference source changed during storage staging"));
    }
    Ok(())
}

/// Called on the actual hidden transition, including a control-channel Home
/// open/close interval between paints. No tick, destruction or owner rebind.
pub(crate) fn hide_app_host(app: &mut crate::SolveApp) {
    if let Some(integration) = &mut app.panel_runtime
        && let Err(reason) = integration.hide_in_app()
    {
        integration.last_error = Some(reason);
    }
}

pub(crate) fn reserve_host_input(app: &mut crate::SolveApp, ctx: &egui::Context) {
    let Some(integration) = &mut app.panel_runtime else {
        return;
    };
    let Some(host) = &mut integration.host else {
        return;
    };
    if host.wants_escape(&integration.runtime) && crate::ui_kit::Container::reserve_host_escape(ctx) {
        host.request_escape();
        app.esc_handled = true;
    }
}

pub(crate) fn finish_app_host(app: &mut crate::SolveApp, ui: &mut egui::Ui) {
    if let Err(reason) = super::production::sync_production_visibility(app).and_then(|()| super::production::refresh_production(app))
        && let Some(integration) = &mut app.panel_runtime
    {
        integration.last_error = Some(reason);
        return;
    }
    if let Some(integration) = &mut app.panel_runtime
        && let Err(reason) = integration.finish_host(&app.session, ui)
    {
        integration.last_error = Some(reason);
    }
    if let Err(reason) = super::production::drain_production_actions(app)
        .and_then(|()| super::production::sync_production_visibility(app))
        .and_then(|()| super::production::refresh_production(app))
        && let Some(integration) = &mut app.panel_runtime
    {
        integration.last_error = Some(reason);
    }
}

impl crate::SolveApp {
    /// Alternate placement only. The caller keeps the app bar, ribbon, existing
    /// viewport/navigation and late feature/system overlays in their normal flow.
    pub(crate) fn universal_workspace_ui(&mut self, ui: &mut egui::Ui) {
        // Capture before either ribbon or bottom chrome consumes root layout.
        // Rendering and picking always share this full domain below the app bar.
        let render_domain = ui.available_rect_before_wrap();
        self.viewport.host_confirm = None;
        crate::ui_kit::Container::begin_host_windows(ui.ctx());
        self.viewport.host_windows = Some(Vec::new());
        if self.session.active_sketch.is_none() {
            self.viewport.palette_rect = None;
        }
        // Clear stale Browser-owned metadata; the retained independent
        // redefine/feature editor publishes its actual paint later in this pass.
        if self.dialog.is_none() {
            self.viewport.dialog_rect = None;
        }
        // Unoccupied dock capacity and ribbon spacing are application backdrop,
        // not panel paint/input regions. Cover the native GPU clear through the
        // shared viewport surface without changing any declared panel geometry.
        let backdrop = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Viewport);
        crate::ui_kit::Container::paint_frame(
            ui.painter(),
            backdrop.edge_frame(solidcraft_theme::Tokens::get().panel),
            ui.available_rect_before_wrap(),
        );
        crate::timeline::follow_view_pick(self);
        crate::render_ui::focus_shortcut(self, ui);
        crate::dialogs::sync_feature_edit(self);
        if cfg!(not(target_arch = "wasm32")) && self.preferences.agent_chat_popped {
            // Keep the previous embedded-window rect for hover shortcuts.
            // `show` owns geometry/lifecycle and also polls in this path.
            crate::agent_chat::show(self, ui);
        } else {
            // The root Assistant panel is suppressed. Forget only presentation
            // geometry and placement, so a later pop uses the saved window place.
            crate::agent_chat::poll(self, ui.ctx());
            if self.panel_runtime.as_ref().is_some_and(|integration| integration.production.is_some()) {
                crate::agent_chat::prepare_shared(self, ui.ctx());
            } else {
                self.agent_chat.panel_rect = None;
                self.agent_chat.drop_rect = None;
                self.agent_chat.dock_space = None;
                self.agent_chat.window_placed = false;
            }
        }
        self.shell.browser = None;
        self.shell.browser_hidden = false;
        self.shell.history = None;
        self.shell.panels.clear();
        self.shell.workspace_panels.clear();
        self.shell.dialog_column = None;
        self.shell.left_space = None;
        self.shell.history_dock_space = None;
        self.shell.history_folding = None;
        self.shell.history_fold_motion = None;
        self.publish_tab_accent(ui.ctx());
        crate::browser::publish_row_prefs(self, ui.ctx());
        let mut placement = render_domain;
        self.shell.status_bar = if crate::status_bar::shown(self) {
            let height = crate::status_bar::HEIGHT.min(placement.height().max(0.0));
            let bar = egui::Rect::from_min_max(egui::pos2(placement.left(), placement.bottom() - height), placement.max);
            placement.max.y = bar.top();
            Some(bar)
        } else {
            None
        };
        if !self.embedded {
            let cards = crate::toolbar::shell_cards(self, placement);
            self.shell.cards = vec![cards.workspace, cards.ribbon];
            placement.min.y = cards.bottom.min(placement.bottom()).max(placement.top());
        } else {
            self.shell.cards.clear();
        }
        if self.panel_runtime.as_ref().is_some_and(|integration| integration.production.is_some()) && crate::render_workspace::active(self) {
            // These remain legacy content surfaces. Their actual edge reservations
            // constrain panel placement without changing the full render domain.
            crate::render_ui::shots_strip(self, ui);
            crate::render_workspace::panel(self, ui);
            if self.ui.show_browser && !crate::render_ui::focused(self) {
                // Keep the unmigrated library in its existing lower card slot.
                // Reserve its real top edge before admitting shared placements.
                let bounds = crate::shell::panel_bounds(self, ui.available_rect_before_wrap());
                let upper = crate::ui_kit::Panel::stack_first_bounds(bounds);
                crate::render_workspace::appearances_card(self, ui, crate::ui_kit::Panel::stack_next_bounds(bounds, upper));
            }
            for (id, (bounds, edge)) in &self.shell.workspace_panels {
                if *id == "render:appearances" {
                    placement.max.y = bounds.top().clamp(placement.top(), placement.bottom());
                    continue;
                }
                match edge {
                    Some(crate::ui_kit::Edge::Left) => placement.min.x = bounds.right().clamp(placement.left(), placement.right()),
                    Some(crate::ui_kit::Edge::Right) => placement.max.x = bounds.left().clamp(placement.left(), placement.right()),
                    Some(crate::ui_kit::Edge::Bottom) => placement.max.y = bounds.top().clamp(placement.top(), placement.bottom()),
                    _ => {}
                }
            }
        }
        let gap = crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Panel).gap();
        placement.min.x += gap.min(placement.width() / 2.0);
        placement.max.x -= gap.min(placement.width());
        placement.max.y -= gap.min(placement.height());
        if self.embedded {
            placement.min.y += gap.min(placement.height());
        }
        let chrome: Vec<_> = self
            .shell
            .cards
            .iter()
            .copied()
            .chain(self.shell.status_bar)
            .chain(self.shell.workspace_panels.values().map(|(bounds, _)| *bounds))
            .map(|bounds| bounds.intersect(render_domain))
            .filter(|bounds| bounds.is_positive() && bounds.is_finite())
            .map(logical_rect)
            .collect();
        let force_opaque = self.services.reduce_transparency.as_ref().and_then(|read| read(true)) == Some(true);
        let direction = crate::i18n::panel_direction(&self.preferences.language);
        let shortcuts = crate::keymap::panel_shortcuts(self);
        let prepared = match &mut self.panel_runtime {
            Some(integration) => {
                let result = (|| {
                    integration.sync_native_direction(direction)?;
                    integration.set_keyboard_shortcuts(shortcuts);
                    if let Some(host) = &mut integration.host {
                        host.note_egui_context(ui.ctx(), force_opaque);
                        host.set_gap(gap)?;
                    }
                    integration.prepare_host_in_domain(&self.session, logical_rect(render_domain), logical_rect(placement), &chrome)
                })();
                if let Err(reason) = &result {
                    integration.last_error = Some(reason.clone());
                }
                result.is_ok()
            }
            None => false,
        };
        self.shell.content = Some(render_domain);
        if render_domain.is_positive() && render_domain.is_finite() {
            crate::preview::update(self, ui.ctx());
            let mut scene = ui.new_child(egui::UiBuilder::new().id_salt("universal_panel_viewport").max_rect(render_domain));
            scene.set_clip_rect(scene.clip_rect().intersect(render_domain));
            self.show_workspace(&mut scene);
        }
        // Chrome paints and receives input above the full-domain scene. Its
        // measured rectangles affect usable space, never the camera projection.
        crate::status_bar::show(self, ui);
        if !self.embedded {
            crate::toolbar::toolbar(self, ui);
        }
        crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Viewport)
            .central()
            .frame(crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Viewport).edge_frame(egui::Color32::TRANSPARENT))
            .show(ui, |ui| {
                if prepared && let Some(mut integration) = self.panel_runtime.take() {
                    let result = if integration.production.is_some() {
                        // Content commands still see the true full-domain geometry
                        // while the runtime and mutable content are borrowed apart.
                        let result = integration.show_production_host(self, ui);
                        self.panel_content_frame = None;
                        result
                    } else {
                        integration.show_host_deferred(&self.session, ui)
                    };
                    if let Err(reason) = result {
                        integration.last_error = Some(reason);
                    }
                    self.panel_runtime = Some(integration);
                }
            });
        if self.panel_runtime.as_ref().is_some_and(|integration| integration.production.is_some()) && !self.preferences.agent_chat_popped {
            crate::agent_chat::finish_shared(self, ui.ctx());
        }
    }
}
