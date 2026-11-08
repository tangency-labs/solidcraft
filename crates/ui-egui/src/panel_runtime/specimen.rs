//! Bounded, explicitly enabled synthetic content in the real SolveApp host.
//! This is an evidence seam, never a production registration or saved preference.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::panels::*;

use super::*;

const WORKSPACE: &str = "panel_fixture.workspace";
const INSPECT: &str = "panel_fixture.inspect";
const PRESETS: &[&str] =
    &["docks", "tabs", "collapsed", "floating", "overflow", "pinned", "columns", "stack_tabs", "content_fill", "inline", "surfaces"];
type SpecimenIdentities = Rc<RefCell<BTreeMap<ControllerKey, Rc<()>>>>;

fn preset(value: &str) -> PanelResult<()> {
    if PRESETS.contains(&value) { Ok(()) } else { Err(error(Rule::Schema, "unknown synthetic panel preset")) }
}

/// The caller explicitly opts into original synthetic content at the actual
/// workspace composition point. An existing runtime is never replaced.
pub fn enable_host_specimen(app: &mut crate::SolveApp, name: &str) -> PanelResult<()> {
    enable_host_specimen_remembered(app, name, false, None)
}

fn enable_host_specimen_remembered(app: &mut crate::SolveApp, name: &str, remember: bool, requested_opacity: Option<f32>) -> PanelResult<()> {
    preset(name)?;
    if app.panel_runtime().is_some() {
        return Err(error(Rule::Lifecycle, "panel specimen cannot take over an existing runtime"));
    }
    let (layout, registry) = specimen_layout(name)?;
    let service = LayoutService::new(registry, layout, HostCapabilities { persistence: remember, ..Default::default() })?;
    let mut runtime = PanelRuntime::new(
        service,
        RuntimeEnvironment {
            workspace: WorkspaceId::new(WORKSPACE)?,
            document: Some(DocumentId::new(app.session.document_id.clone())?),
            window: HostId::new("panel_fixture.window")?,
            viewport: ViewportId::new("panel_fixture.viewport")?,
            direction: crate::i18n::panel_direction(&app.preferences.language),
            motion: Motion::Animated,
        },
    )?;
    let probe = FixtureProbe::default();
    let identities = SpecimenIdentities::default();
    for (kind, editor, ownership) in [
        ("fixture.editor", true, Ownership::Document),
        ("fixture.log", false, Ownership::Workspace),
        ("fixture.fill", false, Ownership::Workspace),
        ("fixture.pin", false, Ownership::Workspace),
    ] {
        let binding = fixture_controller_binding(&probe, editor, ownership);
        let create = binding.create;
        let visible_probe = probe.clone();
        let visible_identities = identities.clone();
        runtime.register(
            PanelKind::new(kind)?,
            ControllerBinding {
                ownership,
                create: Box::new(move |instance, owner| {
                    let inner = create(instance, owner)?;
                    Ok(Box::new(VisibleSpecimen {
                        inner,
                        probe: visible_probe.clone(),
                        identities: visible_identities.clone(),
                        identity: Rc::new(()),
                        key: ControllerKey { instance: instance.id.clone(), owner: owner.clone() },
                        editor,
                        title_key: if editor {
                            "ui.panel_fixture.editor"
                        } else if instance.kind.0 == "fixture.fill" {
                            "ui.panel_fixture.fill"
                        } else if instance.kind.0 == "fixture.pin" {
                            "ui.panel_fixture.pin"
                        } else {
                            "ui.panel_fixture.log"
                        },
                    }))
                }),
            },
        )?;
    }
    let calls = Rc::new(Cell::new(0));
    let services = SpecimenServices {
        models: SpecimenModels { name: name.into(), probe, identities, calls: calls.clone() },
        commands: SpecimenCommands { calls },
        local: FakeLocalServices::default(),
        workspace: WorkspaceId::new(WORKSPACE)?,
    };
    // Stage the complete integration before publishing it. Factories, the first
    // service tick and host option validation may all fail without taking over
    // the app or leaving a partially enabled specimen behind.
    let mut integration = AppPanelRuntime::new(runtime, Box::new(services))?;
    integration.tick(&app.session)?;
    let mut options = HostOptions::default();
    options.geometry.default_opaque = name != "surfaces";
    integration.enable_host(options)?;
    integration.set_requested_opacity(requested_opacity)?;
    let repaired = if remember {
        let mut catalogue = specimen_catalogue(name)?;
        super::preferences::configure_preferences(&mut integration, &app.panel_layout_preferences, &mut catalogue)?
    } else {
        false
    };
    app.panel_runtime = Some(integration);
    if repaired {
        super::preferences::report_notice(app);
    }
    Ok(())
}

