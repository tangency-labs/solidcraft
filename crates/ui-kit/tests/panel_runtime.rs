use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use solidcraft_panel_layout::*;
use solidcraft_ui_kit::panel_runtime::{
    ContentCommandSink, ContentOwner, ContentServices, ContentSlot, ControllerBinding, HostControlTarget, HostDropTargetId, HostFrame, HostOptions,
    HostPointerTarget, HostResizeEdge, HostSharedAction, LifecycleEvent, LocalRequest, LocalScope, Motion, NativeHostAdapter, Ownership,
    PanelContext, PanelController, PanelLayoutStore, PanelLocalServices, PanelModels, PanelPresentation, PanelRuntime, RuntimeEnvironment,
    WorkspacePanelHost, egui_rect, normalize_native_reply,
};
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_ui_kit::panel_runtime::{DesktopNativeAdapter, NativeMonitor};
use solidcraft_ui_kit::type_face::install_weight_families;

#[derive(Default)]
struct StubModels;

impl PanelModels for StubModels {
    fn read(&self, _owner: &ContentOwner, _key: &str) -> PanelResult<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }
}

#[derive(Default)]
struct StubCommands;

impl ContentCommandSink for StubCommands {
    fn execute(&mut self, _owner: &ContentOwner, _command: &str, _input: serde_json::Value) -> PanelResult<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }
}

#[derive(Default)]
struct StubLocalServices {
    scopes: BTreeMap<InstanceId, LocalScope>,
}

impl PanelLocalServices for StubLocalServices {
    fn request(&mut self, scope: &LocalScope, request: LocalRequest) -> PanelResult<()> {
        scope.validate()?;
        request.validate()?;
        if self.scopes.get(request.instance()) != Some(scope) {
            return Err(error(Rule::Placement, "request scope mismatch"));
        }
        Ok(())
    }

    fn release(&mut self, _scope: &LocalScope, instance: &InstanceId) {
        self.scopes.remove(instance);
    }
}

struct StubNativeAdapter {
    capabilities: HostCapabilities,
    hosts: BTreeMap<HostId, SectionId>,
}

impl StubNativeAdapter {
    fn new(native_windows: bool) -> Self {
        Self {
            capabilities: HostCapabilities { native_windows, persistence: true, max_native_hosts: if native_windows { 8 } else { 0 } },
            hosts: BTreeMap::new(),
        }
    }
}

impl NativeHostAdapter for StubNativeAdapter {
    fn capabilities(&self) -> HostCapabilities {
        self.capabilities.clone()
    }

    fn prepare(&self, effects: &[HostEffect]) -> PanelResult<()> {
        for effect in effects {
            if let HostEffect::CreateNative { .. } | HostEffect::RebindNative { .. } = effect
                && !self.capabilities.native_windows
            {
                return Err(error(Rule::Capability, "native windows unsupported"));
            }
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
    }
}

#[derive(Clone, Default)]
struct MemoryLayoutStore {
    saved_json: Rc<RefCell<Option<String>>>,
}

impl MemoryLayoutStore {
    fn save_layout(&self, layout: &Layout, registry: &PanelRegistry) -> PanelResult<()> {
        let json = serde_json::to_string(layout).map_err(|_| error(Rule::Schema, "failed to serialize layout"))?;
        let _: Layout = serde_json::from_str(&json).map_err(|_| error(Rule::Schema, "invalid serialized layout"))?;
        let _ = registry;
        *self.saved_json.borrow_mut() = Some(json);
        Ok(())
    }

    fn load_layout(&self) -> PanelResult<Layout> {
        let guard = self.saved_json.borrow();
        let json = guard.as_deref().ok_or_else(|| error(Rule::MissingTarget, "no saved layout"))?;
        serde_json::from_str(json).map_err(|_| error(Rule::Schema, "failed to deserialize layout"))
    }
}

impl PanelLayoutStore for MemoryLayoutStore {
    fn execute_action(&mut self, action: LayoutAction, sink: &mut dyn ActionSink, registry: &PanelRegistry) -> PanelResult<LayoutReply> {
        let state = sink.inspect();
        let envelope = self.current_envelope(&state, registry)?;
        let preparation = preparation_context(&envelope)?;
        let reply = sink.dispatch_in_context(action, &preparation)?;
        self.save_layout(&reply.layout, registry)?;
        Ok(reply)
    }
}

#[derive(Clone, Default)]
struct ControllerCounts {
    mount_count: usize,
    paint_count: usize,
    was_presented: bool,
}

#[derive(Clone, Default)]
struct StubProbe {
    counts: Rc<RefCell<BTreeMap<InstanceId, ControllerCounts>>>,
}

impl StubProbe {
    fn counts(&self, instance: &InstanceId) -> ControllerCounts {
        self.counts.borrow().get(instance).cloned().unwrap_or_default()
    }
}

struct StubController {
    instance: InstanceId,
    title: String,
    probe: StubProbe,
}

impl PanelController for StubController {
    fn availability(&self, _owner: &ContentOwner, _models: &dyn PanelModels) -> Availability {
        Availability::Available
    }

