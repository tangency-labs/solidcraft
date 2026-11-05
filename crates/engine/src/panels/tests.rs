use super::*;

fn id(value: &str) -> InstanceId {
    InstanceId::new(value).unwrap()
}
fn section(value: &str) -> SectionId {
    SectionId::new(value).unwrap()
}
fn size(width: f32, height: f32) -> Size {
    Size { width, height }
}
fn rect(width: f32, height: f32) -> LogicalRect {
    LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: size(width, height) }
}
fn open(service: &mut LayoutService, instance: &str, group: &str) {
    let index = service.layout().sections.len();
    service.dispatch(transaction(vec![fixture_open("fixture.log", instance, group, Zone::Left, index).unwrap()])).unwrap();
}
fn reject(service: &mut LayoutService, mutations: Vec<Mutation>, rule: Rule) {
    let before = service.inspect();
    assert_eq!(service.dispatch(transaction(mutations)).unwrap_err().rule, rule);
    assert_eq!(service.inspect(), before, "failed action must preserve layout/revision/interaction");
}

#[test]
fn registry_constraints_and_identifiers_are_bounded() {
    let base = synthetic_registry().unwrap().specs().next().unwrap().clone();
    for bad in ["", "UPPER", "two words", &"a".repeat(MAX_ID_BYTES + 1)] {
        assert_eq!(PanelKind::new(bad).unwrap_err().rule, Rule::Id);
    }
    for bad in [f32::NAN, f32::INFINITY, -1.0, 0.0, MAX_LOGICAL + 1.0] {
        let mut spec = base.clone();
        spec.min_size.width = bad;
        assert_eq!(PanelRegistry::default().register(spec).unwrap_err().rule, Rule::Bounds);
    }
    let mut spec = base.clone();
    spec.edge_rule = EdgeRule::PinOuter;
    assert_eq!(PanelRegistry::default().register(spec).unwrap_err().rule, Rule::Placement);
    let mut spec = base.clone();
    spec.allowed_zones.remove(&Zone::Floating);
    assert_eq!(PanelRegistry::default().register(spec).unwrap_err().rule, Rule::Placement);
    let mut registry = PanelRegistry::default();
    registry.register(base.clone()).unwrap();
    assert_eq!(registry.register(base).unwrap_err().rule, Rule::DuplicateId);
}

#[test]
fn single_instance_includes_closed_memory_but_multi_instances_coexist() {
    let mut service = fixture_service(false).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.editor", "editor", "edit", Zone::Left, 0).unwrap()])).unwrap();
    service.dispatch(transaction(vec![Mutation::Close { instance: id("editor") }])).unwrap();
    reject(&mut service, vec![fixture_open("fixture.editor", "editor-two", "edit-two", Zone::Left, 0).unwrap()], Rule::ExactlyOnce);
    open(&mut service, "log-one", "logs-one");
    open(&mut service, "log-two", "logs-two");
    validate(service.layout(), service.registry()).unwrap();
}

#[test]
fn duplicate_missing_and_closed_instances_cannot_enter_sections() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let mut broken = service.layout().clone();
    broken.sections.first_mut().unwrap().tabs.push(id("one"));
    assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::ExactlyOnce);
    broken = service.layout().clone();
    broken.instances.first_mut().unwrap().requested_open = false;
    assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::ExactlyOnce);
    broken = service.layout().clone();
    broken.sections.clear();
    assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::ExactlyOnce);
    broken = service.layout().clone();
    broken.sections.first_mut().unwrap().tabs.clear();
    assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn multi_field_failure_is_atomic_and_inspection_reads_committed_state() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    reject(
        &mut service,
        vec![
            Mutation::SetSize { section: section("first"), size: size(400.0, 300.0) },
            Mutation::Move { instance: id("absent"), destination: Destination::JoinTab { section: section("first"), index: 0 } },
        ],
        Rule::MissingTarget,
    );
    let revision = service.inspect().revision;
    assert_eq!(
        service
            .dispatch(LayoutAction::Apply { expected_revision: Some(revision + 1), mutations: vec![Mutation::Close { instance: id("one") }] })
            .unwrap_err()
            .rule,
        Rule::Revision
    );
    let prepared = service.prepare(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    assert!(service.inspect().layout.instance(&id("one")).unwrap().requested_open);
    service.dispatch(transaction(vec![Mutation::Focus { instance: id("one") }])).unwrap();
    assert_eq!(service.commit(prepared).unwrap_err().rule, Rule::Revision);
}

#[test]
fn pin_outer_conflicts_include_shared_groups_and_follow_logical_side() {
    let mut service = fixture_service(false).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.pin", "pin-one", "pins", Zone::Left, 0).unwrap()])).unwrap();
    reject(
        &mut service,
        vec![Mutation::Open {
            kind: PanelKind::new("fixture.pin").unwrap(),
            instance: id("pin-two"),
            destination: Some(Destination::JoinTab { section: section("pins"), index: 1 }),
        }],
        Rule::PinConflict,
    );
    reject(&mut service, vec![fixture_open("fixture.pin", "pin-two", "pins-two", Zone::Left, 1).unwrap()], Rule::PinConflict);
    service.dispatch(transaction(vec![fixture_open("fixture.pin", "pin-two", "pins-two", Zone::Right, 1).unwrap()])).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("pin-one"),
            destination: Destination::NewSection {
                section: section("free-pin"),
                index: 1,
                location: SectionLocation::Floating { origin: Point { x: 0.0, y: 0.0 } },
                size: size(280.0, 240.0),
            },
        }]))
        .unwrap();
    validate(service.layout(), service.registry()).unwrap();
}

#[test]
fn outermost_order_and_forbidden_bottom_are_declarative() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    reject(&mut service, vec![fixture_open("fixture.pin", "pin", "pins", Zone::Left, 1).unwrap()], Rule::PinOrder);
    reject(&mut service, vec![fixture_open("fixture.pin", "pin", "pins", Zone::Bottom, 1).unwrap()], Rule::Placement);
}

#[test]
fn joins_intersect_two_dimensional_constraints_without_changing_active_tab() {
    let base = synthetic_registry().unwrap().get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    let mut registry = PanelRegistry::default();
    registry.register(base.clone()).unwrap();
    let mut narrow = base.clone();
    narrow.kind = PanelKind::new("fixture.narrow").unwrap();
    narrow.max_size.width = 250.0;
    narrow.default_size.width = 200.0;
    let mut wide = base;
    wide.kind = PanelKind::new("fixture.wide").unwrap();
    wide.min_size.width = 300.0;
    wide.default_size.width = 400.0;
    registry.register(narrow).unwrap();
    registry.register(wide).unwrap();
    let mut service = LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture").unwrap()), HostCapabilities::default()).unwrap();
    open(&mut service, "one", "first");
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("two"),
            destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
        }]))
        .unwrap();
    assert_eq!(service.layout().section(&section("first")).unwrap().active, id("one"));
    // Empty intersection is rejected before testing the group's current size.
    let narrow_open = Mutation::Open {
        kind: PanelKind::new("fixture.narrow").unwrap(),
        instance: id("narrow"),
        destination: Some(Destination::NewSection {
            section: section("narrow-section"),
            index: 1,
            location: SectionLocation::Dock { zone: Zone::Right },
            size: size(200.0, 240.0),
        }),
    };
    service.dispatch(transaction(vec![narrow_open])).unwrap();
    reject(
        &mut service,
        vec![Mutation::Open {
            kind: PanelKind::new("fixture.wide").unwrap(),
            instance: id("wide"),
            destination: Some(Destination::JoinTab { section: section("narrow-section"), index: 1 }),
        }],
        Rule::IncompatibleTabs,
    );
}

#[test]
fn tab_indices_and_new_section_targets_are_checked() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    reject(
        &mut service,
        vec![Mutation::Move { instance: id("two"), destination: Destination::JoinTab { section: section("first"), index: usize::MAX } }],
        Rule::Bounds,
    );
    reject(&mut service, vec![fixture_open("fixture.log", "three", "first", Zone::Left, 0).unwrap()], Rule::DuplicateId);
    reject(&mut service, vec![fixture_open("fixture.log", "three", "third", Zone::Left, usize::MAX).unwrap()], Rule::Bounds);
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("two"),
            destination: Destination::JoinTab { section: section("first"), index: 1 },
        }]))
        .unwrap();
    assert_eq!(service.layout().sections.len(), 1);
    service.dispatch(transaction(vec![Mutation::Activate { instance: id("two") }])).unwrap();
    assert_eq!(service.layout().sections.first().unwrap().active, id("two"));
}

#[test]
fn paired_sashes_conserve_axis_and_reset_both_neighbours() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    service
        .dispatch(transaction(vec![Mutation::ResizePair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 180.0),
            second_size: size(280.0, 300.0),
        }]))
        .unwrap();
    reject(
        &mut service,
        vec![Mutation::ResizePair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 200.0),
            second_size: size(280.0, 300.0),
        }],
        Rule::Bounds,
    );
    reject(
        &mut service,
        vec![Mutation::ResizePair {
            first: section("first"),
            second: section("first"),
            first_size: size(280.0, 200.0),
            second_size: size(280.0, 300.0),
        }],
        Rule::Placement,
    );
    service.dispatch(transaction(vec![Mutation::ResetPair { first: section("first"), second: section("second") }])).unwrap();
    assert_eq!(service.layout().section(&section("first")).unwrap().size.height, 240.0);
    assert_eq!(service.layout().section(&section("second")).unwrap().size.height, 240.0);
}

#[test]
fn unavailable_requests_fail_but_saved_open_layout_stays_dormant() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let layout = service.layout().clone();
    service.set_availability([(id("one"), Availability::Unavailable { reason: "context absent".into() })].into()).unwrap();
    reject(&mut service, vec![Mutation::Focus { instance: id("one") }], Rule::Availability);
    reject(
        &mut service,
        vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: None }],
        Rule::Availability,
    );
    assert_eq!(*service.layout(), layout);
    let dormant = resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    assert!(dormant.panels.is_empty());
    assert_eq!(dormant.unpresented.first().unwrap().reason, UnpresentedReason::Unavailable);
    service.set_availability(AvailabilityMap::new()).unwrap();
    assert_eq!(resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap().panels.len(), 1);
}

#[test]
fn closed_state_reopens_with_placement_size_and_collapse_memory() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    service
        .dispatch(transaction(vec![
            Mutation::SetSize { section: section("first"), size: size(350.0, 300.0) },
            Mutation::Collapse { section: section("first"), collapsed: true },
            Mutation::Close { instance: id("one") },
        ]))
        .unwrap();
    assert!(service.layout().sections.is_empty());
    service
        .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: None }]))
        .unwrap();
    let restored = service.layout().section(&section("first")).unwrap();
    assert_eq!(restored.size, size(350.0, 300.0));
    assert!(restored.collapsed);
    service.dispatch(transaction(vec![Mutation::Focus { instance: id("one") }])).unwrap();
    assert!(!service.layout().section(&section("first")).unwrap().collapsed);
}

#[test]
fn peek_never_rewrites_saved_collapse_or_sizes_and_focus_restores_content() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: true }])).unwrap();
    let layout = service.layout().clone();
    service.dispatch(transaction(vec![Mutation::Peek { instance: id("one") }])).unwrap();
    assert_eq!(*service.layout(), layout);
    assert_eq!(resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap().panels.len(), 1);
    service.dispatch(transaction(vec![Mutation::DismissPeek])).unwrap();
    assert!(resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap().panels.is_empty());
    service.dispatch(transaction(vec![Mutation::Focus { instance: id("one") }])).unwrap();
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    assert!(service.interaction().focused.is_none());
}

#[test]
fn overflow_is_transient_and_never_squeezes_or_rewrites_desired_dimensions() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let before = service.inspect();
    let small = resolve_geometry(&service, rect(80.0, 80.0), Direction::Ltr, 48.0).unwrap();
    assert!(small.panels.is_empty());
    assert_eq!(small.unpresented.first().unwrap().reason, UnpresentedReason::MinimumOverflow);
    let large = resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    assert_eq!(large.panels.first().unwrap().outer.size, size(280.0, 240.0));
    assert_eq!(service.inspect(), before);
}

#[test]
fn logical_rtl_flow_hysteresis_and_size_classes_use_usable_units() {
    assert_eq!(Direction::Rtl.edge(Zone::Left).unwrap(), PhysicalEdge::Right);
    for (width, expected) in [(239.0, SizeClass::Compact), (240.0, SizeClass::Regular), (480.0, SizeClass::Regular), (481.0, SizeClass::Wide)] {
        assert_eq!(responsive(Zone::Floating, size(width, 600.0), None).unwrap().1, expected);
    }
    assert_eq!(responsive(Zone::Floating, size(105.0, 100.0), Some(Flow::Tall)).unwrap().0, Flow::Tall);
    assert_eq!(responsive(Zone::Window, size(95.0, 100.0), Some(Flow::Wide)).unwrap().0, Flow::Wide);
    assert_eq!(responsive(Zone::Window, size(89.0, 100.0), Some(Flow::Wide)).unwrap().0, Flow::Tall);
    assert_eq!(responsive(Zone::Bottom, size(100.0, 400.0), None).unwrap().0, Flow::Wide);
}

#[test]
fn pointer_regions_cannot_intercept_unpainted_viewport_space() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let mut panel = resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap().panels.remove(0);
    assert!(panel.consumes_pointer(Point { x: 20.0, y: 20.0 }));
    assert!(!panel.consumes_pointer(Point { x: 500.0, y: 500.0 }));
    panel.input.first_mut().unwrap().bounds = rect(1000.0, 800.0);
    assert_eq!(panel.validate().unwrap_err().rule, Rule::Bounds);
}

#[test]
fn native_capabilities_and_close_redock_are_atomic() {
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    let native = Destination::NewSection {
        section: section("native"),
        index: 0,
        location: SectionLocation::Window {
            host: HostId::new("native-host").unwrap(),
            origin: Point { x: 0.0, y: 0.0 },
            return_to: DockReturn { zone: Zone::Left, index: 0, size: size(280.0, 240.0), collapsed: false, join: None },
        },
        size: size(300.0, 250.0),
    };
    let reply = service.dispatch(transaction(vec![Mutation::Move { instance: id("one"), destination: native.clone() }])).unwrap();
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::CreateNative { .. })));
    let reply = service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }])).unwrap();
    assert_eq!(service.layout().containing(&id("one")).unwrap().location.zone(), Zone::Left);
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::ReleaseNative { .. })));
    let mut web = fixture_service(false).unwrap();
    open(&mut web, "one", "first");
    reject(&mut web, vec![Mutation::Move { instance: id("one"), destination: native }], Rule::Capability);
}

#[test]
fn envelope_schema_roundtrip_and_version_fallback_preserve_original_input() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let envelope = LayoutEnvelope::new(service.layout().clone());
    let bytes = encode(&envelope, service.registry()).unwrap();
    assert_eq!(decode(&bytes, service.registry(), service.layout()).unwrap().envelope, envelope);
    for schema in [layout_action_schema(), layout_reply_schema(), layout_envelope_schema()] {
        assert!(serde_json::to_value(schema).unwrap().is_object());
    }
    let mut future = envelope;
    future.version += 1;
    let raw = serde_json::to_vec(&future).unwrap();
    let loaded = decode(&raw, service.registry(), service.layout()).unwrap();
    assert_eq!(loaded.notice.unwrap().rule, Rule::Version);
    assert_eq!(loaded.rejected.unwrap(), raw);
    assert_eq!(loaded.envelope.layout, *service.layout());
    assert!(decode(b"broken", service.registry(), service.layout()).unwrap().notice.is_some());
}