fn specimen_catalogue(current: &str) -> PanelResult<commands::InMemoryCatalogue> {
    let mut catalogue = commands::InMemoryCatalogue::default();
    for (name, preset, suffix, title) in [
        ("Classic", current, "", "classic"),
        ("Modelling", "docks", ".modelling", "modelling"),
        ("Sketch", "tabs", ".sketch", "sketch"),
        ("Render", "columns", ".render", "render"),
        ("Review", "floating", ".review", "review"),
    ] {
        let (mut layout, registry) = specimen_layout(preset)?;
        layout.workspace = WorkspaceId::new(format!("{WORKSPACE}{suffix}"))?;
        let workspace = layout.workspace.clone();
        catalogue.register_builtin(name, &LayoutEnvelope::new(layout), &registry)?;
        catalogue.register_workspace(commands::WorkspaceDescriptor {
            id: workspace,
            title_key: format!("ui.panel_layout.workspace.{title}"),
            default_preset: name.into(),
        })?;
    }
    Ok(catalogue)
}

fn specimen_workspace(workspace: &WorkspaceId) -> bool {
    ["", ".modelling", ".sketch", ".render", ".review"].iter().any(|suffix| workspace.0 == format!("{WORKSPACE}{suffix}"))
}

fn specimen_layout(name: &str) -> PanelResult<(Layout, PanelRegistry)> {
    let mut registry = PanelRegistry::default();
    for (kind, title, pinned, fill, editor) in [
        ("fixture.editor", "ui.panel_fixture.editor", false, false, true),
        ("fixture.log", "ui.panel_fixture.log", false, false, false),
        ("fixture.fill", "ui.panel_fixture.fill", false, true, false),
        ("fixture.pin", "ui.panel_fixture.pin", true, false, false),
    ] {
        let mut allowed_zones = [Zone::Left, Zone::Right, Zone::Bottom, Zone::Floating].into_iter().collect::<std::collections::BTreeSet<_>>();
        if pinned {
            allowed_zones.remove(&Zone::Bottom);
        }
        registry.register(PanelSpec {
            kind: PanelKind::new(kind)?,
            title_key: title.into(),
            icon: "list".into(),
            allowed_zones,
            default_zone: Zone::Left,
            min_size: Size { width: 240.0, height: if editor { 120.0 } else { 40.0 } },
            default_size: Size { width: 280.0, height: 240.0 },
            max_size: Size { width: MAX_LOGICAL, height: 1200.0 },
            edge_rule: if pinned { EdgeRule::PinOuter } else { EdgeRule::None },
            surface: if name == "surfaces" && fill {
                PanelSurface::Opaque
            } else if name == "surfaces" && pinned {
                PanelSurface::Translucent
            } else {
                PanelSurface::FollowZone
            },
            features: PanelFeatures { multi_instance: !editor, popout: false, hot_closable: editor, ..PanelFeatures::default() },
            side_height_policy: if fill { SideHeightPolicy::Fill } else { SideHeightPolicy::Content },
            fixed_bottom_height: None,
            header_actions: if editor {
                vec![HeaderActionSpec { id: ControlId::new("apply")?, label_key: "ui.panel_fixture.apply".into(), icon: "list".into() }]
            } else {
                Vec::new()
            },
            text_heavy: editor,
        })?;
    }
    let mut layout = Layout::empty(WorkspaceId::new(WORKSPACE)?);
    let count = if name == "surfaces" {
        6
    } else if name == "overflow" {
        24
    } else if name == "columns" {
        3
    } else if name == "content_fill" {
        2
    } else if name == "inline" {
        1
    } else {
        4
    };
    for index in 0..count {
        let id = InstanceId::new(format!("specimen-{index}"))?;
        let kind = if name == "inline" {
            "fixture.log"
        } else if (name == "content_fill" && index == 1) || (name == "surfaces" && index == 3) {
            "fixture.fill"
        } else if (name == "pinned" && index == 1) || (name == "surfaces" && index == 2) {
            "fixture.pin"
        } else if index == 0 {
            "fixture.editor"
        } else {
            "fixture.log"
        };
        let zone = if name == "surfaces" {
            match index {
                0 | 1 => Zone::Left,
                2 | 3 => Zone::Right,
                4 => Zone::Bottom,
                _ => Zone::Floating,
            }
        } else if name == "inline" {
            Zone::Bottom
        } else if matches!(name, "columns" | "stack_tabs") {
            Zone::Left
        } else if name == "content_fill" {
            if index == 0 { Zone::Left } else { Zone::Right }
        } else {
            match index % 3 {
                0 => Zone::Left,
                1 => Zone::Right,
                _ => Zone::Bottom,
            }
        };
        let location = if name == "floating" || (name == "surfaces" && index == 5) {
            SectionLocation::Floating { origin: Point { x: 48.0 + index as f32 * 64.0, y: 48.0 + index as f32 * 48.0 } }
        } else {
            SectionLocation::Dock { zone }
        };
        let group = if name == "surfaces" && index == 1 {
            0
        } else if name == "tabs" {
            index % 2
        } else if name == "stack_tabs" && index == 3 {
            1
        } else {
            index
        };
        let section_id = SectionId::new(format!("specimen-section-{group}"))?;
        let column = if location.zone().side() {
            Some(ColumnId::new(if name == "columns" {
                format!("specimen-column-{index}")
            } else {
                format!("specimen-column-{}", if zone == Zone::Left { "left" } else { "right" })
            })?)
        } else {
            None
        };
        let size = if name == "inline" { Size { width: 760.0, height: 64.0 } } else { Size { width: 280.0, height: 240.0 } };
        let collapsed = name == "collapsed";
        let (location, column, collapsed, tab_index) = if let Some(section) = layout.sections.iter_mut().find(|section| section.id == section_id) {
            let tab_index = section.tabs.len();
            section.tabs.push(id.clone());
            if name == "surfaces" {
                // The inactive text-heavy editor still controls FollowZone.
                section.active = id.clone();
            }
            (section.location.clone(), section.column.clone(), section.collapsed, tab_index)
        } else {
            if let Some(column) = &column
                && !layout.columns.iter().any(|existing| existing.id == *column)
            {
                layout.columns.push(Column { id: column.clone(), zone, width: size.width });
            }
            layout.sections.push(Section {
                id: section_id.clone(),
                location: location.clone(),
                column: column.clone(),
                weight: 1.0,
                preferred_extent: None,
                extent_priority: 0,
                tabs: vec![id.clone()],
                active: id.clone(),
                size,
                collapsed,
            });
            (location, column, collapsed, 0)
        };
        let column_index = layout
            .columns
            .iter()
            .filter(|existing| existing.zone == location.zone())
            .position(|existing| Some(&existing.id) == column.as_ref())
            .unwrap_or(0);
        layout.instances.push(PanelInstance {
            id,
            kind: PanelKind::new(kind)?,
            requested_open: true,
            memory: PlacementMemory {
                section: section_id,
                location,
                section_index: group,
                tab_index,
                size,
                collapsed,
                column: column.clone(),
                column_index,
                column_width: column.map(|_| size.width),
                weight: 1.0,
                preferred_extent: None,
                extent_priority: 0,
            },
        });
    }
    // Columns own contiguous section runs even when fixture creation interleaves
    // logical zones. This ordering is independent of instance identity.
    layout.sections.sort_by_key(|section| {
        section.column.as_ref().and_then(|id| layout.columns.iter().position(|column| column.id == *id)).unwrap_or(MAX_SECTIONS)
    });
    for (section_index, section) in layout.sections.iter().enumerate() {
        for (tab_index, id) in section.tabs.iter().enumerate() {
            if let Some(instance) = layout.instances.iter_mut().find(|instance| instance.id == *id) {
                instance.memory.section_index = section_index;
                instance.memory.tab_index = tab_index;
            }
        }
    }
    validate(&layout, &registry)?;
    Ok((layout, registry))
}
struct VisibleSpecimen {
    inner: Box<dyn PanelController>,
    probe: FixtureProbe,
    identities: SpecimenIdentities,
    identity: Rc<()>,
    key: ControllerKey,
    editor: bool,
    title_key: &'static str,
}
impl PanelController for VisibleSpecimen {
    fn availability(&self, owner: &ContentOwner, models: &dyn PanelModels) -> Availability {
        self.inner.availability(owner, models)
    }
    fn update(&mut self, owner: &ContentOwner, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.update(owner, services)?;
        // Publish only from a live update, never from a staged factory. Keeping
        // the token until teardown also prevents address reuse in evidence.
        let mut identities = self.identities.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "specimen identity probe borrowed"))?;
        if identities.len() >= MAX_RETAINED_CONTROLLERS && !identities.contains_key(&self.key) {
            return Err(error(Rule::CollectionLimit, "specimen identity probe cap"));
        }
        identities.insert(self.key.clone(), self.identity.clone());
        Ok(())
    }
    fn presentation(&self) -> PanelPresentation {
        let snapshot = self.probe.state(&self.key).ok();
        PanelPresentation {
            title: solidcraft_ui_text::format(
                "ui.panel_fixture.title",
                &[("title", solidcraft_ui_text::tr(self.title_key)), ("id", &self.key.instance.0)],
            ),
            icon: "list".into(),
            status: snapshot.map(|state| solidcraft_ui_text::format("ui.panel_fixture.updates", &[("count", &state.updates.to_string())])),
            attention: self.editor,
            actions: if self.editor {
                [("apply", "ui.panel_fixture.apply"), ("cancel", "ui.panel_fixture.cancel"), ("finish", "ui.panel_fixture.finish")]
                    .into_iter()
                    .map(|(id, key)| RuntimeAction {
                        pressed: None,
                        id: ControlId(id.into()),
                        label: solidcraft_ui_text::tr(key).into(),
                        enabled: true,
                        disabled_reason: None,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
    fn measure(&self, _owner: &ContentOwner, _models: &dyn PanelModels, spec: &PanelSpec) -> PanelResult<Arc<dyn HostContentMetrics>> {
        let t = crate::theme::Tokens::get();
        let rows = if self.editor { 7.0 } else { 5.0 };
        Ok(Arc::new(SpecimenMetrics {
            // The actual body below uses fixed-height truncated labels and a
            // three-row editor. No hidden renderer or draft enumeration needed.
            body_height: rows * t.control_height + (rows - 1.0) * t.space_4 + 2.0 * t.space_2,
            policy: spec.side_height_policy,
        }))
    }
    fn prepare_lifecycle(&self, event: &LifecycleEvent) -> PanelResult<()> {
        self.inner.prepare_lifecycle(event)
    }
    fn lifecycle(&mut self, event: &LifecycleEvent) {
        self.inner.lifecycle(event);
    }
    fn render(&mut self, slot: ContentSlot, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.render(slot, ui, context, services)?;
        match slot {
            ContentSlot::Header => {
                ui.label(solidcraft_ui_text::tr("ui.panel_fixture.caption"));
            }
            ContentSlot::Body => {
                // Content owns its scrolling; the host still clips all three slots.
                let mut result = Ok(());
                let mut focus_result = Ok(());
                egui::ScrollArea::vertical().id_salt(("specimen-body", &self.key.instance.0)).show(ui, |ui| {
                    result = self.probe.edit(&self.key, |state| {
                        let t = crate::theme::Tokens::get();
                        ui.spacing_mut().item_spacing.y = t.space_4;
                        ui.spacing_mut().interact_size.y = t.control_height;
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                        ui.add_sized(
                            [ui.available_width(), t.control_height],
                            egui::Label::new(solidcraft_ui_text::tr("ui.panel_fixture.explanation")).truncate(),
                        );
                        let draft_label = solidcraft_ui_text::tr("ui.panel_fixture.draft");
                        let label = ui.add_sized([ui.available_width(), t.control_height], egui::Label::new(draft_label).truncate());
                        let draft_id = egui::Id::unique(("specimen-draft", &context.instance, &context.owner));
                        let edit = crate::ui_kit::text_area(ui, draft_label, draft_id, &mut state.draft, 3, 1024);
                        focus_result = services.local.register_focus_response(
                            &LocalScope {
                                workspace: context.workspace.clone(),
                                document: context.document.clone(),
                                viewport: context.viewport.clone(),
                            },
                            &context.instance,
                            &edit,
                        );
                        let _ = edit.labelled_by(label.id);
                        if self.editor {
                            let guard = crate::ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.panel_fixture.guard"), &mut state.reject_cancel);
                            if focus_result.is_ok() {
                                focus_result = services.local.register_focus_response(
                                    &LocalScope {
                                        workspace: context.workspace.clone(),
                                        document: context.document.clone(),
                                        viewport: context.viewport.clone(),
                                    },
                                    &context.instance,
                                    &guard,
                                );
                            }
                            ui.add_sized(
                                [ui.available_width(), t.control_height],
                                egui::Label::new(solidcraft_ui_text::tr(if state.transaction_live {
                                    "ui.panel_fixture.live"
                                } else {
                                    "ui.panel_fixture.finished"
                                }))
                                .truncate(),
                            );
                        }
                    });
                });
                result?;
                focus_result?;
            }
            ContentSlot::Footer => {
                ui.label(solidcraft_ui_text::tr("ui.panel_fixture.footer"));
            }
            ContentSlot::Inline => {
                self.render_inline(ui, context, services)?;
            }
        }
        Ok(())
    }
    fn action(&mut self, owner: &ContentOwner, action: &ControlId, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.action(owner, action, services)
    }
    fn render_inline(&mut self, ui: &mut egui::Ui, context: &PanelContext, services: &mut ContentServices<'_>) -> PanelResult<()> {
        self.inner.render(ContentSlot::Body, ui, context, services)?;
        let mut focus_result = Ok(());
        self.probe.edit(&self.key, |state| {
            // A real editable synthetic value occupies the same total row as
            // shared actions. This is not a production History timeline.
            ui.horizontal_centered(|ui| {
                ui.add(egui::Label::new(solidcraft_ui_text::tr("ui.panel_fixture.inline")).truncate());
                let id = egui::Id::unique(("specimen-draft", &context.instance, &context.owner));
                let response = crate::ui_kit::cell_editor(ui, id, &mut state.draft, solidcraft_ui_text::tr("ui.panel_fixture.draft"), false);
                focus_result = services.local.register_focus_response(
                    &LocalScope { workspace: context.workspace.clone(), document: context.document.clone(), viewport: context.viewport.clone() },
                    &context.instance,
                    &response,
                );
            });
        })?;
        focus_result
    }
}

#[derive(Debug)]
struct SpecimenMetrics {
    body_height: f32,
    policy: SideHeightPolicy,
}
impl HostContentMetrics for SpecimenMetrics {
    fn measure(&self, request: &ContentMeasureRequest) -> PanelResult<SideContentMeasure> {
        Ok(SideContentMeasure { request: request.clone(), needed_height: request.chrome_height + self.body_height, policy: self.policy })
    }
    fn revision(&self) -> u64 {
        u64::from(self.body_height.to_bits()) * 2 + u64::from(self.policy == SideHeightPolicy::Fill)
    }
}

struct SpecimenModels {
    name: String,
    probe: FixtureProbe,
    identities: SpecimenIdentities,
    calls: Rc<Cell<u64>>,
}
impl PanelModels for SpecimenModels {
    fn read(&self, _owner: &ContentOwner, key: &str) -> PanelResult<Value> {
        if key == "fixture.editable" {
            return Ok(json!(true));
        }
        if key != INSPECT {
            return Err(error(Rule::MissingTarget, "unknown specimen model read"));
        }
        let mut states = Vec::new();
        let identities = self.identities.try_borrow().map_err(|_| error(Rule::Lifecycle, "specimen identity probe borrowed"))?;
        for (key, identity) in identities.iter() {
            let state = self.probe.state(key)?;
            states.push(json!({
                "instance": key.instance, "owner": owner_observation(&key.owner),
                "controller_identity":format!("{:p}", Rc::as_ptr(identity)),
                "updates": state.updates, "renders": state.renders, "cancels": state.cancels,
                "reopens": state.reopens, "applies": state.applies, "finishes": state.finishes, "pulses":state.pulses,
                "reparents": state.reparents, "destroys": state.destroys, "draft": state.draft,
                "transaction_live": state.transaction_live, "reject_cancel": state.reject_cancel,
                "state": state.last_state.map(|s| json!({"open":s.requested_open,"presented":s.presented,"activated":s.activated,"destroyed":s.destroyed})),
                "last_clip": state.last_clip.map(|r| [r.min.x,r.min.y,r.max.x,r.max.y]),
            }));
        }
        Ok(json!({"preset":self.name,"controllers":states,"content_command_calls":self.calls.get()}))
    }
}

struct SpecimenCommands {
    calls: Rc<Cell<u64>>,
}
impl ContentCommandSink for SpecimenCommands {
    fn execute(&mut self, _owner: &ContentOwner, command: &str, input: Value) -> PanelResult<Value> {
        if !matches!(command, "fixture.apply" | "fixture.finish") {
            return Err(error(Rule::MissingTarget, "unknown specimen content action"));
        }
        bounded_json(&input)?;
        self.calls.set(self.calls.get().saturating_add(1));
        // Explicit synthetic actions record calls; they never execute a Session command.
        Ok(Value::Null)
    }
}

struct SpecimenServices {
    models: SpecimenModels,
    commands: SpecimenCommands,
    local: FakeLocalServices,
    workspace: WorkspaceId,
}
impl AppPanelServices for SpecimenServices {
    fn refresh_workspace(&mut self, workspace: &WorkspaceId) -> PanelResult<()> {
        if !specimen_workspace(workspace) {
            return Err(error(Rule::Placement, "not a registered specimen workspace"));
        }
        self.workspace = workspace.clone();
        Ok(())
    }
    fn refresh_read_only(&mut self, session: &Session) -> PanelResult<()> {
        let document = DocumentId::new(session.document_id.clone())?;
        let scope = LocalScope { workspace: self.workspace.clone(), document: Some(document), viewport: ViewportId::new("panel_fixture.viewport")? };
        // The bounded registered presets contain at most 24 stable identities.
        // Scope bindings are input services, not panel registrations or bodies.
        for index in 0..24 {
            self.local.bind(InstanceId::new(format!("specimen-{index}"))?, scope.clone())?;
        }
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

/// Read-only observations from the current Session. Pointer strings are opaque
/// process-local equality tokens, not stable IDs. No meshing/evaluation is called.
/// The live-model counter excludes trial evaluations of cloned models.
fn session_observation(app: &crate::SolveApp) -> Value {
    let session = &app.session;
    let state = session.model.state();
    let meshes: Vec<_> = state.bodies.iter().take(128).map(|body| body.cached_mesh().map(|mesh| format!("{:p}", Arc::as_ptr(&mesh)))).collect();
    json!({
        "document_id":session.document_id,"uid":session.uid,"revision":session.revision,
        "dirty":session.is_dirty(),"undo_depth":session.undo.len(),"redo_depth":session.redo.len(),
        "document_identity":format!("{:p}", Arc::as_ptr(&session.doc)),
        "model_identity":format!("{:p}", Arc::as_ptr(&state)),
        "cache_identity":format!("{:p}", Arc::as_ptr(&session.model.cache)),
        "recomputed":session.model.recomputed,"last_recomputed":session.model.last_recomputed,
        "cached_mesh_identities":meshes,"body_count":state.bodies.len(),"mesh_sample_limit":128,
        "camera":app.cam,"evaluation_count":session.evaluation_count(),
    })
}

/// Authenticated control transport and scenarios share this explicit opt-in.
/// An empty object inspects without ticking services or dispatching layout/model actions.
pub(crate) fn control_specimen(app: &mut crate::SolveApp, params: &Value) -> Result<Value, String> {
    let object = params.as_object().ok_or("panel_fixture params must be an object")?;
    if object.keys().any(|key| !matches!(key.as_str(), "enabled" | "preset" | "remember" | "translucency_percent" | "reduced_motion")) {
        return Err("unknown panel_fixture parameter".into());
    }
    let enabled = object.get("enabled").map(|v| v.as_bool().ok_or("enabled must be a boolean")).transpose()?;
    let name = object.get("preset").map(|v| v.as_str().ok_or("preset must be a string")).transpose()?;
    let remember = object.get("remember").map(|v| v.as_bool().ok_or("remember must be a boolean")).transpose()?;
    let reduced_motion = object.get("reduced_motion").map(|v| v.as_bool().ok_or("reduced_motion must be a boolean")).transpose()?;
    let requested_opacity = object
        .get("translucency_percent")
        .map(|value| {
            let percent = value
                .as_f64()
                .filter(|percent| percent.is_finite() && (0.0..=100.0).contains(percent))
                .ok_or("translucency_percent must be a finite number from 0 to 100")?;
            Ok::<f32, &str>((1.0 - percent / 100.0) as f32)
        })
        .transpose()?;
    if let Some(name) = name {
        preset(name).map_err(|e| e.to_string())?;
    }
    if enabled == Some(false) && (name.is_some() || remember.is_some() || requested_opacity.is_some() || reduced_motion.is_some()) {
        return Err("preset, remember, translucency_percent or reduced_motion cannot be combined with enabled=false".into());
    }
    if remember.is_some() && enabled != Some(true) && name.is_none() {
        return Err("remember requires enabling a panel specimen".into());
    }
    if enabled == Some(true) || name.is_some() {
        enable_host_specimen_remembered(app, name.unwrap_or("docks"), remember.unwrap_or(false), requested_opacity).map_err(|e| e.to_string())?;
    } else if requested_opacity.is_some() || reduced_motion.is_some() {
        if !app.universal_panel_host_enabled() || !app.panel_runtime().is_some_and(|runtime| specimen_workspace(&runtime.environment().workspace)) {
            return Err("presentation options require an enabled panel specimen host".into());
        }
        let integration = app.panel_runtime_mut().ok_or("panel specimen runtime disappeared")?;
        // Confirm the provider as well as the workspace marker before changing
        // an existing host. Updating presentation retains controllers and drafts.
        integration.with_services(|_, services| services.models.read(&ContentOwner::Application, INSPECT)).map_err(|e| e.to_string())?;
        if let Some(opacity) = requested_opacity {
            integration.set_requested_opacity(Some(opacity)).map_err(|e| e.to_string())?;
        }
    }
    if let Some(reduced) = reduced_motion {
        app.panel_runtime_mut()
            .ok_or("panel specimen runtime disappeared")?
            .set_motion(if reduced { Motion::Reduced } else { Motion::Animated })
            .map_err(|e| e.to_string())?;
    }
    let owns_runtime = app.panel_runtime().is_some_and(|runtime| specimen_workspace(&runtime.environment().workspace));
    if enabled == Some(false) && app.panel_runtime().is_some() {
        if !owns_runtime {
            return Err("panel specimen cannot tear down another runtime".into());
        }
        // The workspace marker alone never authorizes teardown of injected
        // content: the retained specimen provider must also identify itself.
        app.panel_runtime_mut()
            .ok_or("panel specimen runtime disappeared")?
            .with_services(|_, services| services.models.read(&ContentOwner::Application, INSPECT))
            .map_err(|e| e.to_string())?;
        // Runtime shutdown preflights every controller before destroying any.
        // B's app integration also releases its host after successful shutdown.
        app.disable_panel_runtime().map_err(|e| e.to_string())?;
    }
    inspect_host_specimen(app).map_err(|e| e.to_string())
}

/// Observe the current host, synthetic probe and actual Session without a tick,
/// action dispatch, geometry evaluation or cold mesh creation.
pub fn inspect_host_specimen(app: &mut crate::SolveApp) -> PanelResult<Value> {
    let owns_runtime = app.panel_runtime().is_some_and(|runtime| specimen_workspace(&runtime.environment().workspace));
    let mut reply = json!({"enabled":false,"session":session_observation(app),"host":host_observation(app),
        "effective_usable":app.viewport.layout.as_ref().and_then(|layout| layout.usable.as_ref()).map(|usable|
            json!({"domain":&usable.domain,"obstacles":&usable.obstacles,"free":&usable.free}))});
    if owns_runtime {
        let integration = app.panel_runtime_mut().ok_or_else(|| error(Rule::Lifecycle, "panel specimen runtime disappeared"))?;
        let probe = integration.with_services(|runtime, services| {
            let probe = services.models.read(&ContentOwner::Application, INSPECT)?;
            Ok(json!({"probe":probe,"layout":runtime.layout().inspect(),"controller_count":runtime.controller_count(),"reduced_motion":runtime.environment().motion == Motion::Reduced}))
        })?;
        reply["enabled"] = json!(app.universal_panel_host_enabled());
        reply["runtime"] = probe;
    }
    Ok(reply)
}

fn host_observation(app: &crate::SolveApp) -> Value {
    let Some(frame) = app.universal_panel_frame() else {
        return Value::Null;
    };
    let controls: Vec<_> = frame
        .controls
        .iter()
        .map(|control| {
            json!({
                "id":format!("{:016x}",control.id.value()), "target":target_observation(&control.target),
                "bounds":control.bounds,"role":control.role,
                "accessible_name":control.accessible_name,"enabled":control.enabled,
            })
        })
        .collect();
    let layout = &frame.layout;
    let sections: Vec<_> = layout
        .sections
        .iter()
        .map(|section| {
            json!({
                "section":section.section,"instance":section.instance,"zone":section.zone,
                "outer":section.outer,"header":section.header,"body":section.body,"footer":section.footer,
                "collapsed":section.collapsed,"inline":section.inline,"surface":section.surface,"paint_alpha":section.paint_alpha,"contrast":section.contrast,"hidden_tabs":section.hidden_tabs,
            })
        })
        .collect();
    let overflow: Vec<_> =
        layout.overflow.iter().map(|entry| json!({"zone":entry.zone,"sections":entry.sections,"tabs":entry.tabs,"bounds":entry.bounds})).collect();
    let events: Vec<_> = frame
        .events
        .iter()
        .map(|event| match event {
            HostEvent::Regions { scope, revision, painted, input, occlusion, reservations } => json!({
                "type":"regions","scope":{"workspace":scope.workspace,"document":scope.document,"viewport":scope.viewport},
                "revision":revision,"painted":painted,"input":input,"occlusion":occlusion,"reservations":reservations,
            }),
            HostEvent::Committed { action, reply } => json!({"type":"committed","action":action,"reply":reply}),
            HostEvent::PreparedCommitted { reply } => json!({"type":"prepared_committed","reply":reply}),
            HostEvent::Rejected { action, error } => json!({"type":"rejected","action":action,"error":error}),
            HostEvent::ContentAction { instance, action, result } => {
                json!({"type":"content_action","instance":instance,"action":action,"result":result})
            }
            HostEvent::Focus { target } => json!({"type":"focus","target":target_observation(target)}),
        })
        .collect();
    json!({
        "controls":controls,"sections":sections,"overflow":overflow,"resolved":layout.resolved,"events":events,
        "capacity":layout.capacity,"viewport":layout.viewport,"extent":layout.extent,"scroll":layout.allocation_environment.scroll,"revision":layout.revision,"direction":layout.direction,
        "render_domain":layout.render_domain,"chrome_obstacles":layout.chrome_obstacles,
        "usable":{"domain":layout.usable.domain,"obstacles":layout.usable.obstacles,"free":layout.usable.free},
        "painted":layout.painted,"input":layout.input,"occlusion":layout.occlusion,"reservations":layout.reservations,
        "menu_bounds":frame.menu_bounds,
        "preview":frame.preview.as_ref().map(|preview| json!({"action":preview.action,"bounds":preview.bounds,"destination":preview.destination,"rejection":preview.rejection,
            "target":preview.target.as_ref().map(drop_target_observation),"targets":preview.targets.iter().map(|target| json!({"id":drop_target_observation(&target.id),"bounds":target.bounds,"destination":target.destination,"rejection":target.rejection})).collect::<Vec<_>>(),
            "tab_marker":preview.tab_marker,"projected":preview.projected.as_ref().map(|projected| json!({"sections":projected.sections.iter().map(|section| json!({"section":section.section,"instance":section.instance,"outer":section.outer,"body":section.body})).collect::<Vec<_>>(),"resolved":projected.resolved}))})),
        "reply":frame.reply,
    })
}

fn drop_target_observation(target: &HostDropTargetId) -> Value {
    match target {
        HostDropTargetId::Dock { zone, before } => json!({"type":"dock","zone":zone,"before":before}),
        HostDropTargetId::Tab { section, before } => json!({"type":"tab","section":section,"before":before}),
        HostDropTargetId::Relative { section, placement } => json!({"type":"relative","section":section,"placement":placement}),
    }
}

fn owner_observation(owner: &ContentOwner) -> Value {
    match owner {
        ContentOwner::Application => json!({"type":"application"}),
        ContentOwner::Workspace(id) => json!({"type":"workspace","id":id}),
        ContentOwner::Document(id) => json!({"type":"document","id":id}),
        ContentOwner::Window(id) => json!({"type":"window","id":id}),
    }
}

fn target_observation(target: &HostControlTarget) -> Value {
    match target {
        HostControlTarget::Pointer(pointer) => match pointer {
            HostPointerTarget::Header { instance } => json!({"type":"header","instance":instance}),
            HostPointerTarget::Tab { instance } => json!({"type":"tab","instance":instance}),
            HostPointerTarget::Collapse { instance } => json!({"type":"collapse","instance":instance}),
            HostPointerTarget::Close { instance } => json!({"type":"close","instance":instance}),
            HostPointerTarget::HeaderAction { instance, action } => json!({"type":"header_action","instance":instance,"action":action}),
            HostPointerTarget::Rail { instance } => json!({"type":"rail","instance":instance}),
            HostPointerTarget::Surface { instance } => json!({"type":"surface","instance":instance}),
            HostPointerTarget::Menu { instance } => json!({"type":"menu","instance":instance}),
            HostPointerTarget::TabOverflow { section } => json!({"type":"tab_overflow","section":section}),
            HostPointerTarget::Sash { first, second } => json!({"type":"sash","first":first,"second":second}),
            HostPointerTarget::Resize { section, edge } => json!({"type":"resize","section":section,"edge":match edge {
                HostResizeEdge::Left => "left", HostResizeEdge::Right => "right", HostResizeEdge::Top => "top",
                HostResizeEdge::Bottom => "bottom", HostResizeEdge::BottomRight => "bottom_right",
            }}),
            HostPointerTarget::Overflow { zone } => json!({"type":"overflow","zone":zone}),
            HostPointerTarget::OverflowAll => json!({"type":"overflow_all"}),
        },
        HostControlTarget::Content { instance, action } => json!({"type":"content_action","instance":instance,"action":action}),
        HostControlTarget::PanelMenu { instance } => json!({"type":"panel_menu","instance":instance}),
        HostControlTarget::Scroll { horizontal } => json!({"type":"scroll","horizontal":horizontal}),
        HostControlTarget::Shared { instance, action } => {
            let action = match action {
                HostSharedAction::Focus => json!({"type":"focus"}),
                HostSharedAction::Close => json!({"type":"close"}),
                HostSharedAction::Collapse => json!({"type":"collapse"}),
                HostSharedAction::Move(zone) => json!({"type":"move","zone":zone}),
                HostSharedAction::Join(section) => json!({"type":"join","section":section}),
                HostSharedAction::Popout => json!({"type":"popout"}),
            };
            json!({"type":"shared_action","instance":instance,"action":action})
        }
    }
}