    fn update(&mut self, _owner: &ContentOwner, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Ok(())
    }

    fn presentation(&self) -> PanelPresentation {
        PanelPresentation { title: self.title.clone(), icon: "list".into(), status: None, attention: false, actions: Vec::new() }
    }

    fn prepare_lifecycle(&self, _event: &LifecycleEvent) -> PanelResult<()> {
        Ok(())
    }

    fn lifecycle(&mut self, event: &LifecycleEvent) {
        if let LifecycleEvent::State(state) = event {
            let mut map = self.probe.counts.borrow_mut();
            let entry = map.entry(self.instance.clone()).or_default();
            if state.presented && !entry.was_presented {
                entry.mount_count += 1;
            }
            entry.was_presented = state.presented;
        }
    }

    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, _context: &PanelContext, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        if slot == ContentSlot::Body {
            ui.label(&self.title);
            let mut map = self.probe.counts.borrow_mut();
            map.entry(self.instance.clone()).or_default().paint_count += 1;
        }
        Ok(())
    }

    fn action(&mut self, _owner: &ContentOwner, _action: &ControlId, _services: &mut ContentServices<'_>) -> PanelResult<()> {
        Ok(())
    }
}

struct TestHost {
    ctx: egui::Context,
    runtime: PanelRuntime,
    host: WorkspacePanelHost,
    models: StubModels,
    commands: StubCommands,
    local: StubLocalServices,
    native: StubNativeAdapter,
    store: MemoryLayoutStore,
    capacity: LogicalRect,
    time: f64,
}