#[test]
fn unknown_panels_are_quarantined_losslessly_and_reappear_when_registered() {
    let mut known = fixture_service(false).unwrap();
    open(&mut known, "one", "first");
    let mut envelope = serde_json::to_value(LayoutEnvelope::new(known.layout().clone())).unwrap();
    envelope["layout"]["instances"][0]["kind"] = serde_json::json!("extension.panel");
    envelope["layout"]["instances"][0]["future_content"] = serde_json::json!({"keep":"conversation"});
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let fallback = Layout::empty(known.layout().workspace.clone());
    let mut loaded = decode(&bytes, known.registry(), &fallback).unwrap().envelope;
    assert!(loaded.layout.instances.is_empty());
    assert_eq!(loaded.quarantine.len(), 1);
    assert_eq!(loaded.quarantine.first().unwrap().instance_data["future_content"]["keep"], "conversation");
    let mut registry = known.registry().clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    registry.register(spec).unwrap();
    restore_quarantine(&mut loaded, &registry).unwrap();
    assert!(loaded.quarantine.is_empty());
    assert_eq!(loaded.layout.instances.len(), 1);
    assert_eq!(loaded.retained_data[&id("one")]["instance"]["future_content"]["keep"], "conversation");
    validate(&loaded.layout, &registry).unwrap();
}

#[test]
fn persistence_failure_preserves_unrelated_preferences_and_old_layout() {
    let mut persistence = FakePersistence::default();
    let service = fixture_service(false).unwrap();
    persistence.unrelated_preferences.insert("theme".into(), "raw authored intent".into());
    let bytes = encode(&LayoutEnvelope::new(service.layout().clone()), service.registry()).unwrap();
    persistence.save(&service.layout().workspace, &bytes).unwrap();
    persistence.fail_save = true;
    assert!(persistence.save(&service.layout().workspace, b"replacement").is_err());
    assert_eq!(persistence.load(&service.layout().workspace).unwrap().unwrap(), bytes);
    assert_eq!(persistence.unrelated_preferences.get("theme").unwrap(), "raw authored intent");
}

#[test]
fn collection_byte_and_depth_limits_reject_hostile_wire_values() {
    let mut service = fixture_service(false).unwrap();
    reject(&mut service, Vec::new(), Rule::CollectionLimit);
    reject(&mut service, vec![Mutation::DismissPeek; MAX_MUTATIONS + 1], Rule::CollectionLimit);
    let oversized = vec![b' '; MAX_ENVELOPE_BYTES + 1];
    assert_eq!(decode_action(&oversized).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(decode(&oversized, service.registry(), service.layout()).unwrap().notice.unwrap().rule, Rule::CollectionLimit);
    let mut nested = serde_json::Value::Null;
    for _ in 0..34 {
        nested = serde_json::json!([nested]);
    }
    assert_eq!(bounded_json(&nested).unwrap_err().rule, Rule::CollectionLimit);
    let mut layout = service.layout().clone();
    open(&mut service, "one", "first");
    layout.instances = vec![service.layout().instances.first().unwrap().clone(); MAX_INSTANCES + 1];
    assert_eq!(validate(&layout, service.registry()).unwrap_err().rule, Rule::CollectionLimit);
    layout = service.layout().clone();
    layout.sections = vec![layout.sections.first().unwrap().clone(); MAX_SECTIONS + 1];
    assert_eq!(validate(&layout, service.registry()).unwrap_err().rule, Rule::CollectionLimit);
    layout = service.layout().clone();
    layout.sections.first_mut().unwrap().tabs = vec![id("one"); MAX_TABS + 1];
    assert_eq!(validate(&layout, service.registry()).unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn deterministic_hostile_action_sequences_preserve_all_invariants() {
    let mut service = fixture_service(true).unwrap();
    let mut state = 0x3141_5926_u64;
    for step in 0..1024 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let target = id(&format!("instance-{}", state % 8));
        let group = section(&format!("section-{}", (state >> 8) % 8));
        let mutation = match (state >> 16) % 8 {
            0 => fixture_open("fixture.log", &target.0, &group.0, Zone::Left, 0).unwrap(),
            1 => Mutation::Close { instance: target },
            2 => Mutation::Focus { instance: target },
            3 => Mutation::SetSize { section: group, size: size(if step % 2 == 0 { f32::NAN } else { 280.0 }, 240.0) },
            4 => Mutation::Collapse { section: group, collapsed: state.is_multiple_of(2) },
            5 => Mutation::Move { instance: target, destination: Destination::JoinTab { section: group, index: (state % 40) as usize } },
            6 => Mutation::Peek { instance: target },
            _ => Mutation::Destroy { instance: target },
        };
        let before = service.inspect();
        if service.dispatch(transaction(vec![mutation])).is_err() {
            assert_eq!(service.inspect(), before);
        }
        validate(service.layout(), service.registry()).unwrap();
    }
}

#[test]
fn layout_work_never_touches_document_dirty_revision_undo_geometry_or_tessellation() {
    let mut session = crate::Session::default();
    session.execute("solid.box", &serde_json::json!({"length":10,"width":10,"height":10})).unwrap();
    session.mark_saved();
    let document = session.doc.clone();
    let model_state = session.model.state();
    let meshes: Vec<_> = model_state.bodies.iter().map(|b| b.cached_mesh()).collect();
    let revision = session.revision;
    let undo = session.history.len();
    let recomputed = session.model.recomputed;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    for width in [280.0, 350.0, 500.0] {
        service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(width, 240.0) }])).unwrap();
        resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    }
    assert!(!session.is_dirty());
    assert_eq!(session.revision, revision);
    assert_eq!(session.history.len(), undo);
    assert_eq!(session.model.recomputed, recomputed);
    assert!(std::sync::Arc::ptr_eq(&document, &session.doc));
    assert!(std::sync::Arc::ptr_eq(&model_state, &session.model.state()));
    for (before, after) in meshes.iter().zip(session.model.state().bodies.iter().map(|b| b.cached_mesh())) {
        match (before, after) {
            (Some(a), Some(b)) => assert!(std::sync::Arc::ptr_eq(a, &b)),
            (None, None) => {}
            _ => panic!("layout touched tessellation cache"),
        }
    }
}

#[test]
fn surface_precedence_and_truthful_contrast_fallback_have_wire_contracts() {
    let registry = synthetic_registry().unwrap();
    let mut spec = registry.specs().next().unwrap().clone();
    assert_eq!(group_surface(Zone::Bottom, [&spec]), PanelSurface::Opaque);
    spec.surface = PanelSurface::Translucent;
    assert_eq!(group_surface(Zone::Bottom, [&spec]), PanelSurface::Translucent);
    let mut opaque = spec.clone();
    opaque.surface = PanelSurface::Opaque;
    assert_eq!(group_surface(Zone::Left, [&spec, &opaque]), PanelSurface::Opaque);
    let outcome = ContrastOutcome::RetainReadableFallback { reason: ContrastFailure::UnreadableAtFullOpacity };
    let roundtrip: ContrastOutcome = serde_json::from_value(serde_json::to_value(&outcome).unwrap()).unwrap();
    assert_eq!(roundtrip, outcome);
    assert!(ContrastOutcome::Readable { effective_background_alpha: 0.2 }.validate(0.3).is_err());
    assert!(ContrastOutcome::Readable { effective_background_alpha: f32::NAN }.validate(0.3).is_err());
    assert!(ContrastOutcome::Readable { effective_background_alpha: 0.8 }.validate(0.3).is_ok());
}

#[test]
fn versioned_fixture_and_action_schema_are_consumable_without_a_ui() {
    let service = fixture_service(false).unwrap();
    let loaded = decode(FIXTURE_LAYOUT_V1, service.registry(), service.layout()).unwrap();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.envelope.layout.instances.len(), 1);
    let mut live = LayoutService::new(service.registry().clone(), loaded.envelope.layout, HostCapabilities::default()).unwrap();
    let action = decode_action(br#"{"type":"apply","expected_revision":0,"mutations":[{"type":"focus","instance":"editor"}]}"#).unwrap();
    let sink: &mut dyn ActionSink = &mut live;
    assert_eq!(sink.dispatch(action).unwrap().interaction.focused, Some(id("editor")));
    assert_eq!(serde_json::to_value(CompatibilityPreset::Classic).unwrap(), "classic");
}

#[test]
fn instance_section_registry_and_tab_limits_allow_the_boundary_then_reject() {
    let mut service = fixture_service(false).unwrap();
    for index in 0..MAX_TABS {
        let mutation = if index == 0 {
            fixture_open("fixture.log", "log-0", "logs", Zone::Left, 0).unwrap()
        } else {
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id(&format!("log-{index}")),
                destination: Some(Destination::JoinTab { section: section("logs"), index }),
            }
        };
        service.dispatch(transaction(vec![mutation])).unwrap();
    }
    reject(
        &mut service,
        vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("excess"),
            destination: Some(Destination::JoinTab { section: section("logs"), index: MAX_TABS }),
        }],
        Rule::CollectionLimit,
    );
    let base = synthetic_registry().unwrap().specs().next().unwrap().clone();
    let mut registry = PanelRegistry::default();
    for i in 0..MAX_INSTANCES {
        let mut spec = base.clone();
        spec.kind = PanelKind::new(format!("panel-{i}")).unwrap();
        registry.register(spec).unwrap();
    }
    let mut excess = base;
    excess.kind = PanelKind::new("excess").unwrap();
    assert_eq!(registry.register(excess).unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn feature_constraints_and_height_bounds_are_enforced_for_every_member() {
    let mut spec = synthetic_registry().unwrap().get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.features.closable = false;
    spec.features.collapsible = false;
    spec.features.tabbable = false;
    spec.features.popout = false;
    let mut registry = PanelRegistry::default();
    registry.register(spec).unwrap();
    let mut service = LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture").unwrap()), HostCapabilities::default()).unwrap();
    open(&mut service, "one", "first");
    reject(&mut service, vec![Mutation::Close { instance: id("one") }], Rule::FeatureDisabled);
    reject(&mut service, vec![Mutation::Collapse { section: section("first"), collapsed: true }], Rule::FeatureDisabled);
    reject(&mut service, vec![Mutation::SetSize { section: section("first"), size: size(280.0, 119.0) }], Rule::Bounds);
    reject(
        &mut service,
        vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("two"),
            destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
        }],
        Rule::FeatureDisabled,
    );
}

#[test]
fn prepared_candidates_cannot_commit_to_a_different_live_service() {
    let mut first = fixture_service(false).unwrap();
    let mut other = fixture_service(false).unwrap();
    let candidate = first.prepare(transaction(vec![fixture_open("fixture.log", "one", "first", Zone::Left, 0).unwrap()])).unwrap();
    let before = other.inspect();
    assert_eq!(other.commit(candidate).unwrap_err().rule, Rule::Revision);
    assert_eq!(other.inspect(), before);
    open(&mut first, "one", "first");
}

#[test]
fn native_close_returns_to_a_stable_tab_group_without_stealing_activation() {
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("two"),
            destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
        }]))
        .unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("two"),
            destination: Destination::NewSection {
                section: section("native"),
                index: 1,
                location: SectionLocation::Window {
                    host: HostId::new("native-host").unwrap(),
                    origin: Point { x: 0.0, y: 0.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 0,
                        size: size(280.0, 240.0),
                        collapsed: false,
                        join: Some(ReturnTab { section: section("first"), index: 1 }),
                    },
                },
                size: size(280.0, 240.0),
            },
        }]))
        .unwrap();
    service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }])).unwrap();
    let group = service.layout().section(&section("first")).unwrap();
    assert_eq!(group.tabs, vec![id("one"), id("two")]);
    assert_eq!(group.active, id("one"));
    assert_eq!(service.layout().sections.len(), 1);
}

#[test]
fn activation_preserves_collapse_and_does_not_request_focus_or_reopen() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: true }])).unwrap();
    let reply = service.dispatch(transaction(vec![Mutation::Activate { instance: id("one") }])).unwrap();
    assert!(reply.effects.is_empty());
    assert!(reply.interaction.focused.is_none());
    assert!(reply.layout.sections.first().unwrap().collapsed);
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    reject(&mut service, vec![Mutation::Activate { instance: id("one") }], Rule::MissingTarget);
    service.set_availability([(id("future"), Availability::Unavailable { reason: "context missing".into() })].into()).unwrap();
    reject(&mut service, vec![fixture_open("fixture.log", "future", "future-section", Zone::Left, 0).unwrap()], Rule::Availability);
}

#[test]
fn complete_layout_application_keeps_one_revision_and_validates_workspace_identities() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let saved = service.layout().clone();
    let revision = service.inspect().revision;
    let away = Layout::empty(WorkspaceId::new("other-workspace").unwrap());
    let reply = service.dispatch(LayoutAction::Replace { expected_revision: Some(revision), layout: away.clone() }).unwrap();
    assert_eq!(reply.layout, away);
    assert_eq!(reply.revision, revision + 1);
    let before = service.inspect();
    assert_eq!(
        service.dispatch(LayoutAction::Replace { expected_revision: Some(revision), layout: saved.clone() }).unwrap_err().rule,
        Rule::Revision
    );
    assert_eq!(service.inspect(), before);
    let mut changed_kind = saved.clone();
    changed_kind.instances.first_mut().unwrap().kind = PanelKind::new("fixture.editor").unwrap();
    assert_eq!(service.dispatch(LayoutAction::Replace { expected_revision: None, layout: changed_kind }).unwrap_err().rule, Rule::DuplicateId);
    let mut unknown = saved.clone();
    unknown.instances.first_mut().unwrap().kind = PanelKind::new("not.registered").unwrap();
    assert_eq!(service.dispatch(LayoutAction::Replace { expected_revision: None, layout: unknown }).unwrap_err().rule, Rule::MissingTarget);
    assert_eq!(service.inspect(), before);
    service.dispatch(LayoutAction::Replace { expected_revision: Some(before.revision), layout: saved.clone() }).unwrap();
    assert_eq!(service.layout(), &saved);
}

#[test]
fn lifecycle_intents_are_rejected_before_intermediate_mutations() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let reopen = Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: None };
    for batch in [
        vec![Mutation::Close { instance: id("one") }, reopen.clone()],
        vec![
            Mutation::Close { instance: id("one") },
            Mutation::Move { instance: id("one"), destination: Destination::JoinTab { section: section("second"), index: 0 } },
            reopen.clone(),
        ],
        vec![Mutation::Destroy { instance: id("one") }, reopen],
    ] {
        reject(&mut service, batch, Rule::Lifecycle);
    }
    service
        .dispatch(transaction(vec![
            Mutation::Close { instance: id("one") },
            Mutation::SetSize { section: section("second"), size: size(300.0, 260.0) },
        ]))
        .unwrap();
    assert!(!service.layout().instance(&id("one")).unwrap().requested_open);
    assert_eq!(service.layout().section(&section("second")).unwrap().size, size(300.0, 260.0));
}

fn future_group() -> serde_json::Value {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "a", "group");
    for (index, name) in [(1, "b"), (2, "c")] {
        service
            .dispatch(transaction(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id(name),
                destination: Some(Destination::JoinTab { section: section("group"), index }),
            }]))
            .unwrap();
    }
    let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    for instance in raw["layout"]["instances"].as_array_mut().unwrap() {
        let name = instance["id"].as_str().unwrap().to_owned();
        instance["kind"] = serde_json::json!(format!("extension.{name}"));
        instance["future_draft"] = serde_json::json!(format!("draft-{name}"));
    }
    raw["layout"]["sections"][0]["tabs"] = serde_json::json!(["c", "b", "a"]);
    raw["layout"]["sections"][0]["active"] = serde_json::json!("c");
    raw
}

fn future_registry(names: &[&str]) -> PanelRegistry {
    let mut registry = synthetic_registry().unwrap();
    let base = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    for name in names {
        let mut spec = base.clone();
        spec.kind = PanelKind::new(format!("extension.{name}")).unwrap();
        registry.register(spec).unwrap();
    }
    registry
}

