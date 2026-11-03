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
    pub runtime: PanelRuntime,
    services: Box<dyn AppPanelServices>,
    last_error: Option<PanelError>,
}
impl AppPanelRuntime {
    pub fn new(runtime: PanelRuntime, services: Box<dyn AppPanelServices>) -> Self {
        Self { runtime, services, last_error: None }
    }
    pub fn last_error(&self) -> Option<&PanelError> {
        self.last_error.as_ref()
    }
    /// All forwarding borrows the exact provider injected at enablement.
    pub fn with_services<R>(&mut self, use_services: impl FnOnce(&mut PanelRuntime, &mut ContentServices<'_>) -> PanelResult<R>) -> PanelResult<R> {
        let mut services = self.services.services();
        use_services(&mut self.runtime, &mut services)
    }
    pub fn dispatch(&mut self, action: LayoutAction, native: &mut dyn NativeHostAdapter) -> PanelResult<LayoutReply> {
        self.with_services(|runtime, services| runtime.dispatch(action, native, services))
    }
    pub fn mount(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.with_services(|runtime, services| runtime.mount(resolved, host, services))
    }
    pub fn content_action(&mut self, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
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
        let services = self.services.services();
        self.runtime.shutdown(services.local)
    }
}

impl crate::SolveApp {
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
    pub fn dispatch_panel_layout(&mut self, action: LayoutAction, native: &mut dyn NativeHostAdapter) -> PanelResult<LayoutReply> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.dispatch(action, native)
    }
    pub fn mount_panels(&mut self, resolved: &ResolvedLayout, host: &mut dyn PanelHost) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.mount(resolved, host)
    }
    pub fn panel_content_action(&mut self, instance: &InstanceId, action: &ControlId) -> PanelResult<()> {
        self.panel_runtime.as_mut().ok_or_else(|| error(Rule::Lifecycle, "panel runtime disabled"))?.content_action(instance, action)
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

pub(crate) fn tick_app(app: &mut crate::SolveApp) {
    if let Some(integration) = &mut app.panel_runtime {
        // Errors remain inspectable without inventing untranslated production UI.
        integration.last_error = integration.tick(&app.session).err();
    }
}