impl TestHost {
    fn new(native_windows: bool, initial_layout: Option<Layout>, probe: StubProbe) -> Self {
        let ctx = egui::Context::default();
        install_weight_families(&ctx);

        let workspace = WorkspaceId::new("ext.workspace").unwrap();
        let mut registry = PanelRegistry::default();
        let spec = PanelSpec {
            kind: PanelKind::new("ext.panel").unwrap(),
            title_key: "ext.panel.title".into(),
            icon: "list".into(),
            allowed_zones: [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating, Zone::Window].into_iter().collect(),
            default_zone: Zone::Right,
            min_size: Size { width: 200.0, height: 140.0 },
            default_size: Size { width: 280.0, height: 240.0 },
            max_size: Size { width: 640.0, height: 720.0 },
            edge_rule: EdgeRule::None,
            surface: PanelSurface::FollowZone,
            features: PanelFeatures { closable: true, collapsible: true, popout: true, multi_instance: true, ..PanelFeatures::default() },
            side_height_policy: SideHeightPolicy::Content,
            fixed_bottom_height: None,
            header_actions: Vec::new(),
            text_heavy: false,
        };
        registry.register(spec).unwrap();

        let capabilities = HostCapabilities { native_windows, persistence: true, max_native_hosts: if native_windows { 8 } else { 0 } };
        let layout = initial_layout.unwrap_or_else(|| Layout::empty(workspace.clone()));
        let service = LayoutService::new(registry, layout, capabilities).unwrap();
        let environment = RuntimeEnvironment {
            workspace,
            document: Some(DocumentId::new("ext.doc").unwrap()),
            window: HostId::new("ext.window").unwrap(),
            viewport: ViewportId::new("ext.viewport").unwrap(),
            direction: Direction::Ltr,
            motion: Motion::Reduced,
        };
        let mut runtime = PanelRuntime::new(service, environment).unwrap();
        runtime
            .register(
                PanelKind::new("ext.panel").unwrap(),
                ControllerBinding {
                    ownership: Ownership::Workspace,
                    create: Box::new(move |instance, _owner| {
                        Ok(Box::new(StubController {
                            instance: instance.id.clone(),
                            title: format!("Panel {}", instance.id.0),
                            probe: probe.clone(),
                        }))
                    }),
                },
            )
            .unwrap();

        Self {
            ctx,
            runtime,
            host: WorkspacePanelHost::new(HostOptions::default()).unwrap(),
            models: StubModels,
            commands: StubCommands,
            local: StubLocalServices::default(),
            native: StubNativeAdapter::new(native_windows),
            store: MemoryLayoutStore::default(),
            capacity: LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1200.0, height: 800.0 } },
            time: 0.0,
        }
    }

    fn dispatch(&mut self, mutations: Vec<Mutation>) -> PanelResult<LayoutReply> {
        let revision = self.runtime.layout().inspect().revision;
        let mut services = ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local };
        self.runtime.dispatch(LayoutAction::Apply { expected_revision: Some(revision), mutations }, &mut self.native, &mut services)
    }

    fn frame(&mut self, events: Vec<egui::Event>) -> HostFrame {
        let mut services = ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local };
        self.runtime.update(&mut services).unwrap();
        self.host.capture_metrics(&self.runtime, &self.models).unwrap();
        self.host.capture_reservations(&self.runtime, &self.store).unwrap();
        self.host.prepare(&self.runtime, self.capacity).unwrap();
        self.time += 1.0 / 60.0;
        let input =
            egui::RawInput { screen_rect: Some(egui_rect(self.capacity)), events, time: Some(self.time), focused: true, ..Default::default() };
        let mut output = self.ctx.run_ui(input, |ui| {
            let mut services = ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local };
            self.host.show(ui, &mut self.runtime, &mut self.native, &mut services, &mut self.store).unwrap();
        });
        output.textures_delta.clear();
        self.host.frame().unwrap().clone()
    }

    fn control_center(&self, target: &HostControlTarget) -> egui::Pos2 {
        let bounds = self
            .host
            .frame()
            .unwrap()
            .controls
            .iter()
            .find(|control| &control.target == target)
            .unwrap_or_else(|| panic!("control missing: {target:?}"))
            .bounds;
        egui_rect(bounds).center()
    }

    fn click(&mut self, target: HostControlTarget) -> HostFrame {
        let pos = self.control_center(&target);
        self.frame(vec![egui::Event::PointerMoved(pos)]);
        self.frame(vec![egui::Event::PointerButton { pos, pressed: true, button: egui::PointerButton::Primary, modifiers: egui::Modifiers::NONE }]);
        self.frame(vec![egui::Event::PointerButton { pos, pressed: false, button: egui::PointerButton::Primary, modifiers: egui::Modifiers::NONE }]);
        self.frame(Vec::new())
    }

    fn open_menu(&mut self, instance: InstanceId) -> HostFrame {
        self.click(HostControlTarget::Pointer(HostPointerTarget::Menu { instance }));
        self.frame(Vec::new());
        let frame = self.frame(Vec::new());
        assert!(frame.menu_bounds.is_some(), "must open the shared panel actions menu");
        frame
    }
}

fn id(name: &str) -> InstanceId {
    InstanceId::new(name).unwrap()
}

fn sec(name: &str) -> SectionId {
    SectionId::new(name).unwrap()
}

fn open_in_zone(instance: &str, section_name: &str, zone: Zone, index: usize) -> Mutation {
    let location = match zone {
        Zone::Left | Zone::Right | Zone::Bottom => SectionLocation::Dock { zone },
        Zone::Floating => SectionLocation::Floating { origin: Point { x: 120.0, y: 120.0 } },
        Zone::Window => SectionLocation::Window {
            host: HostId::new(format!("host-{instance}")).unwrap(),
            origin: Point { x: 80.0, y: 80.0 },
            return_to: DockReturn {
                zone: Zone::Right,
                index: 0,
                size: Size { width: 280.0, height: 240.0 },
                collapsed: false,
                section: None,
                column: None,
                column_index: 0,
                column_width: None,
                weight: 1.0,
                preferred_extent: None,
                extent_priority: 0,
                join: None,
            },
        },
    };
    Mutation::Open {
        kind: PanelKind::new("ext.panel").unwrap(),
        instance: id(instance),
        destination: Some(Destination::NewSection { section: sec(section_name), index, location, size: Size { width: 280.0, height: 240.0 } }),
    }
}