#[test]
fn shared_future_metadata_successfully_decodes_and_reencodes_without_duplication() {
    let registry = future_registry(&["a"]);
    let fallback = fixture_service(false).unwrap().layout().clone();
    let mut raw = future_group();
    raw["layout"]["sections"][0]["future_metadata"] = serde_json::json!(vec![serde_json::Value::Null; 5000]);
    let bytes = serde_json::to_vec(&raw).unwrap();
    bounded_json(&raw).unwrap();
    let loaded = decode(&bytes, &registry, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.envelope.quarantine.len(), 2);
    assert_eq!(loaded.envelope.section_snapshots.len(), 1);
    let encoded = encode(&loaded.envelope, &registry).unwrap();
    let reopened = decode(&encoded, &registry, &fallback).unwrap();
    assert!(reopened.notice.is_none());
    assert_eq!(reopened.envelope, loaded.envelope);
    assert_eq!(reopened.envelope.section_snapshots[&section("group")].data["future_metadata"].as_array().unwrap().len(), 5000);
}

#[test]
fn all_unknown_tabs_restore_original_order_and_activation_across_registry_stages() {
    let fallback = fixture_service(false).unwrap().layout().clone();
    let raw = future_group();
    let bytes = serde_json::to_vec(&raw).unwrap();
    let mut envelope = decode(&bytes, &future_registry(&[]), &fallback).unwrap().envelope;
    assert!(envelope.layout.sections.is_empty());
    restore_quarantine(&mut envelope, &future_registry(&["a"])).unwrap();
    assert_eq!(envelope.layout.sections.first().unwrap().tabs, vec![id("a")]);
    restore_quarantine(&mut envelope, &future_registry(&["a", "b"])).unwrap();
    assert_eq!(envelope.layout.sections.first().unwrap().tabs, vec![id("b"), id("a")]);
    restore_quarantine(&mut envelope, &future_registry(&["a", "b", "c"])).unwrap();
    let group = envelope.layout.sections.first().unwrap();
    assert_eq!(group.tabs, vec![id("c"), id("b"), id("a")]);
    assert_eq!(group.active, id("c"));
    assert!(envelope.quarantine.is_empty());
    for name in ["a", "b", "c"] {
        assert_eq!(envelope.retained_data[&id(name)]["instance"]["future_draft"], format!("draft-{name}"));
    }
    assert_eq!(envelope.section_snapshots[&section("group")].data["active"], "c");
    encode(&envelope, &future_registry(&["a", "b", "c"])).unwrap();
}

#[test]
fn mixed_tab_reappearance_retains_unavailable_or_invalid_entries_and_future_fields() {
    let fallback = fixture_service(false).unwrap().layout().clone();
    let raw = future_group();
    let mut envelope = decode(&serde_json::to_vec(&raw).unwrap(), &future_registry(&["a"]), &fallback).unwrap().envelope;
    let mut registry = future_registry(&["a", "b"]);
    let mut narrow = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    narrow.kind = PanelKind::new("extension.c").unwrap();
    narrow.default_size.width = 200.0;
    narrow.max_size.width = 200.0;
    registry.register(narrow).unwrap();
    let unavailable = [(id("b"), Availability::Unavailable { reason: "extension unavailable".into() })].into();
    restore_quarantine_with_availability(&mut envelope, &registry, &unavailable).unwrap();
    assert_eq!(envelope.quarantine.len(), 2);
    assert_eq!(envelope.layout.sections.first().unwrap().active, id("a"));
    restore_quarantine(&mut envelope, &future_registry(&["a", "b", "c"])).unwrap();
    let group = envelope.layout.sections.first().unwrap();
    assert_eq!(group.tabs, vec![id("c"), id("b"), id("a")]);
    assert_eq!(group.active, id("c"));
    assert_eq!(envelope.retained_data[&id("b")]["instance"]["future_draft"], "draft-b");
}

#[test]
fn lossless_representation_over_budget_returns_recovery_instead_of_decode_success() {
    let registry = future_registry(&[]);
    let fallback = fixture_service(false).unwrap().layout().clone();
    let mut raw = future_group();
    raw["layout"]["sections"][0]["future_metadata"] = serde_json::json!("");
    let base = serde_json::to_vec(&raw).unwrap().len();
    raw["layout"]["sections"][0]["future_metadata"] = serde_json::json!("x".repeat(MAX_ENVELOPE_BYTES - base));
    let bytes = serde_json::to_vec(&raw).unwrap();
    assert_eq!(bytes.len(), MAX_ENVELOPE_BYTES);
    let loaded = decode(&bytes, &registry, &fallback).unwrap();
    assert_eq!(loaded.notice.unwrap().rule, Rule::CollectionLimit);
    assert_eq!(loaded.rejected.unwrap(), bytes);
    assert_eq!(loaded.envelope.layout, fallback);
}

#[test]
fn restored_metadata_survives_unregister_register_and_known_survivor_activation() {
    let fallback = fixture_service(false).unwrap().layout().clone();
    let raw = future_group();
    let all = future_registry(&["a", "b", "c"]);
    let only_a = future_registry(&["a"]);
    let mut envelope = decode(&serde_json::to_vec(&raw).unwrap(), &only_a, &fallback).unwrap().envelope;
    restore_quarantine(&mut envelope, &all).unwrap();
    let bytes = encode(&envelope, &all).unwrap();
    let mut again = decode(&bytes, &only_a, &fallback).unwrap().envelope;
    assert_eq!(again.quarantine.len(), 2);
    restore_quarantine(&mut again, &all).unwrap();
    assert_eq!(again.layout.sections.first().unwrap().active, id("c"));
    assert_eq!(again.retained_data[&id("c")]["instance"]["future_draft"], "draft-c");
    // A surviving known active tab is not displaced while some unrelated member
    // reappears. The saved active member only reactivates when it itself returns.
    let a_c = future_registry(&["a", "c"]);
    let mut mixed = decode(&serde_json::to_vec(&raw).unwrap(), &a_c, &fallback).unwrap().envelope;
    mixed.layout.sections.first_mut().unwrap().active = id("a");
    restore_quarantine(&mut mixed, &all).unwrap();
    assert_eq!(mixed.layout.sections.first().unwrap().active, id("a"));
    assert_eq!(mixed.layout.sections.first().unwrap().tabs, vec![id("c"), id("b"), id("a")]);
}

#[test]
fn complete_layout_wire_contract_checks_native_capacity_and_destroyed_identity_reuse() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let snapshot = service.layout().clone();
    let action = LayoutAction::Replace { expected_revision: Some(service.inspect().revision), layout: snapshot.clone() };
    let wire = serde_json::to_vec(&action).unwrap();
    assert_eq!(decode_action(&wire).unwrap(), action);
    let mut native = snapshot.clone();
    native.sections.first_mut().unwrap().location = SectionLocation::Window {
        host: HostId::new("native").unwrap(),
        origin: Point { x: 0.0, y: 0.0 },
        return_to: DockReturn { zone: Zone::Left, index: 0, size: size(280.0, 240.0), collapsed: false, join: None },
    };
    let before = service.inspect();
    assert_eq!(service.dispatch(LayoutAction::Replace { expected_revision: None, layout: native }).unwrap_err().rule, Rule::Capability);
    assert_eq!(service.inspect(), before);
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    service.dispatch(transaction(vec![Mutation::Destroy { instance: id("one") }])).unwrap();
    let mut reused = snapshot;
    reused.instances.first_mut().unwrap().kind = PanelKind::new("fixture.editor").unwrap();
    service.dispatch(LayoutAction::Replace { expected_revision: None, layout: reused }).unwrap();
    assert_eq!(service.layout().instance(&id("one")).unwrap().kind, PanelKind::new("fixture.editor").unwrap());
}

fn future_sections() -> serde_json::Value {
    let mut service = fixture_service(false).unwrap();
    for (member, group) in [("a", "s0"), ("b", "s1"), ("c", "s2")] {
        open(&mut service, member, group);
    }
    let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    let instances = raw["layout"]["instances"].as_array_mut().unwrap();
    for instance in instances.iter_mut() {
        let name = instance["id"].as_str().unwrap().to_owned();
        instance["kind"] = serde_json::json!(format!("extension.{name}"));
        instance["future_draft"] = serde_json::json!(format!("draft-{name}"));
    }
    instances.reverse(); // Exact regression: member processing order s2, s1, s0.
    for group in raw["layout"]["sections"].as_array_mut().unwrap() {
        let name = group["id"].as_str().unwrap().to_owned();
        group["future_surface"] = serde_json::json!({"keep":name});
    }
    raw
}

fn section_ids(envelope: &LayoutEnvelope) -> Vec<SectionId> {
    envelope.layout.sections.iter().map(|section| section.id.clone()).collect()
}

fn persistence_fallback() -> Layout {
    fixture_service(false).unwrap().layout().clone()
}

#[test]
fn whole_section_order_restores_from_reverse_instance_order_in_one_or_many_stages() {
    let fallback = persistence_fallback();
    let raw = future_sections();
    let bytes = serde_json::to_vec(&raw).unwrap();
    let all = future_registry(&["a", "b", "c"]);
    let loaded = decode(&bytes, &future_registry(&[]), &fallback).unwrap();
    assert!(loaded.notice.is_none());
    let original = loaded.envelope;
    assert!(original.layout.sections.is_empty());
    assert_eq!(original.section_order, vec![section("s0"), section("s1"), section("s2")]);
    let mut together = original.clone();
    restore_quarantine(&mut together, &all).unwrap();
    assert_eq!(section_ids(&together), vec![section("s0"), section("s1"), section("s2")]);
    let mut staged = original;
    for (registry, expected) in [
        (future_registry(&["c"]), vec![section("s2")]),
        (future_registry(&["b", "c"]), vec![section("s1"), section("s2")]),
        (all, vec![section("s0"), section("s1"), section("s2")]),
    ] {
        restore_quarantine(&mut staged, &registry).unwrap();
        assert_eq!(section_ids(&staged), expected);
        let saved = encode(&staged, &registry).unwrap();
        let reopened = decode(&saved, &registry, &fallback).unwrap();
        assert!(reopened.notice.is_none());
        staged = reopened.envelope;
    }
    assert!(staged.quarantine.is_empty());
    for (index, name) in ["s0", "s1", "s2"].into_iter().enumerate() {
        let snapshot = &staged.section_snapshots[&section(name)];
        assert_eq!(snapshot.index, index);
        assert_eq!(snapshot.data["future_surface"]["keep"], name);
    }
}

fn command_destination(group: &str, index: usize, zone: &str) -> serde_json::Value {
    serde_json::json!({"type":"new_section","section":group,"index":index,"location":{"type":"dock","zone":zone},"size":{"width":280,"height":240}})
}
fn command_target(instance: &str) -> serde_json::Value {
    serde_json::json!({"type":"instance","id":instance})
}
fn command_ready_backend() -> commands::InMemoryPanelBackend {
    let mut backend = fixture_command_backend().unwrap();
    backend
        .execute("panel.open", &serde_json::json!({"kind":"fixture.log","instance":"one","destination":command_destination("s-one",0,"left")}))
        .unwrap();
    backend
        .execute("panel.open", &serde_json::json!({"kind":"fixture.log","instance":"two","destination":command_destination("s-two",1,"left")}))
        .unwrap();
    backend
}
fn command_inputs() -> Vec<(&'static str, serde_json::Value)> {
    use serde_json::json;
    let selected = json!({"target":command_target("one")});
    let mut floating = command_destination("free", 1, "left");
    floating["location"] = json!({"type":"floating","origin":{"x":20,"y":30}});
    let mut native = floating.clone();
    native["location"] = json!({"type":"window","host":"native","origin":{"x":20,"y":30},"return_to":{"zone":"left","index":0,"size":{"width":280,"height":240},"collapsed":false}});
    vec![
        ("layout.apply", json!({"name":"Fixture default"})),
        ("layout.get", json!({})),
        ("layout.reset", json!({})),
        ("layout.save", json!({"name":"Saved"})),
        ("layout.workspace.select", json!({"workspace":"fixture.other"})),
        ("panel.activate", selected.clone()),
        ("panel.close", selected.clone()),
        ("panel.collapse", json!({"target":command_target("one"),"collapsed":true})),
        ("panel.dismiss", json!({})),
        ("panel.float", json!({"target":command_target("one"),"destination":floating})),
        ("panel.focus", selected.clone()),
        ("panel.list", json!({})),
        ("panel.move", json!({"target":command_target("one"),"destination":command_destination("moved",1,"right")})),
        ("panel.native_close", json!({"target":{"type":"native_host","id":"native"}})),
        (
            "panel.open",
            json!({"kind":"fixture.application","instance":"application","destination":command_destination("application-section",2,"right")}),
        ),
        ("panel.peek", selected),
        ("panel.popout", json!({"target":command_target("one"),"destination":native})),
        ("panel.reset_pair", json!({"first":"s-one","second":"s-two"})),
        ("panel.resize", json!({"target":command_target("one"),"size":{"width":300,"height":300}})),
        (
            "panel.resize_pair",
            json!({"first":"s-one","second":"s-two","first_size":{"width":280,"height":260},"second_size":{"width":280,"height":220}}),
        ),
    ]
}

#[test]
fn every_panel_command_has_strict_input_output_contracts_and_hostile_field_rejection() {
    use commands::LayoutCatalogue;
    let inputs = command_inputs();
    assert_eq!(inputs.len(), commands::IDS.len());
    for (id, input) in inputs {
        let spec = crate::find_command(id).unwrap();
        assert!(!spec.undoable && !spec.reads_model, "{id}");
        let contract = spec.contract.unwrap();
        assert!(contract.check(&input).is_empty(), "{id}: {input}");
        assert_eq!(contract.input_schema().as_value()["additionalProperties"], false, "{id}");
        let output = (spec.output.unwrap())();
        assert!(output.as_value().is_object(), "{id}");
        assert_eq!(output.as_value()["additionalProperties"], false, "{id}");
        let mut backend = command_ready_backend();
        let before = backend.service.inspect();
        let catalogue = backend.catalogue.state().clone();
        for junk in [serde_json::json!(false), serde_json::json!([]), serde_json::json!(["bad"]), serde_json::json!("bad"), {
            let mut value = input.clone();
            value["unknown"] = serde_json::json!(true);
            value
        }] {
            assert!(!contract.check(&junk).is_empty(), "{id}: {junk}");
            assert_eq!(commands::parse_command(id, &junk).unwrap_err().rule, Rule::Schema, "{id}");
            assert!(backend.execute(id, &junk).is_err(), "{id}");
            assert_eq!(backend.service.inspect(), before, "{id}");
            assert_eq!(backend.catalogue.state(), &catalogue, "{id}");
        }
        for field in ["id", "instance", "kind", "first", "second", "workspace"] {
            if input.get(field).is_some() {
                let mut hostile = input.clone();
                hostile[field] = serde_json::json!("UPPER/invalid");
                assert!(commands::parse_command(id, &hostile).is_err(), "{id}/{field}");
            }
        }
        let mut oversized = input.clone();
        oversized["unknown"] = serde_json::json!("x".repeat(commands::MAX_COMMAND_BYTES + 1));
        assert_eq!(commands::parse_command(id, &oversized).unwrap_err().rule, Rule::CollectionLimit, "{id}");
    }
}

#[test]
fn panel_command_success_replies_describe_committed_state_for_every_executable_operation() {
    for (id, input) in command_inputs() {
        let mut backend = command_ready_backend();
        if id == "panel.peek" {
            backend.execute("panel.collapse", &serde_json::json!({"target":command_target("one"),"collapsed":true})).unwrap();
        }
        let result = backend.execute(id, &input);
        if matches!(id, "panel.popout" | "panel.native_close") {
            assert!(result.is_err(), "{id}");
            continue;
        }
        let result = result.unwrap_or_else(|e| panic!("{id}: {e}"));
        let reply: LayoutReply = serde_json::from_value(result.get("state").unwrap_or(&result).clone()).unwrap();
        let live = backend.service.inspect();
        assert_eq!(reply.layout, live.layout, "{id}");
        assert_eq!(reply.revision, live.revision, "{id}");
        assert_eq!(reply.interaction, live.interaction, "{id}");
        assert_eq!(reply.capabilities, live.capabilities, "{id}");
        assert_eq!(reply.availability, live.availability, "{id}");
    }
}

