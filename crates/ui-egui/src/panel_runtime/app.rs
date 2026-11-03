use solidcraft_engine::Session;
use solidcraft_engine::panels::*;

use super::*;

/// Later content adapters supply their own services. The app integration exposes
/// only an immutable Session during refresh; a layout tick cannot run a document
/// command. Explicit content commands stay on the separate supplied sink.
pub trait AppPanelServices {
    fn refresh_read_only(&mut self, session: &Session) -> PanelResult<()>;
    fn services(&mut self) -> ContentServices<'_>;
}

pub struct AppPanelRuntime {
    runtime: PanelRuntime,
    services: Box<dyn AppPanelServices>,
    last_error: Option<PanelError>,
    catalogue: Box<dyn commands::LayoutCatalogue>,
    native: Box<dyn NativeHostAdapter>,
    adapters_set: bool,
}
impl AppPanelRuntime {
    pub fn new(runtime: PanelRuntime, services: Box<dyn AppPanelServices>) -> Self {
        Self {
            runtime,
            services,
            last_error: None,
            catalogue: Box::<commands::InMemoryCatalogue>::default(),
            native: Box::new(UnavailableNative),
            adapters_set: false,
        }
    }
    pub fn last_error(&self) -> Option<&PanelError> {
        self.last_error.as_ref()
    }
    pub fn runtime(&self) -> &PanelRuntime {
        &self.runtime
    }
    /// Internal capability injection. Production preferences/native realization
    /// remain peer work. Installation is once-only and precedes native hosts.
    pub fn set_command_adapters(&mut self, catalogue: Box<dyn commands::LayoutCatalogue>, native: Box<dyn NativeHostAdapter>) -> PanelResult<()> {
        if self.adapters_set {
            return Err(error(Rule::Lifecycle, "application panel adapters are already installed"));
        }
        if self.runtime.layout().layout().sections.iter().any(|s| matches!(&s.location, SectionLocation::Window { .. })) {
            return Err(error(Rule::Lifecycle, "cannot replace adapters with live native hosts"));
        }
        self.catalogue = catalogue;
        self.native = native;
        self.adapters_set = true;
        Ok(())
    }
    /// Actual application boundaries refresh the current document/model context
    /// even when no frame tick ran between two sequential programmatic steps.
    fn refresh_context(&mut self, session: &Session) -> PanelResult<()> {
        self.services.refresh_read_only(session)?;
        let mut environment = self.runtime.environment().clone();
        environment.document = Some(DocumentId::new(session.document_id.clone())?);
        let services = self.services.services();
        self.runtime.set_environment(environment, services.local)?;
        self.runtime.refresh_context(services.models, services.local)
    }
    pub fn command(&mut self, session: &Session, id: &str, params: &serde_json::Value) -> PanelResult<serde_json::Value> {
        self.refresh_context(session)?;
        let registry = self.runtime.layout().registry().clone();
        let mut services = self.services.services();
        let mut sink = RuntimeActionSink { runtime: &mut self.runtime, native: self.native.as_mut(), services: &mut services };
        commands::execute(
            id,
            params,
            &mut commands::PanelCommandContext {
                sink: &mut sink,
                registry: &registry,
                catalogue: self.catalogue.as_mut(),
                web: cfg!(target_arch = "wasm32"),
            },
        )
    }
    pub fn command_batch(&mut self, session: &Session, steps: &[serde_json::Value]) -> PanelResult<serde_json::Value> {
        // Unsupported intent mixtures refuse before even context lifecycle or
        // availability publication; supported calls then read current context.
        commands::preflight_batch(steps, &self.runtime.layout().layout().workspace)?;
        self.refresh_context(session)?;
        let registry = self.runtime.layout().registry().clone();
        let mut services = self.services.services();
        let mut sink = RuntimeActionSink { runtime: &mut self.runtime, native: self.native.as_mut(), services: &mut services };
        commands::execute_batch(
            steps,
            &mut commands::PanelCommandContext {
                sink: &mut sink,
                registry: &registry,
                catalogue: self.catalogue.as_mut(),
                web: cfg!(target_arch = "wasm32"),
            },
        )
    }
    /// All forwarding borrows the exact provider injected at enablement.
    fn with_services<R>(&mut self, use_services: impl FnOnce(&mut PanelRuntime, &mut ContentServices<'_>) -> PanelResult<R>) -> PanelResult<R> {
        let mut services = self.services.services();
        use_services(&mut self.runtime, &mut services)
    }
    pub fn dispatch(&mut self, session: &Session, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.refresh_context(session)?;
        let mut services = self.services.services();
        RuntimeActionSink { runtime: &mut self.runtime, native: self.native.as_mut(), services: &mut services }.dispatch(action)
    }
    pub fn mount(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.with_services(|runtime, services| runtime.mount(resolved, host, services))
    }
    pub fn content_action(&mut self, session: &Session, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
        self.refresh_context(session)?;
        self.with_services(|runtime, services| runtime.content_action(instance, action, services))
    }
    pub fn tick(&mut self, session: &Session) -> PanelResult<()> {
        let refresh = self.services.refresh_read_only(session);
        let mut environment = self.runtime.environment().clone();
        environment.document = Some(DocumentId::new(session.document_id.clone())?);
        let mut services = self.services.services();
        self.runtime.set_environment(environment, services.local)?;
        let update = self.runtime.update(&mut services);
        refresh.and(update)
    }
    /// Explicit teardown preflights all owners before any controller is destroyed.
    pub fn shutdown(&mut self) -> PanelResult<()> {
        if self.runtime.layout().layout().sections.iter().any(|section| matches!(&section.location, SectionLocation::Window { .. })) {
            return Err(error(Rule::Lifecycle, "redock native panels before disabling their application adapter"));
        }
        let services = self.services.services();
        self.runtime.shutdown(services.local)
    }
}

impl crate::SolveApp {
    pub fn run_panel_command(&mut self, id: &str, params: &serde_json::Value) -> PanelResult<serde_json::Value> {
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
    /// Internal opt-in only: no preference, control method, command or default change.
    pub fn enable_panel_runtime(&mut self, runtime: PanelRuntime, services: Box<dyn AppPanelServices>) -> PanelResult<()> {
        if self.panel_runtime.is_some() {
            return Err(error(Rule::Lifecycle, "panel runtime already enabled"));
        }
        let mut integration = AppPanelRuntime::new(runtime, services);
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
    pub fn dispatch_panel_layout(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.dispatch(&self.session, action)
    }
    pub fn mount_panels(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.mount(resolved, host)
    }
    pub fn panel_content_action(&mut self, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.content_action(&self.session, instance, action)
    }
    pub fn tick_panel_runtime(&mut self) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.tick(&self.session)
    }
    pub fn disable_panel_runtime(&mut self) -> PanelResult<()> {
        if let Some(integration) = &mut self.panel_runtime {
            integration.shutdown()?;
        }
        self.panel_runtime = None;
        Ok(())
    }
}

/// No platform window is claimed without an injected native implementation.
struct UnavailableNative;
impl NativeHostAdapter for UnavailableNative {
    fn capabilities(&self) -> HostCapabilities {
        HostCapabilities::default()
    }
    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        if effects
            .iter()
            .any(|effect| matches!(effect, HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. } | HostEffect::ReleaseNative { .. }))
        {
            return Err(error(Rule::Capability, "native adapter not injected"));
        }
        Ok(())
    }
    fn commit(&mut self, _effects: &[HostEffect]) {}
}

pub(crate) fn tick_app(app: &mut crate::SolveApp) {
    if let Some(integration) = &mut app.panel_runtime {
        // Errors remain inspectable without inventing untranslated production UI.
        integration.last_error = integration.tick(&app.session).err();
    }
}