#[test]
fn floating_panel_places_resizes_and_clamps_within_viewport() {
    let probe = StubProbe::default();
    let mut host = TestHost::new(false, None, probe);
    host.dispatch(vec![open_in_zone("float-a", "sec-float", Zone::Floating, 0)]).unwrap();
    let frame = host.frame(Vec::new());
    let card = frame.layout.sections.iter().find(|s| s.instance == id("float-a")).unwrap();
    assert!(host.capacity.contains_rect(card.outer), "initial floating rect {:?} must clamp inside {:?}", card.outer, host.capacity);

    // Oversized SetFloatingRect near bottom-right corner must clamp inside viewport.
    host.dispatch(vec![Mutation::SetFloatingRect {
        section: sec("sec-float"),
        origin: Point { x: 1050.0, y: 700.0 },
        size: Size { width: 600.0, height: 600.0 },
    }])
    .unwrap();
    let frame = host.frame(Vec::new());
    let card = frame.layout.sections.iter().find(|s| s.instance == id("float-a")).unwrap();
    assert!(host.capacity.contains_rect(card.outer), "clamped rect {:?} exceeded viewport {:?}", card.outer, host.capacity);
    let before_size = card.outer.size;

    // Resize inward via BottomRight handle and verify clamped updated rect.
    let handle =
        host.control_center(&HostControlTarget::Pointer(HostPointerTarget::Resize { section: sec("sec-float"), edge: HostResizeEdge::BottomRight }));
    let end = handle - egui::vec2(50.0, 40.0);
    host.frame(vec![egui::Event::PointerMoved(handle)]);
    host.frame(vec![egui::Event::PointerButton {
        pos: handle,
        pressed: true,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }]);
    host.frame(vec![egui::Event::PointerMoved(end)]);
    host.frame(vec![egui::Event::PointerButton { pos: end, pressed: false, button: egui::PointerButton::Primary, modifiers: egui::Modifiers::NONE }]);
    let frame = host.frame(Vec::new());
    let resized = frame.layout.sections.iter().find(|s| s.instance == id("float-a")).unwrap();
    assert!(host.capacity.contains_rect(resized.outer));
    assert!(resized.outer.size.width < before_size.width);
    assert!(resized.outer.size.height < before_size.height);
}

#[test]
fn floating_panel_drags_to_right_dock_and_undocks_back_to_floating() {
    let probe = StubProbe::default();
    let mut host = TestHost::new(false, None, probe);
    host.dispatch(vec![open_in_zone("drag-a", "sec-drag", Zone::Floating, 0)]).unwrap();
    host.frame(Vec::new());
    host.frame(Vec::new());

    // Drag floating header to right dock band.
    let start = host.control_center(&HostControlTarget::Pointer(HostPointerTarget::Header { instance: id("drag-a") }));
    let right_band = egui::pos2(host.capacity.size.width - 8.0, 320.0);
    host.frame(vec![egui::Event::PointerMoved(start)]);
    host.frame(vec![egui::Event::PointerButton {
        pos: start,
        pressed: true,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }]);
    let preview_frame = host.frame(vec![egui::Event::PointerMoved(right_band)]);
    let preview = preview_frame.preview.as_ref().expect("right dock drag must show a drop preview");
    assert_eq!(preview.target, Some(HostDropTargetId::Dock { zone: Zone::Right, before: None }));
    host.frame(vec![egui::Event::PointerButton {
        pos: right_band,
        pressed: false,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }]);
    host.frame(Vec::new());
    assert_eq!(host.runtime.layout().layout().containing(&id("drag-a")).unwrap().location, SectionLocation::Dock { zone: Zone::Right });

    // Undock via the panel actions menu back to Floating.
    host.open_menu(id("drag-a"));
    host.click(HostControlTarget::Shared { instance: id("drag-a"), action: HostSharedAction::Move(Zone::Floating) });
    assert_eq!(host.runtime.layout().layout().containing(&id("drag-a")).unwrap().location.zone(), Zone::Floating);
}