#[test]
fn panel_commands_reject_nested_unknowns_sizes_indices_and_nonfinite_values_before_effects() {
    let mut backend = command_ready_backend();
    for value in [
        serde_json::json!(0),
        serde_json::json!(-1),
        serde_json::json!(32769),
        serde_json::json!(1e100),
        serde_json::Value::Null,
        serde_json::json!("NaN"),
    ] {
        let before = backend.service.inspect();
        assert!(backend.execute("panel.resize", &serde_json::json!({"target":command_target("one"),"size":{"width":value,"height":240}})).is_err());
        assert_eq!(backend.service.inspect(), before);
    }
    for path in ["target", "destination", "location", "size"] {
        let mut input = serde_json::json!({"target":command_target("one"),"destination":command_destination("moved",0,"right")});
        match path {
            "target" => input["target"]["unknown"] = true.into(),
            "destination" => input["destination"]["unknown"] = true.into(),
            "location" => input["destination"]["location"]["unknown"] = true.into(),
            _ => input["destination"]["size"]["unknown"] = true.into(),
        }
        assert_eq!(commands::parse_command("panel.move", &input).unwrap_err().rule, Rule::Schema, "{path}");
    }
    for index in [serde_json::json!(-1), serde_json::json!(65), serde_json::json!(1e100)] {
        let mut dest = command_destination("moved", 0, "left");
        dest["index"] = index;
        assert!(commands::parse_command("panel.move", &serde_json::json!({"target":command_target("one"),"destination":dest})).is_err());
    }
    for hostile in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mutation = Mutation::SetSize { section: section("s-one"), size: size(hostile, 240.0) };
        assert!(backend.service.project(transaction(vec![mutation])).is_err());
    }
}

#[test]
fn command_target_resolution_never_guesses_a_multi_instance_or_mismatched_target() {
    let mut backend = command_ready_backend();
    assert_eq!(backend.execute("panel.focus", &serde_json::json!({"id":"fixture.log"})).unwrap_err().rule, Rule::ExactlyOnce);
    backend.execute("panel.focus", &serde_json::json!({"id":"one"})).unwrap();
    backend
        .execute(
            "panel.open",
            &serde_json::json!({"kind":"fixture.application","instance":"app","destination":command_destination("app-section",2,"right")}),
        )
        .unwrap();
    backend.execute("panel.focus", &serde_json::json!({"id":"fixture.application"})).unwrap();
    for target in [serde_json::json!({"type":"workspace","id":"fixture.workspace"}), serde_json::json!({"type":"section","id":"s-one"})] {
        assert_eq!(backend.execute("panel.focus", &serde_json::json!({"target":target})).unwrap_err().rule, Rule::Schema);
    }
    assert!(backend.execute("panel.focus", &serde_json::json!({"target":command_target("one"),"id":"one"})).is_err());
    backend.execute("panel.close", &serde_json::json!({"target":command_target("one")})).unwrap();
    assert_eq!(backend.execute("panel.activate", &serde_json::json!({"target":command_target("one")})).unwrap_err().rule, Rule::MissingTarget);
    backend.execute("panel.focus", &serde_json::json!({"target":command_target("one")})).unwrap();
    backend.service.set_availability([(id("one"), Availability::Unavailable { reason: "fixture unavailable".into() })].into()).unwrap();
    for command in ["panel.focus", "panel.activate", "panel.peek"] {
        assert_eq!(backend.execute(command, &serde_json::json!({"target":command_target("one")})).unwrap_err().rule, Rule::Availability);
    }
}

#[test]
fn command_storage_native_stale_and_lifecycle_failures_preserve_layout_and_catalogue() {
    use commands::LayoutCatalogue;
    let mut backend = command_ready_backend();
    let before = backend.service.inspect();
    let catalogue = backend.catalogue.state().clone();
    backend.catalogue.fail_stage = true;
    assert_eq!(backend.execute("panel.close", &serde_json::json!({"target":command_target("one")})).unwrap_err().rule, Rule::Capability);
    backend.catalogue.fail_stage = false;
    assert_eq!(
        backend.execute("panel.close", &serde_json::json!({"target":command_target("one"),"expected_revision":0})).unwrap_err().rule,
        Rule::Revision
    );
    let native = command_inputs().into_iter().find(|(id, _)| *id == "panel.popout").unwrap().1;
    assert_eq!(backend.execute("panel.popout", &native).unwrap_err().rule, Rule::Capability);
    let steps = vec![
        serde_json::json!({"command":"panel.close","params":{"target":command_target("one")}}),
        serde_json::json!({"command":"panel.open","params":{"kind":"fixture.log","instance":"one"}}),
    ];
    assert_eq!(backend.batch(&steps).unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
    let invalid = vec![
        serde_json::json!({"command":"panel.collapse","params":{"target":command_target("one"),"collapsed":true}}),
        serde_json::json!({"command":"panel.resize","params":{"target":command_target("two"),"size":{"width":1,"height":1}}}),
    ];
    assert!(backend.batch(&invalid).is_err());
    assert_eq!(backend.service.inspect(), before);
}

#[test]
fn command_presets_enforce_unicode_names_overwrite_builtins_and_user_count() {
    let mut backend = fixture_command_backend().unwrap();
    for invalid in ["".into(), "   ".into(), "x\n".into(), "x\u{0000}".into(), "🦀".repeat(65), " ".repeat(1025)] {
        assert!(backend.execute("layout.save", &serde_json::json!({"name":invalid})).is_err());
    }
    let name = "🦀".repeat(64);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":format!("  {name}  ")})).unwrap()["name"], name);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":name})).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":name,"overwrite":true})).unwrap()["overwritten"], true);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"Fixture default","overwrite":true})).unwrap_err().rule, Rule::Capability);
    for i in 1..commands::MAX_USER_PRESETS {
        backend.execute("layout.save", &serde_json::json!({"name":format!("user-{i}")})).unwrap();
    }
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"overflow"})).unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn command_apply_reset_and_workspace_roundtrip_never_select_implicitly() {
    use serde_json::json;
    let mut backend = command_ready_backend();
    let a = backend.service.layout().clone();
    backend.execute("layout.save", &json!({"name":"Saved"})).unwrap();
    backend.execute("layout.reset", &json!({})).unwrap();
    assert_eq!(backend.service.layout().workspace, a.workspace);
    assert!(backend.service.layout().instances.is_empty());
    backend.execute("layout.apply", &json!({"name":"Saved"})).unwrap();
    assert_eq!(backend.service.layout(), &a);
    let selected = backend.execute("layout.workspace.select", &json!({"workspace":"fixture.other"})).unwrap();
    assert_eq!(selected["source"], "default_preset");
    backend.execute("layout.apply", &json!({"name":"Saved"})).unwrap();
    assert_eq!(backend.service.layout().workspace.0, "fixture.other");
    backend.execute("panel.collapse", &json!({"target":command_target("one"),"collapsed":true})).unwrap();
    let b = backend.service.layout().clone();
    assert_eq!(backend.execute("layout.workspace.select", &json!({"workspace":"fixture.workspace"})).unwrap()["source"], "remembered");
    assert_eq!(backend.service.layout(), &a);
    backend.execute("layout.workspace.select", &json!({"workspace":"fixture.other"})).unwrap();
    assert_eq!(backend.service.layout(), &b);
    backend.execute("panel.peek", &json!({"target":command_target("one")})).unwrap();
    let before = backend.service.inspect();
    backend.catalogue.fail_stage = true;
    assert_eq!(backend.execute("layout.workspace.select", &json!({"workspace":"fixture.other"})).unwrap()["source"], "unchanged");
    assert_eq!(backend.service.inspect(), before, "same workspace preserves revision/focus/peek even when storage is unavailable");
    backend.catalogue.fail_stage = false;
    assert_eq!(backend.execute("layout.workspace.select", &json!({"workspace":"missing"})).unwrap_err().rule, Rule::MissingTarget);
    let summaries = backend.execute("layout.get", &json!({})).unwrap();
    assert_eq!(summaries["workspaces"].as_array().unwrap().len(), 2);
    backend.execute("layout.reset", &json!({})).unwrap();
    assert_eq!(backend.service.layout().workspace.0, "fixture.other");
    backend.execute("layout.apply", &json!({"name":"Saved"})).unwrap();
}

#[test]
fn command_save_apply_reset_preserve_full_unknown_envelopes_and_recovery() {
    use commands::LayoutCatalogue;
    let registry = synthetic_registry().unwrap();
    let fallback = persistence_fallback();
    let raw = future_sections();
    let envelope = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &fallback).unwrap().envelope;
    let mut backend = fixture_command_backend().unwrap();
    backend.catalogue.remember(&envelope, Some(b"rejected future envelope"), &registry).unwrap();
    backend.execute("layout.save", &serde_json::json!({"name":"Unknown"})).unwrap();
    backend.execute("layout.reset", &serde_json::json!({})).unwrap();
    backend.execute("layout.apply", &serde_json::json!({"name":"Unknown"})).unwrap();
    let stored = backend.catalogue.state().remembered(&fallback.workspace).unwrap();
    assert_eq!(stored.rejected(), Some(b"rejected future envelope".as_slice()));
    let saved = stored.envelope(&registry, &fallback.workspace).unwrap();
    assert_eq!(saved.quarantine, envelope.quarantine);
    assert_eq!(saved.section_order, envelope.section_order);
    assert_eq!(saved.section_snapshots, envelope.section_snapshots);
    let mut appeared = saved;
    restore_quarantine(&mut appeared, &future_registry(&["a", "b", "c"])).unwrap();
    assert_eq!(section_ids(&appeared), vec![section("s0"), section("s1"), section("s2")]);
    assert_eq!(appeared.retained_data[&id("a")]["instance"]["future_draft"], "draft-a");
}

#[test]
fn command_sidecar_collisions_and_aggregate_catalogue_growth_are_rejected_atomically() {
    use commands::LayoutCatalogue;
    let mut backend = fixture_command_backend().unwrap();
    let mut first = LayoutEnvelope::new(backend.service.layout().clone());
    first.retained_data.insert(id("future"), serde_json::json!({"draft":"first"}));
    backend.catalogue.remember(&first, None, backend.service.registry()).unwrap();
    backend.execute("layout.save", &serde_json::json!({"name":"First"})).unwrap();
    let mut second = first.clone();
    second.retained_data.insert(id("future"), serde_json::json!({"draft":"second"}));
    backend.catalogue.remember(&second, None, backend.service.registry()).unwrap();
    let before = backend.service.inspect();
    let catalogue = backend.catalogue.state().clone();
    assert_eq!(backend.execute("layout.apply", &serde_json::json!({"name":"First"})).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
    let mut large = first;
    large.retained_data.insert(id("large"), serde_json::json!({"opaque":"x".repeat(200_000)}));
    backend.catalogue.remember(&large, None, backend.service.registry()).unwrap();
    let mut rejected = false;
    for i in 0..commands::MAX_USER_PRESETS {
        let before = backend.catalogue.state().clone();
        if let Err(reason) = backend.execute("layout.save", &serde_json::json!({"name":format!("large-{i}")})) {
            assert_eq!(reason.rule, Rule::CollectionLimit);
            assert_eq!(backend.catalogue.state(), &before);
            rejected = true;
            break;
        }
    }
    assert!(rejected, "aggregate bytes reject before count cap");
}

#[test]
fn session_only_panel_commands_report_capability_without_touching_document_or_model() {
    let mut session = crate::Session::default();
    let doc = session.doc.clone();
    let model = session.model.state();
    let revision = session.revision;
    let dirty = session.is_dirty();
    let undo = session.history.len();
    let evaluations = session.evaluations;
    for level in crate::schema::ParamCheck::ALL {
        session.param_check = level;
        for (id, input) in command_inputs() {
            let reason = session.execute(id, &input).unwrap_err();
            assert!(matches!(reason, crate::EngineError::Panel(PanelError { rule: Rule::Capability, .. })), "{id}: {reason}");
            assert!(commands::structured_error(&reason.to_string()).is_some());
        }
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &session.model.state()));
    assert_eq!(session.revision, revision);
    assert_eq!(session.history.len(), undo);
    assert_eq!(session.is_dirty(), dirty);
    assert_eq!(session.evaluations, evaluations);
}

#[test]
fn all_mutating_panel_commands_reject_stale_revisions_before_catalogue_publication() {
    use commands::LayoutCatalogue;
    for (id, mut input) in command_inputs() {
        if matches!(id, "panel.list" | "layout.get") {
            continue;
        }
        let mut backend = command_ready_backend();
        let before = backend.service.inspect();
        let catalogue = backend.catalogue.state().clone();
        input["expected_revision"] = serde_json::json!(u64::MAX);
        assert_eq!(backend.execute(id, &input).unwrap_err().rule, Rule::Revision, "{id}");
        assert_eq!(backend.service.inspect(), before, "{id}");
        assert_eq!(backend.catalogue.state(), &catalogue, "{id}");
    }
}

#[test]
fn panel_command_pin_tab_intersections_and_paired_sashes_keep_foundation_rules() {
    use serde_json::json;
    let mut backend = command_ready_backend();
    backend
        .execute("panel.open", &json!({"kind":"fixture.pin","instance":"pin","destination":command_destination("pin-section",0,"right")}))
        .unwrap();
    let before = backend.service.inspect();
    assert_eq!(
        backend
            .execute("panel.open", &json!({"kind":"fixture.pin","instance":"pin-two","destination":command_destination("pin-two-section",1,"right")}))
            .unwrap_err()
            .rule,
        Rule::PinConflict
    );
    assert_eq!(backend.service.inspect(), before);
    let bad_pair = json!({"first":"s-one","second":"s-two","first_size":{"width":281,"height":250},"second_size":{"width":280,"height":230}});
    assert_eq!(backend.execute("panel.resize_pair", &bad_pair).unwrap_err().rule, Rule::Bounds);
    assert_eq!(backend.service.inspect(), before);
    backend
        .execute(
            "panel.resize_pair",
            &json!({"first":"s-one","second":"s-two","first_size":{"width":280,"height":260},"second_size":{"width":280,"height":220}}),
        )
        .unwrap();
    let total: f32 = backend.service.layout().sections.iter().filter(|s| s.location.zone() == Zone::Left).map(|s| s.size.height).sum();
    assert_eq!(total, 480.0);
    backend.execute("panel.reset_pair", &json!({"first":"s-one","second":"s-two"})).unwrap();
    assert_eq!(backend.service.layout().section(&section("s-one")).unwrap().size.height, 240.0);
    let mut registry = synthetic_registry().unwrap();
    let mut small = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    small.kind = PanelKind::new("fixture.small").unwrap();
    small.min_size = size(160.0, 120.0);
    small.default_size = size(180.0, 140.0);
    small.max_size = size(200.0, 180.0);
    registry.register(small).unwrap();
    let mut large = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    large.kind = PanelKind::new("fixture.large").unwrap();
    large.min_size = size(400.0, 400.0);
    large.default_size = size(450.0, 450.0);
    large.max_size = size(500.0, 500.0);
    registry.register(large).unwrap();
    let service =
        LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    let mut bounded = commands::InMemoryPanelBackend::new(service, fixture_command_catalogue(&registry).unwrap());
    bounded.execute("panel.open", &json!({"kind":"fixture.log","instance":"log","destination":command_destination("shared",0,"left")})).unwrap();
    let before = bounded.service.inspect();
    assert_eq!(
        bounded
            .execute("panel.open", &json!({"kind":"fixture.small","instance":"small","destination":{"type":"join_tab","section":"shared","index":1}}))
            .unwrap_err()
            .rule,
        Rule::Bounds
    );
    assert_eq!(bounded.service.inspect(), before);
    bounded.execute("panel.resize", &json!({"target":command_target("log"),"size":{"width":180,"height":140}})).unwrap();
    bounded
        .execute("panel.open", &json!({"kind":"fixture.small","instance":"small","destination":{"type":"join_tab","section":"shared","index":1}}))
        .unwrap();
    let before = bounded.service.inspect();
    assert_eq!(
        bounded
            .execute("panel.open", &json!({"kind":"fixture.large","instance":"large","destination":{"type":"join_tab","section":"shared","index":2}}))
            .unwrap_err()
            .rule,
        Rule::IncompatibleTabs
    );
    assert_eq!(bounded.service.inspect(), before);
}

