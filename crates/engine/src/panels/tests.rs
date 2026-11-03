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
    let undo = session.undo.len();
    let recomputed = session.model.recomputed;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    for width in [280.0, 350.0, 500.0] {
        service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(width, 240.0) }])).unwrap();
        resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    }
    assert!(!session.is_dirty());
    assert_eq!(session.revision, revision);
    assert_eq!(session.undo.len(), undo);
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