#[test]
fn panels_stack_on_same_side_and_switch_tabs() {
    let probe = StubProbe::default();
    let mut host = TestHost::new(false, None, probe.clone());
    host.dispatch(vec![
        open_in_zone("panel-a", "sec-a", Zone::Right, 0),
        Mutation::Open {
            kind: PanelKind::new("ext.panel").unwrap(),
            instance: id("panel-b"),
            destination: Some(Destination::Relative {
                section: sec("sec-a"),
                placement: RelativePlacement::Below,
                new_section: sec("sec-b"),
                new_column: None,
                size: Size { width: 280.0, height: 240.0 },
            }),
        },
        Mutation::Open {
            kind: PanelKind::new("ext.panel").unwrap(),
            instance: id("panel-c"),
            destination: Some(Destination::JoinTab { section: sec("sec-b"), index: 1 }),
        },
        Mutation::Activate { instance: id("panel-b") },
    ])
    .unwrap();
    let frame = host.frame(Vec::new());
    let card_a = frame.layout.sections.iter().find(|s| s.section == sec("sec-a")).unwrap();
    let card_b = frame.layout.sections.iter().find(|s| s.section == sec("sec-b")).unwrap();
    assert!(
        card_a.outer.origin.y + card_a.outer.size.height <= card_b.outer.origin.y + 0.5,
        "stacked sections must not overlap vertically: a={:?}, b={:?}",
        card_a.outer,
        card_b.outer
    );

    let before_c = probe.counts(&id("panel-c"));
    assert_eq!(host.runtime.layout().layout().section(&sec("sec-b")).unwrap().active, id("panel-b"));

    // Click inactive tab for panel-c.
    host.click(HostControlTarget::Pointer(HostPointerTarget::Tab { instance: id("panel-c") }));
    assert_eq!(host.runtime.layout().layout().section(&sec("sec-b")).unwrap().active, id("panel-c"));
    let after_c = probe.counts(&id("panel-c"));
    assert!(after_c.mount_count > before_c.mount_count, "switching tab must mount newly active panel");
    assert!(after_c.paint_count > before_c.paint_count, "switching tab must paint newly active panel");
}

#[test]
fn native_popout_and_headless_capability_handling() {
    let probe = StubProbe::default();
    let mut host = TestHost::new(false, None, probe);
    host.dispatch(vec![open_in_zone("head-a", "sec-head", Zone::Right, 0)]).unwrap();
    let frame = host.frame(Vec::new());

    // Verify normalize_native_reply clamps native capability flags on headless hosts.
    let mut inflated = frame.reply.clone();
    inflated.capabilities = HostCapabilities { native_windows: true, persistence: true, max_native_hosts: 4 };
    let normalized = normalize_native_reply(inflated, &HostCapabilities { native_windows: false, persistence: true, max_native_hosts: 0 });
    assert!(!normalized.capabilities.native_windows);
    assert_eq!(normalized.capabilities.max_native_hosts, 0);

    // Open panel actions menu and assert Popout is omitted when native_windows is false.
    let menu_frame = host.open_menu(id("head-a"));
    assert!(
        menu_frame.controls.iter().all(|c| c.target != HostControlTarget::Shared { instance: id("head-a"), action: HostSharedAction::Popout }),
        "headless host must omit Popout action from panel menu"
    );

    // Requesting a Window popout directly must fail with Rule::Capability.
    let err = host
        .dispatch(vec![Mutation::Move {
            instance: id("head-a"),
            destination: Destination::NewSection {
                section: sec("sec-win"),
                index: 0,
                location: SectionLocation::Window {
                    host: HostId::new("popout-host").unwrap(),
                    origin: Point { x: 80.0, y: 80.0 },
                    return_to: DockReturn {
                        zone: Zone::Right,
                        index: 0,
                        size: Size { width: 320.0, height: 280.0 },
                        collapsed: false,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 1.0,
                        preferred_extent: None,
                        extent_priority: 0,
                        join: None,
                    },
                },
                size: Size { width: 320.0, height: 280.0 },
            },
        }])
        .unwrap_err();
    assert_eq!(err.rule, Rule::Capability);
    assert!(host.native.hosts.is_empty());

    // On desktop, exercise DesktopNativeAdapter in-memory popout, monitor clamping, and redock lifecycle.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut desktop = DesktopNativeAdapter::default();
        let monitor = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 720.0));

        let pop_host = HostId::new("desktop-pop").unwrap();
        let pop_section = sec("desktop-sec");
        let pop_instance = id("desktop-panel");
        let create = [HostEffect::CreateNative { host: pop_host.clone(), section: pop_section.clone() }];
        desktop.prepare(&create).unwrap();
        desktop.commit(&create);
        desktop.update_monitors(|| vec![NativeMonitor { name: Some("Primary".into()), bounds: monitor, scale: 1.0, primary: true }]);
        assert!(desktop.owns(&pop_host, &pop_section));
        assert!(desktop.viewport_id(&pop_host).is_some());

        let min = Size { width: 200.0, height: 140.0 };
        let max = Size { width: 640.0, height: 720.0 };
        let ctx = egui::Context::default();
        let landing = desktop.landing(&ctx, &pop_instance, egui::pos2(1150.0, 620.0), Size { width: 320.0, height: 280.0 }, (min, max)).unwrap();
        assert!(monitor.contains_rect(landing), "landing rect {landing:?} must clamp inside monitor {monitor:?}");

        let builder =
            desktop.builder(&ctx, &pop_host, &pop_instance, "Desktop Panel".into(), Size { width: 320.0, height: 280.0 }, (min, max)).unwrap();
        assert!(builder.inner_size.is_some());

        // Redock releases the native host registration cleanly.
        let release = [HostEffect::ReleaseNative { host: pop_host.clone() }];
        desktop.prepare(&release).unwrap();
        desktop.commit(&release);
        assert!(desktop.viewport_id(&pop_host).is_none());
    }
}