#[test]
fn web_popout_explicitly_lowers_to_floating_while_desktop_requires_native_capability() {
    let mut backend = command_ready_backend();
    let registry = backend.service.registry().clone();
    let input = command_inputs().into_iter().find(|(id, _)| *id == "panel.popout").unwrap().1;
    let before = backend.service.inspect();
    assert_eq!(backend.execute("panel.popout", &input).unwrap_err().rule, Rule::Capability);
    assert_eq!(backend.service.inspect(), before);
    let result = commands::execute(
        "panel.popout",
        &input,
        &mut commands::PanelCommandContext { sink: &mut backend.service, registry: &registry, catalogue: &mut backend.catalogue, web: true },
    )
    .unwrap();
    assert_eq!(result["layout"]["sections"][1]["location"]["type"], "floating");
    assert!(result["effects"].as_array().unwrap().iter().all(|e| e["type"] != "create_native"));
}

#[test]
fn catalogue_batch_stages_all_saves_and_replacements_once_and_rolls_back_failed_plans() {
    use commands::LayoutCatalogue;
    let mut backend = command_ready_backend();
    let before = backend.service.inspect();
    let result = backend
        .batch(&[
            serde_json::json!({"command":"layout.save","params":{"name":"Batch saved"}}),
            serde_json::json!({"command":"layout.reset"}),
            serde_json::json!({"command":"layout.apply","params":{"name":"Batch saved"}}),
            serde_json::json!({"command":"layout.get"}),
        ])
        .unwrap();
    assert_eq!(result["completed"], 4);
    assert_eq!(backend.service.layout(), &before.layout);
    let before = backend.service.inspect();
    let catalogue = backend.catalogue.state().clone();
    assert!(
        backend
            .batch(&[
                serde_json::json!({"command":"layout.save","params":{"name":"Not saved"}}),
                serde_json::json!({"command":"layout.workspace.select","params":{"workspace":"missing"}}),
            ])
            .is_err()
    );
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
    let mixed = backend
        .batch(&[
            serde_json::json!({"command":"panel.close","params":{"target":command_target("one")}}),
            serde_json::json!({"command":"layout.reset"}),
        ])
        .unwrap_err();
    assert_eq!(mixed.rule, Rule::Capability);
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
}

#[test]
fn catalogue_staging_cannot_publish_a_plan_after_an_external_revision_change() {
    use commands::{CatalogueState, LayoutCatalogue, PreparedCatalogue};
    use std::cell::RefCell;
    use std::rc::Rc;
    struct SharedSink(Rc<RefCell<LayoutService>>);
    impl ActionSink for SharedSink {
        fn inspect(&self) -> LayoutReply {
            self.0.borrow().inspect()
        }
        fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
            self.0.borrow().project(action)
        }
        fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
            self.0.borrow_mut().dispatch(action)
        }
    }
    struct RacingCatalogue {
        catalogue: commands::InMemoryCatalogue,
        service: Rc<RefCell<LayoutService>>,
        published: bool,
    }
    impl LayoutCatalogue for RacingCatalogue {
        fn state(&self) -> &CatalogueState {
            self.catalogue.state()
        }
        fn prepare(&mut self, next: CatalogueState) -> PanelResult<PreparedCatalogue> {
            self.service.borrow_mut().set_availability([(id("one"), Availability::Unavailable { reason: "external update".into() })].into())?;
            PreparedCatalogue::new(next)
        }
        fn publish(&mut self, prepared: PreparedCatalogue) {
            self.published = true;
            self.catalogue.publish(prepared);
        }
    }
    let backend = command_ready_backend();
    let registry = backend.service.registry().clone();
    let shared = Rc::new(RefCell::new(backend.service));
    let mut sink = SharedSink(shared.clone());
    let mut catalogue = RacingCatalogue { catalogue: backend.catalogue, service: shared, published: false };
    let before = catalogue.state().clone();
    let reason = commands::execute(
        "panel.close",
        &serde_json::json!({"target":command_target("one")}),
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap_err();
    assert_eq!(reason.rule, Rule::Revision);
    assert!(!catalogue.published);
    assert_eq!(catalogue.state(), &before);
    assert!(sink.inspect().layout.instance(&id("one")).unwrap().requested_open);
}

#[test]
fn catalogue_workspace_count_and_metadata_budgets_are_independent_and_atomic() {
    use commands::LayoutCatalogue;
    let registry = synthetic_registry().unwrap();
    let envelope = LayoutEnvelope::new(persistence_fallback());
    let mut catalogue = commands::InMemoryCatalogue::default();
    catalogue.register_builtin("Default", &envelope, &registry).unwrap();
    for i in 0..commands::MAX_WORKSPACES {
        catalogue
            .register_workspace(commands::WorkspaceDescriptor {
                id: WorkspaceId::new(format!("workspace-{i}")).unwrap(),
                title_key: "ui.panel".into(),
                default_preset: "Default".into(),
            })
            .unwrap();
    }
    let before = catalogue.state().clone();
    assert_eq!(
        catalogue
            .register_workspace(commands::WorkspaceDescriptor {
                id: WorkspaceId::new("overflow").unwrap(),
                title_key: "ui.panel".into(),
                default_preset: "Default".into()
            })
            .unwrap_err()
            .rule,
        Rule::CollectionLimit
    );
    assert_eq!(catalogue.state(), &before);
    let mut metadata = commands::InMemoryCatalogue::default();
    for i in 0..64 {
        metadata.register_builtin(&format!("{}{:02}", "🦀".repeat(62), i), &envelope, &registry).unwrap();
    }
    let name = format!("{}00", "🦀".repeat(62));
    metadata
        .register_workspace(commands::WorkspaceDescriptor {
            id: WorkspaceId::new("first").unwrap(),
            title_key: "ui.panel".into(),
            default_preset: name.clone(),
        })
        .unwrap();
    let before = metadata.state().clone();
    assert_eq!(
        metadata
            .register_workspace(commands::WorkspaceDescriptor {
                id: WorkspaceId::new("second").unwrap(),
                title_key: "ui.panel".into(),
                default_preset: name
            })
            .unwrap_err()
            .rule,
        Rule::CollectionLimit
    );
    assert_eq!(metadata.state(), &before);
}

#[test]
fn headless_cannot_claim_native_support_even_when_an_injected_service_sets_native_flags() {
    let service = fixture_service(true).unwrap();
    let catalogue = fixture_command_catalogue(service.registry()).unwrap();
    let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
    backend
        .execute("panel.open", &serde_json::json!({"kind":"fixture.log","instance":"one","destination":command_destination("s-one",0,"left")}))
        .unwrap();
    assert_eq!(backend.execute("panel.list", &serde_json::json!({})).unwrap()["state"]["capabilities"]["native_windows"], false);
    let mut input = command_inputs().into_iter().find(|(id, _)| *id == "panel.popout").unwrap().1;
    input["destination"]["index"] = serde_json::json!(0);
    let before = backend.service.inspect();
    assert_eq!(backend.execute("panel.popout", &input).unwrap_err().rule, Rule::Capability);
    assert_eq!(backend.service.inspect(), before);
}

#[test]
fn registry_growth_save_and_live_mutation_keep_uncommitted_reappearance_retryable() {
    use commands::LayoutCatalogue;
    let absent = future_registry(&["a"]);
    let grown = future_registry(&["a", "b", "c"]);
    let fallback = persistence_fallback();
    let raw = future_group();
    let dormant = decode(&serde_json::to_vec(&raw).unwrap(), &absent, &fallback).unwrap().envelope;
    let service = LayoutService::new(grown.clone(), dormant.layout.clone(), HostCapabilities::default()).unwrap();
    let mut catalogue = fixture_command_catalogue(&grown).unwrap();
    catalogue.remember(&dormant, Some(b"recovery"), &grown).unwrap();
    let snapshot = catalogue.state().export().unwrap();
    let catalogue = commands::InMemoryCatalogue::from_snapshot(&snapshot, &grown).unwrap();
    let hydrated = catalogue.state().remembered(&fallback.workspace).unwrap().envelope(&grown, &fallback.workspace).unwrap();
    assert_eq!(hydrated.quarantine, dormant.quarantine);
    assert_eq!(hydrated.section_order, dormant.section_order);
    assert_eq!(hydrated.section_snapshots, dormant.section_snapshots, "including materialized projected sections");
    let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
    backend.execute("layout.save", &serde_json::json!({"name":"Registry grew"})).unwrap();
    backend
        .execute(
            "panel.open",
            &serde_json::json!({"kind":"fixture.application","instance":"app","destination":command_destination("app-section",1,"right")}),
        )
        .unwrap();
    backend.execute("panel.resize", &serde_json::json!({"target":command_target("app"),"size":{"width":300,"height":280}})).unwrap();
    let remembered = backend.catalogue.state().remembered(&fallback.workspace).unwrap();
    let retained = remembered.envelope(&grown, &fallback.workspace).unwrap();
    assert_eq!(retained.quarantine.len(), 2, "temporary decode is not a committed restore");
    assert_eq!(retained.section_snapshots[&section("group")].data["active"], "c");
    assert_eq!(retained.section_snapshots[&section("group")].data["tabs"], serde_json::json!(["c", "b", "a"]));
    assert_eq!(retained.quarantine.iter().find(|q| q.instance == id("b")).unwrap().instance_data["future_draft"], "draft-b");
    assert_eq!(remembered.rejected(), Some(b"recovery".as_slice()));
    let restored = decode(remembered.bytes(), &grown, &fallback).unwrap().envelope;
    assert_eq!(restored.layout.section(&section("group")).unwrap().active, id("c"));
    assert_eq!(restored.layout.section(&section("group")).unwrap().tabs, vec![id("c"), id("b"), id("a")]);
    let before = backend.catalogue.state().clone();
    let live = backend.service.inspect();
    backend.catalogue.fail_stage = true;
    assert!(backend.execute("layout.apply", &serde_json::json!({"name":"Registry grew"})).is_err());
    assert_eq!(backend.catalogue.state(), &before);
    assert_eq!(backend.service.inspect(), live);
    backend.catalogue.fail_stage = false;
    backend.execute("layout.apply", &serde_json::json!({"name":"Registry grew"})).unwrap();
    assert_eq!(backend.service.layout().section(&section("group")).unwrap().active, id("c"));
    let committed = backend.catalogue.state().remembered(&fallback.workspace).unwrap().envelope(&grown, &fallback.workspace).unwrap();
    assert!(committed.quarantine.is_empty());
    assert_eq!(committed.retained_data[&id("b")]["instance"]["future_draft"], "draft-b");
}

#[test]
fn disabled_popout_rejects_native_and_web_before_lowering_while_float_remains_allowed() {
    use commands::LayoutCatalogue;
    for web in [false, true] {
        let mut registry = synthetic_registry().unwrap();
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new("fixture.no-popout").unwrap();
        spec.features.popout = false;
        registry.register(spec).unwrap();
        let mut service = LayoutService::new(
            registry.clone(),
            persistence_fallback(),
            HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: false },
        )
        .unwrap();
        service.dispatch(transaction(vec![fixture_open("fixture.no-popout", "one", "s-one", Zone::Left, 0).unwrap()])).unwrap();
        let mut catalogue = fixture_command_catalogue(&registry).unwrap();
        let before = service.inspect();
        let stored = catalogue.state().clone();
        let mut popout = command_inputs().into_iter().find(|(id, _)| *id == "panel.popout").unwrap().1;
        popout["destination"]["index"] = serde_json::json!(0);
        let reason = commands::execute(
            "panel.popout",
            &popout,
            &mut commands::PanelCommandContext { sink: &mut service, registry: &registry, catalogue: &mut catalogue, web },
        )
        .unwrap_err();
        assert_eq!(reason.rule, Rule::FeatureDisabled);
        assert_eq!(service.inspect(), before);
        assert_eq!(catalogue.state(), &stored);
        popout["destination"]["location"] = serde_json::json!({"type":"floating","origin":{"x":20,"y":30}});
        commands::execute(
            "panel.float",
            &popout,
            &mut commands::PanelCommandContext { sink: &mut service, registry: &registry, catalogue: &mut catalogue, web },
        )
        .unwrap();
        assert_eq!(service.layout().containing(&id("one")).unwrap().location.zone(), Zone::Floating);
    }
}

// Deliberately uses only public consumer interfaces, as an independent storage
// crate would; user presets are never reconstructed by fake live commands.
mod catalogue_consumer {
    use crate::panels::commands::{
        CatalogueState, InMemoryCatalogue, InMemoryPanelBackend, LayoutCatalogue, MAX_CATALOGUE_BYTES, MAX_CATALOGUE_SNAPSHOT_BYTES,
    };
    use crate::panels::*;

    fn maximum_record(mut envelope: LayoutEnvelope, registry: &PanelRegistry, bytes: usize) -> LayoutEnvelope {
        let padding = InstanceId::new("padding").unwrap();
        envelope.retained_data.insert(padding.clone(), serde_json::json!(""));
        let overhead = encode(&envelope, registry).unwrap().len();
        let length = bytes.checked_sub(overhead).expect("test record has room for opaque padding");
        envelope.retained_data.insert(padding, serde_json::json!("x".repeat(length)));
        assert_eq!(encode(&envelope, registry).unwrap().len(), bytes);
        envelope
    }

    fn unmaterialized_section_record(with_snapshot: bool) -> (PanelRegistry, LayoutEnvelope) {
        let mut service = fixture_service(false).unwrap();
        service.dispatch(transaction(vec![fixture_open("fixture.log", "log", "logs", Zone::Left, 0).unwrap()])).unwrap();
        let registry = service.registry().clone();
        let mut envelope = LayoutEnvelope::new(service.layout().clone());
        envelope.section_order.clear(); // Encode-valid earlier-v1 intent, not missing content.
        if with_snapshot {
            let section = envelope.layout.sections.first().unwrap();
            let mut data = serde_json::to_value(section).unwrap();
            data["future_surface"] = serde_json::json!({"opaque":"preserve exactly"});
            envelope.section_snapshots.insert(section.id.clone(), SectionSnapshot { data, index: 0, projected: None });
        }
        (registry, envelope)
    }

    fn without_kind(registry: &PanelRegistry, missing: &str) -> PanelRegistry {
        let mut absent = PanelRegistry::default();
        for spec in registry.specs().filter(|spec| spec.kind.0 != missing) {
            absent.register(spec.clone()).unwrap();
        }
        absent
    }

    fn assert_same_placement(actual: &Layout, expected: &Layout) {
        assert_eq!(actual.version, expected.version);
        assert_eq!(actual.workspace, expected.workspace);
        assert_eq!(actual.sections, expected.sections, "complete section order, tab order and activation");
        assert_eq!(actual.instances.len(), expected.instances.len());
        for instance in &expected.instances {
            assert_eq!(actual.instance(&instance.id).unwrap(), instance);
        }
    }