#[test]
fn layout_save_and_restore_via_panel_layout_store() {
    let probe = StubProbe::default();
    let mut host = TestHost::new(false, None, probe);
    let custom_float_origin = Point { x: 180.0, y: 140.0 };
    let custom_float_size = Size { width: 320.0, height: 260.0 };

    host.dispatch(vec![
        open_in_zone("panel-a", "sec-a", Zone::Right, 0),
        Mutation::Open {
            kind: PanelKind::new("ext.panel").unwrap(),
            instance: id("panel-b"),
            destination: Some(Destination::Relative {
                section: sec("sec-a"),
                placement: RelativePlacement::Below,
                new_section: sec("sec-b"),
                new_column: None,
                size: Size { width: 280.0, height: 240.0 },
            }),
        },
        Mutation::Open {
            kind: PanelKind::new("ext.panel").unwrap(),
            instance: id("panel-c"),
            destination: Some(Destination::JoinTab { section: sec("sec-b"), index: 1 }),
        },
        Mutation::Activate { instance: id("panel-c") },
        Mutation::Open {
            kind: PanelKind::new("ext.panel").unwrap(),
            instance: id("panel-d"),
            destination: Some(Destination::NewSection {
                section: sec("sec-d"),
                index: 2,
                location: SectionLocation::Floating { origin: custom_float_origin },
                size: custom_float_size,
            }),
        },
    ])
    .unwrap();
    let saved_frame = host.frame(Vec::new());
    let saved_layout = host.runtime.layout().layout().clone();
    host.store.save_layout(&saved_layout, host.runtime.layout().registry()).unwrap();

    // Deserialize into a fresh runtime and host, run a headless frame, and compare.
    let restored_layout = host.store.load_layout().unwrap();
    let mut restored_host = TestHost::new(false, Some(restored_layout), StubProbe::default());
    let restored_frame = restored_host.frame(Vec::new());

    let restored = restored_host.runtime.layout().layout();
    assert_eq!(restored.section(&sec("sec-a")).unwrap().location, SectionLocation::Dock { zone: Zone::Right });
    assert_eq!(restored.section(&sec("sec-b")).unwrap().location, SectionLocation::Dock { zone: Zone::Right });
    assert_eq!(restored.section(&sec("sec-b")).unwrap().tabs, vec![id("panel-b"), id("panel-c")]);
    assert_eq!(restored.section(&sec("sec-b")).unwrap().active, id("panel-c"));
    assert_eq!(restored.section(&sec("sec-d")).unwrap().location, SectionLocation::Floating { origin: custom_float_origin });
    assert_eq!(restored.section(&sec("sec-d")).unwrap().size, custom_float_size);
    for name in ["panel-a", "panel-b", "panel-c", "panel-d"] {
        assert!(restored.instance(&id(name)).unwrap().requested_open);
    }
    for s in ["sec-a", "sec-b", "sec-d"] {
        let old_card = saved_frame.layout.sections.iter().find(|c| c.section == sec(s)).unwrap();
        let new_card = restored_frame.layout.sections.iter().find(|c| c.section == sec(s)).unwrap();
        assert_eq!(new_card.outer, old_card.outer, "restored rect mismatch for {s}");
    }
}