    fn registered_extension_record() -> (PanelRegistry, LayoutEnvelope) {
        let mut registry = synthetic_registry().unwrap();
        let mut extension = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        extension.kind = PanelKind::new("extension.a").unwrap();
        registry.register(extension).unwrap();
        let mut service =
            LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
        service
            .dispatch(transaction(vec![
                fixture_open("fixture.log", "ordinary", "group", Zone::Left, 0).unwrap(),
                Mutation::Open {
                    kind: PanelKind::new("extension.a").unwrap(),
                    instance: InstanceId::new("extension").unwrap(),
                    destination: Some(Destination::JoinTab { section: SectionId::new("group").unwrap(), index: 0 }),
                },
                fixture_open("fixture.application", "app", "app-section", Zone::Right, 1).unwrap(),
                Mutation::Activate { instance: InstanceId::new("extension").unwrap() },
            ]))
            .unwrap();
        let mut envelope = LayoutEnvelope::new(service.layout().clone());
        envelope.retained_data.insert(InstanceId::new("extension").unwrap(), serde_json::json!({"future_draft":{"text":"keep draft","cursor":7}}));
        for (index, section) in envelope.layout.sections.iter().enumerate() {
            let mut data = serde_json::to_value(section).unwrap();
            data["future_surface"] = serde_json::json!({"opaque":section.id.0});
            envelope.section_snapshots.insert(section.id.clone(), SectionSnapshot { data, index, projected: Some(section.clone()) });
        }
        (registry, envelope)
    }

    #[test]
    fn registry_shrink_hydrates_exact_users_builtins_and_memories_then_stages_reappearance() {
        let (registry, envelope) = registered_extension_record();
        let absent = without_kind(&registry, "extension.a");
        let mut catalogue = fixture_command_catalogue(&registry).unwrap();
        catalogue.register_builtin("Extension default", &envelope, &registry).unwrap();
        catalogue
            .register_workspace(commands::WorkspaceDescriptor {
                id: WorkspaceId::new("extension.workspace").unwrap(),
                title_key: "ui.panel".into(),
                default_preset: "Extension default".into(),
            })
            .unwrap();
        let mut source = InMemoryPanelBackend::new(fixture_service(false).unwrap(), catalogue);
        source.execute("layout.save", &serde_json::json!({"name":"Ordinary user"})).unwrap();
        // Use the real extended registry/service when creating the saved user.
        source.service = LayoutService::new(registry.clone(), envelope.layout.clone(), HostCapabilities::default()).unwrap();
        let rejected = [0, 255, 128, 0, 17];
        source.catalogue.remember(&envelope, Some(&rejected), &registry).unwrap();
        source.execute("layout.save", &serde_json::json!({"name":"Extension user"})).unwrap();
        let snapshot = source.catalogue.state().export().unwrap();
        let hydrated = InMemoryCatalogue::from_snapshot(&snapshot, &absent).unwrap();
        assert_eq!(hydrated.state(), source.catalogue.state());
        assert_eq!(hydrated.state().export().unwrap(), snapshot);
        assert_eq!(hydrated.state().user_presets().map(|(name, _)| name).collect::<Vec<_>>(), vec!["Extension user", "Ordinary user"]);
        let record = hydrated.state().preset("Extension user").unwrap();
        let exact = record.envelope(&absent, &envelope.layout.workspace).unwrap();
        assert_eq!(exact.layout, envelope.layout, "inspection keeps the original requested-open instance and activation");
        assert_eq!(exact.retained_data, envelope.retained_data);
        assert_eq!(record.rejected(), Some(rejected.as_slice()));
        let mut staged = record.project_envelope(&absent, &envelope.layout.workspace).unwrap();
        validate(&staged.layout, &absent).unwrap();
        assert!(staged.layout.instances.iter().all(|instance| instance.kind.0 != "extension.a"));
        assert_eq!(staged.quarantine.len(), 1);
        assert_eq!(staged.section_snapshots[&SectionId::new("group").unwrap()].data["active"], "extension");
        let dormant = staged.clone();
        let availability = [(InstanceId::new("extension").unwrap(), Availability::Unavailable { reason: "temporarily absent provider".into() })]
            .into_iter()
            .collect();
        restore_quarantine_with_availability(&mut staged, &registry, &availability).unwrap();
        assert_eq!(staged, dormant, "a refused reappearance preserves retryable intent");
        restore_quarantine(&mut staged, &registry).unwrap();
        assert_same_placement(&staged.layout, &envelope.layout);
        assert_eq!(staged.section_order, envelope.section_order);
        assert!(staged.quarantine.is_empty());
        assert_eq!(staged.retained_data[&InstanceId::new("extension").unwrap()], envelope.retained_data[&InstanceId::new("extension").unwrap()]);
        assert_eq!(staged.section_snapshots[&SectionId::new("group").unwrap()].data["future_surface"]["opaque"], "group");
        let mut absent_backend = InMemoryPanelBackend::new(
            LayoutService::new(absent.clone(), Layout::empty(envelope.layout.workspace.clone()), HostCapabilities::default()).unwrap(),
            hydrated,
        );
        absent_backend.execute("layout.apply", &serde_json::json!({"name":"Ordinary user"})).unwrap();
        absent_backend.execute("layout.workspace.select", &serde_json::json!({"workspace":"fixture.other"})).unwrap();
        absent_backend.execute("layout.reset", &serde_json::json!({})).unwrap();
        absent_backend.execute("layout.workspace.select", &serde_json::json!({"workspace":"extension.workspace"})).unwrap();
        assert!(absent_backend.service.layout().instances.iter().all(|instance| instance.kind.0 != "extension.a"));
        let remembered = absent_backend
            .catalogue
            .state()
            .remembered(&WorkspaceId::new("extension.workspace").unwrap())
            .unwrap()
            .envelope(&absent, &WorkspaceId::new("extension.workspace").unwrap())
            .unwrap();
        assert_eq!(remembered.quarantine.len(), 1, "selecting a missing peer stages quarantine, never an unknown live instance");
        absent_backend.execute("layout.save", &serde_json::json!({"name":"Still dormant"})).unwrap();
        let after_shrink = absent_backend.catalogue.state().export().unwrap();
        let grown = InMemoryCatalogue::from_snapshot(&after_shrink, &registry).unwrap();
        assert_eq!(grown.state().export().unwrap(), after_shrink);
        assert_eq!(grown.state().preset("Extension user").unwrap().bytes(), source.catalogue.state().preset("Extension user").unwrap().bytes());
        assert_eq!(grown.state().preset("Extension user").unwrap().rejected(), Some(rejected.as_slice()));
        let service =
            LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("recovery.workspace").unwrap()), HostCapabilities::default())
                .unwrap();
        let mut grown_backend = InMemoryPanelBackend::new(service, grown);
        grown_backend.execute("layout.apply", &serde_json::json!({"name":"Still dormant"})).unwrap();
        let mut expected = envelope.layout;
        expected.workspace = WorkspaceId::new("recovery.workspace").unwrap();
        assert_same_placement(grown_backend.service.layout(), &expected);
    }

    #[test]
    fn registry_shrink_maximum_records_remain_exact_and_projection_failure_is_retryable() {
        for with_snapshot in [false, true] {
            let (registry, envelope) = unmaterialized_section_record(with_snapshot);
            let absent = without_kind(&registry, "fixture.log");
            let envelope = maximum_record(envelope, &registry, MAX_ENVELOPE_BYTES);
            let original = encode(&envelope, &registry).unwrap();
            let rejected = [0, 255, 128, 0];
            let mut catalogue = InMemoryCatalogue::default();
            catalogue.register_builtin("Maximum", &envelope, &registry).unwrap();
            let ordinary = LayoutEnvelope::new(Layout::empty(WorkspaceId::new("unrelated.workspace").unwrap()));
            catalogue.register_builtin("Ordinary", &ordinary, &registry).unwrap();
            catalogue.remember(&envelope, Some(&rejected), &registry).unwrap();
            let snapshot = catalogue.state().export().unwrap();
            let hydrated = InMemoryCatalogue::from_snapshot(&snapshot, &absent).unwrap();
            assert_eq!(hydrated.state(), catalogue.state());
            assert_eq!(hydrated.state().export().unwrap(), snapshot);
            let stored = hydrated.state().remembered(&envelope.layout.workspace).unwrap();
            assert_eq!(stored.bytes(), original.as_slice());
            assert_eq!(stored.rejected(), Some(rejected.as_slice()));
            assert_eq!(stored.envelope(&absent, &envelope.layout.workspace).unwrap(), envelope);
            if with_snapshot {
                let projected = stored.project_envelope(&absent, &envelope.layout.workspace).unwrap();
                validate(&projected.layout, &absent).unwrap();
                assert_eq!(projected.quarantine.len(), 1);
                assert_eq!(projected.section_snapshots[&SectionId::new("logs").unwrap()].data["active"], "log");
            } else {
                // Capturing a previously absent snapshot needs additional bytes.
                assert_eq!(stored.project_envelope(&absent, &envelope.layout.workspace).unwrap_err().rule, Rule::CollectionLimit);
            }
            let mut backend =
                InMemoryPanelBackend::new(LayoutService::new(absent.clone(), ordinary.layout, HostCapabilities::default()).unwrap(), hydrated);
            backend.execute("layout.apply", &serde_json::json!({"name":"Ordinary"})).unwrap();
            let before = backend.catalogue.state().clone();
            let live = backend.service.inspect();
            if with_snapshot {
                // Existing snapshot storage frees enough space when its absent
                // live section is quarantined; no cap is raised to permit it.
                backend.execute("layout.apply", &serde_json::json!({"name":"Maximum"})).unwrap();
                assert!(backend.service.layout().instances.is_empty());
                let remembered =
                    backend.catalogue.state().remembered(&live.layout.workspace).unwrap().envelope(&absent, &live.layout.workspace).unwrap();
                assert_eq!(remembered.quarantine.len(), 1);
                assert_eq!(remembered.retained_data, envelope.retained_data);
            } else {
                assert_eq!(backend.execute("layout.apply", &serde_json::json!({"name":"Maximum"})).unwrap_err().rule, Rule::CollectionLimit);
                assert_eq!(backend.catalogue.state(), &before);
                assert_eq!(backend.service.inspect(), live);
            }
            let grown = CatalogueState::hydrate(&snapshot, &registry).unwrap();
            assert_eq!(grown.export().unwrap(), snapshot);
            assert_eq!(
                grown.preset("Maximum").unwrap().project_envelope(&registry, &envelope.layout.workspace).unwrap(),
                envelope,
                "returning registration needs no read-time metadata expansion"
            );
        }
    }

    #[test]
    fn registry_shrink_and_growth_preserve_exact_aggregate_budget_and_recovery() {
        let (registry, base) = unmaterialized_section_record(true);
        let absent = without_kind(&registry, "fixture.log");
        let full = maximum_record(base.clone(), &registry, MAX_ENVELOPE_BYTES);
        let ordinary =
            maximum_record(LayoutEnvelope::new(Layout::empty(WorkspaceId::new("ordinary.workspace").unwrap())), &registry, MAX_ENVELOPE_BYTES);
        let rejected = [255, 0, 128, 0, 1];
        let tail = maximum_record(base, &registry, MAX_ENVELOPE_BYTES - rejected.len());
        let mut catalogue = InMemoryCatalogue::default();
        for index in 0..14 {
            catalogue.register_builtin(&format!("Missing-{index}"), &full, &registry).unwrap();
        }
        catalogue.register_builtin("Ordinary", &ordinary, &registry).unwrap();
        catalogue.remember(&tail, Some(&rejected), &registry).unwrap();
        let snapshot = catalogue.state().export().unwrap();
        assert_eq!(
            14 * encode(&full, &registry).unwrap().len()
                + encode(&ordinary, &registry).unwrap().len()
                + encode(&tail, &registry).unwrap().len()
                + rejected.len(),
            MAX_CATALOGUE_BYTES
        );
        let absent_state = CatalogueState::hydrate(&snapshot, &absent).unwrap();
        assert_eq!(absent_state, catalogue.state().clone());
        assert_eq!(absent_state.export().unwrap(), snapshot);
        assert_eq!(absent_state.preset("Ordinary").unwrap().project_envelope(&absent, &ordinary.layout.workspace).unwrap(), ordinary);
        assert_eq!(absent_state.remembered(&tail.layout.workspace).unwrap().rejected(), Some(rejected.as_slice()));
        let grown = CatalogueState::hydrate(&absent_state.export().unwrap(), &registry).unwrap();
        assert_eq!(grown.export().unwrap(), snapshot);
        assert_eq!(grown.preset("Missing-0").unwrap().envelope(&registry, &full.layout.workspace).unwrap(), full);
        let mut hydrated = InMemoryCatalogue::from_snapshot(&snapshot, &absent).unwrap();
        let before = hydrated.state().clone();
        assert_eq!(
            hydrated.register_builtin("Overflow", &LayoutEnvelope::new(Layout::empty(ordinary.layout.workspace)), &absent).unwrap_err().rule,
            Rule::CollectionLimit
        );
        assert_eq!(hydrated.state(), &before);
    }

    #[test]
    fn optional_peer_raw_future_fields_survive_exact_hydration_quarantine_and_reappearance() {
        let (registry, mut envelope) = registered_extension_record();
        // A future writer may also carry opaque keys in the saved typed records.
        // Keep a separate existing opaque sidecar to exercise both storage paths.
        let draft = envelope.retained_data.remove(&InstanceId::new("extension").unwrap()).unwrap();
        envelope.retained_data.insert(InstanceId::new("unrelated-opaque").unwrap(), draft);
        let mut raw = serde_json::to_value(&envelope).unwrap();
        raw["layout"]["instances"][1]["future_editor"] = serde_json::json!({"draft":"original raw draft","selection":[3,9]});
        raw["layout"]["sections"][0]["future_surface"] = serde_json::json!({"raw":"keep raw section intent"});
        let bytes = serde_json::to_vec(&raw).unwrap();
        let rejected = [0, 255, 128, 0];
        let saved = snapshot(
            &[("Builtin", &bytes, None)],
            &[("User", &bytes, Some(&rejected))],
            &[("fixture.workspace", "ui.panel", "Builtin")],
            &[("fixture.workspace", &bytes, Some(&rejected))],
        );
        let original = CatalogueState::hydrate(&saved, &registry).unwrap().export().unwrap();
        let absent = without_kind(&registry, "extension.a");
        let hydrated = CatalogueState::hydrate(&original, &absent).unwrap();
        assert_eq!(hydrated.export().unwrap(), original);
        for record in
            [hydrated.preset("Builtin").unwrap(), hydrated.preset("User").unwrap(), hydrated.remembered(&envelope.layout.workspace).unwrap()]
        {
            assert_eq!(record.bytes(), bytes.as_slice());
            let mut staged = record.project_envelope(&absent, &envelope.layout.workspace).unwrap();
            assert_eq!(staged.quarantine[0].instance_data["future_editor"], raw["layout"]["instances"][1]["future_editor"]);
            assert_eq!(staged.section_snapshots[&SectionId::new("group").unwrap()].data["future_surface"]["raw"], "keep raw section intent");
            restore_quarantine(&mut staged, &registry).unwrap();
            assert_same_placement(&staged.layout, &envelope.layout);
            assert_eq!(
                staged.retained_data[&InstanceId::new("extension").unwrap()]["instance"]["future_editor"],
                raw["layout"]["instances"][1]["future_editor"]
            );
            assert_eq!(
                staged.retained_data[&InstanceId::new("unrelated-opaque").unwrap()],
                envelope.retained_data[&InstanceId::new("unrelated-opaque").unwrap()]
            );
        }
        let grown = CatalogueState::hydrate(&hydrated.export().unwrap(), &registry).unwrap();
        assert_eq!(grown.export().unwrap(), original);
        assert_eq!(grown.preset("User").unwrap().rejected(), Some(rejected.as_slice()));
    }

    #[test]
    fn missing_peer_does_not_hide_corrupt_structure_or_known_member_constraints() {
        let (registry, envelope) = registered_extension_record();
        let absent = without_kind(&registry, "extension.a");
        let valid = serde_json::to_value(&envelope).unwrap();
        let mut hostile = Vec::new();
        let mut value = valid.clone();
        value["layout"]["instances"][1]["memory"]["size"]["width"] = serde_json::json!(0);
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["instances"][1]["memory"]["tab_index"] = serde_json::json!(MAX_TABS);
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["sections"][0]["active"] = serde_json::json!("ghost");
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["instances"][1]["id"] = serde_json::json!("ordinary");
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["instances"][0]["memory"]["size"]["width"] = serde_json::json!(1);
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["sections"][0]["size"]["width"] = serde_json::json!(1);
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["instances"][0]["kind"] = serde_json::json!("fixture.application");
        hostile.push(value); // Repeats known single-instance kind.
        let mut value = valid.clone();
        value["layout"]["sections"][0]["tabs"] = serde_json::json!(vec!["ordinary"; MAX_TABS + 1]);
        hostile.push(value);
        let mut value = valid.clone();
        value["layout"]["instances"][0]["requested_open"] = serde_json::json!(false);
        value["layout"]["sections"][0]["tabs"] = serde_json::json!(["extension"]);
        for section in value["layout"]["sections"].as_array_mut().unwrap() {
            section["location"] = serde_json::json!({"type":"window","host":"same-host","origin":{"x":0,"y":0},
                "return_to":{"zone":"left","index":0,"size":{"width":280,"height":240},"collapsed":false}});
        }
        hostile.push(value); // Duplicate native host, including an entirely absent-kind section.
        let mut deep = serde_json::json!(null);
        for _ in 0..34 {
            deep = serde_json::json!({"future":deep});
        }
        let mut value = valid.clone();
        value["retained_data"]["future"] = deep;
        hostile.push(value);
        for value in hostile {
            let bytes = serde_json::to_vec(&value).unwrap();
            let snapshot = snapshot(&[], &[("Saved", &bytes, None)], &[], &[]);
            assert!(CatalogueState::hydrate(&snapshot, &absent).is_err());
            assert!(project_retained_record(&bytes, &absent, &envelope.layout.workspace).is_err());
        }
        // A known member's no-tab flag applies even when its only peer is absent.
        let mut no_tabs = PanelRegistry::default();
        for spec in absent.specs() {
            let mut spec = spec.clone();
            if spec.kind.0 == "fixture.log" {
                spec.features.tabbable = false;
            }
            no_tabs.register(spec).unwrap();
        }
        let bytes = encode(&envelope, &registry).unwrap();
        assert_eq!(CatalogueState::hydrate(&snapshot(&[], &[("Saved", &bytes, None)], &[], &[]), &no_tabs).unwrap_err().rule, Rule::FeatureDisabled);
        // An absent-only outer section still precedes a known pinned section.
        let mut pin_service =
            LayoutService::new(registry.clone(), Layout::empty(envelope.layout.workspace.clone()), HostCapabilities::default()).unwrap();
        pin_service
            .dispatch(transaction(vec![
                fixture_open("fixture.pin", "pin", "pin-section", Zone::Left, 0).unwrap(),
                fixture_open("extension.a", "missing", "missing-section", Zone::Right, 1).unwrap(),
            ]))
            .unwrap();
        let mut wire = serde_json::to_value(LayoutEnvelope::new(pin_service.layout().clone())).unwrap();
        wire["layout"]["sections"].as_array_mut().unwrap().reverse();
        wire["layout"]["sections"][0]["location"] = serde_json::json!({"type":"dock","zone":"left"});
        let bytes = serde_json::to_vec(&wire).unwrap();
        assert_eq!(CatalogueState::hydrate(&snapshot(&[], &[("Pinned", &bytes, None)], &[], &[]), &absent).unwrap_err().rule, Rule::PinOrder);
    }

    #[test]
    fn accepted_maximum_records_roundtrip_without_materializing_order_or_projection() {
        for with_snapshot in [false, true] {
            let (registry, envelope) = unmaterialized_section_record(with_snapshot);
            let envelope = maximum_record(envelope, &registry, MAX_ENVELOPE_BYTES);
            let original = encode(&envelope, &registry).unwrap();
            let rejected = [0, 255, 0, 127, 128, 42, 1];
            let mut catalogue = InMemoryCatalogue::default();
            catalogue.register_builtin("Maximum", &envelope, &registry).unwrap();
            catalogue.remember(&envelope, Some(&rejected), &registry).unwrap();
            let snapshot = catalogue.state().export().unwrap();
            let hydrated = InMemoryCatalogue::from_snapshot(&snapshot, &registry).unwrap();
            assert_eq!(hydrated.state(), catalogue.state());
            assert_eq!(hydrated.state().export().unwrap(), snapshot, "same-registry hydration retains the original records byte for byte");
            for record in [hydrated.state().preset("Maximum").unwrap(), hydrated.state().remembered(&envelope.layout.workspace).unwrap()] {
                assert_eq!(record.bytes(), original.as_slice());
                let loaded = record.envelope(&registry, &envelope.layout.workspace).unwrap();
                assert_eq!(loaded, envelope);
                assert!(loaded.section_order.is_empty());
                assert!(loaded.section_snapshots.values().all(|snapshot| snapshot.projected.is_none()));
            }
            assert_eq!(hydrated.state().remembered(&envelope.layout.workspace).unwrap().rejected(), Some(rejected.as_slice()));
            let mut oversized = envelope.clone();
            oversized.retained_data.insert(
                InstanceId::new("padding").unwrap(),
                serde_json::json!(format!("{}x", envelope.retained_data[&InstanceId::new("padding").unwrap()].as_str().unwrap())),
            );
            let before = catalogue.state().clone();
            assert_eq!(catalogue.register_builtin("Oversized", &oversized, &registry).unwrap_err().rule, Rule::CollectionLimit);
            assert_eq!(catalogue.state(), &before);
            let mut corrupt = snapshot;
            corrupt.push(0);
            assert_eq!(CatalogueState::hydrate(&corrupt, &registry).unwrap_err().rule, Rule::Schema);
        }
    }

    #[test]
    fn accepted_aggregate_limit_roundtrips_exact_records_and_rejected_binary_bytes() {
        let (registry, base) = unmaterialized_section_record(true);
        let full = maximum_record(base.clone(), &registry, MAX_ENVELOPE_BYTES);
        let full_bytes = encode(&full, &registry).unwrap();
        let rejected = [0, 255, 0, 127, 128, 42, 1];
        let tail = maximum_record(base, &registry, MAX_ENVELOPE_BYTES - rejected.len());
        let tail_bytes = encode(&tail, &registry).unwrap();
        assert_eq!(15 * full_bytes.len() + tail_bytes.len() + rejected.len(), MAX_CATALOGUE_BYTES);
        let mut catalogue = InMemoryCatalogue::default();
        for index in 0..15 {
            catalogue.register_builtin(&format!("Maximum-{index}"), &full, &registry).unwrap();
        }
        catalogue.remember(&tail, Some(&rejected), &registry).unwrap();
        let snapshot = catalogue.state().export().unwrap();
        let hydrated = CatalogueState::hydrate(&snapshot, &registry).unwrap();
        assert_eq!(hydrated, catalogue.state().clone());
        assert_eq!(hydrated.export().unwrap(), snapshot, "no per-record expansion may silently exceed the aggregate cap");
        for index in 0..15 {
            let record = hydrated.preset(&format!("Maximum-{index}")).unwrap();
            assert_eq!(record.bytes(), full_bytes.as_slice());
            assert_eq!(record.envelope(&registry, &full.layout.workspace).unwrap(), full);
        }
        let remembered = hydrated.remembered(&tail.layout.workspace).unwrap();
        assert_eq!(remembered.bytes(), tail_bytes.as_slice());
        assert_eq!(remembered.rejected(), Some(rejected.as_slice()));
        let before = catalogue.state().clone();
        let empty = LayoutEnvelope::new(Layout::empty(full.layout.workspace.clone()));
        assert_eq!(catalogue.register_builtin("One beyond aggregate budget", &empty, &registry).unwrap_err().rule, Rule::CollectionLimit);
        assert_eq!(catalogue.state(), &before);
    }

    #[test]
    fn maximum_retained_records_defer_live_refresh_and_reappearance_atomically() {
        let mut service = fixture_service(false).unwrap();
        service.dispatch(transaction(vec![fixture_open("fixture.log", "future", "future-section", Zone::Left, 0).unwrap()])).unwrap();
        let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
        raw["layout"]["instances"][0]["kind"] = serde_json::json!("extension.future");
        raw["layout"]["instances"][0]["future_draft"] = serde_json::json!("pending conversation");
        let absent = synthetic_registry().unwrap();
        let fallback = Layout::empty(service.layout().workspace.clone());
        let mut envelope = decode(&serde_json::to_vec(&raw).unwrap(), &absent, &fallback).unwrap().envelope;
        envelope.section_order.clear();
        let mut grown = absent;
        let mut spec = grown.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new("extension.future").unwrap();
        grown.register(spec).unwrap();
        let envelope = maximum_record(envelope, &grown, MAX_ENVELOPE_BYTES);
        let rejected = [255, 0, 128, 0, 1];
        let mut catalogue = InMemoryCatalogue::default();
        catalogue.remember(&envelope, Some(&rejected), &grown).unwrap();
        let snapshot = catalogue.state().export().unwrap();
        let hydrated = CatalogueState::hydrate(&snapshot, &grown).unwrap();
        let loaded = hydrated.remembered(&fallback.workspace).unwrap().envelope(&grown, &fallback.workspace).unwrap();
        assert_eq!(loaded, envelope, "registered quarantine is not a read-time restoration");
        assert_eq!(loaded.quarantine.first().unwrap().instance_data["future_draft"], "pending conversation");
        assert_eq!(with_live_layout(&loaded, loaded.layout.clone(), &grown).unwrap_err().rule, Rule::CollectionLimit);
        let mut candidate = loaded.clone();
        assert_eq!(restore_quarantine(&mut candidate, &grown).unwrap_err().rule, Rule::CollectionLimit);
        assert_eq!(candidate, loaded, "failed materialization keeps all retryable intent and opaque records");
        assert_eq!(hydrated.export().unwrap(), snapshot);
        assert_eq!(hydrated.remembered(&fallback.workspace).unwrap().rejected(), Some(rejected.as_slice()));
    }

    #[test]
    fn external_storage_roundtrip_restores_mutable_users_full_records_and_workspaces() {
        let mut backend = fixture_command_backend().unwrap();
        backend.service.dispatch(transaction(vec![fixture_open("fixture.log", "log", "logs", Zone::Left, 0).unwrap()])).unwrap();
        let mut envelope = LayoutEnvelope::new(backend.service.layout().clone());
        envelope.retained_data.insert(InstanceId::new("opaque").unwrap(), serde_json::json!({"future":{"draft":"keep"}}));
        backend.catalogue.remember(&envelope, Some(&[0, 255, 1, 0]), backend.service.registry()).unwrap();
        backend.execute("layout.save", &serde_json::json!({"name":"User saved"})).unwrap();
        let snapshot = backend.catalogue.state().export().unwrap();
        let users: Vec<_> = backend.catalogue.state().user_presets().map(|(name, _)| name).collect();
        assert_eq!(users, vec!["User saved"]);
        // No executor is called while the independent consumer reconstructs state.
        let registry = synthetic_registry().unwrap();
        let state = CatalogueState::hydrate(&snapshot, &registry).unwrap();
        assert_eq!(state.summaries(), backend.catalogue.state().summaries());
        assert_eq!(state.user_presets().map(|(name, _)| name).collect::<Vec<_>>(), users);
        let saved = state.preset("User saved").unwrap();
        assert_eq!(saved.rejected(), Some([0, 255, 1, 0].as_slice()));
        let restored = saved.envelope(&registry, &envelope.layout.workspace).unwrap();
        assert_eq!(restored.retained_data, envelope.retained_data);
        assert_eq!(state.remembered(&envelope.layout.workspace).unwrap().rejected(), Some([0, 255, 1, 0].as_slice()));
        let catalogue = InMemoryCatalogue::from_snapshot(&snapshot, &registry).unwrap();
        assert_eq!(catalogue.state().user_count(), 1);
        let mut reloaded = InMemoryPanelBackend::new(fixture_service(false).unwrap(), catalogue);
        assert_eq!(reloaded.execute("layout.save", &serde_json::json!({"name":"User saved"})).unwrap_err().rule, Rule::DuplicateId);
        reloaded.execute("layout.save", &serde_json::json!({"name":"User saved","overwrite":true})).unwrap();
        assert_eq!(reloaded.catalogue.state().user_count(), 1);
        assert_eq!(
            reloaded.execute("layout.save", &serde_json::json!({"name":"Fixture default","overwrite":true})).unwrap_err().rule,
            Rule::Capability
        );
    }

    type Entry<'a> = (&'a str, &'a [u8], Option<&'a [u8]>);
    fn length(out: &mut Vec<u8>, count: usize) {
        out.extend_from_slice(&(count as u32).to_le_bytes());
    }
    fn bytes(out: &mut Vec<u8>, value: &[u8]) {
        length(out, value.len());
        out.extend_from_slice(value);
    }
    fn entries(out: &mut Vec<u8>, records: &[Entry<'_>]) {
        length(out, records.len());
        for (name, envelope, rejected) in records {
            bytes(out, name.as_bytes());
            bytes(out, envelope);
            match rejected {
                Some(rejected) => {
                    out.push(1);
                    bytes(out, rejected);
                }
                None => out.push(0),
            }
        }
    }
    fn snapshot(builtins: &[Entry<'_>], users: &[Entry<'_>], workspaces: &[(&str, &str, &str)], remembered: &[Entry<'_>]) -> Vec<u8> {
        let mut out = b"SCPCAT01".to_vec();
        entries(&mut out, builtins);
        entries(&mut out, users);
        length(&mut out, workspaces.len());
        for (id, title, default) in workspaces {
            bytes(&mut out, id.as_bytes());
            bytes(&mut out, title.as_bytes());
            bytes(&mut out, default.as_bytes());
        }
        entries(&mut out, remembered);
        out
    }

    #[test]
    fn catalogue_hydration_rejects_duplicate_names_workspaces_and_corrupt_framing_before_publication() {
        let registry = synthetic_registry().unwrap();
        let envelope = LayoutEnvelope::new(fixture_service(false).unwrap().layout().clone());
        let raw = encode(&envelope, &registry).unwrap();
        let record = ("Saved", raw.as_slice(), None);
        for duplicate in [
            snapshot(&[], &[record, record], &[], &[]),
            snapshot(&[record], &[record], &[], &[]),
            snapshot(&[record], &[], &[("a", "ui.panel", "Saved"), ("a", "ui.panel", "Saved")], &[]),
            snapshot(&[], &[], &[], &[("fixture.workspace", raw.as_slice(), None), ("fixture.workspace", raw.as_slice(), None)]),
        ] {
            assert_eq!(CatalogueState::hydrate(&duplicate, &registry).unwrap_err().rule, Rule::DuplicateId);
        }
        let valid = snapshot(&[], &[record], &[], &[]);
        for size in [0, 7, 12, valid.len() - 1] {
            assert!(CatalogueState::hydrate(&valid[..size], &registry).is_err());
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(CatalogueState::hydrate(&trailing, &registry).is_err());
        let mut future = valid;
        future[7] = b'2';
        assert_eq!(CatalogueState::hydrate(&future, &registry).unwrap_err().rule, Rule::Version);
        assert!(CatalogueState::hydrate(&snapshot(&[], &[("Saved", b"corrupt", None)], &[], &[]), &registry).is_err());
        assert!(CatalogueState::hydrate(&snapshot(&[], &[], &[], &[("wrong-workspace", raw.as_slice(), None)]), &registry).is_err());
        let live = fixture_command_backend().unwrap();
        let before = live.catalogue.state().clone();
        let layout = live.service.inspect();
        assert!(InMemoryCatalogue::from_snapshot(b"corrupt", &registry).is_err());
        assert_eq!(live.catalogue.state(), &before);
        assert_eq!(live.service.inspect(), layout);
    }

    #[test]
    fn catalogue_hydration_bounds_counts_strings_depth_nodes_recovery_and_total_bytes() {
        let registry = synthetic_registry().unwrap();
        let envelope = LayoutEnvelope::new(fixture_service(false).unwrap().layout().clone());
        let raw = encode(&envelope, &registry).unwrap();
        let mut count = b"SCPCAT01".to_vec();
        length(&mut count, 65);
        assert_eq!(CatalogueState::hydrate(&count, &registry).unwrap_err().rule, Rule::CollectionLimit);
        for name in ["".to_string(), " not-normalised ".into(), "bad\n".into(), "x".repeat(257)] {
            assert!(CatalogueState::hydrate(&snapshot(&[], &[(&name, &raw, None)], &[], &[]), &registry).is_err());
        }
        let rejected = vec![0; MAX_ENVELOPE_BYTES + 1];
        assert_eq!(
            CatalogueState::hydrate(&snapshot(&[], &[("Saved", &raw, Some(&rejected))], &[], &[]), &registry).unwrap_err().rule,
            Rule::CollectionLimit
        );
        let mut value = serde_json::to_value(&envelope).unwrap();
        let mut deep = serde_json::json!(null);
        for _ in 0..34 {
            deep = serde_json::json!({"future":deep});
        }
        for hostile in [deep, serde_json::json!(vec![serde_json::Value::Null; 8193]), serde_json::json!("x".repeat(MAX_ENVELOPE_BYTES))] {
            value["retained_data"]["future"] = hostile;
            let hostile = serde_json::to_vec(&value).unwrap();
            assert!(CatalogueState::hydrate(&snapshot(&[], &[("Saved", &hostile, None)], &[], &[]), &registry).is_err());
        }
        assert_eq!(CatalogueState::hydrate(&vec![0; MAX_CATALOGUE_SNAPSHOT_BYTES + 1], &registry).unwrap_err().rule, Rule::CollectionLimit);
    }

    #[test]
    fn catalogue_hydration_rejects_aggregate_payload_metadata_and_duplicate_envelope_identities() {
        let registry = synthetic_registry().unwrap();
        let envelope = LayoutEnvelope::new(fixture_service(false).unwrap().layout().clone());
        let raw = encode(&envelope, &registry).unwrap();
        let mut large = envelope.clone();
        large.retained_data.insert(InstanceId::new("future").unwrap(), serde_json::json!({"opaque":"x".repeat(200_000)}));
        let large = encode(&large, &registry).unwrap();
        let names: Vec<_> = (0..21).map(|i| format!("user-{i}")).collect();
        let users: Vec<_> = names.iter().map(|name| (name.as_str(), large.as_slice(), None)).collect();
        let oversized = snapshot(&[], &users, &[], &[]);
        assert!(oversized.len() < MAX_CATALOGUE_SNAPSHOT_BYTES, "payload cap is independent of framing cap");
        assert_eq!(CatalogueState::hydrate(&oversized, &registry).unwrap_err().rule, Rule::CollectionLimit);
        let names: Vec<_> = (0..64).map(|i| format!("{}{:02}", "🦀".repeat(62), i)).collect();
        let builtins: Vec<_> = names.iter().map(|name| (name.as_str(), raw.as_slice(), None)).collect();
        let default = names.first().unwrap().as_str();
        let oversized = snapshot(&builtins, &[], &[("first", "ui.panel", default), ("second", "ui.panel", default)], &[]);
        assert_eq!(CatalogueState::hydrate(&oversized, &registry).unwrap_err().rule, Rule::CollectionLimit);
        let mut service = fixture_service(false).unwrap();
        service.dispatch(transaction(vec![fixture_open("fixture.log", "same", "group", Zone::Left, 0).unwrap()])).unwrap();
        let mut duplicate = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
        let repeated = duplicate["layout"]["instances"][0].clone();
        duplicate["layout"]["instances"].as_array_mut().unwrap().push(repeated);
        let duplicate = serde_json::to_vec(&duplicate).unwrap();
        assert!(CatalogueState::hydrate(&snapshot(&[], &[("Saved", &duplicate, None)], &[], &[]), &registry).is_err());
    }
}

#[test]
fn whole_section_ranks_include_known_survivors_and_preserve_intervening_live_order() {
    let fallback = persistence_fallback();
    let raw = future_sections();
    let bytes = serde_json::to_vec(&raw).unwrap();
    let all = future_registry(&["a", "b", "c"]);
    let mut mixed = decode(&bytes, &future_registry(&["b"]), &fallback).unwrap().envelope;
    assert_eq!(section_ids(&mixed), vec![section("s1")]);
    assert!(!mixed.section_snapshots.contains_key(&section("s1")));
    restore_quarantine(&mut mixed, &future_registry(&["b", "c"])).unwrap();
    assert_eq!(section_ids(&mixed), vec![section("s1"), section("s2")]);
    restore_quarantine(&mut mixed, &all).unwrap();
    assert_eq!(section_ids(&mixed), vec![section("s0"), section("s1"), section("s2")]);
    let a_c = future_registry(&["a", "c"]);
    let mut edited = decode(&bytes, &a_c, &fallback).unwrap().envelope;
    edited.layout.sections.reverse(); // Visible saved user order is now s2, s0.
    let reloaded = decode(&encode(&edited, &a_c).unwrap(), &a_c, &fallback).unwrap();
    assert!(reloaded.notice.is_none());
    edited = reloaded.envelope;
    assert_eq!(edited.section_order, vec![section("s2"), section("s1"), section("s0")]);
    restore_quarantine(&mut edited, &all).unwrap();
    assert_eq!(section_ids(&edited), vec![section("s2"), section("s1"), section("s0")]);
}

#[test]
fn new_visible_sections_anchor_to_live_neighbours_while_old_sections_are_dormant() {
    let fallback = persistence_fallback();
    let b = future_registry(&["b"]);
    let all = future_registry(&["a", "b", "c"]);
    let mut envelope = decode(&serde_json::to_vec(&future_sections()).unwrap(), &b, &fallback).unwrap().envelope;
    let mut live = LayoutService::new(b, envelope.layout.clone(), HostCapabilities::default()).unwrap();
    live.dispatch(transaction(vec![fixture_open("fixture.log", "new", "new-section", Zone::Right, 0).unwrap()])).unwrap();
    envelope.layout = live.layout().clone();
    restore_quarantine(&mut envelope, &all).unwrap();
    assert_eq!(section_ids(&envelope), vec![section("s0"), section("new-section"), section("s1"), section("s2")]);
    assert!(envelope.quarantine.is_empty());
    encode(&envelope, &all).unwrap();
}

#[test]
fn a_new_whole_section_episode_uses_the_latest_saved_complete_section_order() {
    let fallback = persistence_fallback();
    let all = future_registry(&["a", "b", "c"]);
    let none = future_registry(&[]);
    let mut envelope = decode(&serde_json::to_vec(&future_sections()).unwrap(), &none, &fallback).unwrap().envelope;
    restore_quarantine(&mut envelope, &all).unwrap();
    envelope.layout.sections.rotate_right(1);
    assert_eq!(section_ids(&envelope), vec![section("s2"), section("s0"), section("s1")]);
    let reloaded = decode(&encode(&envelope, &all).unwrap(), &none, &fallback).unwrap();
    assert!(reloaded.notice.is_none());
    envelope = reloaded.envelope;
    assert_eq!(envelope.section_order, vec![section("s2"), section("s0"), section("s1")]);
    restore_quarantine(&mut envelope, &all).unwrap();
    assert_eq!(section_ids(&envelope), vec![section("s2"), section("s0"), section("s1")]);
    assert_eq!(envelope.section_snapshots[&section("s0")].data["future_surface"]["keep"], "s0");
}

fn future_pair(active: &str) -> serde_json::Value {
    let mut raw = future_group();
    raw["layout"]["instances"].as_array_mut().unwrap().retain(|instance| instance["id"] != "c");
    raw["layout"]["sections"][0]["tabs"] = serde_json::json!(["a", "b"]);
    raw["layout"]["sections"][0]["active"] = serde_json::json!(active);
    raw["layout"]["sections"][0]["future_metadata"] = serde_json::json!({"keep":"opaque section data"});
    raw
}

#[test]
fn a_new_quarantine_episode_refreshes_saved_tab_order_activation_and_outer_semantics() {
    let fallback = persistence_fallback();
    let a = future_registry(&["a"]);
    let all = future_registry(&["a", "b"]);
    for initial_active in ["a", "b"] {
        let mut envelope = decode(&serde_json::to_vec(&future_pair(initial_active)).unwrap(), &a, &fallback).unwrap().envelope;
        restore_quarantine(&mut envelope, &all).unwrap();
        let group = envelope.layout.sections.first_mut().unwrap();
        group.tabs = vec![id("b"), id("a")];
        group.active = id("b");
        group.size = size(400.0, 300.0);
        group.collapsed = true;
        group.location = SectionLocation::Dock { zone: Zone::Right };
        let mut saved: serde_json::Value = serde_json::from_slice(&encode(&envelope, &all).unwrap()).unwrap();
        saved["layout"]["sections"][0]["new_future_field"] = serde_json::json!("latest opaque intent");
        let loaded = decode(&serde_json::to_vec(&saved).unwrap(), &a, &fallback).unwrap();
        assert!(loaded.notice.is_none());
        envelope = loaded.envelope;
        let memory = &envelope.section_snapshots[&section("group")];
        assert_eq!(memory.data["tabs"], serde_json::json!(["b", "a"]));
        assert_eq!(memory.data["active"], "b");
        assert_eq!(envelope.layout.sections.first().unwrap().active, id("a")); // Generated fallback, not a user edit.
        let loaded = decode(&encode(&envelope, &a).unwrap(), &a, &fallback).unwrap();
        assert!(loaded.notice.is_none());
        envelope = loaded.envelope;
        restore_quarantine(&mut envelope, &all).unwrap();
        let group = envelope.layout.sections.first().unwrap();
        assert_eq!(group.tabs, vec![id("b"), id("a")]);
        assert_eq!(group.active, id("b"));
        assert_eq!(group.size, size(400.0, 300.0));
        assert!(group.collapsed);
        assert_eq!(group.location.zone(), Zone::Right);
        let memory = &envelope.section_snapshots[&section("group")];
        assert_eq!(memory.data["future_metadata"]["keep"], "opaque section data");
        assert_eq!(memory.data["new_future_field"], "latest opaque intent");
        assert_eq!(envelope.retained_data[&id("b")]["instance"]["future_draft"], "draft-b");
        encode(&envelope, &all).unwrap();
    }
}

#[test]
fn mixed_partial_episodes_keep_dormant_members_and_latest_visible_user_choices() {
    let fallback = persistence_fallback();
    let a = future_registry(&["a"]);
    let a_b = future_registry(&["a", "b"]);
    let all = future_registry(&["a", "b", "c"]);
    let mut raw = future_group();
    raw["layout"]["sections"][0]["tabs"] = serde_json::json!(["a", "b", "c"]);
    raw["layout"]["sections"][0]["future_metadata"] = serde_json::json!({"opaque":true});
    let mut envelope = decode(&serde_json::to_vec(&raw).unwrap(), &a_b, &fallback).unwrap().envelope;
    assert_eq!(envelope.layout.sections.first().unwrap().tabs, vec![id("a"), id("b")]);
    assert_eq!(envelope.layout.sections.first().unwrap().active, id("a"));
    let group = envelope.layout.sections.first_mut().unwrap();
    group.tabs.reverse();
    group.active = id("b");
    let saved = encode(&envelope, &a_b).unwrap();
    let loaded = decode(&saved, &a, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    envelope = loaded.envelope;
    assert_eq!(envelope.quarantine.len(), 2);
    assert_eq!(envelope.section_snapshots[&section("group")].data["tabs"], serde_json::json!(["b", "a", "c"]));
    assert_eq!(envelope.section_snapshots[&section("group")].data["active"], "b");
    restore_quarantine(&mut envelope, &a_b).unwrap();
    assert_eq!(envelope.quarantine.len(), 1);
    assert_eq!(envelope.layout.sections.first().unwrap().tabs, vec![id("b"), id("a")]);
    assert_eq!(envelope.layout.sections.first().unwrap().active, id("b"));
    let mut unchanged = envelope.clone();
    restore_quarantine(&mut unchanged, &all).unwrap();
    assert_eq!(unchanged.layout.sections.first().unwrap().tabs, vec![id("b"), id("a"), id("c")]);
    assert_eq!(unchanged.layout.sections.first().unwrap().active, id("b"));
    // A direct live edit between restoration stages is authoritative too.
    let group = envelope.layout.sections.first_mut().unwrap();
    group.tabs.reverse();
    group.active = id("a");
    restore_quarantine(&mut envelope, &all).unwrap();
    assert_eq!(envelope.layout.sections.first().unwrap().tabs, vec![id("a"), id("b"), id("c")]);
    assert_eq!(envelope.layout.sections.first().unwrap().active, id("a"));
    assert_eq!(envelope.section_snapshots[&section("group")].data["future_metadata"]["opaque"], true);
    encode(&envelope, &all).unwrap();
}

#[test]
fn unchanged_partial_save_does_not_replace_pending_activation_with_a_fallback() {
    let fallback = persistence_fallback();
    let a_b = future_registry(&["a", "b"]);
    let all = future_registry(&["a", "b", "c"]);
    let mut raw = future_group();
    raw["layout"]["sections"][0]["tabs"] = serde_json::json!(["a", "b", "c"]);
    let mut envelope = decode(&serde_json::to_vec(&raw).unwrap(), &a_b, &fallback).unwrap().envelope;
    for _ in 0..2 {
        let loaded = decode(&encode(&envelope, &a_b).unwrap(), &a_b, &fallback).unwrap();
        assert!(loaded.notice.is_none());
        envelope = loaded.envelope;
    }
    assert_eq!(envelope.section_snapshots[&section("group")].data["active"], "c");
    restore_quarantine(&mut envelope, &all).unwrap();
    assert_eq!(envelope.layout.sections.first().unwrap().active, id("c"));
}

#[test]
fn earlier_v1_rank_metadata_and_new_order_projection_bounds_are_supported() {
    let fallback = persistence_fallback();
    let none = future_registry(&[]);
    let all = future_registry(&["a", "b", "c"]);
    let envelope = decode(&serde_json::to_vec(&future_sections()).unwrap(), &none, &fallback).unwrap().envelope;
    let mut earlier = serde_json::to_value(&envelope).unwrap();
    earlier.as_object_mut().unwrap().remove("section_order");
    for snapshot in earlier["section_snapshots"].as_object_mut().unwrap().values_mut() {
        snapshot.as_object_mut().unwrap().remove("projected");
    }
    let loaded = decode(&serde_json::to_vec(&earlier).unwrap(), &none, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    let mut restored = loaded.envelope;
    restore_quarantine(&mut restored, &all).unwrap();
    assert_eq!(section_ids(&restored), vec![section("s0"), section("s1"), section("s2")]);
    let mut duplicate = envelope.clone();
    duplicate.section_order = vec![section("s0"), section("s0")];
    assert_eq!(encode(&duplicate, &none).unwrap_err().rule, Rule::DuplicateId);
    let mut excessive = envelope;
    excessive.section_order = (0..=MAX_SECTIONS).map(|index| section(&format!("s-{index}"))).collect();
    assert_eq!(encode(&excessive, &none).unwrap_err().rule, Rule::CollectionLimit);
    let snapshot = restored.section_snapshots.get_mut(&section("s0")).unwrap();
    snapshot.projected.as_mut().unwrap().tabs = vec![id("a"); MAX_TABS + 1];
    assert_eq!(encode(&restored, &all).unwrap_err().rule, Rule::CollectionLimit);
}
