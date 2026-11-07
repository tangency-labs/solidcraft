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

fn floating_group_service() -> LayoutService {
    let mut service = fixture_service(false).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.log", "one", "floating-group", Zone::Floating, 0).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("two"),
                destination: Some(Destination::JoinTab { section: section("floating-group"), index: 1 }),
            },
            Mutation::Activate { instance: id("two") },
        ]))
        .unwrap();
    service
}

#[test]
fn floating_rect_prepares_atomically_preserving_group_focus_peek_and_other_sections() {
    let mut service = floating_group_service();
    open(&mut service, "three", "other-section");
    open(&mut service, "four", "peek-section");
    service
        .dispatch(transaction(vec![
            Mutation::Focus { instance: id("two") },
            Mutation::Collapse { section: section("peek-section"), collapsed: true },
            Mutation::Peek { instance: id("four") },
        ]))
        .unwrap();
    let before = service.inspect();
    let origin = Point { x: 520.0, y: 0.0 };
    let dimensions = size(250.0, 240.0);
    let action = LayoutAction::Apply {
        expected_revision: Some(before.revision),
        mutations: vec![Mutation::SetFloatingRect { section: section("floating-group"), origin, size: dimensions }],
    };
    assert_eq!(decode_action(&serde_json::to_vec(&action).unwrap()).unwrap(), action);
    let prepared = service.prepare(action.clone()).unwrap();
    assert_eq!(service.inspect(), before, "preparing a rect has no saved or interaction effects");
    assert!(prepared.effects().is_empty());
    let reply = service.commit(prepared).unwrap();
    assert_eq!(reply.revision, before.revision + 1);
    assert!(reply.effects.is_empty());
    assert_eq!(reply.interaction, before.interaction);
    let group = reply.layout.section(&section("floating-group")).unwrap();
    assert_eq!(group.id, section("floating-group"));
    assert_eq!(group.tabs, vec![id("one"), id("two")]);
    assert_eq!(group.active, id("two"));
    assert_eq!(group.location, SectionLocation::Floating { origin });
    assert_eq!(group.size, dimensions);
    assert!(!group.collapsed);
    assert_eq!(reply.layout.sections.len(), before.layout.sections.len());
    assert_eq!(reply.layout.instances.len(), before.layout.instances.len());
    for (index, instance) in [id("one"), id("two")].iter().enumerate() {
        let memory = &reply.layout.instance(instance).unwrap().memory;
        assert_eq!(memory.section, group.id);
        assert_eq!(memory.location, group.location);
        assert_eq!(memory.size, dimensions);
        assert_eq!(memory.section_index, 0);
        assert_eq!(memory.tab_index, index);
        assert!(!memory.collapsed);
    }
    for name in ["other-section", "peek-section"] {
        assert_eq!(reply.layout.section(&section(name)).unwrap(), before.layout.section(&section(name)).unwrap());
    }
    for name in ["three", "four"] {
        assert_eq!(reply.layout.instance(&id(name)).unwrap(), before.layout.instance(&id(name)).unwrap());
    }
    assert_eq!(service.dispatch(action).unwrap_err().rule, Rule::Revision);
    assert_eq!(service.inspect().revision, reply.revision);
    service
        .dispatch(transaction(vec![
            Mutation::Collapse { section: section("floating-group"), collapsed: true },
            Mutation::Peek { instance: id("two") },
        ]))
        .unwrap();
    let peek = service.interaction().clone();
    let reply = service
        .dispatch(transaction(vec![Mutation::SetFloatingRect {
            section: section("floating-group"),
            origin: Point { x: 521.0, y: 0.0 },
            size: dimensions,
        }]))
        .unwrap();
    assert_eq!(reply.interaction, peek);
    assert!(reply.effects.is_empty());
    assert!(reply.layout.section(&section("floating-group")).unwrap().collapsed);
    assert!(reply.layout.instance(&id("one")).unwrap().memory.collapsed);
    assert!(reply.layout.instance(&id("two")).unwrap().memory.collapsed);
    validate(service.layout(), service.registry()).unwrap();
}

#[test]
fn floating_rect_enforces_group_intersection_hostile_numbers_and_atomic_failure() {
    let mut registry = synthetic_registry().unwrap();
    let mut narrow = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    narrow.kind = PanelKind::new("fixture.rect-bounds").unwrap();
    narrow.min_size = size(200.0, 180.0);
    narrow.max_size = size(300.0, 260.0);
    registry.register(narrow).unwrap();
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.log", "one", "floating-group", Zone::Floating, 0).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.rect-bounds").unwrap(),
                instance: id("two"),
                destination: Some(Destination::JoinTab { section: section("floating-group"), index: 1 }),
            },
        ]))
        .unwrap();
    let origin = Point { x: 520.0, y: 0.0 };
    service.dispatch(transaction(vec![Mutation::SetFloatingRect { section: section("floating-group"), origin, size: size(250.0, 220.0) }])).unwrap();
    for dimensions in [size(199.0, 220.0), size(301.0, 220.0), size(250.0, 179.0), size(250.0, 261.0)] {
        reject(&mut service, vec![Mutation::SetFloatingRect { section: section("floating-group"), origin, size: dimensions }], Rule::Bounds);
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, MAX_LOGICAL + 1.0, -MAX_LOGICAL - 1.0] {
        for hostile_origin in [Point { x: bad, y: 0.0 }, Point { x: 0.0, y: bad }] {
            reject(
                &mut service,
                vec![Mutation::SetFloatingRect { section: section("floating-group"), origin: hostile_origin, size: size(250.0, 220.0) }],
                Rule::Bounds,
            );
        }
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, MAX_LOGICAL + 1.0] {
        for dimensions in [size(bad, 220.0), size(250.0, bad)] {
            reject(&mut service, vec![Mutation::SetFloatingRect { section: section("floating-group"), origin, size: dimensions }], Rule::Bounds);
        }
    }
    reject(
        &mut service,
        vec![Mutation::SetFloatingRect { section: section("floating-group"), origin: Point { x: MAX_LOGICAL, y: 0.0 }, size: size(250.0, 220.0) }],
        Rule::Bounds,
    );
    reject(&mut service, vec![Mutation::SetFloatingRect { section: SectionId("UPPER".into()), origin, size: size(250.0, 220.0) }], Rule::Id);
    reject(&mut service, vec![Mutation::SetFloatingRect { section: section("missing"), origin, size: size(250.0, 220.0) }], Rule::MissingTarget);
    reject(
        &mut service,
        vec![
            Mutation::SetFloatingRect { section: section("floating-group"), origin: Point { x: 10.0, y: 20.0 }, size: size(260.0, 230.0) },
            Mutation::SetSize { section: section("floating-group"), size: size(301.0, 230.0) },
        ],
        Rule::Bounds,
    );
    for boundary in [Point { x: MAX_LOGICAL - 250.0, y: MAX_LOGICAL - 220.0 }, Point { x: -MAX_LOGICAL, y: -MAX_LOGICAL }] {
        service
            .dispatch(transaction(vec![Mutation::SetFloatingRect { section: section("floating-group"), origin: boundary, size: size(250.0, 220.0) }]))
            .unwrap();
        assert_eq!(service.layout().section(&section("floating-group")).unwrap().location, SectionLocation::Floating { origin: boundary });
        validate(service.layout(), service.registry()).unwrap();
    }
}

#[test]
fn floating_rect_cannot_reparent_a_dock_or_bypass_native_close() {
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "docked");
    open(&mut service, "two", "native");
    let host = HostId::new("rect-native-host").unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("two"),
            destination: Destination::NewSection {
                section: section("native"),
                index: 1,
                location: SectionLocation::Window {
                    host: host.clone(),
                    origin: Point { x: 0.0, y: 0.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 1,
                        size: size(280.0, 240.0),
                        collapsed: false,
                        join: None,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 1.0,
                        preferred_extent: None,
                        extent_priority: 0,
                    },
                },
                size: size(280.0, 240.0),
            },
        }]))
        .unwrap();
    for target in ["docked", "native"] {
        reject(
            &mut service,
            vec![Mutation::SetFloatingRect { section: section(target), origin: Point { x: 20.0, y: 30.0 }, size: size(250.0, 220.0) }],
            Rule::Placement,
        );
    }
    assert!(
        matches!(&service.layout().section(&section("native")).unwrap().location, SectionLocation::Window { host: actual, .. } if *actual == host)
    );
}

#[test]
fn floating_rect_updates_each_tab_reopen_memory_without_section_identity_churn() {
    let mut service = floating_group_service();
    let origin = Point { x: 520.0, y: 0.0 };
    service.dispatch(transaction(vec![Mutation::SetFloatingRect { section: section("floating-group"), origin, size: size(250.0, 240.0) }])).unwrap();
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }, Mutation::Close { instance: id("two") }])).unwrap();
    assert!(service.layout().sections.is_empty());
    for name in ["two", "one"] {
        service
            .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id(name), destination: None }]))
            .unwrap();
    }
    let group = service.layout().section(&section("floating-group")).unwrap();
    assert_eq!(group.tabs, vec![id("one"), id("two")]);
    assert_eq!(group.active, id("two"));
    assert_eq!(group.location, SectionLocation::Floating { origin });
    assert_eq!(group.size, size(250.0, 240.0));
    validate(service.layout(), service.registry()).unwrap();
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
    let peek = resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    let header = peek.panels.first().unwrap();
    assert_eq!(header.outer.size, size(280.0, 24.0));
    assert!(header.body.is_none());
    service.dispatch(transaction(vec![Mutation::DismissPeek])).unwrap();
    assert_eq!(resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap(), peek);
    assert_eq!(*service.layout(), layout);
    service.dispatch(transaction(vec![Mutation::Focus { instance: id("one") }])).unwrap();
    assert!(resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap().panels[0].body.is_some());
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    assert!(service.interaction().focused.is_none());
}

#[test]
fn overflow_is_transient_and_never_squeezes_or_rewrites_desired_dimensions() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let before = service.inspect();
    let small = resolve_geometry(&service, rect(80.0, 80.0), Direction::Ltr, 48.0).unwrap();
    let card = small.panels.first().unwrap();
    assert!(card.outer.size.width >= 160.0 && card.outer.size.height >= 120.0);
    assert!(card.painted.iter().all(|painted| rect(80.0, 80.0).contains_rect(*painted)));
    assert!(small.unpresented.is_empty());
    let mut env = allocated_pair_environment(&service, 80.0);
    env.capacity = rect(80.0, 80.0);
    env.scroll = Point { x: MAX_LOGICAL, y: MAX_LOGICAL };
    for request in allocation_measure_requests(service.layout(), service.registry(), &env).unwrap() {
        env.content.push(SideContentMeasure { request, needed_height: 240.0, policy: SideHeightPolicy::Content });
    }
    let scrolled = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(scrolled.scroll.x, (scrolled.extent.width - 80.0).max(0.0));
    assert_eq!(scrolled.scroll.y, (scrolled.extent.height - 80.0).max(0.0));
    let large = resolve_geometry(&service, rect(1000.0, 800.0), Direction::Ltr, 48.0).unwrap();
    assert_eq!(large.panels.first().unwrap().outer.size, size(280.0, 240.0));
    assert_eq!(service.inspect(), before);
}

#[test]
fn logical_rtl_flow_aspect_and_size_classes_use_usable_units() {
    assert_eq!(Direction::Rtl.edge(Zone::Left).unwrap(), PhysicalEdge::Right);
    for (width, expected) in [(239.0, SizeClass::Compact), (240.0, SizeClass::Regular), (480.0, SizeClass::Regular), (481.0, SizeClass::Wide)] {
        assert_eq!(responsive(Zone::Floating, size(width, 600.0), None).unwrap().1, expected);
    }
    assert_eq!(responsive(Zone::Floating, size(105.0, 100.0), Some(Flow::Tall)).unwrap().0, Flow::Wide);
    assert_eq!(responsive(Zone::Window, size(95.0, 100.0), Some(Flow::Wide)).unwrap().0, Flow::Tall);
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
            return_to: DockReturn {
                zone: Zone::Left,
                index: 0,
                size: size(280.0, 240.0),
                collapsed: false,
                join: None,
                section: None,
                column: None,
                column_index: 0,
                column_width: None,
                weight: 1.0,
                preferred_extent: None,
                extent_priority: 0,
            },
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
    assert_eq!(loaded.envelope.version, 2);
    assert_eq!(loaded.envelope.layout.version, 2);
    assert_eq!(loaded.envelope.layout.columns.len(), 1);
    assert_eq!(loaded.envelope.layout.sections[0].weight, 240.0);
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
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 1.0,
                        preferred_extent: None,
                        extent_priority: 0,
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
        return_to: DockReturn {
            zone: Zone::Left,
            index: 0,
            size: size(280.0, 240.0),
            collapsed: false,
            join: None,
            section: None,
            column: None,
            column_index: 0,
            column_width: None,
            weight: 1.0,
            preferred_extent: None,
            extent_priority: 0,
        },
    };
    native.sections.first_mut().unwrap().column = None;
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

fn renamed_arrangement(layout: &Layout) -> Layout {
    fn location(location: &mut SectionLocation) {
        if let SectionLocation::Window { return_to, .. } = location {
            if let Some(id) = &mut return_to.section {
                id.0 = format!("renamed.{}", id.0);
            }
            if let Some(id) = &mut return_to.column {
                id.0 = format!("renamed.{}", id.0);
            }
            if let Some(join) = &mut return_to.join {
                join.section.0 = format!("renamed.{}", join.section.0);
            }
        }
    }
    let mut result = layout.clone();
    for column in &mut result.columns {
        column.id.0 = format!("renamed.{}", column.id.0);
    }
    for section in &mut result.sections {
        section.id.0 = format!("renamed.{}", section.id.0);
        if let Some(id) = &mut section.column {
            id.0 = format!("renamed.{}", id.0);
        }
        location(&mut section.location);
    }
    for instance in &mut result.instances {
        instance.memory.section.0 = format!("renamed.{}", instance.memory.section.0);
        if let Some(id) = &mut instance.memory.column {
            id.0 = format!("renamed.{}", id.0);
        }
        location(&mut instance.memory.location);
    }
    for record in &mut result.extent_priority_context.records {
        match &mut record.target {
            ExtentPriorityTarget::Section(id) | ExtentPriorityTarget::SectionReturn(id) => id.0 = format!("renamed.{}", id.0),
            _ => {}
        }
        if let Some(id) = &mut record.column {
            id.0 = format!("renamed.{}", id.0);
        }
    }
    result
}

#[test]
fn arrangement_comparison_ignores_generated_names_but_keeps_order_geometry_and_tabs() {
    let mut backend = command_ready_backend();
    let layout = backend.service.layout().clone();
    let renamed = renamed_arrangement(&layout);
    assert_ne!(layout, renamed, "authoritative Layout equality still includes exact identities");
    assert!(arrangements_equal(&layout, &renamed));
    for changed in 0..4 {
        let mut different = renamed.clone();
        match changed {
            0 => different.sections.swap(0, 1),
            1 => different.columns.first_mut().unwrap().width += 1.0,
            2 => different.sections.first_mut().unwrap().weight += 1.0,
            _ => different.sections.first_mut().unwrap().collapsed = true,
        }
        assert!(!arrangements_equal(&layout, &different));
    }
    backend
        .execute("panel.move", &serde_json::json!({"target":command_target("two"),"destination":{"type":"join_tab","section":"s-one","index":1}}))
        .unwrap();
    let tabs = backend.service.layout().clone();
    let mut reversed = renamed_arrangement(&tabs);
    reversed.sections.first_mut().unwrap().tabs.swap(0, 1);
    assert!(!arrangements_equal(&tabs, &reversed));
    let mut excessive = layout.clone();
    excessive.sections = vec![layout.sections.first().unwrap().clone(); MAX_SECTIONS + 1];
    assert!(!arrangements_equal(&excessive, &excessive));
}

#[test]
fn moving_a_panel_away_and_back_matches_its_saved_arrangement_with_new_group_identity() {
    let mut backend = command_ready_backend();
    let saved = backend.service.layout().clone();
    let original = saved.containing(&id("one")).unwrap().clone();
    backend
        .execute(
            "panel.move",
            &serde_json::json!({
                "target":command_target("one"),
                "destination":command_destination("away-generated", 0, "right"),
            }),
        )
        .unwrap();
    assert!(!arrangements_equal(&saved, backend.service.layout()));
    assert_eq!(backend.service.layout().containing(&id("one")).unwrap().location.zone(), Zone::Right);
    backend
        .execute(
            "panel.move",
            &serde_json::json!({
                "target":command_target("one"),
                "destination":command_destination("returned-generated", 0, "left"),
            }),
        )
        .unwrap();
    let returned = backend.service.layout();
    let group = returned.containing(&id("one")).unwrap();
    assert_ne!(group.id, original.id, "a real move allocates a new group instead of restoring its old string ID");
    assert_eq!((group.size, group.weight, group.collapsed, &group.tabs), (original.size, original.weight, original.collapsed, &original.tabs));
    assert_eq!(returned.columns, saved.columns, "temporary empty columns are pruned rather than left as Custom-only metadata");
    assert_eq!(
        returned.instances.iter().map(|instance| (&instance.id, instance.memory.section_index, instance.memory.tab_index)).collect::<Vec<_>>(),
        saved.instances.iter().map(|instance| (&instance.id, instance.memory.section_index, instance.memory.tab_index)).collect::<Vec<_>>()
    );
    assert_ne!(returned, &saved);
    assert!(arrangements_equal(&saved, returned), "reverting actual placement, sizes, proportions and memories reselects the saved tile");
}

#[test]
fn arrangement_comparison_preserves_closed_placement_and_native_return_relationships() {
    let mut backend = command_ready_backend();
    backend.execute("panel.close", &serde_json::json!({"target":command_target("two")})).unwrap();
    let mut layout = backend.service.layout().clone();
    let live_section = layout.sections.first().unwrap().id.clone();
    let column = layout.columns.first().unwrap().id.clone();
    let closed = layout.instances.iter_mut().find(|instance| instance.id == id("two")).unwrap();
    closed.memory.location = SectionLocation::Window {
        host: HostId::new("remembered-host").unwrap(),
        origin: Point { x: 40.0, y: 50.0 },
        return_to: DockReturn {
            zone: Zone::Left,
            index: 0,
            size: closed.memory.size,
            collapsed: false,
            section: Some(live_section.clone()),
            column: Some(column),
            column_index: 0,
            column_width: Some(280.0),
            weight: 1.0,
            preferred_extent: None,
            extent_priority: 0,
            join: Some(ReturnTab { section: live_section.clone(), index: 0 }),
        },
    };
    closed.memory.column = None;
    closed.memory.column_width = None;
    let closed_section = closed.memory.section.clone();
    layout.extent_priority_context.records = vec![
        ExtentPriorityRecord {
            target: ExtentPriorityTarget::Section(closed_section),
            zone: Zone::Left,
            column: None,
            stored_priority: 0,
            priority: 1,
        },
        ExtentPriorityRecord {
            target: ExtentPriorityTarget::SectionReturn(live_section),
            zone: Zone::Left,
            column: None,
            stored_priority: 0,
            priority: 2,
        },
    ];
    validate(&layout, backend.service.registry()).unwrap();
    let mut renamed = renamed_arrangement(&layout);
    renamed.extent_priority_context.records.reverse();
    assert!(arrangements_equal(&layout, &renamed));
    let mut different = renamed.clone();
    different.instances.iter_mut().find(|instance| instance.id == id("two")).unwrap().memory.section = different.sections.first().unwrap().id.clone();
    assert!(!arrangements_equal(&layout, &different), "a closed panel remembering a live group is not a renamed separate group");
    let mut different = renamed;
    let closed = different.instances.iter_mut().find(|instance| instance.id == id("two")).unwrap();
    if let SectionLocation::Window { return_to, .. } = &mut closed.memory.location {
        return_to.join.as_mut().unwrap().section = SectionId::new("other-return-group").unwrap();
    }
    assert!(!arrangements_equal(&layout, &different), "native return joins retain the group they refer to");
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
        ("panel.action", json!({"target":command_target("one"),"action":"refresh"})),
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
        for field in ["action", "id", "instance", "kind", "first", "second", "workspace"] {
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
        if id == "panel.action" {
            assert_eq!(result.unwrap_err().rule, Rule::Capability, "headless cannot invoke an unbound controller");
            continue;
        }
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
fn thickness_only_bottom_command_matches_pointer_mutation_and_preserves_saved_width_intent() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    for selector in [json!({"target":{"type":"section","id":"first"}}), json!({"target":command_target("one")}), json!({"id":"one"})] {
        let mut registry = synthetic_registry().unwrap();
        let mut constrained = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        constrained.kind = PanelKind::new("fixture.short-height").unwrap();
        constrained.min_size.height = 140.0;
        constrained.max_size.height = 300.0;
        registry.register(constrained).unwrap();
        let mut service =
            LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
        service
            .dispatch(transaction(vec![
                fixture_open("fixture.log", "one", "first", Zone::Bottom, 0).unwrap(),
                fixture_open("fixture.log", "two", "second", Zone::Bottom, 1).unwrap(),
                Mutation::Open {
                    kind: PanelKind::new("fixture.short-height").unwrap(),
                    instance: id("inactive"),
                    destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
                },
                Mutation::SetSize { section: section("first"), size: size(320.0, 240.0) },
                Mutation::SetSize { section: section("second"), size: size(450.0, 500.0) },
            ]))
            .unwrap();
        let mut imported = service.layout().clone();
        let memory = &mut imported.instances.iter_mut().find(|p| p.id == id("one")).unwrap().memory;
        memory.size.width = 800.0;
        memory.preferred_extent = Some(600.0);
        memory.extent_priority = 77;
        memory.weight = 3.0;
        let service = LayoutService::new(registry.clone(), imported.clone(), HostCapabilities::default()).unwrap();
        let expected = service.prepare(transaction(vec![Mutation::SetBottomHeight { section: section("first"), height: 180.0 }])).unwrap();
        let mut backend = commands::InMemoryPanelBackend::new(service, fixture_command_catalogue(&registry).unwrap());
        let mut params = selector;
        // The valid ignored width is outside every panel's width bounds.
        params["size"] = json!({"width":1,"height":180});
        params["thickness_only"] = json!(true);
        backend.execute("panel.resize", &params).unwrap();
        assert_eq!(backend.service.layout(), expected.layout());
        for (old, new) in imported.sections.iter().zip(&backend.service.layout().sections) {
            let mut expected = old.clone();
            expected.size.height = 180.0;
            assert_eq!(new, &expected, "row thickness preserves section width preference, tier and weight");
        }
        for (old, new) in imported.instances.iter().zip(&backend.service.layout().instances) {
            let mut expected = old.clone();
            expected.memory.size.height = 180.0;
            assert_eq!(new, &expected, "inactive and divergent live memories retain width intent");
        }
        let stored = backend.catalogue.state().remembered(&imported.workspace).unwrap().project_envelope(&registry, &imported.workspace).unwrap();
        assert_eq!(stored.layout.instances, backend.service.layout().instances);
        assert_eq!(stored.layout.sections, backend.service.layout().sections);
        let before = backend.service.inspect();
        let catalogue = backend.catalogue.state().clone();
        params["size"]["height"] = json!(301);
        assert_eq!(backend.execute("panel.resize", &params).unwrap_err().rule, Rule::Bounds, "inactive tab limits the common row");
        assert_eq!(backend.service.inspect(), before);
        assert_eq!(backend.catalogue.state(), &catalogue);
    }
}

#[test]
fn thickness_only_side_command_resolves_columns_sections_instances_and_legacy_selectors() {
    use serde_json::json;
    for zone in [Zone::Left, Zone::Right] {
        for selector in ["column", "section", "instance", "legacy"] {
            let mut service = fixture_service(false).unwrap();
            service
                .dispatch(transaction(vec![
                    fixture_open("fixture.log", "one", "first", zone, 0).unwrap(),
                    fixture_open("fixture.log", "two", "second", zone, 1).unwrap(),
                    Mutation::SetSize { section: section("first"), size: size(280.0, 300.0) },
                    Mutation::SetSize { section: section("second"), size: size(280.0, 500.0) },
                ]))
                .unwrap();
            let before = service.layout().clone();
            let column = before.section(&section("first")).unwrap().column.clone().unwrap();
            assert_eq!(before.section(&section("second")).unwrap().column, Some(column.clone()));
            let expected = service.prepare(transaction(vec![Mutation::SetColumnWidth { column: column.clone(), width: 360.0 }])).unwrap();
            let catalogue = fixture_command_catalogue(service.registry()).unwrap();
            let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
            let mut params = match selector {
                "column" => json!({"target":{"type":"column","id":column}}),
                "section" => json!({"target":{"type":"section","id":"first"}}),
                "instance" => json!({"target":command_target("one")}),
                _ => json!({"id":"one"}),
            };
            params["size"] = json!({"width":360,"height":1});
            params["thickness_only"] = json!(true);
            backend.execute("panel.resize", &params).unwrap();
            assert_eq!(backend.service.layout(), expected.layout());
            for (old, new) in before.sections.iter().zip(&backend.service.layout().sections) {
                let mut expected = old.clone();
                expected.size.width = 360.0;
                assert_eq!(new, &expected, "side thickness must not author a height preference or tier");
            }
            for (old, new) in before.instances.iter().zip(&backend.service.layout().instances) {
                let mut expected = old.clone();
                expected.memory.size.width = 360.0;
                expected.memory.column_width = Some(360.0);
                assert_eq!(new, &expected);
            }
        }
    }
}

#[test]
fn resize_omission_and_false_keep_full_size_behavior_in_dock_and_floating_hosts() {
    use serde_json::json;
    for zone in [Zone::Left, Zone::Bottom, Zone::Floating] {
        for flag in [None, Some(false)] {
            let mut service = fixture_service(false).unwrap();
            service.dispatch(transaction(vec![fixture_open("fixture.log", "one", "first", zone, 0).unwrap()])).unwrap();
            let expected = service.prepare(transaction(vec![Mutation::SetSize { section: section("first"), size: size(360.0, 320.0) }])).unwrap();
            let catalogue = fixture_command_catalogue(service.registry()).unwrap();
            let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
            let mut params = json!({"target":command_target("one"),"size":{"width":360,"height":320}});
            if let Some(flag) = flag {
                params["thickness_only"] = json!(flag);
            }
            backend.execute("panel.resize", &params).unwrap();
            assert_eq!(backend.service.layout(), expected.layout());
            let group = backend.service.layout().section(&section("first")).unwrap();
            assert_eq!(group.size, size(360.0, 320.0));
            if zone.docked() {
                assert_eq!(group.preferred_extent, Some(if zone == Zone::Bottom { 360.0 } else { 320.0 }));
                assert!(group.extent_priority > 0);
            }
        }
    }
}

#[test]
fn thickness_only_refuses_free_native_and_nondock_targets_without_layout_or_storage_effects() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    for native in [false, true] {
        let mut service = fixture_service(native).unwrap();
        if native {
            open(&mut service, "one", "first");
            service
                .dispatch(transaction(vec![Mutation::Move {
                    instance: id("one"),
                    destination: Destination::NewSection {
                        section: section("free"),
                        index: 0,
                        size: size(280.0, 240.0),
                        location: SectionLocation::Window {
                            host: HostId::new("thickness-native").unwrap(),
                            origin: Point { x: 0.0, y: 0.0 },
                            return_to: DockReturn {
                                zone: Zone::Left,
                                index: 0,
                                size: size(280.0, 240.0),
                                collapsed: false,
                                join: None,
                                section: None,
                                column: None,
                                column_index: 0,
                                column_width: None,
                                weight: 1.0,
                                preferred_extent: None,
                                extent_priority: 0,
                            },
                        },
                    },
                }]))
                .unwrap();
        } else {
            service.dispatch(transaction(vec![fixture_open("fixture.log", "one", "free", Zone::Floating, 0).unwrap()])).unwrap();
        }
        let catalogue = fixture_command_catalogue(service.registry()).unwrap();
        let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
        for target in [
            command_target("one"),
            json!({"type":"section","id":"free"}),
            json!({"type":"native_host","id":"thickness-native"}),
            json!({"type":"workspace","id":"fixture.workspace"}),
        ] {
            let before = backend.service.inspect();
            let catalogue = backend.catalogue.state().clone();
            let result = backend.execute("panel.resize", &json!({"target":target,"size":{"width":300,"height":200},"thickness_only":true}));
            assert_eq!(
                result.unwrap_err().rule,
                if target["type"] == "instance" || target["type"] == "section" { Rule::Placement } else { Rule::Schema }
            );
            assert_eq!(backend.service.inspect(), before);
            assert_eq!(backend.catalogue.state(), &catalogue);
        }
    }
}

#[test]
fn thickness_only_schema_unused_dimensions_and_atomic_failures_use_existing_command_boundaries() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    let spec = crate::find_command("panel.resize").unwrap();
    let contract = spec.contract.unwrap();
    let schema = contract.input_schema();
    assert_eq!(schema.as_value()["properties"]["thickness_only"]["default"], false);
    assert_eq!(schema.as_value()["properties"]["thickness_only"]["type"], "boolean");
    let mut backend = command_ready_backend();
    for (field, value) in [
        ("thickness_only", json!(null)),
        ("thickness_only", json!(1)),
        ("thickness_only", json!("true")),
        ("thickness_only", json!([])),
        ("thickness_only", json!({})),
        ("unknown", json!(true)),
    ] {
        let mut params = json!({"target":command_target("one"),"size":{"width":300,"height":200},"thickness_only":true});
        params[field] = value;
        assert!(!contract.check(&params).is_empty());
        assert_eq!(commands::parse_command("panel.resize", &params).unwrap_err().rule, Rule::Schema);
        let before = backend.service.inspect();
        let catalogue = backend.catalogue.state().clone();
        assert!(backend.execute("panel.resize", &params).is_err());
        assert_eq!(backend.service.inspect(), before);
        assert_eq!(backend.catalogue.state(), &catalogue);
    }
    backend.execute("panel.open", &json!({"kind":"fixture.log","instance":"bottom","destination":command_destination("bottom",2,"bottom")})).unwrap();
    let column = backend.service.layout().section(&section("s-one")).unwrap().column.clone().unwrap();
    for (target, axis) in [(command_target("one"), "height"), (json!({"type":"column","id":column}), "height"), (command_target("bottom"), "width")] {
        for value in [json!(0), json!(-1), json!(MAX_LOGICAL + 1.0), json!(1e100), json!(null), json!("NaN")] {
            let mut params = json!({"target":target,"size":{"width":300,"height":200},"thickness_only":true});
            params["size"][axis] = value;
            let before = backend.service.inspect();
            let catalogue = backend.catalogue.state().clone();
            assert!(!contract.check(&params).is_empty(), "unused {axis} remains hostile input");
            assert!(backend.execute("panel.resize", &params).is_err());
            assert_eq!(backend.service.inspect(), before);
            assert_eq!(backend.catalogue.state(), &catalogue);
        }
    }
    for (panel, unused_width) in [("one", false), ("bottom", true)] {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let params = crate::schema::panels::Resize {
                target: Some(crate::schema::panels::Target::Instance(id(panel))),
                id: None,
                size: crate::schema::panels::OuterSize {
                    width: if unused_width { value } else { 300.0 },
                    height: if unused_width { 200.0 } else { value },
                },
                thickness_only: true,
                expected_revision: None,
            };
            let before = backend.service.inspect();
            let catalogue = backend.catalogue.state().clone();
            let registry = backend.service.registry().clone();
            let result = commands::execute_command(
                commands::PanelCommand::Resize(params),
                &mut commands::PanelCommandContext { sink: &mut backend.service, registry: &registry, catalogue: &mut backend.catalogue, web: false },
            );
            assert_eq!(result.unwrap_err().rule, Rule::Bounds);
            assert_eq!(backend.service.inspect(), before);
            assert_eq!(backend.catalogue.state(), &catalogue);
        }
    }
    for params in [
        json!({"target":command_target("one"),"thickness_only":true}),
        json!({"target":command_target("one"),"size":{"width":300,"height":200,"unknown":true},"thickness_only":true}),
    ] {
        assert_eq!(commands::parse_command("panel.resize", &params).unwrap_err().rule, Rule::Schema);
    }
    let before = backend.service.inspect();
    let catalogue = backend.catalogue.state().clone();
    assert!(
        backend
            .batch(&[
                json!({"command":"panel.resize","params":{"target":command_target("bottom"),"size":{"width":1,"height":180},"thickness_only":true}}),
                json!({"command":"panel.resize","params":{"target":command_target("one"),"size":{"width":1,"height":1},"thickness_only":true}}),
            ])
            .is_err()
    );
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
    backend.catalogue.fail_stage = true;
    assert_eq!(
        backend
            .execute("panel.resize", &json!({"target":command_target("bottom"),"size":{"width":1,"height":180},"thickness_only":true}))
            .unwrap_err()
            .rule,
        Rule::Capability
    );
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state(), &catalogue);
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
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"USER-1","replace":true})).unwrap()["name"], "user-1");
    assert_eq!(backend.catalogue.state().user_count(), commands::MAX_USER_PRESETS, "replacement at the limit does not consume another slot");
}

#[test]
fn saved_layout_names_ignore_case_and_replacement_preserves_the_named_slot() {
    let mut backend = command_ready_backend();
    backend.execute("layout.save", &serde_json::json!({"name":"  Bench setup  "})).unwrap();
    let original = backend.service.layout().clone();
    backend.execute("panel.collapse", &serde_json::json!({"target":command_target("one"),"collapsed":true})).unwrap();
    let changed = backend.service.layout().clone();
    let catalogue = backend.catalogue.state().clone();
    let state = backend.service.inspect();
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"BENCH SETUP"})).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(backend.catalogue.state(), &catalogue);
    assert_eq!(backend.service.inspect(), state);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"bench setup","replace":true})).unwrap()["name"], "Bench setup");
    assert_eq!(backend.catalogue.state().user_count(), 1);
    assert_eq!(backend.catalogue.state().preset_kind(" BENCH setup ").unwrap(), Some(commands::PresetKind::User));
    backend.execute("layout.apply", &serde_json::json!({"name":"bENCH SETUP"})).unwrap();
    assert_eq!(backend.service.layout(), &changed);
    assert_ne!(backend.service.layout(), &original);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"fIXTURE DEFAULT","replace":true})).unwrap_err().rule, Rule::Capability);
    assert_eq!(backend.catalogue.state().preset_kind("FIXTURE DEFAULT").unwrap(), Some(commands::PresetKind::BuiltIn));
    let catalogue = backend.catalogue.state().clone();
    assert_eq!(
        backend.execute("layout.save", &serde_json::json!({"name":"Bench setup","replace":false,"overwrite":true})).unwrap_err().rule,
        Rule::Schema
    );
    assert_eq!(backend.catalogue.state(), &catalogue);
    backend.execute("layout.save", &serde_json::json!({"name":"Ärger"})).unwrap();
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"ärGER"})).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"ärGER","overwrite":true})).unwrap()["name"], "Ärger");
    backend.execute("layout.save", &serde_json::json!({"name":"ΟΣ"})).unwrap();
    for same in ["οσ", "ος"] {
        assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":same})).unwrap_err().rule, Rule::DuplicateId);
        assert_eq!(backend.catalogue.state().preset_kind(same).unwrap(), Some(commands::PresetKind::User));
    }
    backend.execute("layout.save", &serde_json::json!({"name":"Straße"})).unwrap();
    assert_eq!(backend.execute("layout.save", &serde_json::json!({"name":"STRASSE"})).unwrap_err().rule, Rule::DuplicateId);
    let snapshot = backend.catalogue.state().export().unwrap();
    let hydrated = commands::CatalogueState::hydrate(&snapshot, backend.service.registry()).unwrap();
    assert_eq!(hydrated.preset_kind("ος").unwrap(), Some(commands::PresetKind::User));
    assert_eq!(hydrated.preset_kind("strasse").unwrap(), Some(commands::PresetKind::User));
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
    let undo = session.undo.len();
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
    assert_eq!(session.undo.len(), undo);
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
fn saving_an_overflowing_arrangement_only_stages_catalogue_metadata() {
    use commands::LayoutCatalogue;
    use std::cell::Cell;
    struct OverflowSink {
        service: LayoutService,
        projections: Cell<usize>,
        dispatches: usize,
    }
    impl ActionSink for OverflowSink {
        fn inspect(&self) -> LayoutReply {
            self.service.inspect()
        }
        fn project(&self, _action: LayoutAction) -> PanelResult<Layout> {
            self.projections.set(self.projections.get() + 1);
            Err(error(Rule::NoSpace, "candidate does not improve existing overflow"))
        }
        fn dispatch(&mut self, _action: LayoutAction) -> PanelResult<LayoutReply> {
            self.dispatches += 1;
            Err(error(Rule::NoSpace, "candidate does not improve existing overflow"))
        }
    }
    let backend = command_ready_backend();
    let registry = backend.service.registry().clone();
    let environment = allocated_pair_environment(&backend.service, 40.0);
    let geometry = allocate_layout(backend.service.layout(), &registry, &environment).unwrap();
    assert!(geometry.extent.height > environment.capacity.size.height);
    let mut sink = OverflowSink { service: backend.service, projections: Cell::new(0), dispatches: 0 };
    let mut catalogue = backend.catalogue;
    let before = sink.inspect();
    let saved = commands::execute(
        "layout.save",
        &serde_json::json!({"name":"Narrow window","expected_revision":before.revision}),
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap();
    assert_eq!(saved["state"]["revision"], before.revision);
    assert_eq!(sink.inspect(), before, "saving never changes the authoritative layout or its revision");
    assert_eq!(catalogue.state().preset("Narrow window").unwrap().envelope(&registry, &before.layout.workspace).unwrap().layout, before.layout);
    let saved_catalogue = catalogue.state().clone();
    catalogue.fail_stage = true;
    assert!(
        commands::execute(
            "layout.save",
            &serde_json::json!({"name":"Failed save"}),
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
        )
        .is_err()
    );
    assert_eq!(catalogue.state(), &saved_catalogue);
    assert_eq!(sink.inspect(), before);
    assert_eq!(sink.projections.get(), 0);
    assert_eq!(sink.dispatches, 0);
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
    for (command, args) in
        [("panel.close", serde_json::json!({"target":command_target("one")})), ("layout.save", serde_json::json!({"name":"Stale save"}))]
    {
        let backend = command_ready_backend();
        let registry = backend.service.registry().clone();
        let shared = Rc::new(RefCell::new(backend.service));
        let mut sink = SharedSink(shared.clone());
        let mut catalogue = RacingCatalogue { catalogue: backend.catalogue, service: shared, published: false };
        let before = catalogue.state().clone();
        let reason = commands::execute(
            command,
            &args,
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
        )
        .unwrap_err();
        assert_eq!(reason.rule, Rule::Revision);
        assert!(!catalogue.published);
        assert_eq!(catalogue.state(), &before);
        assert!(sink.inspect().layout.instance(&id("one")).unwrap().requested_open);
    }
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
        let column = wire["layout"]["sections"][0]["column"].clone();
        let columns = wire["layout"]["columns"].as_array_mut().unwrap();
        columns.iter_mut().find(|saved| saved["id"] == column).unwrap()["zone"] = serde_json::json!("left");
        columns.reverse();
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
        let mapped = state
            .map_envelopes(&registry, |_, mut envelope| {
                for section in &mut envelope.layout.sections {
                    section.collapsed = true;
                }
                Ok(envelope)
            })
            .unwrap();
        let mapped_user = mapped.preset("user SAVED").unwrap();
        assert_eq!(mapped_user.rejected(), Some([0, 255, 1, 0].as_slice()));
        assert_eq!(mapped_user.envelope(&registry, &envelope.layout.workspace).unwrap().retained_data, envelope.retained_data);
        assert_eq!(state.export().unwrap(), snapshot, "mapping never mutates the source catalogue");
        let mut visits = 0;
        assert_eq!(
            state
                .map_envelopes(&registry, |_, envelope| {
                    visits += 1;
                    if visits == 2 { Err(error(Rule::Capability, "refused portable mapping")) } else { Ok(envelope) }
                })
                .unwrap_err()
                .rule,
            Rule::Capability
        );
        assert_eq!(state.export().unwrap(), snapshot, "a late mapper failure cannot publish a partial catalogue");
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
    fn catalogue_case_only_collisions_are_refused_instead_of_dropping_records() {
        let registry = synthetic_registry().unwrap();
        let envelope = encode(&LayoutEnvelope::new(Layout::empty(WorkspaceId::new("fixture.workspace").unwrap())), &registry).unwrap();
        for (builtins, users) in [
            (vec![("Default", envelope.as_slice(), None)], vec![("DEFAULT", envelope.as_slice(), None)]),
            (vec![("Default", envelope.as_slice(), None), ("default", envelope.as_slice(), None)], vec![]),
            (vec![("Default", envelope.as_slice(), None)], vec![("Bench", envelope.as_slice(), None), ("bENCH", envelope.as_slice(), None)]),
        ] {
            let bytes = snapshot(&builtins, &users, &[("fixture.workspace", "ui.panel", "Default")], &[]);
            assert_eq!(CatalogueState::hydrate(&bytes, &registry).unwrap_err().rule, Rule::DuplicateId);
        }
    }

    #[test]
    fn maximum_original_v1_record_hydrates_and_projects_without_claiming_expanded_encodability() {
        use crate::panels::commands::StoredEnvelope;
        let registry = synthetic_registry().unwrap();
        let absent = without_kind(&registry, "fixture.editor");
        let fallback = Layout::empty(WorkspaceId::new("fixture.workspace").unwrap());
        let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
        raw["future_envelope"] = serde_json::json!({"intent":"keep exact legacy bytes"});
        raw["retained_data"]["padding"] = serde_json::json!("");
        let overhead = serde_json::to_vec(&raw).unwrap().len();
        raw["retained_data"]["padding"] = serde_json::json!("x".repeat(MAX_ENVELOPE_BYTES - overhead));
        let legacy = serde_json::to_vec(&raw).unwrap();
        assert_eq!(legacy.len(), MAX_ENVELOPE_BYTES);
        bounded_json(&raw).unwrap(); // Original v1 validity, not a padded v2 record.
        let source = snapshot(
            &[("Legacy", &legacy, None)],
            &[],
            &[("fixture.workspace", "ui.panel", "Legacy")],
            &[("fixture.workspace", &legacy, Some(b"original recovery"))],
        );
        for current in [&registry, &absent] {
            let retained = decode_retained_record(&legacy, current).unwrap();
            assert_eq!(retained.retained_v1_bytes(), Some(legacy.as_slice()));
            assert!(retained.layout.sections.iter().all(|s| s.preferred_extent.is_none()));
            assert!(retained.layout.instances.iter().all(|p| p.memory.preferred_extent.is_none()));
            assert!(retained.layout.sections.iter().all(|s| s.extent_priority == 0));
            assert!(retained.layout.instances.iter().all(|p| p.memory.extent_priority == 0));
            let typed = serde_json::to_value(&retained).unwrap();
            assert!(typed["layout"]["sections"][0].get("preferred_extent").is_none());
            assert!(typed["layout"]["instances"][0]["memory"].get("preferred_extent").is_none());
            assert!(typed["layout"]["sections"][0].get("extent_priority").is_none());
            assert!(typed["layout"]["instances"][0]["memory"].get("extent_priority").is_none());
            assert_eq!(decode_retained(&legacy, current, &fallback.workspace).unwrap().retained_v1_bytes(), Some(legacy.as_slice()));
            let projected = project_retained_record(&legacy, current, &fallback.workspace).unwrap();
            let loaded = decode(&legacy, current, &fallback).unwrap();
            assert!(loaded.notice.is_none() && loaded.rejected.is_none());
            assert_eq!(loaded.envelope.retained_v1_bytes(), Some(legacy.as_slice()));
            assert_eq!(projected.retained_v1_bytes(), Some(legacy.as_slice()));
            assert_eq!(encode(&projected, current).unwrap_err().rule, Rule::CollectionLimit);
            assert_eq!(StoredEnvelope::new(&projected, None, current).unwrap_err().rule, Rule::CollectionLimit);
            assert_eq!(with_live_layout(&projected, projected.layout.clone(), current).unwrap_err().rule, Rule::CollectionLimit);
            let catalogue = CatalogueState::hydrate(&source, current).unwrap();
            assert_eq!(catalogue.export().unwrap(), source);
            assert_eq!(
                catalogue.map_envelopes(current, |_, envelope| Ok(envelope)).unwrap().export().unwrap(),
                source,
                "an unchanged mapper must retain maximum original v1 bytes without expanding them"
            );
            let stored = catalogue.preset("Legacy").unwrap();
            assert_eq!(stored.bytes(), legacy.as_slice());
            assert_eq!(stored.project_envelope(current, &fallback.workspace).unwrap().retained_v1_bytes(), Some(legacy.as_slice()));
            assert_eq!(catalogue.remembered(&fallback.workspace).unwrap().rejected(), Some(b"original recovery".as_slice()));
            let catalogue = InMemoryCatalogue::from_snapshot(&source, current).unwrap();
            let service = LayoutService::new(current.clone(), projected.layout.clone(), HostCapabilities::default()).unwrap();
            let captured = commands::current_envelope_snapshot(&catalogue, &service.inspect(), current).unwrap();
            assert_eq!(captured.retained_v1_bytes(), Some(legacy.as_slice()));
            assert_eq!(encode(&captured, current).unwrap_err().rule, Rule::CollectionLimit);
            assert_eq!(catalogue.state().export().unwrap(), source);
        }
    }

    #[test]
    fn maximum_priority_record_hydrates_without_overhead_and_expanded_shadow_staging_is_atomic() {
        let (registry, envelope) = super::priority_recovery_envelope(u16::MAX, false);
        let envelope = maximum_record(envelope, &registry, MAX_ENVELOPE_BYTES);
        let bytes = encode(&envelope, &registry).unwrap();
        assert_eq!(bytes.len(), MAX_ENVELOPE_BYTES);
        let retained = decode_retained_record(&bytes, &registry).unwrap();
        assert!(retained.layout.extent_priority_context.complete);
        assert!(retained.extent_priority_overrides.is_empty());
        assert_eq!(encode(&retained, &registry).unwrap(), bytes);
        let context = preparation_context(&retained).unwrap();
        let service = LayoutService::new(registry.clone(), retained.layout.clone(), HostCapabilities::default()).unwrap();
        let before = service.inspect();
        let prepared = service
            .prepare_with_context(
                transaction(vec![Mutation::SetSize { section: SectionId::new("a").unwrap(), size: Size { width: 280.0, height: 350.0 } }]),
                &AvailabilityMap::new(),
                &context,
            )
            .unwrap();
        assert_eq!(with_live_layout(&retained, prepared.layout().clone(), &registry).unwrap_err().rule, Rule::CollectionLimit);
        assert_eq!(service.inspect(), before);
        assert_eq!(encode(&retained, &registry).unwrap(), bytes);
        let source = snapshot(&[("Full", &bytes, None)], &[], &[], &[("fixture.workspace", &bytes, None)]);
        let catalogue = InMemoryCatalogue::from_snapshot(&source, &registry).unwrap();
        assert_eq!(catalogue.state().export().unwrap(), source);
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
        let mut service = LayoutService::new(all.clone(), envelope.layout.clone(), HostCapabilities::default()).unwrap();
        service.dispatch(transaction(vec![Mutation::Close { instance: id("a") }, Mutation::Close { instance: id("b") }])).unwrap();
        service
            .dispatch(transaction(vec![
                fixture_open("extension.b", "b", "group", Zone::Right, 0).unwrap(),
                Mutation::Open {
                    kind: PanelKind::new("extension.a").unwrap(),
                    instance: id("a"),
                    destination: Some(Destination::JoinTab { section: section("group"), index: 1 }),
                },
                Mutation::Activate { instance: id("b") },
                Mutation::SetSize { section: section("group"), size: size(400.0, 300.0) },
                Mutation::Collapse { section: section("group"), collapsed: true },
            ]))
            .unwrap();
        envelope = with_live_layout(&envelope, service.layout().clone(), &all).unwrap();
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

#[test]
fn amended_registration_defaults_are_generic_and_header_metadata_is_bounded() {
    let registry = synthetic_registry().unwrap();
    let base = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap();
    let mut raw = serde_json::to_value(base).unwrap();
    for field in ["allowed_zones", "side_height_policy", "header_actions", "text_heavy"] {
        raw.as_object_mut().unwrap().remove(field);
    }
    raw["features"].as_object_mut().unwrap().remove("hot_closable");
    let spec: PanelSpec = serde_json::from_value(raw).unwrap();
    assert_eq!(spec.allowed_zones, all_zones());
    assert_eq!(spec.side_height_policy, SideHeightPolicy::Content);
    assert!(!spec.text_heavy && !spec.features.hot_closable && spec.header_actions.is_empty());
    let mut duplicate = base.clone();
    let action = HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() };
    duplicate.header_actions = vec![action.clone(), action.clone()];
    assert_eq!(PanelRegistry::default().register(duplicate).unwrap_err().rule, Rule::DuplicateId);
    let mut oversized = base.clone();
    oversized.header_actions = vec![action; MAX_HEADER_ACTIONS + 1];
    assert_eq!(PanelRegistry::default().register(oversized).unwrap_err().rule, Rule::CollectionLimit);
    let mut visible_disabled = base.clone();
    visible_disabled.features.closable = false;
    visible_disabled.features.hot_closable = true;
    let mut registry = PanelRegistry::default();
    registry.register(visible_disabled.clone()).unwrap();
    assert!(registry.get(&visible_disabled.kind).unwrap().features.hot_closable);
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.log", "fixed", "fixed-group", Zone::Left, 0).unwrap()])).unwrap();
    reject(&mut service, vec![Mutation::Close { instance: id("fixed") }], Rule::FeatureDisabled);
}

#[test]
fn all_relative_placements_keep_exact_prepared_columns_and_stack_order() {
    for placement in [RelativePlacement::BeforeColumn, RelativePlacement::AfterColumn, RelativePlacement::Above, RelativePlacement::Below] {
        let mut service = fixture_service(false).unwrap();
        open(&mut service, "anchor", "anchor-group");
        open(&mut service, "mover", "source");
        let before = service.inspect();
        let column = next_column_id(service.layout()).unwrap();
        let split = matches!(placement, RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn);
        let action = transaction(vec![
            Mutation::Move {
                instance: id("mover"),
                destination: Destination::Relative {
                    section: section("anchor-group"),
                    placement,
                    new_section: section("landing"),
                    new_column: split.then_some(column.clone()),
                    size: size(280.0, 240.0),
                },
            },
            Mutation::Activate { instance: id("mover") },
        ]);
        let first = service.prepare(action.clone()).unwrap();
        let second = service.prepare(action).unwrap();
        assert_eq!(first.layout(), second.layout());
        assert_eq!(service.inspect(), before, "preview must not publish IDs or state");
        let shown = second.layout().clone();
        drop(first);
        let committed = service.commit(second).unwrap();
        assert_eq!(committed.layout, shown);
        let anchor = committed.layout.section(&section("anchor-group")).unwrap();
        let landing = committed.layout.section(&section("landing")).unwrap();
        assert_eq!(landing.active, id("mover"));
        assert_eq!(anchor.column == landing.column, !split);
        let ordered: Vec<_> = committed.layout.sections.iter().map(|s| s.id.clone()).collect();
        let before_anchor = matches!(placement, RelativePlacement::BeforeColumn | RelativePlacement::Above);
        assert_eq!(
            ordered,
            if before_anchor { vec![section("landing"), section("anchor-group")] } else { vec![section("anchor-group"), section("landing")] }
        );
        if split {
            assert_eq!(landing.column, Some(column));
        }
    }
}

#[test]
fn relative_stacks_allocate_equal_heights_across_source_and_anchor_weight_scales() {
    for placement in [RelativePlacement::Above, RelativePlacement::Below] {
        for anchor_weight in [1.0, 240.0, 0.0001] {
            for source_weight in [1.0, 240.0, 0.0001] {
                let mut service = fixture_service(false).unwrap();
                open(&mut service, "anchor", "anchor-group");
                service.dispatch(transaction(vec![fixture_open("fixture.log", "mover", "source", Zone::Floating, 1).unwrap()])).unwrap();
                let mut layout = service.layout().clone();
                layout.sections[0].weight = anchor_weight;
                layout.sections[1].weight = source_weight;
                layout.instances[0].memory.weight = anchor_weight;
                layout.instances[1].memory.weight = source_weight;
                let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
                let mut env = allocated_pair_environment(&service, 800.0);
                env.gap = 4.0;
                let before = service.inspect();
                let prepared = service
                    .prepare(transaction(vec![Mutation::Move {
                        instance: id("mover"),
                        destination: Destination::Relative {
                            section: section("anchor-group"),
                            placement,
                            new_section: section("landing"),
                            new_column: None,
                            size: size(280.0, 240.0),
                        },
                    }]))
                    .unwrap();
                assert_eq!(service.inspect(), before, "preparation cannot publish the stack");
                let anchor = prepared.layout().section(&section("anchor-group")).unwrap();
                let landing = prepared.layout().section(&section("landing")).unwrap();
                assert_eq!(anchor, before.layout.section(&section("anchor-group")).unwrap());
                assert_eq!(landing.weight, anchor_weight);
                assert_eq!(landing.size.height, 240.0, "desired height remains independent of surplus weight");
                let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
                assert_eq!(shown.sections.len(), 2);
                assert_eq!(
                    shown.sections.iter().map(|s| s.section.clone()).collect::<Vec<_>>(),
                    if placement == RelativePlacement::Above {
                        vec![section("landing"), section("anchor-group")]
                    } else {
                        vec![section("anchor-group"), section("landing")]
                    }
                );
                for (index, actual) in shown.sections.iter().enumerate() {
                    assert!(
                        (actual.outer.size.height - 398.0).abs() < 0.001,
                        "{placement:?}, anchor {anchor_weight}, source {source_weight}: {actual:?}"
                    );
                    assert!((actual.outer.origin.y - (17.0 + index as f32 * 402.0)).abs() < 0.001);
                }
                service.commit(prepared).unwrap();
                assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
                assert_eq!(service.layout().instance(&id("mover")).unwrap().memory.weight, anchor_weight);
            }
        }
    }
}

#[test]
fn relative_stacks_keep_zero_anchor_at_minimum_and_give_new_section_surplus() {
    for placement in [RelativePlacement::Above, RelativePlacement::Below] {
        let mut service = fixture_service(false).unwrap();
        open(&mut service, "anchor", "anchor-group");
        let mut layout = service.layout().clone();
        layout.sections[0].weight = 0.0;
        layout.instances[0].memory.weight = 0.0;
        let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
        service
            .dispatch(transaction(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("new"),
                destination: Some(Destination::Relative {
                    section: section("anchor-group"),
                    placement,
                    new_section: section("landing"),
                    new_column: None,
                    size: size(280.0, 240.0),
                }),
            }]))
            .unwrap();
        let mut env = allocated_pair_environment(&service, 800.0);
        env.gap = 4.0;
        let geometry = allocate_layout(service.layout(), service.registry(), &env).unwrap();
        assert_eq!(geometry.sections.len(), 2);
        assert_eq!(service.layout().section(&section("anchor-group")).unwrap().weight, 0.0);
        assert_eq!(service.layout().section(&section("landing")).unwrap().weight, 1.0);
        assert_eq!(service.layout().instance(&id("anchor")).unwrap().memory.weight, 0.0);
        for actual in &geometry.sections {
            let expected = if actual.section == section("anchor-group") { 120.0 } else { 676.0 };
            assert!((actual.outer.size.height - expected).abs() < 0.001);
        }
    }
}

#[test]
fn relative_same_column_moves_preserve_authored_extents_ranks_and_other_saved_weights() {
    for placement in [RelativePlacement::Above, RelativePlacement::Below] {
        let mut service = fixture_service(false).unwrap();
        for (panel, group) in [("anchor", "anchor-group"), ("other", "other-group"), ("mover", "source"), ("closed", "closed-group")] {
            open(&mut service, panel, group);
        }
        service
            .dispatch(transaction(vec![
                Mutation::SetSize { section: section("anchor-group"), size: size(280.0, 300.0) },
                Mutation::SetSize { section: section("source"), size: size(280.0, 200.0) },
                Mutation::SetSize { section: section("closed-group"), size: size(280.0, 350.0) },
                Mutation::Close { instance: id("closed") },
            ]))
            .unwrap();
        let mut layout = service.layout().clone();
        for (group, weight) in layout.sections.iter_mut().zip([0.0002, 0.0004, 0.0009]) {
            group.weight = weight;
        }
        for panel in &mut layout.instances {
            if let Some(group) = layout.sections.iter().find(|s| s.tabs.contains(&panel.id)) {
                panel.memory.weight = group.weight;
            }
        }
        let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
        let before = service.layout().clone();
        let mut env = allocated_pair_environment(&service, 800.0);
        env.gap = 4.0;
        let prepared = service
            .prepare(transaction(vec![Mutation::Move {
                instance: id("mover"),
                destination: Destination::Relative {
                    section: section("anchor-group"),
                    placement,
                    new_section: section("landing"),
                    new_column: None,
                    size: size(280.0, 200.0),
                },
            }]))
            .unwrap();
        let mut expected = before.section(&section("source")).unwrap().clone();
        expected.id = section("landing");
        expected.weight = before.section(&section("anchor-group")).unwrap().weight;
        assert_eq!(prepared.layout().section(&section("landing")).unwrap(), &expected);
        for name in ["anchor-group", "other-group"] {
            assert_eq!(prepared.layout().section(&section(name)).unwrap(), before.section(&section(name)).unwrap());
        }
        assert_eq!(prepared.layout().instance(&id("closed")).unwrap(), before.instance(&id("closed")).unwrap());
        let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
        assert_eq!(shown.sections.len(), 3);
        for actual in &shown.sections {
            let expected = match actual.section.0.as_str() {
                "anchor-group" => 300.0,
                "landing" => 200.0,
                "other-group" => 292.0,
                _ => panic!("unexpected section"),
            };
            assert!((actual.outer.size.height - expected).abs() < 0.001);
        }
        service.commit(prepared).unwrap();
        assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
        service.dispatch(transaction(vec![Mutation::Collapse { section: section("landing"), collapsed: true }])).unwrap();
        let folded = allocate_layout(service.layout(), service.registry(), &env).unwrap();
        assert_eq!(folded.sections.iter().find(|s| s.section == section("landing")).unwrap().outer.size.height, 20.0);
        assert_eq!(service.layout().section(&section("landing")).unwrap(), &Section { collapsed: true, ..expected.clone() });
        service.dispatch(transaction(vec![Mutation::Collapse { section: section("landing"), collapsed: false }])).unwrap();
        assert_eq!(service.layout().section(&section("landing")).unwrap(), &expected);
        assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
    }
}

#[test]
fn relative_requests_reject_self_targets_and_malformed_column_intents_atomically() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let before = service.inspect();
    for (target, placement, column) in [
        ("second", RelativePlacement::Above, None),
        ("first", RelativePlacement::BeforeColumn, None),
        ("first", RelativePlacement::Below, Some(ColumnId::new("unexpected").unwrap())),
    ] {
        reject(
            &mut service,
            vec![Mutation::Move {
                instance: id("two"),
                destination: Destination::Relative {
                    section: section(target),
                    placement,
                    new_section: section("new"),
                    new_column: column,
                    size: size(280.0, 240.0),
                },
            }],
            Rule::Placement,
        );
    }
    assert_eq!(service.inspect(), before);
}

#[test]
fn shared_column_width_includes_inactive_tabs_and_all_stacked_sections() {
    let mut registry = synthetic_registry().unwrap();
    let mut tight = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    tight.kind = PanelKind::new("fixture.tight").unwrap();
    tight.min_size.width = 200.0;
    tight.max_size.width = 320.0;
    registry.register(tight).unwrap();
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.tight").unwrap(),
            instance: id("tight"),
            destination: Some(Destination::JoinTab { section: section("second"), index: 1 }),
        }]))
        .unwrap();
    let column = service.layout().section(&section("first")).unwrap().column.clone().unwrap();
    assert_eq!(column_bounds(service.layout(), service.registry(), &column).unwrap(), (200.0, 320.0));
    assert_eq!(service.layout().section(&section("second")).unwrap().active, id("two"));
    reject(&mut service, vec![Mutation::SetColumnWidth { column: column.clone(), width: 321.0 }], Rule::Bounds);
    reject(&mut service, vec![Mutation::SetSize { section: section("first"), size: size(321.0, 240.0) }], Rule::Bounds);
    service.dispatch(transaction(vec![Mutation::SetColumnWidth { column: column.clone(), width: 300.0 }])).unwrap();
    assert_eq!(service.layout().column(&column).unwrap().width, 300.0);
    for name in ["one", "two", "tight"] {
        let memory = &service.layout().instance(&id(name)).unwrap().memory;
        assert_eq!(memory.column_width, Some(300.0));
        assert_eq!(memory.size.width, 300.0);
    }
}

#[test]
fn column_pairs_preserve_saved_proportions_and_reject_cross_column_or_collapsed_sashes() {
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
    let a = service.layout().section(&section("first")).unwrap().weight;
    let b = service.layout().section(&section("second")).unwrap().weight;
    assert!((a / b - 0.6).abs() < 0.0001);
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: true }])).unwrap();
    reject(&mut service, vec![Mutation::ResetPair { first: section("first"), second: section("second") }], Rule::Placement);
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: false }])).unwrap();
    assert_eq!(service.layout().section(&section("first")).unwrap().weight, a);
    assert_eq!(service.layout().section(&section("second")).unwrap().weight, b);
    let column = next_column_id(service.layout()).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("two"),
            destination: Destination::Relative {
                section: section("first"),
                placement: RelativePlacement::AfterColumn,
                new_section: section("other"),
                new_column: Some(column),
                size: size(280.0, 300.0),
            },
        }]))
        .unwrap();
    reject(&mut service, vec![Mutation::ResetPair { first: section("first"), second: section("other") }], Rule::Placement);
}

#[test]
fn closed_panel_reopens_with_column_identity_width_rank_and_saved_weight() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let column = next_column_id(service.layout()).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("two"),
            destination: Destination::Relative {
                section: section("first"),
                placement: RelativePlacement::AfterColumn,
                new_section: section("inner"),
                new_column: Some(column.clone()),
                size: size(310.0, 300.0),
            },
        }]))
        .unwrap();
    service
        .dispatch(transaction(vec![Mutation::Collapse { section: section("inner"), collapsed: true }, Mutation::Close { instance: id("two") }]))
        .unwrap();
    let memory = service.layout().instance(&id("two")).unwrap().memory.clone();
    assert_eq!(memory.column, Some(column.clone()));
    assert_eq!(memory.column_index, 1);
    assert_eq!(memory.column_width, Some(310.0));
    assert_eq!(memory.weight, 300.0);
    assert_ne!(next_column_id(service.layout()).unwrap(), column);
    service
        .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("two"), destination: None }]))
        .unwrap();
    let restored = service.layout().containing(&id("two")).unwrap();
    assert_eq!(restored.id, memory.section);
    assert_eq!(restored.column, memory.column);
    assert_eq!(restored.weight, memory.weight);
    assert!(restored.collapsed);
}

#[test]
fn native_redock_preserves_original_section_column_width_and_proportion() {
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    let original = service.layout().section(&section("first")).unwrap().clone();
    let column = original.column.clone().unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("one"),
            destination: Destination::NewSection {
                section: section("native"),
                index: 0,
                size: size(320.0, 280.0),
                location: SectionLocation::Window {
                    host: HostId::new("host").unwrap(),
                    origin: Point { x: 10.0, y: 10.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 0,
                        size: original.size,
                        collapsed: original.collapsed,
                        join: None,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 1.0,
                        preferred_extent: None,
                        extent_priority: 0,
                    },
                },
            },
        }]))
        .unwrap();
    let SectionLocation::Window { return_to, .. } = &service.layout().containing(&id("one")).unwrap().location else {
        panic!("native destination");
    };
    assert_eq!(return_to.section, Some(original.id.clone()));
    assert_eq!(return_to.column, Some(column.clone()));
    assert_eq!(return_to.weight, original.weight);
    assert_ne!(next_section_id(service.layout()).unwrap(), original.id);
    assert_ne!(next_column_id(service.layout()).unwrap(), column);
    service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("host").unwrap() }])).unwrap();
    assert_eq!(service.layout().containing(&id("one")).unwrap(), &original);
}

#[test]
fn command_drag_activation_is_atomic_and_background_join_keeps_destination_active() {
    let mut backend = command_ready_backend();
    let before = backend.service.inspect();
    backend
        .execute("panel.move", &serde_json::json!({"target":command_target("two"),"destination":{"type":"join_tab","section":"s-one","index":1}}))
        .unwrap();
    assert_eq!(backend.service.layout().section(&section("s-one")).unwrap().active, id("one"));
    let revision = backend.service.inspect().revision;
    backend
        .execute(
            "panel.move",
            &serde_json::json!({"target":command_target("two"),"activate":true,"destination":{"type":"join_tab","section":"s-one","index":0}}),
        )
        .unwrap();
    assert_eq!(backend.service.inspect().revision, revision + 1);
    assert_eq!(backend.service.layout().section(&section("s-one")).unwrap().active, id("two"));
    assert_eq!(backend.service.inspect().revision, before.revision + 2);
    let committed = backend.service.inspect();
    assert!(
        backend
            .execute(
                "panel.move",
                &serde_json::json!({"target":command_target("one"),"activate":true,"destination":{"type":"join_tab","section":"s-one","index":9}})
            )
            .is_err()
    );
    assert_eq!(backend.service.inspect(), committed);
}

#[test]
fn pin_follows_other_side_allows_ordinary_tabs_and_rejects_column_displacement() {
    let mut service = fixture_service(false).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.pin", "pin", "pinned", Zone::Left, 0).unwrap()])).unwrap();
    open(&mut service, "ordinary", "ordinary-section");
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("ordinary"),
            destination: Destination::JoinTab { section: section("pinned"), index: 1 },
        }]))
        .unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("pin"),
            destination: Destination::NewSection {
                section: section("right-pin"),
                index: 1,
                location: SectionLocation::Dock { zone: Zone::Right },
                size: size(280.0, 240.0),
            },
        }]))
        .unwrap();
    let column = next_column_id(service.layout()).unwrap();
    reject(
        &mut service,
        vec![Mutation::Move {
            instance: id("ordinary"),
            destination: Destination::Relative {
                section: section("right-pin"),
                placement: RelativePlacement::BeforeColumn,
                new_section: section("displacing"),
                new_column: Some(column),
                size: size(280.0, 240.0),
            },
        }],
        Rule::PinOrder,
    );
    assert_eq!(service.layout().containing(&id("pin")).unwrap().location.zone(), Zone::Right);
}

#[test]
fn header_action_intents_validate_registration_active_state_and_availability() {
    let mut registry = synthetic_registry().unwrap();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("fixture.action").unwrap();
    spec.header_actions = vec![HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() }];
    registry.register(spec).unwrap();
    let mut service =
        LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.action", "action", "actions", Zone::Left, 0).unwrap()])).unwrap();
    let action = ControlId::new("refresh").unwrap();
    let reply = service.dispatch(transaction(vec![Mutation::HeaderAction { instance: id("action"), action: action.clone() }])).unwrap();
    assert_eq!(reply.effects, vec![HostEffect::HeaderAction { instance: id("action"), action: action.clone() }]);
    reject(&mut service, vec![Mutation::HeaderAction { instance: id("action"), action: ControlId::new("unknown").unwrap() }], Rule::Capability);
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("actions"), collapsed: true }])).unwrap();
    assert!(
        service.dispatch(transaction(vec![Mutation::HeaderAction { instance: id("action"), action: action.clone() }])).is_ok(),
        "collapsed headers retain custom actions"
    );
    service
        .dispatch(transaction(vec![
            Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("other"),
                destination: Some(Destination::JoinTab { section: section("actions"), index: 1 }),
            },
            Mutation::Activate { instance: id("other") },
        ]))
        .unwrap();
    reject(&mut service, vec![Mutation::HeaderAction { instance: id("action"), action: action.clone() }], Rule::Availability);
    service.dispatch(transaction(vec![Mutation::Activate { instance: id("action") }])).unwrap();
    service.set_availability([(id("action"), Availability::Unavailable { reason: "owner unavailable".into() })].into()).unwrap();
    reject(&mut service, vec![Mutation::HeaderAction { instance: id("action"), action: action.clone() }], Rule::Availability);
    service.set_availability(AvailabilityMap::new()).unwrap();
    let catalogue = fixture_command_catalogue(&registry).unwrap();
    let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
    let before = backend.service.inspect();
    assert_eq!(
        backend.execute("panel.action", &serde_json::json!({"target":command_target("action"),"action":"refresh"})).unwrap_err().rule,
        Rule::Capability
    );
    assert_eq!(backend.service.inspect(), before, "missing controller cannot publish an action effect");
}

#[test]
fn geometry_admission_failure_keeps_candidate_private_and_consumes_no_identity() {
    let mut service = fixture_service(false).unwrap();
    let before = service.inspect();
    let section_id = next_section_id(service.layout()).unwrap();
    let mutation = fixture_open("fixture.log", "one", &section_id.0, Zone::Left, 0).unwrap();
    let proposed = service.prepare_admitted(transaction(vec![mutation.clone()]), |committed, next| {
        assert!(committed.sections.is_empty());
        assert_eq!(next.sections.len(), 1);
        Err(error(Rule::NoSpace, "minimums cannot fit destination"))
    });
    assert!(matches!(proposed, Err(PanelError { rule: Rule::NoSpace, .. })));
    assert_eq!(service.inspect(), before);
    assert_eq!(next_section_id(service.layout()).unwrap(), section_id);
    let admitted = service.prepare_admitted(transaction(vec![mutation]), |_, _| Ok(())).unwrap();
    service.commit(admitted).unwrap();
    assert_eq!(service.layout().sections[0].id, section_id);
}

#[test]
fn v1_migration_preserves_unknown_wire_fields_and_explicit_saved_proportions() {
    let registry = synthetic_registry().unwrap();
    let fallback = persistence_fallback();
    let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
    raw["future_envelope"] = serde_json::json!({"version":37,"keep":[1,2,3]});
    raw["layout"]["future_layout"] = serde_json::json!({"intent":"preserve"});
    raw["layout"]["sections"][0]["future_section"] = serde_json::json!({"keep":true});
    raw["layout"]["sections"][0]["weight"] = serde_json::json!(2.5);
    raw["layout"]["instances"][0]["memory"]["future_memory"] = serde_json::json!({"draft":"saved"});
    raw["layout"]["instances"][0]["memory"]["weight"] = serde_json::json!(2.5);
    let mut unknown = raw["layout"]["instances"][0].clone();
    unknown["id"] = serde_json::json!("unknown");
    unknown["kind"] = serde_json::json!("extension.panel");
    unknown["memory"]["section"] = serde_json::json!("unknown-section");
    unknown["memory"]["section_index"] = serde_json::json!(1);
    unknown["future_payload"] = serde_json::json!({"draft":"retained"});
    raw["layout"]["instances"].as_array_mut().unwrap().push(unknown);
    let mut unknown_section = raw["layout"]["sections"][0].clone();
    unknown_section["id"] = serde_json::json!("unknown-section");
    unknown_section["tabs"] = serde_json::json!(["unknown"]);
    unknown_section["active"] = serde_json::json!("unknown");
    raw["layout"]["sections"].as_array_mut().unwrap().push(unknown_section);
    raw["section_order"].as_array_mut().unwrap().push(serde_json::json!("unknown-section"));
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    assert!(loaded.rejected.is_none());
    assert_eq!(loaded.envelope.layout.sections[0].weight, 2.5);
    assert_eq!(loaded.envelope.quarantine.len(), 1);
    assert!(loaded.envelope.layout.instance(&id("unknown")).is_err());
    let pending = loaded.envelope.quarantine[0].instance_data.clone();
    assert_eq!(pending["future_payload"]["draft"], "retained");
    let mut service = LayoutService::new(registry.clone(), loaded.envelope.layout.clone(), HostCapabilities::default()).unwrap();
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }])).unwrap();
    let edited = with_live_layout(&loaded.envelope, service.layout().clone(), &registry).unwrap();
    assert_eq!(edited.quarantine[0].instance_data, pending);
    let mut unexplained = loaded.envelope.clone();
    unexplained.recovery_wire.as_mut().unwrap()["layout"]["instances"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"id":"unexplained","future_payload":true}));
    assert_eq!(with_live_layout(&unexplained, service.layout().clone(), &registry).unwrap_err().rule, Rule::MissingTarget);
    let encoded: serde_json::Value = serde_json::from_slice(&encode(&edited, &registry).unwrap()).unwrap();
    assert_eq!(encoded["future_envelope"], raw["future_envelope"]);
    assert_eq!(encoded["layout"]["future_layout"], raw["layout"]["future_layout"]);
    assert_eq!(encoded["layout"]["sections"][0]["future_section"], raw["layout"]["sections"][0]["future_section"]);
    assert_eq!(encoded["layout"]["instances"][0]["memory"]["future_memory"], raw["layout"]["instances"][0]["memory"]["future_memory"]);
    assert_eq!(encoded["layout"]["sections"][0]["weight"], 2.5);
    assert_eq!(encoded["layout"]["sections"][0]["collapsed"], true);
    let roundtrip = decode(&serde_json::to_vec(&encoded).unwrap(), &registry, &fallback).unwrap();
    assert!(roundtrip.notice.is_none());
    assert_eq!(roundtrip.envelope.quarantine[0].instance_data, pending);
    let mut restored = roundtrip.envelope;
    let mut grown = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    grown.register(spec).unwrap();
    restore_quarantine(&mut restored, &grown).unwrap();
    assert!(restored.quarantine.is_empty());
    assert_eq!(restored.layout.section(&section("unknown-section")).unwrap().tabs, vec![id("unknown")]);
    assert!(restored.layout.section(&section("editor-section")).unwrap().collapsed);
    assert_eq!(restored.retained_data[&id("unknown")]["instance"], pending);
    let restored = decode(&encode(&restored, &grown).unwrap(), &grown, &fallback).unwrap();
    assert!(restored.notice.is_none());
    assert_eq!(restored.envelope.retained_data[&id("unknown")]["instance"], pending);
    let mut future = encoded;
    future["layout"]["columns"][0]["future_column"] = serde_json::json!({"keep":"column intent"});
    let loaded = decode(&serde_json::to_vec(&future).unwrap(), &registry, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    let replaced = with_live_layout(&loaded.envelope, Layout::empty(fallback.workspace.clone()), &registry).unwrap();
    assert!(replaced.layout.instances.is_empty() && replaced.layout.sections.is_empty());
    let preserved: serde_json::Value = serde_json::from_slice(&encode(&replaced, &registry).unwrap()).unwrap();
    assert_eq!(preserved["retained_data"]["editor"]["instance"]["memory"]["future_memory"]["draft"], "saved");
    assert_eq!(preserved["section_snapshots"]["editor-section"]["data"]["future_section"]["keep"], true);
    assert_eq!(preserved["layout"]["columns"][0]["future_column"]["keep"], "column intent");
}

#[test]
fn quarantined_and_closed_memories_reserve_section_and_column_allocation() {
    let mut raw = future_group();
    raw["layout"]["sections"][0]["id"] = serde_json::json!("section-1");
    raw["layout"]["sections"][0]["column"] = serde_json::json!("column-1");
    raw["layout"]["columns"][0]["id"] = serde_json::json!("column-1");
    raw["section_order"] = serde_json::json!(["section-1"]);
    for instance in raw["layout"]["instances"].as_array_mut().unwrap() {
        instance["memory"]["section"] = serde_json::json!("section-1");
        instance["memory"]["column"] = serde_json::json!("column-1");
    }
    let none = future_registry(&[]);
    let fallback = persistence_fallback();
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &none, &fallback).unwrap();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.envelope.quarantine.len(), 3);
    let reserved = reserve_envelope_identities(&loaded.envelope).unwrap();
    assert_eq!(next_section_id(&reserved).unwrap(), section("section-2"));
    assert_eq!(next_column_id(&reserved).unwrap(), ColumnId::new("column-2").unwrap());
    let mut restored = loaded.envelope;
    restore_quarantine(&mut restored, &future_registry(&["a", "b", "c"])).unwrap();
    assert!(restored.quarantine.is_empty());
    assert_eq!(restored.layout.sections[0].id, section("section-1"));
    assert_eq!(restored.layout.sections[0].column, Some(ColumnId::new("column-1").unwrap()));
}

#[test]
fn v1_live_native_closed_and_quarantined_snapshots_migrate_without_losing_weights() {
    use serde_json::json;
    let registry = synthetic_registry().unwrap();
    let fallback = persistence_fallback();
    let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
    let closed = json!({"id":"closed","kind":"fixture.log","requested_open":false,"memory":{
        "section":"closed-section","location":{"type":"dock","zone":"left"},"section_index":1,"tab_index":0,
        "size":{"width":280,"height":360},"collapsed":true
    }});
    let native_location = json!({"type":"window","host":"native-host","origin":{"x":20,"y":30},"return_to":{
        "zone":"left","index":1,"size":{"width":280,"height":260},"collapsed":false,
        "join":{"section":"editor-section","index":1},"weight":7.0,"future_monitor":"keep"
    }});
    let native = json!({"id":"native","kind":"fixture.log","requested_open":true,"memory":{
        "section":"native-section","location":native_location,"section_index":1,"tab_index":0,
        "size":{"width":300,"height":260},"collapsed":false,"weight":3.5
    }});
    let native_section = json!({"id":"native-section","location":native["memory"]["location"],"tabs":["native"],"active":"native",
        "size":{"width":300,"height":260},"collapsed":false,"weight":3.5});
    raw["layout"]["instances"].as_array_mut().unwrap().extend([closed, native]);
    raw["layout"]["sections"].as_array_mut().unwrap().push(native_section);
    let unknown = json!({"id":"unknown","kind":"extension.panel","requested_open":true,"future_payload":{"draft":"retained"},"memory":{
        "section":"dormant-section","location":{"type":"dock","zone":"right"},"section_index":2,"tab_index":0,
        "size":{"width":280,"height":180},"collapsed":false,"weight":9.0
    }});
    raw["quarantine"] = json!([{"instance":"unknown","kind":"extension.panel","instance_data":unknown,"section":"dormant-section"}]);
    raw["section_snapshots"] = json!({"dormant-section":{"index":2,"data":{
        "id":"dormant-section","location":{"type":"dock","zone":"right"},"tabs":["unknown"],"active":"unknown",
        "size":{"width":280,"height":180},"collapsed":false,"weight":9.0,"future_snapshot":"keep"
    }}});
    raw["section_order"] = json!(["editor-section", "native-section", "dormant-section"]);
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &fallback).unwrap();
    assert!(loaded.notice.is_none(), "{:?}", loaded.notice);
    let mut envelope = loaded.envelope;
    let closed = &envelope.layout.instance(&id("closed")).unwrap().memory;
    assert_eq!(closed.weight, 360.0);
    assert!(closed.column.is_some());
    assert_eq!(closed.column_width, Some(280.0));
    let native = envelope.layout.instance(&id("native")).unwrap();
    assert_eq!(native.memory.weight, 3.5);
    let SectionLocation::Window { return_to, .. } = &native.memory.location else {
        panic!("native memory");
    };
    assert_eq!(return_to.weight, 7.0);
    assert_eq!(return_to.column, envelope.layout.sections[0].column);
    let pending: PanelInstance = serde_json::from_value(envelope.quarantine[0].instance_data.clone()).unwrap();
    assert_eq!(pending.memory.weight, 9.0);
    assert!(pending.memory.column.is_some());
    let mut with_extension = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    with_extension.register(spec).unwrap();
    restore_quarantine(&mut envelope, &with_extension).unwrap();
    assert!(envelope.quarantine.is_empty());
    assert_eq!(envelope.layout.section(&section("dormant-section")).unwrap().weight, 9.0);
    let encoded: serde_json::Value = serde_json::from_slice(&encode(&envelope, &with_extension).unwrap()).unwrap();
    assert_eq!(encoded["section_snapshots"]["dormant-section"]["data"]["future_snapshot"], "keep");
    assert_eq!(encoded["layout"]["instances"][2]["memory"]["location"]["return_to"]["future_monitor"], "keep");
    assert_eq!(envelope.retained_data[&id("unknown")]["instance"]["future_payload"]["draft"], "retained");
}

#[test]
fn v1_distinct_preferred_widths_remain_saved_in_the_original_vertical_stack() {
    use serde_json::json;
    let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
    for (index, width) in [(1, 320), (2, 280)] {
        let name = format!("log-{index}");
        let group = format!("group-{index}");
        raw["layout"]["instances"].as_array_mut().unwrap().push(json!({"id":name,"kind":"fixture.log","requested_open":true,"memory":{
            "section":group,"location":{"type":"dock","zone":"left"},"section_index":index,"tab_index":0,
            "size":{"width":width,"height":200},"collapsed":false
        }}));
        raw["layout"]["sections"].as_array_mut().unwrap().push(json!({"id":group,"location":{"type":"dock","zone":"left"},
            "tabs":[name],"active":name,"size":{"width":width,"height":200},"collapsed":false}));
        raw["section_order"].as_array_mut().unwrap().push(json!(group));
    }
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &synthetic_registry().unwrap(), &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let layout = loaded.envelope.layout;
    assert_eq!(layout.columns.iter().map(|c| c.width).collect::<Vec<_>>(), vec![280.0]);
    assert_eq!(layout.sections.iter().map(|s| s.weight).collect::<Vec<_>>(), vec![240.0, 200.0, 200.0]);
    assert_eq!(layout.sections.iter().map(|s| s.size.width).collect::<Vec<_>>(), vec![280.0, 320.0, 280.0]);
    assert!(layout.sections.iter().all(|s| s.column.as_ref() == Some(&layout.columns[0].id)));
}

fn allocated_pair_environment(service: &LayoutService, height: f32) -> AllocationEnvironment {
    AllocationEnvironment {
        capacity: LogicalRect { origin: Point { x: 11.0, y: 17.0 }, size: size(1200.0, height) },
        direction: Direction::Ltr,
        header_height: 20.0,
        footer_height: 0.0,
        gap: 0.0,
        inline_max_height: 88.0,
        min_viewport: size(1.0, 1.0),
        scroll: Point { x: 0.0, y: 0.0 },
        available: service.layout().instances.iter().map(|p| p.id.clone()).collect(),
        content: Vec::new(),
        content_revision: 0,
    }
}

#[test]
fn oversubscribed_highest_tier_fractional_residue_never_reaches_lower_preference_or_weight() {
    for weighted_lower in [false, true] {
        let mut registry = PanelRegistry::default();
        let mut spec = synthetic_registry().unwrap().get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.min_size.height = 1.0;
        registry.register(spec).unwrap();
        let mut service =
            LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
        for (panel, group) in [("one", "first"), ("two", "second"), ("three", "third")] {
            open(&mut service, panel, group);
        }
        let mut layout = service.layout().clone();
        for (group, extent) in layout.sections.iter_mut().zip([2.0, 3.0, 2.0]) {
            let lower = group.id == section("third");
            group.preferred_extent = if lower && weighted_lower { None } else { Some(extent) };
            group.extent_priority = if lower { if weighted_lower { 0 } else { 1 } } else { 2 };
            group.weight = if lower { 1000.0 } else { 1.0 };
        }
        for panel in &mut layout.instances {
            let group = layout.sections.iter().find(|s| s.tabs.contains(&panel.id)).unwrap();
            panel.memory.preferred_extent = group.preferred_extent;
            panel.memory.extent_priority = group.extent_priority;
            panel.memory.weight = group.weight;
        }
        let service = LayoutService::new(registry.clone(), layout, HostCapabilities::default()).unwrap();
        let mut environment = allocated_pair_environment(&service, 4.0);
        environment.header_height = 1.0;
        let geometry = allocate_layout(service.layout(), &registry, &environment).unwrap();
        assert_eq!(geometry.sections.len(), 3);
        let height = |name| geometry.sections.iter().find(|s| s.section == section(name)).unwrap().outer.size.height;
        // Minima consume three units. The highest tier requests three surplus
        // units and receives the remaining one in the ratio 1:2. Inward f32
        // rounding must not turn its residue into a lower tier allocation.
        assert!((height("first") - 4.0 / 3.0).abs() < 0.000001);
        assert!((height("second") - 5.0 / 3.0).abs() < 0.000001);
        assert_eq!(height("third"), 1.0, "an oversubscribed tier exhausts all surplus, including fractional residue");
        assert!(geometry.sections.iter().map(|s| f64::from(s.outer.size.height)).sum::<f64>() <= 4.0);
    }
}

#[test]
fn allocated_pair_reaches_actual_minimum_and_maximum_endpoints_and_returns_without_drift() {
    for (height, low, high) in [(800.0, 120.0, 680.0), (1320.0, 120.0, 1200.0)] {
        for swapped in [false, true] {
            let mut service = fixture_service(false).unwrap();
            open(&mut service, "one", "first");
            open(&mut service, "two", "second");
            let env = allocated_pair_environment(&service, height);
            let initial = allocate_layout(service.layout(), service.registry(), &env).unwrap();
            let (a, b) = normalized_pair_weights(service.layout(), &section("first"), &section("second")).unwrap();
            assert_eq!((a, b), (0.5, 0.5));
            let (first_height, second_height, first_weight, second_weight) = if swapped { (high, low, 1.0, 0.0) } else { (low, high, 0.0, 1.0) };
            let before = service.inspect();
            let prepared = service
                .prepare(transaction(vec![Mutation::ResizeAllocatedPair {
                    first: section("first"),
                    second: section("second"),
                    first_size: size(280.0, first_height),
                    second_size: size(280.0, second_height),
                    first_weight,
                    second_weight,
                    other_weight_scale: 1.0,
                    prefer_allocated: false,
                }]))
                .unwrap();
            assert_eq!(service.inspect(), before);
            let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
            assert!((shown.sections[0].outer.size.height - first_height).abs() < 0.001);
            assert!((shown.sections[1].outer.size.height - second_height).abs() < 0.001);
            service.commit(prepared).unwrap();
            assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
            service
                .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
                    first: section("first"),
                    second: section("second"),
                    first_size: size(280.0, height / 2.0),
                    second_size: size(280.0, height / 2.0),
                    first_weight: 0.5,
                    second_weight: 0.5,
                    other_weight_scale: 1.0,
                    prefer_allocated: false,
                }]))
                .unwrap();
            assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), initial);
        }
    }
}

#[test]
fn allocated_pair_preserves_saturated_third_rectangle_and_all_saved_group_shares() {
    let mut registry = synthetic_registry().unwrap();
    let mut short = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    short.kind = PanelKind::new("fixture.short").unwrap();
    short.default_size.height = 180.0;
    short.max_size.height = 180.0;
    registry.register(short).unwrap();
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.short").unwrap(),
            instance: id("short"),
            destination: Some(Destination::NewSection {
                section: section("third"),
                index: 2,
                location: SectionLocation::Dock { zone: Zone::Left },
                size: size(280.0, 180.0),
            }),
        }]))
        .unwrap();
    open(&mut service, "closed", "remembered");
    service.dispatch(transaction(vec![Mutation::Close { instance: id("closed") }])).unwrap();
    let env = allocated_pair_environment(&service, 1000.0);
    let initial = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    let (a, b) = normalized_pair_weights(service.layout(), &section("first"), &section("second")).unwrap();
    let total = 240.0 + 240.0 + 180.0 + 240.0;
    assert!((a - 240.0 / total).abs() < WEIGHT_SHARE_TOLERANCE);
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 120.0),
            second_size: size(280.0, 700.0),
            first_weight: 0.0,
            second_weight: a + b,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    let after = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(after.sections[2].outer.size, initial.sections[2].outer.size);
    assert_eq!(after.sections[2].outer.origin.x, initial.sections[2].outer.origin.x);
    assert!(
        (after.sections[2].outer.origin.y - initial.sections[2].outer.origin.y).abs() < 0.001,
        "unrelated saturated neighbour keeps its rectangle within float precision"
    );
    assert!((after.sections[0].outer.size.height - 120.0).abs() < 0.001);
    assert!((after.sections[1].outer.size.height - 700.0).abs() < 0.001);
    assert!((service.layout().sections[2].weight - 180.0 / total).abs() < WEIGHT_SHARE_TOLERANCE);
    assert!((service.layout().instance(&id("closed")).unwrap().memory.weight - 240.0 / total).abs() < WEIGHT_SHARE_TOLERANCE);
    let before_collapse = service.layout().sections[0].weight;
    service
        .dispatch(transaction(vec![
            Mutation::Collapse { section: section("first"), collapsed: true },
            Mutation::Collapse { section: section("first"), collapsed: false },
        ]))
        .unwrap();
    assert_eq!(service.layout().sections[0].weight, before_collapse);
}

#[test]
fn allocated_pair_rejects_hostile_shares_and_bounds_atomically_and_handles_large_legacy_weights() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    for (first_weight, second_weight) in [(f32::NAN, 0.5), (0.5, f32::INFINITY), (-0.1, 1.1), (0.0, 0.0)] {
        reject(
            &mut service,
            vec![Mutation::ResizeAllocatedPair {
                first: section("first"),
                second: section("second"),
                first_size: size(280.0, 120.0),
                second_size: size(280.0, 680.0),
                first_weight,
                second_weight,
                other_weight_scale: 1.0,
                prefer_allocated: false,
            }],
            Rule::Bounds,
        );
    }
    reject(
        &mut service,
        vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 119.0),
            second_size: size(280.0, 681.0),
            first_weight: 0.0,
            second_weight: 1.0,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }],
        Rule::Bounds,
    );
    let mut large = service.layout().clone();
    for group in &mut large.sections {
        group.weight = MAX_LOGICAL;
    }
    let mut large = LayoutService::new(service.registry().clone(), large, HostCapabilities::default()).unwrap();
    assert_eq!(normalized_pair_weights(large.layout(), &section("first"), &section("second")).unwrap(), (0.5, 0.5));
    large
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 120.0),
            second_size: size(280.0, 680.0),
            first_weight: 0.0,
            second_weight: 1.0,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    assert_eq!(large.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), vec![0.0, 1.0]);
    let mut zero = service.layout().clone();
    for group in &mut zero.sections {
        group.weight = 0.0;
    }
    for p in &mut zero.instances {
        p.memory.weight = 0.0;
    }
    assert_eq!(normalized_pair_weights(&zero, &section("first"), &section("second")).unwrap_err().rule, Rule::Bounds);
}

#[test]
fn allocated_bottom_pair_reaches_minimum_in_rtl_with_fractional_actual_sizes() {
    let mut service = fixture_service(false).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.log", "one", "first", Zone::Bottom, 0).unwrap(),
            fixture_open("fixture.log", "two", "second", Zone::Bottom, 1).unwrap(),
        ]))
        .unwrap();
    let mut env = allocated_pair_environment(&service, 600.0);
    env.direction = Direction::Rtl;
    env.capacity.size.width = 800.5;
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(160.0, 240.0),
            second_size: size(640.5, 240.0),
            first_weight: 0.0,
            second_weight: 1.0,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    let geometry = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert!((geometry.sections[0].outer.size.width - 160.0).abs() < 0.001);
    assert!((geometry.sections[1].outer.size.width - 640.5).abs() < 0.001);
    assert!(geometry.sections[0].outer.origin.x > geometry.sections[1].outer.origin.x);
    assert_eq!(geometry.sections[1].outer.origin.x, env.capacity.origin.x);
}

#[test]
fn allocated_pair_normalizes_collapsed_and_unavailable_surrounding_sections_without_resizing_them() {
    let mut service = fixture_service(false).unwrap();
    for (instance, group) in [("one", "first"), ("two", "second"), ("folded", "folded"), ("dormant", "dormant")] {
        open(&mut service, instance, group);
    }
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("folded"), collapsed: true }])).unwrap();
    service.set_availability([(id("dormant"), Availability::Unavailable { reason: "owner unavailable".into() })].into()).unwrap();
    let mut env = allocated_pair_environment(&service, 800.0);
    env.available.retain(|instance| *instance != id("dormant"));
    let before = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    let shares = normalized_pair_weights(service.layout(), &section("first"), &section("second")).unwrap();
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 120.0),
            second_size: size(280.0, 660.0),
            first_weight: 0.0,
            second_weight: shares.0 + shares.1,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    let after = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(after.sections.len(), 3);
    assert_eq!(after.sections[2].outer, before.sections[2].outer);
    assert_eq!(service.layout().section(&section("folded")).unwrap().weight, 0.25);
    assert_eq!(service.layout().section(&section("dormant")).unwrap().weight, 0.25);
    assert_eq!(service.layout().section(&section("dormant")).unwrap().size, size(280.0, 240.0));
}

fn legacy_instance_wire(
    instance: &str,
    kind: &str,
    group: &str,
    location: serde_json::Value,
    preferred: Size,
    index: usize,
    requested_open: bool,
) -> serde_json::Value {
    serde_json::json!({"id":instance,"kind":kind,"requested_open":requested_open,"memory":{
        "section":group,"location":location,"section_index":index,"tab_index":0,"size":preferred,"collapsed":false
    }})
}

fn legacy_section_wire(group: &str, instance: &str, location: serde_json::Value, preferred: Size) -> serde_json::Value {
    serde_json::json!({"id":group,"location":location,"tabs":[instance],"active":instance,"size":preferred,"collapsed":false})
}

#[test]
fn complete_v1_order_splits_compatible_live_peers_around_a_disjoint_dormant_section() {
    use serde_json::json;
    let mut registry = synthetic_registry().unwrap();
    for (kind, min, preferred, max) in [("fixture.narrow", 160.0, 200.0, 240.0), ("extension.wide", 360.0, 400.0, 440.0)] {
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new(kind).unwrap();
        spec.min_size.width = min;
        spec.default_size.width = preferred;
        spec.max_size.width = max;
        registry.register(spec).unwrap();
    }
    let dock = json!({"type":"dock","zone":"left"});
    let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
    raw["layout"]["instances"] = json!([
        legacy_instance_wire("a", "fixture.narrow", "a-section", dock.clone(), size(200.0, 240.0), 0, true),
        legacy_instance_wire("c", "fixture.narrow", "c-section", dock.clone(), size(200.0, 240.0), 2, true)
    ]);
    raw["layout"]["sections"] = json!([
        legacy_section_wire("a-section", "a", dock.clone(), size(200.0, 240.0)),
        legacy_section_wire("c-section", "c", dock.clone(), size(200.0, 240.0))
    ]);
    let mut pending = legacy_instance_wire("b", "extension.wide", "b-section", dock.clone(), size(400.0, 240.0), 1, true);
    pending["future_payload"] = json!({"keep":"draft"});
    raw["quarantine"] = json!([{"instance":"b","kind":"extension.wide","section":"b-section","instance_data":pending}]);
    let mut snapshot = legacy_section_wire("b-section", "b", dock, size(400.0, 240.0));
    snapshot["location"]["future_monitor"] = json!({"keep":"monitor"});
    raw["section_snapshots"] = json!({"b-section":{"index":1,"data":snapshot}});
    raw["section_order"] = json!(["a-section", "b-section", "c-section"]);
    let bytes = serde_json::to_vec(&raw).unwrap();
    let mut absent = PanelRegistry::default();
    for spec in registry.specs().filter(|spec| spec.kind.0 != "extension.wide") {
        absent.register(spec.clone()).unwrap();
    }
    for current in [&registry, &absent] {
        let loaded = decode(&bytes, current, &persistence_fallback()).unwrap();
        assert!(loaded.notice.is_none(), "{:?}", loaded.notice);
        assert_eq!(loaded.envelope.retained_v1_bytes(), Some(bytes.as_slice()));
        let mut envelope = loaded.envelope;
        restore_quarantine(&mut envelope, &registry).unwrap();
        assert!(envelope.quarantine.is_empty());
        assert_eq!(
            envelope.layout.sections.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            vec![section("a-section"), section("b-section"), section("c-section")]
        );
        assert_eq!(envelope.layout.columns.iter().map(|c| c.width).collect::<Vec<_>>(), vec![200.0, 400.0, 200.0]);
        assert_eq!(
            envelope.layout.sections.iter().map(|s| s.column.clone().unwrap()).collect::<Vec<_>>(),
            envelope.layout.columns.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        );
        assert_eq!(envelope.retained_data[&id("b")]["instance"]["future_payload"]["keep"], "draft");
        assert_eq!(envelope.section_snapshots[&section("b-section")].data["location"]["future_monitor"]["keep"], "monitor");
    }
}

#[test]
fn closed_quarantine_section_and_native_return_reservations_reject_unrelated_commands_then_restore() {
    use serde_json::json;
    for native_memory in [false, true] {
        let registry = synthetic_registry().unwrap();
        let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
        raw["layout"]["instances"] = json!([]);
        raw["layout"]["sections"] = json!([]);
        raw["section_order"] = json!([]);
        let location = if native_memory {
            json!({"type":"window","host":"remembered-host","origin":{"x":20,"y":30},
            "return_to":{"zone":"left","index":0,"section":"section-1","size":{"width":280,"height":240},"collapsed":false}})
        } else {
            json!({"type":"floating","origin":{"x":20,"y":30}})
        };
        let remembered = if native_memory { "native-memory" } else { "section-1" };
        let pending = legacy_instance_wire("extension", "extension.panel", remembered, location, size(280.0, 240.0), 0, false);
        raw["quarantine"] = json!([{"instance":"extension","kind":"extension.panel","section":null,"instance_data":pending}]);
        let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
        assert!(loaded.notice.is_none());
        let envelope = loaded.envelope;
        let service = LayoutService::new(registry.clone(), envelope.layout.clone(), HostCapabilities::default()).unwrap();
        let mut catalogue = commands::InMemoryCatalogue::default();
        catalogue.remember(&envelope, None, &registry).unwrap();
        let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
        let before = backend.service.inspect();
        let command = json!({"kind":"fixture.log","instance":"unrelated","destination":{"type":"new_section","section":"section-1","index":0,
            "location":{"type":"floating","origin":{"x":40,"y":40}},"size":{"width":280,"height":240}}});
        assert_eq!(backend.execute("panel.open", &command).unwrap_err().rule, Rule::DuplicateId);
        assert_eq!(backend.service.inspect(), before);
        let mut grown = registry.clone();
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new("extension.panel").unwrap();
        grown.register(spec).unwrap();
        let mut restored = envelope;
        restore_quarantine(&mut restored, &grown).unwrap();
        assert!(restored.quarantine.is_empty());
        assert!(!restored.layout.instance(&id("extension")).unwrap().requested_open);
        if !native_memory {
            let mut service = LayoutService::new(grown.clone(), restored.layout.clone(), HostCapabilities::default()).unwrap();
            service
                .dispatch(transaction(vec![Mutation::Open {
                    kind: PanelKind::new("extension.panel").unwrap(),
                    instance: id("extension"),
                    destination: None,
                }]))
                .unwrap();
            assert_eq!(service.layout().containing(&id("extension")).unwrap().id, section("section-1"));
            with_live_layout(&restored, service.layout().clone(), &grown).unwrap();
        }
    }
}

#[test]
fn missing_v1_native_return_weights_use_saved_extent_for_live_closed_and_redocked_groups() {
    use serde_json::json;
    let registry = synthetic_registry().unwrap();
    let dock = json!({"type":"dock","zone":"left"});
    let native_location = json!({"type":"window","host":"host","origin":{"x":20,"y":30},"return_to":{
        "zone":"left","section":"returned","index":1,"size":{"width":280,"height":360},"collapsed":false}});
    let closed_location = json!({"type":"window","host":"closed-host","origin":{"x":20,"y":30},"return_to":{
        "zone":"left","section":"closed-return","index":2,"size":{"width":280,"height":300},"collapsed":false}});
    let mut raw: serde_json::Value = serde_json::from_slice(FIXTURE_LAYOUT_V1).unwrap();
    raw["layout"]["instances"] = json!([
        legacy_instance_wire("sibling", "fixture.log", "sibling", dock.clone(), size(280.0, 240.0), 0, true),
        legacy_instance_wire("native", "fixture.log", "native", native_location.clone(), size(280.0, 360.0), 1, true),
        legacy_instance_wire("closed", "fixture.log", "closed", closed_location, size(280.0, 300.0), 2, false)
    ]);
    raw["layout"]["sections"] = json!([
        legacy_section_wire("sibling", "sibling", dock, size(280.0, 240.0)),
        legacy_section_wire("native", "native", native_location, size(280.0, 360.0))
    ]);
    raw["section_order"] = json!(["sibling", "native"]);
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    for (name, expected) in [("native", 360.0), ("closed", 300.0)] {
        let SectionLocation::Window { return_to, .. } = &loaded.envelope.layout.instance(&id(name)).unwrap().memory.location else {
            panic!("native memory");
        };
        assert_eq!(return_to.weight, expected);
    }
    let mut service = LayoutService::new(registry, loaded.envelope.layout, HostCapabilities::default()).unwrap();
    service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("host").unwrap() }])).unwrap();
    assert_eq!(service.layout().section(&section("returned")).unwrap().weight, 360.0);
    assert_eq!(service.layout().section(&section("sibling")).unwrap().weight, 240.0);
}

#[test]
fn unchanged_snapshot_refresh_preserves_nested_opaque_locations_and_current_known_fields() {
    use serde_json::json;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let registry = service.registry().clone();
    let mut envelope = LayoutEnvelope::new(service.layout().clone());
    let mut data = serde_json::to_value(&envelope.layout.sections[0]).unwrap();
    data["location"]["future_monitor"] = json!({"id":"monitor-a","dpi":{"scale":2.0}});
    data["size"]["future_units"] = json!({"keep":"logical"});
    envelope.section_snapshots.insert(section("first"), SectionSnapshot { data, index: 0, projected: None });
    let bytes = encode(&envelope, &registry).unwrap();
    let loaded = decode(&bytes, &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let unchanged = with_live_layout(&loaded.envelope, loaded.envelope.layout.clone(), &registry).unwrap();
    let reloaded = decode(&encode(&unchanged, &registry).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(reloaded.notice.is_none());
    assert_eq!(reloaded.envelope.section_snapshots[&section("first")].data["location"]["future_monitor"]["dpi"]["scale"], 2.0);
    assert_eq!(reloaded.envelope.section_snapshots[&section("first")].data["size"]["future_units"]["keep"], "logical");
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(300.0, 260.0) }])).unwrap();
    let edited = with_live_layout(&reloaded.envelope, service.layout().clone(), &registry).unwrap();
    assert_eq!(edited.section_snapshots[&section("first")].data["size"]["width"], 300.0);
    assert_eq!(edited.section_snapshots[&section("first")].data["size"]["future_units"]["keep"], "logical");
    assert_eq!(edited.section_snapshots[&section("first")].data["location"]["future_monitor"]["id"], "monitor-a");
}

#[test]
fn header_action_accepts_the_available_fallback_without_rewriting_saved_activation() {
    let mut registry = synthetic_registry().unwrap();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("fixture.fallback").unwrap();
    spec.header_actions = vec![HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() }];
    registry.register(spec).unwrap();
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    open(&mut service, "unavailable-active", "group");
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.fallback").unwrap(),
            instance: id("fallback"),
            destination: Some(Destination::JoinTab { section: section("group"), index: 1 }),
        }]))
        .unwrap();
    service.set_availability([(id("unavailable-active"), Availability::Unavailable { reason: "owner unavailable".into() })].into()).unwrap();
    let availability = [(id("unavailable-active"), Availability::Unavailable { reason: "owner unavailable".into() })].into();
    assert_eq!(presented_tab(&service.layout().sections[0], &availability), Some(&id("fallback")));
    assert_eq!(presented_tab(&service.layout().sections[0], &AvailabilityMap::new()), Some(&id("unavailable-active")));
    let all_unavailable = [
        (id("unavailable-active"), Availability::Unavailable { reason: "owner unavailable".into() }),
        (id("fallback"), Availability::Unavailable { reason: "owner unavailable".into() }),
    ]
    .into();
    assert_eq!(presented_tab(&service.layout().sections[0], &all_unavailable), None);
    let reply =
        service.dispatch(transaction(vec![Mutation::HeaderAction { instance: id("fallback"), action: ControlId::new("refresh").unwrap() }])).unwrap();
    assert_eq!(reply.effects, vec![HostEffect::HeaderAction { instance: id("fallback"), action: ControlId::new("refresh").unwrap() }]);
    assert_eq!(service.layout().sections[0].active, id("unavailable-active"));
}

#[test]
fn explicit_side_extent_means_requested_height_and_survives_sole_stack_and_collapse_transitions() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let mut env = allocated_pair_environment(&service, 800.0);
    env.gap = 4.0;
    let initial = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(initial.sections.iter().map(|s| s.outer.size.height).collect::<Vec<_>>(), vec![398.0, 398.0]);
    let weights: Vec<_> = service.layout().sections.iter().map(|s| s.weight).collect();
    let before = service.inspect();
    let prepared = service.prepare(transaction(vec![Mutation::SetSize { section: section("first"), size: size(280.0, 400.0) }])).unwrap();
    assert_eq!(service.inspect(), before);
    assert_eq!(prepared.layout().sections[0].preferred_extent, Some(400.0));
    assert_eq!(prepared.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), weights);
    let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
    assert_eq!(
        shown.sections.iter().map(|s| s.outer.size.height).collect::<Vec<_>>(),
        vec![400.0, 396.0],
        "requested feasible dimensions, not a derived weight ratio"
    );
    service.commit(prepared).unwrap();
    assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: true }])).unwrap();
    let folded = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(folded.sections[0].outer.size.height, env.header_height);
    assert_eq!(folded.sections[1].outer.size.height, 776.0);
    service
        .dispatch(transaction(vec![Mutation::Collapse { section: section("first"), collapsed: false }, Mutation::Close { instance: id("two") }]))
        .unwrap();
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("tab-peer"),
            destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
        }]))
        .unwrap();
    env.available.push(id("tab-peer"));
    for request in allocation_measure_requests(service.layout(), service.registry(), &env).unwrap() {
        assert_eq!(request.chrome_height, env.header_height + env.footer_height, "tab groups share one header row");
        env.content.push(SideContentMeasure { request, needed_height: 180.0, policy: SideHeightPolicy::Content });
    }
    let sole = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(sole.sections[0].outer.size.height, 180.0, "sole Content uses captured needed height despite the stored stack request");
    assert_eq!(service.layout().sections[0].preferred_extent, Some(400.0));
    service
        .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("two"), destination: None }]))
        .unwrap();
    let stacked = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(stacked.sections.iter().map(|s| s.outer.size.height).collect::<Vec<_>>(), vec![400.0, 396.0]);
    assert_eq!(service.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), weights);
    let mut tiny = env;
    tiny.capacity.size.height = 150.0;
    let overflow = allocate_layout(service.layout(), service.registry(), &tiny).unwrap();
    assert!(overflow.sections.iter().all(|s| s.outer.size.height >= 120.0));
    assert_eq!(service.layout().sections[0].preferred_extent, Some(400.0));
}

#[test]
fn explicit_bottom_extent_uses_width_without_replacing_surplus_proportions() {
    let mut service = fixture_service(false).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.log", "one", "first", Zone::Bottom, 0).unwrap(),
            fixture_open("fixture.log", "two", "second", Zone::Bottom, 1).unwrap(),
        ]))
        .unwrap();
    let mut env = allocated_pair_environment(&service, 600.0);
    env.capacity.size.width = 800.0;
    env.gap = 4.0;
    env.direction = Direction::Rtl;
    let weights: Vec<_> = service.layout().sections.iter().map(|s| s.weight).collect();
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(400.0, 240.0) }])).unwrap();
    let geometry = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    assert_eq!(geometry.sections.iter().map(|s| s.outer.size.width).collect::<Vec<_>>(), vec![400.0, 396.0]);
    assert_eq!(service.layout().sections[0].preferred_extent, Some(400.0));
    assert_eq!(service.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), weights);
}

#[test]
fn dock_moves_and_explicit_reopens_clear_cross_axis_extent_intent_but_preserve_same_axis_intent() {
    for (source, target) in [(Zone::Left, Zone::Bottom), (Zone::Bottom, Zone::Left), (Zone::Left, Zone::Right), (Zone::Bottom, Zone::Bottom)] {
        for closed in [false, true] {
            for reuse_section in [false, true] {
                let mut service = fixture_service(false).unwrap();
                service
                    .dispatch(transaction(vec![
                        fixture_open("fixture.log", "one", "source", source, 0).unwrap(),
                        fixture_open("fixture.log", "peer", "peer", target, 1).unwrap(),
                        Mutation::SetSize { section: section("source"), size: size(400.0, 400.0) },
                    ]))
                    .unwrap();
                let saved = service.layout().containing(&id("one")).unwrap();
                let inherited = (saved.preferred_extent, saved.extent_priority);
                assert_eq!(inherited, (Some(400.0), 1));
                if closed {
                    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
                }
                let group = section(if reuse_section { "source" } else { "landing" });
                let destination = Destination::NewSection {
                    section: group.clone(),
                    index: 1,
                    location: SectionLocation::Dock { zone: target },
                    size: size(280.0, 240.0),
                };
                let mutation = if closed {
                    Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: Some(destination) }
                } else {
                    Mutation::Move { instance: id("one"), destination }
                };
                service.dispatch(transaction(vec![mutation])).unwrap();
                let expected = if source.side() == target.side() { inherited } else { (None, 0) };
                let moved = service.layout().section(&group).unwrap();
                let memory = &service.layout().instance(&id("one")).unwrap().memory;
                assert_eq!((moved.preferred_extent, moved.extent_priority), expected);
                assert_eq!((memory.preferred_extent, memory.extent_priority), expected);
                let mut env = allocated_pair_environment(&service, 800.0);
                env.capacity.size.width = 800.0;
                env.gap = 4.0;
                let geometry = allocate_layout(service.layout(), service.registry(), &env).unwrap();
                assert_eq!(geometry.sections.len(), 2);
                for actual in &geometry.sections {
                    let expected_extent = if expected.0.is_none() {
                        398.0
                    } else if actual.section == group {
                        400.0
                    } else {
                        396.0
                    };
                    assert_eq!(dock_stack_extent(target, actual.outer.size), Some(expected_extent));
                }
            }
        }
    }
}

#[test]
fn relative_moves_and_explicit_reopens_clear_bottom_width_intent_in_side_sections() {
    for placement in [RelativePlacement::Above, RelativePlacement::Below, RelativePlacement::BeforeColumn, RelativePlacement::AfterColumn] {
        for closed in [false, true] {
            let mut service = fixture_service(false).unwrap();
            service
                .dispatch(transaction(vec![
                    fixture_open("fixture.log", "anchor", "anchor", Zone::Left, 0).unwrap(),
                    fixture_open("fixture.log", "one", "source", Zone::Bottom, 1).unwrap(),
                    Mutation::SetSize { section: section("source"), size: size(400.0, 240.0) },
                ]))
                .unwrap();
            if closed {
                service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
            }
            let destination = Destination::Relative {
                section: section("anchor"),
                placement,
                new_section: section("landing"),
                new_column: matches!(placement, RelativePlacement::BeforeColumn | RelativePlacement::AfterColumn)
                    .then(|| ColumnId::new("landing-column").unwrap()),
                size: size(280.0, 240.0),
            };
            let mutation = if closed {
                Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: Some(destination) }
            } else {
                Mutation::Move { instance: id("one"), destination }
            };
            service.dispatch(transaction(vec![mutation])).unwrap();
            let moved = service.layout().section(&section("landing")).unwrap();
            let memory = &service.layout().instance(&id("one")).unwrap().memory;
            assert_eq!((moved.preferred_extent, moved.extent_priority), (None, 0));
            assert_eq!((memory.preferred_extent, memory.extent_priority), (None, 0));
        }
    }
}

#[test]
fn native_return_extent_inheritance_checks_dock_axes_and_keeps_explicit_return_intent() {
    for target in [Zone::Bottom, Zone::Right] {
        for explicit in [false, true] {
            for closed in [false, true] {
                let mut service = fixture_service(true).unwrap();
                open(&mut service, "one", "source");
                service.dispatch(transaction(vec![Mutation::SetSize { section: section("source"), size: size(280.0, 400.0) }])).unwrap();
                if closed {
                    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
                }
                let destination = Destination::NewSection {
                    section: section("native"),
                    index: 0,
                    size: size(320.0, 280.0),
                    location: SectionLocation::Window {
                        host: HostId::new("host").unwrap(),
                        origin: Point { x: 20.0, y: 30.0 },
                        return_to: DockReturn {
                            zone: target,
                            index: 0,
                            size: size(280.0, 240.0),
                            collapsed: false,
                            section: None,
                            column: None,
                            column_index: 0,
                            column_width: None,
                            weight: 1.0,
                            join: None,
                            preferred_extent: explicit.then_some(500.0),
                            extent_priority: if explicit { 7 } else { 0 },
                        },
                    },
                };
                let mutation = if closed {
                    Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: Some(destination) }
                } else {
                    Mutation::Move { instance: id("one"), destination }
                };
                service.dispatch(transaction(vec![mutation])).unwrap();
                let native = service.layout().containing(&id("one")).unwrap();
                assert_eq!((native.preferred_extent, native.extent_priority), (Some(400.0), 1));
                let SectionLocation::Window { return_to, .. } = &native.location else {
                    panic!("native section");
                };
                let expected = if explicit {
                    (Some(500.0), 7)
                } else if target.side() {
                    (Some(400.0), 1)
                } else {
                    (None, 0)
                };
                assert_eq!((return_to.preferred_extent, return_to.extent_priority), expected);
                assert_eq!(service.layout().instance(&id("one")).unwrap().memory.location, native.location);
                service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("host").unwrap() }])).unwrap();
                let returned = service.layout().containing(&id("one")).unwrap();
                let memory = &service.layout().instance(&id("one")).unwrap().memory;
                assert_eq!((returned.preferred_extent, returned.extent_priority), expected);
                assert_eq!((memory.preferred_extent, memory.extent_priority), expected);
            }
        }
    }
}

#[test]
fn preferred_extent_defaults_omit_wire_fields_and_reject_hostile_values_in_live_and_retained_records() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    assert_eq!(dock_stack_extent(Zone::Left, size(280.0, 400.0)), Some(400.0));
    assert_eq!(dock_stack_extent(Zone::Bottom, size(280.0, 400.0)), Some(280.0));
    assert_eq!(dock_stack_extent(Zone::Window, size(280.0, 400.0)), None);
    let wire = serde_json::to_value(service.layout()).unwrap();
    assert!(wire["sections"][0].get("preferred_extent").is_none());
    assert!(wire["instances"][0]["memory"].get("preferred_extent").is_none());
    let mut null_wire = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    null_wire["layout"]["sections"][0]["preferred_extent"] = serde_json::Value::Null;
    null_wire["layout"]["instances"][0]["memory"]["preferred_extent"] = serde_json::Value::Null;
    let normalized = decode(&serde_json::to_vec(&null_wire).unwrap(), service.registry(), &persistence_fallback()).unwrap();
    assert!(normalized.notice.is_none());
    let normalized: serde_json::Value = serde_json::from_slice(&encode(&normalized.envelope, service.registry()).unwrap()).unwrap();
    assert!(normalized["layout"]["sections"][0].get("preferred_extent").is_none());
    assert!(normalized["layout"]["instances"][0]["memory"].get("preferred_extent").is_none());
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY, MAX_LOGICAL + 1.0] {
        assert_eq!(validate_preferred_extent(Some(bad)).unwrap_err().rule, Rule::Bounds);
        let mut broken = service.layout().clone();
        broken.sections[0].preferred_extent = Some(bad);
        assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::Bounds);
        let mut broken = service.layout().clone();
        broken.instances[0].memory.preferred_extent = Some(bad);
        assert_eq!(validate(&broken, service.registry()).unwrap_err().rule, Rule::Bounds);
        let native = DockReturn {
            zone: Zone::Left,
            index: 0,
            size: size(280.0, 240.0),
            collapsed: false,
            section: None,
            column: None,
            column_index: 0,
            column_width: None,
            weight: 1.0,
            join: None,
            preferred_extent: Some(bad),
            extent_priority: 0,
        };
        assert_eq!(native.validate().unwrap_err().rule, Rule::Bounds);
    }
    let fallback = persistence_fallback();
    let migrated = decode(FIXTURE_LAYOUT_V1, service.registry(), &fallback).unwrap();
    assert!(migrated.notice.is_none());
    assert_eq!(migrated.envelope.layout.sections[0].weight, 240.0);
    assert_eq!(migrated.envelope.layout.sections[0].preferred_extent, None);
    assert_eq!(migrated.envelope.layout.instances[0].memory.preferred_extent, None);
    let mut snapshot = serde_json::to_value(&service.layout().sections[0]).unwrap();
    snapshot["preferred_extent"] = serde_json::json!(0);
    let mut envelope = LayoutEnvelope::new(service.layout().clone());
    envelope.section_snapshots.insert(section("first"), SectionSnapshot { data: snapshot, index: 0, projected: None });
    assert_eq!(encode(&envelope, service.registry()).unwrap_err().rule, Rule::Bounds);
}

#[test]
fn paired_resize_and_reset_clear_explicit_overrides_without_snapshot_resurrection() {
    for operation in 0..3 {
        let mut service = fixture_service(false).unwrap();
        open(&mut service, "one", "first");
        open(&mut service, "two", "second");
        service
            .dispatch(transaction(vec![
                Mutation::SetSize { section: section("first"), size: size(280.0, 400.0) },
                Mutation::SetSize { section: section("second"), size: size(280.0, 396.0) },
            ]))
            .unwrap();
        let mut envelope = LayoutEnvelope::new(service.layout().clone());
        for (index, group) in service.layout().sections.iter().enumerate() {
            let mut data = serde_json::to_value(group).unwrap();
            data["location"]["future_monitor"] = serde_json::json!({"keep":"monitor intent"});
            envelope.section_snapshots.insert(group.id.clone(), SectionSnapshot { data, index, projected: Some(group.clone()) });
        }
        let mutation = match operation {
            0 => Mutation::ResizeAllocatedPair {
                first: section("first"),
                second: section("second"),
                first_size: size(280.0, 120.0),
                second_size: size(280.0, 676.0),
                first_weight: 0.0,
                second_weight: 1.0,
                other_weight_scale: 1.0,
                prefer_allocated: false,
            },
            1 => Mutation::ResetPair { first: section("first"), second: section("second") },
            _ => Mutation::ResizePair {
                first: section("first"),
                second: section("second"),
                first_size: size(280.0, 120.0),
                second_size: size(280.0, 676.0),
            },
        };
        service.dispatch(transaction(vec![mutation])).unwrap();
        assert!(service.layout().sections.iter().all(|s| s.preferred_extent.is_none()));
        assert!(service.layout().instances.iter().all(|p| p.memory.preferred_extent.is_none()));
        assert!(service.layout().sections.iter().all(|s| s.extent_priority == 0));
        assert!(service.layout().instances.iter().all(|p| p.memory.extent_priority == 0));
        let staged = with_live_layout(&envelope, service.layout().clone(), service.registry()).unwrap();
        let bytes = encode(&staged, service.registry()).unwrap();
        let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for group in ["first", "second"] {
            assert!(raw["section_snapshots"][group]["data"].get("preferred_extent").is_none());
            assert!(raw["section_snapshots"][group]["data"].get("extent_priority").is_none());
            assert_eq!(raw["section_snapshots"][group]["data"]["location"]["future_monitor"]["keep"], "monitor intent");
        }
        let restored = decode(&bytes, service.registry(), &persistence_fallback()).unwrap();
        assert!(restored.notice.is_none());
        assert!(restored.envelope.layout.sections.iter().all(|s| s.preferred_extent.is_none()));
    }
}

#[test]
fn explicit_extent_survives_native_sizing_closed_quarantine_reopen_and_redock() {
    use serde_json::json;
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(280.0, 400.0) }])).unwrap();
    let saved_weight = service.layout().sections[0].weight;
    let saved_priority = service.layout().sections[0].extent_priority;
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("one"),
            destination: Destination::NewSection {
                section: section("native"),
                index: 1,
                size: size(320.0, 280.0),
                location: SectionLocation::Window {
                    host: HostId::new("host").unwrap(),
                    origin: Point { x: 20.0, y: 30.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        index: 0,
                        size: size(280.0, 400.0),
                        collapsed: false,
                        join: None,
                        weight: 1.0,
                        preferred_extent: None,
                        extent_priority: 0,
                    },
                },
            },
        }]))
        .unwrap();
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("native"), size: size(350.0, 300.0) }])).unwrap();
    assert_eq!(service.layout().section(&section("native")).unwrap().preferred_extent, Some(400.0));
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    raw["layout"]["instances"][0]["kind"] = json!("extension.panel");
    raw["layout"]["instances"][0]["future_preference"] = json!({"keep":"draft"});
    let registry = service.registry().clone();
    let mut loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let pending: PanelInstance = serde_json::from_value(loaded.envelope.quarantine[0].instance_data.clone()).unwrap();
    assert_eq!(pending.memory.preferred_extent, Some(400.0));
    assert_eq!(pending.memory.extent_priority, saved_priority);
    let SectionLocation::Window { return_to, .. } = &pending.memory.location else {
        panic!("native closed memory");
    };
    assert_eq!(return_to.preferred_extent, Some(400.0));
    assert_eq!(return_to.extent_priority, saved_priority);
    let mut grown = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    grown.register(spec).unwrap();
    restore_quarantine(&mut loaded.envelope, &grown).unwrap();
    let mut service = LayoutService::new(
        grown.clone(),
        loaded.envelope.layout.clone(),
        HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true },
    )
    .unwrap();
    service
        .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("extension.panel").unwrap(), instance: id("one"), destination: None }]))
        .unwrap();
    let opened = with_live_layout(&loaded.envelope, service.layout().clone(), &grown).unwrap();
    service.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("host").unwrap() }])).unwrap();
    let returned = service.layout().section(&section("first")).unwrap();
    assert_eq!(returned.preferred_extent, Some(400.0));
    assert_eq!(returned.weight, saved_weight);
    let persisted = with_live_layout(&opened, service.layout().clone(), &grown).unwrap();
    assert_eq!(persisted.retained_data[&id("one")]["instance"]["future_preference"]["keep"], "draft");
    assert_eq!(persisted.layout.instance(&id("one")).unwrap().memory.preferred_extent, Some(400.0));
    assert_eq!(persisted.layout.instance(&id("one")).unwrap().memory.extent_priority, saved_priority);
}

#[test]
fn nested_quarantined_native_column_reservation_rejects_theft_and_allows_owner_redock() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "anchor", "anchor");
    let registry = service.registry().clone();
    let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    raw["quarantine"] = json!([{"instance":"extension","kind":"extension.panel","section":null,"instance_data":{
        "id":"extension","kind":"extension.panel","requested_open":false,"memory":{
            "section":"native-memory","section_index":1,"tab_index":0,"size":{"width":280,"height":240},"collapsed":false,"weight":240,
            "preferred_extent":350,"location":{"type":"window","host":"return-host","origin":{"x":20,"y":30},"return_to":{
                "section":"return-section","zone":"left","index":1,"column":"column-2","column_index":1,"column_width":280,
                "size":{"width":280,"height":240},"collapsed":false,"weight":240,"preferred_extent":350
            }}
        }
    }}]);
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let envelope = loaded.envelope;
    assert!(envelope.layout.column(&ColumnId::new("column-2").unwrap()).is_err());
    let reserved = reserve_envelope_identities(&envelope).unwrap();
    assert_eq!(next_column_id(&reserved).unwrap(), ColumnId::new("column-3").unwrap());
    let mut catalogue = commands::InMemoryCatalogue::default();
    catalogue.remember(&envelope, None, &registry).unwrap();
    let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
    let before = backend.service.inspect();
    let stored_before = backend.catalogue.state().export().unwrap();
    assert_eq!(backend.execute("panel.open", &json!({"kind":"fixture.log","instance":"unrelated","destination":{
        "type":"relative","section":"anchor","placement":"after_column","new_section":"intruder","new_column":"column-2","size":{"width":280,"height":240}
    }})).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(backend.service.inspect(), before);
    assert_eq!(backend.catalogue.state().export().unwrap(), stored_before);
    let mut grown = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    grown.register(spec).unwrap();
    let mut envelope = envelope;
    restore_quarantine(&mut envelope, &grown).unwrap();
    let mut owner =
        LayoutService::new(grown.clone(), envelope.layout.clone(), HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true })
            .unwrap();
    owner
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("extension.panel").unwrap(),
            instance: id("extension"),
            destination: None,
        }]))
        .unwrap();
    let opened = with_live_layout(&envelope, owner.layout().clone(), &grown).unwrap();
    owner.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("return-host").unwrap() }])).unwrap();
    let redocked = with_live_layout(&opened, owner.layout().clone(), &grown).unwrap();
    assert_eq!(redocked.layout.section(&section("return-section")).unwrap().column, Some(ColumnId::new("column-2").unwrap()));
    assert_eq!(redocked.layout.section(&section("return-section")).unwrap().preferred_extent, Some(350.0));
}

#[test]
fn allocated_pair_uncaps_a_member_without_moving_the_third_or_preserving_old_pair_share() {
    let mut registry = synthetic_registry().unwrap();
    let mut capped = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    capped.kind = PanelKind::new("fixture.capped").unwrap();
    capped.default_size.height = 180.0;
    capped.max_size.height = 200.0;
    registry.register(capped).unwrap();
    let mut service =
        LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.capped").unwrap(),
            instance: id("one"),
            destination: Some(Destination::NewSection {
                section: section("first"),
                index: 0,
                location: SectionLocation::Dock { zone: Zone::Left },
                size: size(280.0, 180.0),
            }),
        }]))
        .unwrap();
    open(&mut service, "two", "second");
    open(&mut service, "three", "third");
    let mut layout = service.layout().clone();
    for group in &mut layout.sections {
        group.weight = 240.0;
    }
    for panel in &mut layout.instances {
        panel.memory.weight = 240.0;
    }
    let mut service = LayoutService::new(registry, layout, HostCapabilities::default()).unwrap();
    let mut env = allocated_pair_environment(&service, 800.0);
    env.gap = 4.0;
    let before = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    for (group, expected) in before.sections.iter().zip([200.0, 296.0, 296.0]) {
        assert!((group.outer.size.height - expected).abs() < 0.001);
    }
    let old = normalized_pair_weights(service.layout(), &section("first"), &section("second")).unwrap();
    // The third's normalized saved weight doubles. Derive the pair from its
    // actual 176 surplus units; this needs a different combined pair share.
    let first_weight = (2.0 / 3.0) * 60.0 / 176.0;
    let second_weight = (2.0 / 3.0) * 196.0 / 176.0;
    assert!((first_weight + second_weight - old.0 - old.1).abs() > 0.01);
    let prepared = service
        .prepare(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 180.0),
            second_size: size(280.0, 316.0),
            first_weight,
            second_weight,
            other_weight_scale: 2.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
    assert!((shown.sections[0].outer.size.height - 180.0).abs() < 0.001);
    assert!((shown.sections[1].outer.size.height - 316.0).abs() < 0.001);
    assert!((shown.sections[2].outer.size.height - 296.0).abs() < 0.001);
    assert!((shown.sections[2].outer.origin.y - before.sections[2].outer.origin.y).abs() < 0.001);
    service.commit(prepared).unwrap();
    assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
    assert!((service.layout().sections[2].weight - 2.0 / 3.0).abs() < 0.00001);
}

#[test]
fn allocated_pair_scales_unpaired_closed_and_native_ratios_and_preserves_their_preferences() {
    let mut service = fixture_service(true).unwrap();
    for (panel, group) in [("one", "first"), ("two", "second"), ("three", "third"), ("closed", "closed"), ("native", "return")] {
        open(&mut service, panel, group);
    }
    service
        .dispatch(transaction(vec![
            Mutation::SetSize { section: section("third"), size: size(280.0, 300.0) },
            Mutation::SetSize { section: section("closed"), size: size(280.0, 350.0) },
            Mutation::SetSize { section: section("return"), size: size(280.0, 400.0) },
        ]))
        .unwrap();
    service.dispatch(transaction(vec![Mutation::Close { instance: id("closed") }])).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("native"),
            destination: Destination::NewSection {
                section: section("window"),
                index: 3,
                size: size(280.0, 400.0),
                location: SectionLocation::Window {
                    host: HostId::new("host").unwrap(),
                    origin: Point { x: 0.0, y: 0.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 4,
                        size: size(280.0, 400.0),
                        collapsed: false,
                        join: None,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 240.0,
                        preferred_extent: None,
                        extent_priority: 0,
                    },
                },
            },
        }]))
        .unwrap();
    let old = service.layout().clone();
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 200.0),
            second_size: size(280.0, 300.0),
            first_weight: 0.1,
            second_weight: 0.2,
            other_weight_scale: 2.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    assert_eq!(service.layout().sections[0].weight, 0.1);
    assert_eq!(service.layout().sections[1].weight, 0.2);
    assert!((service.layout().section(&section("third")).unwrap().weight - 0.4).abs() < 0.00001);
    assert!((service.layout().instance(&id("closed")).unwrap().memory.weight - 0.4).abs() < 0.00001);
    let SectionLocation::Window { return_to, .. } = &service.layout().section(&section("window")).unwrap().location else {
        panic!("native retained");
    };
    assert!((return_to.weight - 0.4).abs() < 0.00001);
    assert_eq!(return_to.preferred_extent, Some(400.0));
    assert_eq!(service.layout().instance(&id("closed")).unwrap().memory.preferred_extent, Some(350.0));
    assert_eq!(service.layout().section(&section("third")).unwrap().preferred_extent, Some(300.0));
    assert_eq!(service.layout().section(&section("third")).unwrap().size, old.section(&section("third")).unwrap().size);
    assert_eq!(service.layout().instance(&id("native")).unwrap().memory.location, service.layout().section(&section("window")).unwrap().location);
}

#[test]
fn allocated_pair_skips_unavailable_sections_but_never_a_presented_collapsed_header() {
    let mut service = fixture_service(false).unwrap();
    for (panel, group) in [("one", "first"), ("hidden", "middle"), ("two", "second")] {
        open(&mut service, panel, group);
    }
    assert!(normalized_pair_weights(service.layout(), &section("first"), &section("second")).is_ok());
    let mutation = Mutation::ResizeAllocatedPair {
        first: section("first"),
        second: section("second"),
        first_size: size(280.0, 180.0),
        second_size: size(280.0, 300.0),
        first_weight: 0.2,
        second_weight: 0.3,
        other_weight_scale: 1.0,
        prefer_allocated: false,
    };
    reject(&mut service, vec![mutation.clone()], Rule::Placement);
    service.set_availability([(id("hidden"), Availability::Unavailable { reason: "absent".into() })].into()).unwrap();
    service.dispatch(transaction(vec![mutation.clone()])).unwrap();
    reject(
        &mut service,
        vec![Mutation::ResizePair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 180.0),
            second_size: size(280.0, 300.0),
        }],
        Rule::Placement,
    );
    service.dispatch(transaction(vec![Mutation::Collapse { section: section("middle"), collapsed: true }])).unwrap();
    service.set_availability(AvailabilityMap::new()).unwrap();
    reject(&mut service, vec![mutation.clone()], Rule::Placement);
    service.set_availability([(id("hidden"), Availability::Unavailable { reason: "absent".into() })].into()).unwrap();
    service.dispatch(transaction(vec![mutation])).unwrap();
    reject(
        &mut service,
        vec![Mutation::ResizeAllocatedPair {
            first: section("second"),
            second: section("first"),
            first_size: size(280.0, 300.0),
            second_size: size(280.0, 180.0),
            first_weight: 0.3,
            second_weight: 0.2,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }],
        Rule::Placement,
    );
}

#[test]
fn allocated_pair_bounds_scale_and_total_without_old_share_equality_and_defaults_scale_on_wire() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, MAX_LOGICAL + 1.0] {
        reject(
            &mut service,
            vec![Mutation::ResizeAllocatedPair {
                first: section("first"),
                second: section("second"),
                first_size: size(280.0, 120.0),
                second_size: size(280.0, 680.0),
                first_weight: 0.4,
                second_weight: 0.4,
                other_weight_scale: scale,
                prefer_allocated: false,
            }],
            Rule::Bounds,
        );
    }
    let mutation: Mutation = serde_json::from_value(serde_json::json!({"type":"resize_allocated_pair","first":"first","second":"second",
        "first_size":{"width":280,"height":120},"second_size":{"width":280,"height":680},"first_weight":0.4,"second_weight":0.4}))
    .unwrap();
    assert!(
        matches!(&mutation, Mutation::ResizeAllocatedPair { other_weight_scale, prefer_allocated, .. } if *other_weight_scale == 1.0 && !prefer_allocated)
    );
    service.dispatch(transaction(vec![mutation])).unwrap();
    assert_eq!(service.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), vec![0.4, 0.4]);
    open(&mut service, "three", "third");
    let mut layout = service.layout().clone();
    for group in &mut layout.sections {
        group.weight = if group.id == section("third") { MAX_LOGICAL } else { 0.000001 };
    }
    for panel in &mut layout.instances {
        panel.memory.weight = if panel.id == id("three") { MAX_LOGICAL } else { 0.000001 };
    }
    let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
    reject(
        &mut service,
        vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 120.0),
            second_size: size(280.0, 680.0),
            first_weight: 1.0,
            second_weight: 1.0,
            other_weight_scale: MAX_LOGICAL,
            prefer_allocated: false,
        }],
        Rule::Bounds,
    );
}

#[test]
fn reservation_aware_prepare_skips_external_ids_and_rejects_explicit_theft_without_mutation() {
    let mut service = fixture_service(false).unwrap();
    let sections = [section("section-1")].into();
    let columns = [ColumnId::new("column-1").unwrap()].into();
    let before = service.inspect();
    let action = transaction(vec![fixture_open("fixture.log", "one", "first", Zone::Left, 0).unwrap()]);
    let prepared = service.prepare_with_reservations(action.clone(), &sections, &columns).unwrap();
    assert_eq!(prepared.layout().columns[0].id, ColumnId::new("column-2").unwrap());
    assert_eq!(service.prepare_with_reservations(action, &sections, &columns).unwrap().layout(), prepared.layout());
    assert_eq!(service.inspect(), before);
    assert_eq!(
        service
            .prepare_with_reservations(
                transaction(vec![fixture_open("fixture.log", "thief", "section-1", Zone::Left, 0).unwrap()]),
                &sections,
                &columns
            )
            .err()
            .unwrap()
            .rule,
        Rule::DuplicateId
    );
    service.commit(prepared).unwrap();
    let before = service.inspect();
    let theft = transaction(vec![Mutation::Open {
        kind: PanelKind::new("fixture.log").unwrap(),
        instance: id("thief"),
        destination: Some(Destination::Relative {
            section: section("first"),
            placement: RelativePlacement::BeforeColumn,
            new_section: section("new"),
            new_column: Some(ColumnId::new("column-1").unwrap()),
            size: size(280.0, 240.0),
        }),
    }]);
    assert_eq!(service.prepare_with_reservations(theft, &sections, &columns).err().unwrap().rule, Rule::DuplicateId);
    let prepared = service
        .prepare_with_reservations(transaction(vec![fixture_open("fixture.log", "right", "right", Zone::Right, 1).unwrap()]), &sections, &columns)
        .unwrap();
    assert_eq!(prepared.layout().section(&section("right")).unwrap().column, Some(ColumnId::new("column-3").unwrap()));
    assert_eq!(service.inspect(), before);
}

#[test]
fn replacement_context_admits_typed_owners_and_refuses_changed_owner_sets() {
    let mut source = fixture_service(true).unwrap();
    open(&mut source, "one", "first");
    source
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("two"),
            destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
        }]))
        .unwrap();
    open(&mut source, "closed", "remembered");
    source.dispatch(transaction(vec![Mutation::Close { instance: id("closed") }])).unwrap();
    let mut incoming = source.layout().clone();
    let memory = &mut incoming.instances.iter_mut().find(|panel| panel.id == id("closed")).unwrap().memory;
    memory.column = None;
    memory.column_index = 0;
    memory.column_width = None;
    memory.location = SectionLocation::Window {
        host: HostId::new("closed-host").unwrap(),
        origin: Point { x: 0.0, y: 0.0 },
        return_to: DockReturn {
            zone: Zone::Left,
            index: 0,
            size: size(280.0, 240.0),
            collapsed: false,
            section: Some(section("return-section")),
            join: Some(ReturnTab { section: section("return-group"), index: 0 }),
            column: Some(ColumnId::new("return-column").unwrap()),
            column_index: 0,
            column_width: Some(280.0),
            weight: 240.0,
            preferred_extent: None,
            extent_priority: 0,
        },
    };
    let dormant = ColumnId::new("dormant").unwrap();
    incoming.columns.push(Column { id: dormant.clone(), zone: Zone::Left, width: 280.0 });
    let service = fixture_service(true).unwrap();
    validate(&incoming, service.registry()).unwrap();
    let context = preparation_context(&LayoutEnvelope::new(incoming.clone())).unwrap();
    let ownership = context.replacement_ownership.as_ref().unwrap();
    assert_eq!(ownership.sections[&section("first")], std::collections::BTreeSet::from([id("one"), id("two")]));
    for name in ["remembered", "return-section", "return-group"] {
        assert_eq!(ownership.sections[&section(name)], std::collections::BTreeSet::from([id("closed")]));
    }
    assert_eq!(ownership.columns[&ColumnId::new("return-column").unwrap()], std::collections::BTreeSet::from([id("closed")]));
    assert!(ownership.columns[&dormant].is_empty());
    let action = LayoutAction::Replace { expected_revision: Some(service.inspect().revision), layout: incoming.clone() };
    let before = service.inspect();
    let mut reservations_only = context.clone();
    reservations_only.replacement_ownership = None;
    assert_eq!(service.prepare_with_context(action.clone(), &AvailabilityMap::new(), &reservations_only).err().unwrap().rule, Rule::DuplicateId);
    let prepared = service.prepare_with_context(action, &AvailabilityMap::new(), &context).unwrap();
    assert_eq!(prepared.layout(), &incoming);
    for attack in 0..3 {
        let mut stolen = incoming.clone();
        match attack {
            0 => stolen.instances.retain(|panel| panel.id != id("two")),
            1 => {
                let mut thief = stolen.instances[0].clone();
                thief.id = id("thief");
                stolen.instances.push(thief);
                stolen.sections[0].tabs.push(id("thief"));
            }
            _ => stolen.sections[0].column = Some(dormant.clone()),
        }
        if attack == 0 {
            stolen.sections[0].tabs.retain(|panel| *panel != id("two"));
        }
        validate(&stolen, service.registry()).unwrap();
        assert_eq!(
            service
                .prepare_with_context(LayoutAction::Replace { expected_revision: None, layout: stolen }, &AvailabilityMap::new(), &context)
                .err()
                .unwrap()
                .rule,
            Rule::DuplicateId
        );
    }
    assert_eq!(service.inspect(), before, "preparation and refused takeovers must not publish state");
}

#[test]
fn replacement_ownership_bounds_identities_and_owner_sets() {
    let mut ownership = ReplacementIdentityOwnership::default();
    ownership.sections.insert(SectionId("INVALID".into()), Default::default());
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::Id);
    ownership.sections.clear();
    ownership.columns.insert(ColumnId("INVALID".into()), Default::default());
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::Id);
    ownership.columns.clear();
    ownership.sections.insert(section("first"), [InstanceId("INVALID".into())].into());
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::Id);
    ownership.sections.insert(section("first"), (0..=MAX_INSTANCES).map(|n| id(&format!("owner-{n}"))).collect());
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::CollectionLimit);
    ownership.sections = (0..=MAX_RESERVED_IDENTITIES).map(|n| (section(&format!("reserved-{n}")), Default::default())).collect();
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::CollectionLimit);
    let owners = (0..MAX_INSTANCES).map(|n| id(&format!("owner-{n}"))).collect::<std::collections::BTreeSet<_>>();
    ownership.sections = (0..=MAX_RESERVED_IDENTITIES / MAX_INSTANCES).map(|n| (section(&format!("reserved-{n}")), owners.clone())).collect();
    assert_eq!(ownership.validate().unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn reservation_aware_prepare_preserves_owner_closed_reopen_and_native_redock() {
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    let column = service.layout().sections[0].column.clone().unwrap();
    let sections = [section("first")].into();
    let columns = [column.clone()].into();
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    let prepared = service
        .prepare_with_reservations(
            transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("one"), destination: None }]),
            &sections,
            &columns,
        )
        .unwrap();
    service.commit(prepared).unwrap();
    let prepared = service
        .prepare_with_reservations(
            transaction(vec![Mutation::Move {
                instance: id("one"),
                destination: Destination::NewSection {
                    section: section("window"),
                    index: 0,
                    size: size(280.0, 240.0),
                    location: SectionLocation::Window {
                        host: HostId::new("host").unwrap(),
                        origin: Point { x: 0.0, y: 0.0 },
                        return_to: DockReturn {
                            zone: Zone::Left,
                            index: 0,
                            size: size(280.0, 240.0),
                            collapsed: false,
                            join: None,
                            section: Some(section("first")),
                            column: Some(column.clone()),
                            column_index: 0,
                            column_width: Some(280.0),
                            weight: 240.0,
                            preferred_extent: None,
                            extent_priority: 0,
                        },
                    },
                },
            }]),
            &sections,
            &columns,
        )
        .unwrap();
    service.commit(prepared).unwrap();
    let before = service.inspect();
    let context = preparation_context(&LayoutEnvelope::new(before.layout.clone())).unwrap();
    let ownership = context.replacement_ownership.as_ref().unwrap();
    assert_eq!(ownership.sections[&section("window")], std::collections::BTreeSet::from([id("one")]));
    assert_eq!(ownership.sections[&section("first")], std::collections::BTreeSet::from([id("one")]));
    assert_eq!(ownership.columns[&column], std::collections::BTreeSet::from([id("one")]));
    let empty = fixture_service(true).unwrap();
    empty
        .prepare_with_context(LayoutAction::Replace { expected_revision: None, layout: before.layout.clone() }, &AvailabilityMap::new(), &context)
        .unwrap();
    let prepared = service
        .prepare_with_reservations(transaction(vec![Mutation::NativeClose { host: HostId::new("host").unwrap() }]), &sections, &columns)
        .unwrap();
    assert_eq!(service.inspect(), before);
    assert_eq!(prepared.layout().sections[0].id, section("first"));
    assert_eq!(prepared.layout().sections[0].column, Some(column));
    service.commit(prepared).unwrap();
}

#[test]
fn reservation_aware_prepare_bounds_and_validates_borrowed_sets_before_proposing() {
    let service = fixture_service(false).unwrap();
    let before = service.inspect();
    let too_many = (0..=MAX_RESERVED_IDENTITIES).map(|n| section(&format!("reserved-{n}"))).collect();
    let empty = std::collections::BTreeSet::new();
    assert_eq!(service.prepare_with_reservations(LayoutAction::Inspect, &too_many, &empty).err().unwrap().rule, Rule::CollectionLimit);
    let bad_sections = [SectionId("UPPER".into())].into();
    assert_eq!(service.prepare_with_reservations(LayoutAction::Inspect, &bad_sections, &empty).err().unwrap().rule, Rule::Id);
    let bad_columns = [ColumnId("UPPER".into())].into();
    assert_eq!(
        service.prepare_with_reservations(LayoutAction::Inspect, &std::collections::BTreeSet::new(), &bad_columns).err().unwrap().rule,
        Rule::Id
    );
    assert_eq!(service.inspect(), before);
}

#[test]
fn reservation_aware_native_destinations_skip_implicit_columns_and_refuse_explicit_nested_theft() {
    let service = fixture_service(true).unwrap();
    let sections = [section("section-1")].into();
    let column = ColumnId::new("column-1").unwrap();
    let columns = [column.clone()].into();
    let returning = |return_column, return_section| {
        transaction(vec![Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("one"),
            destination: Some(Destination::NewSection {
                section: section("native"),
                index: 0,
                size: size(280.0, 240.0),
                location: SectionLocation::Window {
                    host: HostId::new("host").unwrap(),
                    origin: Point { x: 0.0, y: 0.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 0,
                        size: size(280.0, 240.0),
                        collapsed: false,
                        join: None,
                        section: return_section,
                        column: return_column,
                        column_index: 0,
                        column_width: None,
                        weight: 240.0,
                        preferred_extent: None,
                        extent_priority: 0,
                    },
                },
            }),
        }])
    };
    let before = service.inspect();
    assert_eq!(service.prepare_with_reservations(returning(Some(column), None), &sections, &columns).err().unwrap().rule, Rule::DuplicateId);
    assert_eq!(
        service.prepare_with_reservations(returning(None, Some(section("section-1"))), &sections, &columns).err().unwrap().rule,
        Rule::DuplicateId
    );
    let prepared = service.prepare_with_reservations(returning(None, None), &sections, &columns).unwrap();
    let SectionLocation::Window { return_to, .. } = &prepared.layout().sections[0].location else {
        panic!("window");
    };
    assert_eq!(return_to.column, Some(ColumnId::new("column-2").unwrap()));
    assert_eq!(service.inspect(), before);
    assert_eq!(next_section_id_with_reservations(service.layout(), &sections).unwrap(), section("section-2"));
    assert_eq!(next_column_id_with_reservations(service.layout(), &columns).unwrap(), ColumnId::new("column-2").unwrap());
}

#[test]
fn current_workspace_action_preflights_catalogue_then_consumes_the_exact_private_candidate_once() {
    use commands::LayoutCatalogue;
    use std::cell::{Cell, RefCell};
    struct CandidateSink {
        service: LayoutService,
        action: LayoutAction,
        prepared: RefCell<Option<PreparedLayout>>,
        projections: Cell<usize>,
        dispatches: usize,
    }
    impl ActionSink for CandidateSink {
        fn inspect(&self) -> LayoutReply {
            self.service.inspect()
        }
        fn project(&self, action: LayoutAction) -> PanelResult<Layout> {
            assert_eq!(action, self.action);
            self.projections.set(self.projections.get() + 1);
            Ok(self.prepared.borrow().as_ref().unwrap().layout().clone())
        }
        fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
            assert_eq!(action, self.action);
            self.dispatches += 1;
            self.service.commit(self.prepared.get_mut().take().unwrap())
        }
    }
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let registry = service.registry().clone();
    let action = LayoutAction::Apply {
        expected_revision: Some(service.inspect().revision),
        mutations: vec![Mutation::SetSize { section: section("first"), size: size(280.0, 400.0) }],
    };
    let prepared = service.prepare(action.clone()).unwrap();
    let shown = prepared.layout().clone();
    let mut sink =
        CandidateSink { service, action: action.clone(), prepared: RefCell::new(Some(prepared)), projections: Cell::new(0), dispatches: 0 };
    let mut catalogue = fixture_command_catalogue(&registry).unwrap();
    let original = catalogue.state().clone();
    let before = sink.inspect();
    catalogue.fail_stage = true;
    let failed = commands::execute_layout_action(
        action.clone(),
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap_err();
    assert_eq!(failed.rule, Rule::Capability);
    assert_eq!(sink.inspect(), before);
    assert_eq!(sink.dispatches, 0);
    assert!(sink.prepared.borrow().is_some());
    assert_eq!(catalogue.state(), &original);
    catalogue.fail_stage = false;
    let reply = commands::execute_layout_action(
        action,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap();
    assert_eq!(reply.layout, shown);
    assert_eq!(sink.dispatches, 1);
    assert_eq!(sink.projections.get(), 2);
    assert!(sink.prepared.borrow().is_none());
    assert_eq!(
        catalogue.state().remembered(&reply.layout.workspace).unwrap().project_envelope(&registry, &reply.layout.workspace).unwrap().layout,
        shown
    );
    let mut other = shown;
    other.workspace = WorkspaceId::new("fixture.other").unwrap();
    assert_eq!(
        commands::execute_layout_action(
            LayoutAction::Replace { expected_revision: None, layout: other },
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false }
        )
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    assert_eq!(sink.dispatches, 1);
}

#[test]
fn standalone_header_commands_bypass_catalogue_and_no_space_projection_but_keep_authoritative_validation() {
    use commands::{CatalogueState, LayoutCatalogue, PreparedCatalogue};
    use std::cell::Cell;
    struct RefusingPlacementSink {
        service: LayoutService,
        projects: Cell<usize>,
        dispatches: usize,
        controller_enabled: bool,
    }
    impl ActionSink for RefusingPlacementSink {
        fn inspect(&self) -> LayoutReply {
            self.service.inspect()
        }
        fn project(&self, _action: LayoutAction) -> PanelResult<Layout> {
            self.projects.set(self.projects.get() + 1);
            Err(error(Rule::NoSpace, "captured placement overflows"))
        }
        fn dispatch(&mut self, action: LayoutAction) -> PanelResult<LayoutReply> {
            self.dispatches += 1;
            if !self.controller_enabled {
                return Err(error(Rule::Capability, "owning controller disabled"));
            }
            self.service.dispatch(action)
        }
    }
    struct UnavailableCatalogue {
        state: CatalogueState,
        reads: Cell<usize>,
        prepares: usize,
        publishes: usize,
    }
    impl LayoutCatalogue for UnavailableCatalogue {
        fn state(&self) -> &CatalogueState {
            self.reads.set(self.reads.get() + 1);
            &self.state
        }
        fn prepare(&mut self, _state: CatalogueState) -> PanelResult<PreparedCatalogue> {
            self.prepares += 1;
            Err(error(Rule::Capability, "storage unavailable"))
        }
        fn publish(&mut self, _prepared: PreparedCatalogue) {
            self.publishes += 1;
        }
    }
    let mut registry = synthetic_registry().unwrap();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("fixture.action").unwrap();
    spec.header_actions = vec![HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() }];
    registry.register(spec).unwrap();
    let mut service =
        LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.action", "one", "first", Zone::Left, 0).unwrap()])).unwrap();
    let mut imported = service.layout().clone();
    let memory = &mut imported.instances[0].memory;
    memory.section = section("divergent-memory");
    memory.location = SectionLocation::Floating { origin: Point { x: 31.0, y: 47.0 } };
    memory.size = size(320.0, 280.0);
    memory.collapsed = true;
    memory.weight = 7.0;
    memory.preferred_extent = Some(220.0);
    memory.column = None;
    memory.column_width = None;
    memory.column_index = 0;
    imported.columns.push(Column { id: ColumnId::new("quarantine-column").unwrap(), zone: Zone::Right, width: 280.0 });
    let mut raw = serde_json::to_value(LayoutEnvelope::new(imported.clone())).unwrap();
    let mut extension = legacy_instance_wire(
        "extension",
        "extension.panel",
        "extension-memory",
        serde_json::json!({"type":"dock","zone":"right"}),
        size(280.0, 240.0),
        1,
        false,
    );
    extension["memory"]["column"] = serde_json::json!("quarantine-column");
    raw["quarantine"] = serde_json::json!([{"instance":"extension","kind":"extension.panel","section":null,"instance_data":extension}]);
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.envelope.layout, imported);
    let service = LayoutService::new(registry.clone(), loaded.envelope.layout.clone(), HostCapabilities::default()).unwrap();
    let mut stored = commands::InMemoryCatalogue::default();
    stored.remember(&loaded.envelope, None, &registry).unwrap();
    let mut sink = RefusingPlacementSink { service, projects: Cell::new(0), dispatches: 0, controller_enabled: true };
    let mut catalogue = UnavailableCatalogue { state: stored.state().clone(), reads: Cell::new(0), prepares: 0, publishes: 0 };
    let original_catalogue = catalogue.state.clone();
    let original_layout_bytes = serde_json::to_vec(&imported).unwrap();
    let header = Mutation::HeaderAction { instance: id("one"), action: ControlId::new("refresh").unwrap() };
    commands::execute_layout_action(
        transaction(vec![header.clone()]),
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap();
    let params = serde_json::json!({"target":{"type":"instance","id":"one"},"action":"refresh"});
    commands::execute(
        "panel.action",
        &params,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap();
    let batch = [serde_json::json!({"command":"panel.action","params":params})];
    let result = commands::execute_batch(
        &batch,
        &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false },
    )
    .unwrap();
    assert_eq!(result["completed"], 1);
    assert_eq!(sink.dispatches, 3);
    assert_eq!(sink.projects.get(), 0);
    assert_eq!((catalogue.reads.get(), catalogue.prepares, catalogue.publishes), (0, 0, 0));
    assert_eq!(sink.inspect().layout, imported);
    assert_eq!(serde_json::to_vec(&sink.inspect().layout).unwrap(), original_layout_bytes);
    assert_eq!(catalogue.state, original_catalogue);
    let before = sink.inspect();
    let mixed = transaction(vec![header.clone(), Mutation::SetSize { section: section("first"), size: size(280.0, 300.0) }]);
    assert_eq!(
        commands::execute_layout_action(
            mixed.clone(),
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false }
        )
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    assert_eq!(sink.service.dispatch(mixed).unwrap_err().rule, Rule::Capability);
    assert_eq!(
        commands::preflight_batch(
            &[
                serde_json::json!({"command":"panel.action","params":{"target":{"type":"instance","id":"one"},"action":"refresh"}}),
                serde_json::json!({"command":"layout.get"}),
            ],
            &before.layout.workspace
        )
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    assert_eq!(sink.dispatches, 3);
    assert_eq!(sink.inspect(), before);
    let unknown = transaction(vec![Mutation::HeaderAction { instance: id("one"), action: ControlId::new("unknown").unwrap() }]);
    assert_eq!(
        commands::execute_layout_action(
            unknown,
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false }
        )
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    sink.controller_enabled = false;
    assert_eq!(
        commands::execute_layout_action(
            transaction(vec![header.clone()]),
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false }
        )
        .unwrap_err()
        .rule,
        Rule::Capability
    );
    sink.controller_enabled = true;
    sink.service.set_availability([(id("one"), Availability::Unavailable { reason: "not available".into() })].into()).unwrap();
    assert_eq!(
        commands::execute_layout_action(
            transaction(vec![header]),
            &mut commands::PanelCommandContext { sink: &mut sink, registry: &registry, catalogue: &mut catalogue, web: false }
        )
        .unwrap_err()
        .rule,
        Rule::Availability
    );
    assert_eq!(sink.projects.get(), 0);
    assert_eq!((catalogue.reads.get(), catalogue.prepares, catalogue.publishes), (0, 0, 0));
}

#[test]
fn current_envelope_snapshot_without_remembered_state_returns_live_data_without_persisting() {
    use commands::LayoutCatalogue;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let catalogue = commands::InMemoryCatalogue::default();
    let before = catalogue.state().clone();
    let envelope = commands::current_envelope_snapshot(&catalogue, &service.inspect(), service.registry()).unwrap();
    assert_eq!(envelope.layout, *service.layout());
    assert_eq!(catalogue.state(), &before);
}

#[test]
fn persistence_rejects_new_latent_section_and_column_claims_before_native_close_and_preserves_owner_recovery() {
    use serde_json::json;
    for column_only in [false, true] {
        let registry = synthetic_registry().unwrap();
        let mut raw = serde_json::to_value(LayoutEnvelope::new(Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()))).unwrap();
        let location = if column_only {
            json!({"type":"window","host":"extension-host","origin":{"x":20,"y":30},"return_to":{
                "zone":"left","index":0,"column":"reserved-column","column_width":280,"size":{"width":280,"height":240},"collapsed":false
            }})
        } else {
            json!({"type":"floating","origin":{"x":20,"y":30}})
        };
        let mut extension = legacy_instance_wire("extension", "extension.panel", "saved", location, size(280.0, 240.0), 0, false);
        extension["future_draft"] = json!({"keep":"closed extension draft"});
        raw["quarantine"] = json!([{"instance":"extension","kind":"extension.panel","section":null,"instance_data":extension}]);
        let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
        assert!(loaded.notice.is_none());
        let envelope = loaded.envelope;
        let original = encode(&envelope, &registry).unwrap();
        let context = preparation_context(&envelope).unwrap();
        let ownership = context.replacement_ownership.as_ref().unwrap();
        assert!(!ownership.sections.contains_key(&section("saved")));
        assert!(!ownership.columns.contains_key(&ColumnId::new("reserved-column").unwrap()));
        let service = LayoutService::new(
            registry.clone(),
            envelope.layout.clone(),
            HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true },
        )
        .unwrap();
        let before = service.inspect();
        let destination = Destination::NewSection {
            section: section("window"),
            index: 0,
            size: size(280.0, 240.0),
            location: SectionLocation::Window {
                host: HostId::new("unrelated-host").unwrap(),
                origin: Point { x: 40.0, y: 50.0 },
                return_to: DockReturn {
                    zone: Zone::Left,
                    index: 0,
                    size: size(280.0, 240.0),
                    collapsed: false,
                    join: None,
                    section: if column_only { None } else { Some(section("saved")) },
                    column: if column_only { Some(ColumnId::new("reserved-column").unwrap()) } else { None },
                    column_index: 0,
                    column_width: None,
                    weight: 240.0,
                    preferred_extent: None,
                    extent_priority: 0,
                },
            },
        };
        // Deliberately omit preparation reservations: persistence is independently
        // responsible for refusing these incoming, still-unmaterialized claims.
        let prepared = service
            .prepare(transaction(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("unrelated"),
                destination: Some(destination),
            }]))
            .unwrap();
        let incoming = prepared.layout().clone();
        assert!(incoming.section(&section("saved")).is_err());
        assert!(incoming.column(&ColumnId::new("reserved-column").unwrap()).is_err());
        assert_eq!(
            service
                .prepare_with_context(LayoutAction::Replace { expected_revision: None, layout: incoming.clone() }, &AvailabilityMap::new(), &context)
                .err()
                .unwrap()
                .rule,
            Rule::DuplicateId
        );
        assert_eq!(with_live_layout(&envelope, incoming.clone(), &registry).unwrap_err().rule, Rule::DuplicateId);
        let mut closed = incoming.clone();
        closed.instances[0].requested_open = false;
        closed.sections.clear();
        validate(&closed, &registry).unwrap();
        assert_eq!(
            with_live_layout(&envelope, closed, &registry).unwrap_err().rule,
            Rule::DuplicateId,
            "closed native memory must not evade staging"
        );
        if !column_only {
            let mut remembered = incoming.clone();
            remembered.instances[0].requested_open = false;
            remembered.instances[0].memory.section = section("saved");
            remembered.instances[0].memory.location = SectionLocation::Floating { origin: Point { x: 4.0, y: 8.0 } };
            remembered.sections.clear();
            validate(&remembered, &registry).unwrap();
            assert_eq!(with_live_layout(&envelope, remembered, &registry).unwrap_err().rule, Rule::DuplicateId);
            let mut joining = incoming;
            for location in [&mut joining.sections[0].location, &mut joining.instances[0].memory.location] {
                let SectionLocation::Window { return_to, .. } = location else {
                    panic!("native descriptor");
                };
                return_to.section = None;
                return_to.join = Some(ReturnTab { section: section("saved"), index: 0 });
            }
            assert_eq!(with_live_layout(&envelope, joining, &registry).unwrap_err().rule, Rule::DuplicateId);
        }
        assert_eq!(service.inspect(), before);
        assert_eq!(encode(&envelope, &registry).unwrap(), original);
        let mut grown = registry.clone();
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new("extension.panel").unwrap();
        grown.register(spec).unwrap();
        let mut restored = envelope;
        restore_quarantine(&mut restored, &grown).unwrap();
        let mut owner = LayoutService::new(
            grown.clone(),
            restored.layout.clone(),
            HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true },
        )
        .unwrap();
        owner
            .dispatch(transaction(vec![Mutation::Open {
                kind: PanelKind::new("extension.panel").unwrap(),
                instance: id("extension"),
                destination: None,
            }]))
            .unwrap();
        let group = owner.layout().containing(&id("extension")).unwrap().id.clone();
        owner
            .dispatch(transaction(vec![Mutation::Open {
                kind: PanelKind::new("fixture.log").unwrap(),
                instance: id("companion"),
                destination: Some(Destination::JoinTab { section: group, index: 1 }),
            }]))
            .unwrap();
        let opened = with_live_layout(&restored, owner.layout().clone(), &grown).unwrap();
        let recovered = if column_only {
            owner.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("extension-host").unwrap() }])).unwrap();
            let recovered = with_live_layout(&opened, owner.layout().clone(), &grown).unwrap();
            assert_eq!(recovered.layout.containing(&id("extension")).unwrap().column, Some(ColumnId::new("reserved-column").unwrap()));
            recovered
        } else {
            assert_eq!(opened.layout.containing(&id("extension")).unwrap().id, section("saved"));
            opened
        };
        assert_eq!(recovered.retained_data[&id("extension")]["instance"]["future_draft"]["keep"], "closed extension draft");
    }
}

#[test]
fn deferred_projection_and_captured_prepare_admit_a_new_owner_without_mutating_the_service() {
    let mut service = fixture_service(false).unwrap();
    service.set_availability([(id("incoming"), Availability::Unavailable { reason: "outgoing owner".into() })].into()).unwrap();
    let sections = [section("section-1")].into();
    let columns = [ColumnId::new("column-1").unwrap()].into();
    let action = LayoutAction::Apply {
        expected_revision: Some(service.inspect().revision),
        mutations: vec![fixture_open("fixture.log", "incoming", "incoming-section", Zone::Left, 0).unwrap()],
    };
    let before = service.inspect();
    assert_eq!(service.prepare(action.clone()).err().unwrap().rule, Rule::Availability);
    assert_eq!(service.prepare_with_reservations(action.clone(), &sections, &columns).err().unwrap().rule, Rule::Availability);
    let projection: Layout = service.project_with_deferred_availability(action.clone(), &sections, &columns).unwrap();
    assert_eq!(projection.columns[0].id, ColumnId::new("column-2").unwrap());
    let captured = [(id("incoming"), Availability::Available)].into();
    let prepared = service.prepare_with_availability_and_reservations(action.clone(), &captured, &sections, &columns).unwrap();
    assert_eq!(prepared.layout(), &projection);
    assert_eq!(service.inspect(), before);
    let blocked = [(id("incoming"), Availability::Unavailable { reason: "proposed owner refuses".into() })].into();
    assert_eq!(service.prepare_with_availability_and_reservations(action, &blocked, &sections, &columns).err().unwrap().rule, Rule::Availability);
    assert_eq!(service.inspect(), before);
    let reply = service.commit(prepared).unwrap();
    assert_eq!(reply.layout, projection);
    assert_eq!(reply.availability, captured);
}

#[test]
fn proposed_owner_capture_controls_open_move_focus_activate_peek_and_header_permission() {
    for operation in 0..6 {
        let mut registry = synthetic_registry().unwrap();
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new("fixture.action").unwrap();
        spec.header_actions =
            vec![HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() }];
        registry.register(spec).unwrap();
        let mut service =
            LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
        service
            .dispatch(transaction(vec![
                fixture_open("fixture.action", "one", "first", Zone::Left, 0).unwrap(),
                Mutation::Collapse { section: section("first"), collapsed: true },
            ]))
            .unwrap();
        let blocked: AvailabilityMap = [(id("one"), Availability::Unavailable { reason: "old owner".into() })].into();
        service.set_availability(blocked.clone()).unwrap();
        let mutation = match operation {
            0 => Mutation::Open { kind: PanelKind::new("fixture.action").unwrap(), instance: id("one"), destination: None },
            1 => Mutation::Focus { instance: id("one") },
            2 => Mutation::Activate { instance: id("one") },
            3 => Mutation::Peek { instance: id("one") },
            4 => Mutation::Move {
                instance: id("one"),
                destination: Destination::NewSection {
                    section: section("moved"),
                    index: 0,
                    location: SectionLocation::Dock { zone: Zone::Right },
                    size: size(280.0, 240.0),
                },
            },
            _ => Mutation::HeaderAction { instance: id("one"), action: ControlId::new("refresh").unwrap() },
        };
        let action = transaction(vec![mutation]);
        let sections = std::collections::BTreeSet::new();
        let columns = std::collections::BTreeSet::new();
        let before = service.inspect();
        assert_eq!(service.prepare(action.clone()).err().unwrap().rule, Rule::Availability);
        let projection = service.project_with_deferred_availability(action.clone(), &sections, &columns).unwrap();
        assert_eq!(
            service.prepare_with_availability_and_reservations(action.clone(), &blocked, &sections, &columns).err().unwrap().rule,
            Rule::Availability
        );
        let captured = [(id("one"), Availability::Available)].into();
        let prepared = service.prepare_with_availability_and_reservations(action, &captured, &sections, &columns).unwrap();
        assert_eq!(prepared.layout(), &projection);
        assert_eq!(service.inspect(), before);
        let reply = service.commit(prepared).unwrap();
        assert_eq!(reply.layout, projection);
        assert_eq!(reply.availability, captured);
    }
}

#[test]
fn captured_header_fallback_is_validated_without_changing_requested_activation_or_layout() {
    let mut registry = synthetic_registry().unwrap();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("fixture.action").unwrap();
    spec.header_actions = vec![HeaderActionSpec { id: ControlId::new("refresh").unwrap(), label_key: "ui.refresh".into(), icon: "restore".into() }];
    registry.register(spec).unwrap();
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.action", "one", "first", Zone::Left, 0).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.action").unwrap(),
                instance: id("two"),
                destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
            },
        ]))
        .unwrap();
    let action = transaction(vec![Mutation::HeaderAction { instance: id("two"), action: ControlId::new("refresh").unwrap() }]);
    let sections = std::collections::BTreeSet::new();
    let columns = std::collections::BTreeSet::new();
    let before = service.inspect();
    assert_eq!(service.prepare(action.clone()).err().unwrap().rule, Rule::Availability);
    let projection = service.project_with_deferred_availability(action.clone(), &sections, &columns).unwrap();
    assert_eq!(projection, before.layout);
    let captured = [(id("one"), Availability::Unavailable { reason: "incoming owner".into() }), (id("two"), Availability::Available)].into();
    let prepared = service.prepare_with_availability_and_reservations(action.clone(), &captured, &sections, &columns).unwrap();
    assert_eq!(
        service.prepare_with_availability_and_reservations(action, &AvailabilityMap::new(), &sections, &columns).err().unwrap().rule,
        Rule::Availability
    );
    assert_eq!(service.inspect(), before);
    let reply = service.commit(prepared).unwrap();
    assert_eq!(reply.layout, before.layout);
    assert_eq!(reply.layout.sections[0].active, id("one"));
    assert_eq!(reply.effects, vec![HostEffect::HeaderAction { instance: id("two"), action: ControlId::new("refresh").unwrap() }]);
}

#[test]
fn captured_replace_retains_new_owner_availability_while_legacy_replace_still_clears_it() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    service.set_availability([(id("one"), Availability::Unavailable { reason: "old workspace".into() })].into()).unwrap();
    let action = LayoutAction::Replace { expected_revision: Some(service.inspect().revision), layout: service.layout().clone() };
    let empty = std::collections::BTreeSet::new();
    let columns = std::collections::BTreeSet::new();
    let before = service.inspect();
    let projection = service.project_with_deferred_availability(action.clone(), &empty, &columns).unwrap();
    let captured = [(id("one"), Availability::Unavailable { reason: "incoming workspace".into() })].into();
    let prepared = service.prepare_with_availability_and_reservations(action.clone(), &captured, &empty, &columns).unwrap();
    let legacy = service.prepare(action).unwrap();
    assert_eq!(prepared.layout(), &projection);
    assert_eq!(service.inspect(), before);
    assert_eq!(service.commit(prepared).unwrap().availability, captured);
    assert_eq!(service.commit(legacy).unwrap_err().rule, Rule::Revision);
    let legacy = service.prepare(LayoutAction::Replace { expected_revision: None, layout: projection }).unwrap();
    assert!(service.commit(legacy).unwrap().availability.is_empty());
}

#[test]
fn deferred_projection_preserves_presented_pair_metadata_and_final_prepare_rechecks_captured_adjacency() {
    let mut service = fixture_service(false).unwrap();
    for (panel, group) in [("one", "first"), ("hidden", "middle"), ("two", "second")] {
        open(&mut service, panel, group);
    }
    let current: AvailabilityMap = [(id("hidden"), Availability::Unavailable { reason: "old owner".into() })].into();
    service.set_availability(current.clone()).unwrap();
    let action = transaction(vec![Mutation::ResizeAllocatedPair {
        first: section("first"),
        second: section("second"),
        first_size: size(280.0, 180.0),
        second_size: size(280.0, 300.0),
        first_weight: 0.2,
        second_weight: 0.3,
        other_weight_scale: 1.0,
        prefer_allocated: false,
    }]);
    let sections = std::collections::BTreeSet::new();
    let columns = std::collections::BTreeSet::new();
    let before = service.inspect();
    let projection = service.project_with_deferred_availability(action.clone(), &sections, &columns).unwrap();
    assert_eq!(
        service.prepare_with_availability_and_reservations(action.clone(), &AvailabilityMap::new(), &sections, &columns).err().unwrap().rule,
        Rule::Placement
    );
    let prepared = service.prepare_with_availability_and_reservations(action, &current, &sections, &columns).unwrap();
    assert_eq!(prepared.layout(), &projection);
    assert_eq!(service.inspect(), before);
}

#[test]
fn deferred_projection_keeps_identity_capability_lifecycle_and_input_limits_and_captured_maps_are_validated() {
    let service = fixture_service(false).unwrap();
    let sections = [section("reserved")].into();
    let columns = std::collections::BTreeSet::new();
    let before = service.inspect();
    let theft = transaction(vec![fixture_open("fixture.log", "one", "reserved", Zone::Left, 0).unwrap()]);
    assert_eq!(service.project_with_deferred_availability(theft.clone(), &sections, &columns).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(
        service.prepare_with_availability_and_reservations(theft, &AvailabilityMap::new(), &sections, &columns).err().unwrap().rule,
        Rule::DuplicateId
    );
    let lifecycle = transaction(vec![fixture_open("fixture.log", "one", "new", Zone::Left, 0).unwrap(), Mutation::Close { instance: id("one") }]);
    assert_eq!(service.project_with_deferred_availability(lifecycle, &sections, &columns).unwrap_err().rule, Rule::Lifecycle);
    let missing = transaction(vec![Mutation::Open { kind: PanelKind::new("missing.kind").unwrap(), instance: id("one"), destination: None }]);
    assert_eq!(service.project_with_deferred_availability(missing, &sections, &columns).unwrap_err().rule, Rule::MissingTarget);
    let bad_map = [(InstanceId("UPPER".into()), Availability::Available)].into();
    assert_eq!(
        service.prepare_with_availability_and_reservations(LayoutAction::Inspect, &bad_map, &sections, &columns).err().unwrap().rule,
        Rule::Id
    );
    let oversized = (0..=MAX_INSTANCES).map(|n| (id(&format!("instance-{n}")), Availability::Available)).collect();
    assert_eq!(
        service.prepare_with_availability_and_reservations(LayoutAction::Inspect, &oversized, &sections, &columns).err().unwrap().rule,
        Rule::CollectionLimit
    );
    let native = transaction(vec![Mutation::Open {
        kind: PanelKind::new("fixture.log").unwrap(),
        instance: id("one"),
        destination: Some(Destination::NewSection {
            section: section("window"),
            index: 0,
            size: size(280.0, 240.0),
            location: SectionLocation::Window {
                host: HostId::new("host").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn {
                    zone: Zone::Left,
                    index: 0,
                    size: size(280.0, 240.0),
                    collapsed: false,
                    section: None,
                    column: None,
                    column_index: 0,
                    column_width: None,
                    weight: 240.0,
                    preferred_extent: None,
                    extent_priority: 0,
                    join: None,
                },
            },
        }),
    }]);
    assert_eq!(service.project_with_deferred_availability(native, &sections, &columns).unwrap_err().rule, Rule::Capability);
    assert_eq!(service.inspect(), before);
}

#[test]
fn extent_priority_defaults_omit_zero_and_reject_untyped_or_orphaned_ranks() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    let wire = serde_json::to_value(service.layout()).unwrap();
    assert!(wire["sections"][0].get("extent_priority").is_none());
    assert!(wire["instances"][0]["memory"].get("extent_priority").is_none());
    let native = DockReturn {
        zone: Zone::Left,
        index: 0,
        size: size(280.0, 240.0),
        collapsed: false,
        section: None,
        column: None,
        column_index: 0,
        column_width: None,
        weight: 1.0,
        preferred_extent: None,
        extent_priority: 0,
        join: None,
    };
    assert!(serde_json::to_value(&native).unwrap().get("extent_priority").is_none());
    for rank in [serde_json::json!(-1), serde_json::json!(65536), serde_json::json!(1.5), serde_json::json!("future"), serde_json::Value::Null] {
        let mut group = wire["sections"][0].clone();
        group["extent_priority"] = rank.clone();
        assert!(serde_json::from_value::<Section>(group).is_err());
        let mut memory = wire["instances"][0]["memory"].clone();
        memory["extent_priority"] = rank.clone();
        assert!(serde_json::from_value::<PlacementMemory>(memory).is_err());
        let mut returning = serde_json::to_value(&native).unwrap();
        returning["extent_priority"] = rank;
        assert!(serde_json::from_value::<DockReturn>(returning).is_err());
    }
    assert!(validate_extent_priority(Some(240.0), u16::MAX).is_ok());
    assert_eq!(validate_extent_priority(None, 1).unwrap_err().rule, Rule::Bounds);
    let mut invalid = service.layout().clone();
    invalid.sections[0].extent_priority = 1;
    assert_eq!(validate(&invalid, service.registry()).unwrap_err().rule, Rule::Bounds);
    let mut invalid = service.layout().clone();
    invalid.instances[0].memory.extent_priority = 1;
    assert_eq!(validate(&invalid, service.registry()).unwrap_err().rule, Rule::Bounds);
    let mut invalid = native;
    invalid.extent_priority = 1;
    assert_eq!(invalid.validate().unwrap_err().rule, Rule::Bounds);
    let mut native_service = fixture_service(true).unwrap();
    open(&mut native_service, "one", "first");
    native_service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(280.0, 400.0) }])).unwrap();
    reject(
        &mut native_service,
        vec![Mutation::Move {
            instance: id("one"),
            destination: Destination::NewSection {
                section: section("window"),
                index: 0,
                size: size(280.0, 240.0),
                location: SectionLocation::Window { host: HostId::new("host").unwrap(), origin: Point { x: 0.0, y: 0.0 }, return_to: invalid },
            },
        }],
        Rule::Bounds,
    );
    let mut raw = serde_json::to_value(LayoutEnvelope::new(service.layout().clone())).unwrap();
    raw["layout"]["sections"][0]["extent_priority"] = serde_json::json!(0);
    raw["layout"]["instances"][0]["memory"]["extent_priority"] = serde_json::json!(0);
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), service.registry(), &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let encoded: serde_json::Value = serde_json::from_slice(&encode(&loaded.envelope, service.registry()).unwrap()).unwrap();
    assert!(encoded["layout"]["sections"][0].get("extent_priority").is_none());
    assert!(encoded["layout"]["instances"][0]["memory"].get("extent_priority").is_none());
    let mut data = serde_json::to_value(&service.layout().sections[0]).unwrap();
    data["extent_priority"] = serde_json::json!(1);
    let mut invalid = LayoutEnvelope::new(service.layout().clone());
    invalid.section_snapshots.insert(section("first"), SectionSnapshot { data, index: 0, projected: None });
    assert_eq!(encode(&invalid, service.registry()).unwrap_err().rule, Rule::Bounds);
}

#[test]
fn dock_set_size_authors_a_new_tier_and_free_window_sizing_retains_its_latent_rank() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let weights: Vec<_> = service.layout().sections.iter().map(|s| s.weight).collect();
    for (group, extent, rank) in [("first", 400.0, 1), ("second", 300.0, 2), ("first", 350.0, 3)] {
        service.dispatch(transaction(vec![Mutation::SetSize { section: section(group), size: size(280.0, extent) }])).unwrap();
        let saved = service.layout().section(&section(group)).unwrap();
        assert_eq!((saved.preferred_extent, saved.extent_priority), (Some(extent), rank));
        for panel in service.layout().instances.iter().filter(|p| saved.tabs.contains(&p.id)) {
            assert_eq!((panel.memory.preferred_extent, panel.memory.extent_priority), (Some(extent), rank));
        }
    }
    assert_eq!(service.layout().sections.iter().map(|s| s.weight).collect::<Vec<_>>(), weights);
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("one"),
            destination: Destination::NewSection {
                section: section("float"),
                index: 1,
                location: SectionLocation::Floating { origin: Point { x: 20.0, y: 30.0 } },
                size: size(300.0, 280.0),
            },
        }]))
        .unwrap();
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("float"), size: size(500.0, 600.0) }])).unwrap();
    let floating = service.layout().section(&section("float")).unwrap();
    assert_eq!(floating.size, size(500.0, 600.0));
    assert_eq!((floating.preferred_extent, floating.extent_priority), (Some(350.0), 3));
    service.dispatch(transaction(vec![Mutation::Close { instance: id("one") }])).unwrap();
    service.dispatch(transaction(vec![Mutation::Focus { instance: id("one") }])).unwrap();
    assert_eq!(service.layout().containing(&id("one")).unwrap().extent_priority, 3);
}

#[test]
fn exhausted_extent_tiers_compact_live_closed_native_ties_and_preserve_other_group_intent_atomically() {
    let mut service = fixture_service(true).unwrap();
    for (panel, group) in [("one", "first"), ("two", "second"), ("zero", "zero"), ("closed", "closed"), ("native", "native-source")] {
        open(&mut service, panel, group);
    }
    service.dispatch(transaction(vec![Mutation::Close { instance: id("closed") }])).unwrap();
    service
        .dispatch(transaction(vec![Mutation::Move {
            instance: id("native"),
            destination: Destination::NewSection {
                section: section("window"),
                index: 3,
                size: size(280.0, 240.0),
                location: SectionLocation::Window {
                    host: HostId::new("host").unwrap(),
                    origin: Point { x: 0.0, y: 0.0 },
                    return_to: DockReturn {
                        zone: Zone::Left,
                        index: 3,
                        size: size(280.0, 240.0),
                        collapsed: false,
                        section: None,
                        column: None,
                        column_index: 0,
                        column_width: None,
                        weight: 240.0,
                        preferred_extent: None,
                        extent_priority: 0,
                        join: None,
                    },
                },
            },
        }]))
        .unwrap();
    service.dispatch(transaction(vec![fixture_open("fixture.log", "right", "right", Zone::Right, 4).unwrap()])).unwrap();
    let mut layout = service.layout().clone();
    for group in &mut layout.sections {
        match group.id.0.as_str() {
            "first" => {
                group.preferred_extent = Some(200.0);
                group.extent_priority = 40000;
            }
            "second" => {
                group.preferred_extent = Some(200.0);
                group.extent_priority = 17;
            }
            "zero" => {
                group.preferred_extent = Some(200.0);
            }
            "right" => {
                group.preferred_extent = Some(500.0);
                group.extent_priority = u16::MAX;
            }
            _ => {}
        }
        if let SectionLocation::Window { return_to, .. } = &mut group.location {
            return_to.preferred_extent = Some(260.0);
            return_to.extent_priority = u16::MAX;
        }
    }
    for panel in &mut layout.instances {
        if let Some(group) = layout.sections.iter().find(|s| s.tabs.contains(&panel.id)) {
            panel.memory.preferred_extent = group.preferred_extent;
            panel.memory.extent_priority = group.extent_priority;
            panel.memory.location = group.location.clone();
        } else {
            panel.memory.preferred_extent = Some(250.0);
            panel.memory.extent_priority = 17;
        }
    }
    let mut service = LayoutService::new(service.registry().clone(), layout, service.capabilities().clone()).unwrap();
    let before = service.inspect();
    reject(
        &mut service,
        vec![
            Mutation::SetSize { section: section("first"), size: size(280.0, 350.0) },
            Mutation::SetBottomHeight { section: section("first"), height: 200.0 },
        ],
        Rule::Placement,
    );
    let prepared = service.prepare(transaction(vec![Mutation::SetSize { section: section("first"), size: size(280.0, 350.0) }])).unwrap();
    assert_eq!(service.inspect(), before);
    let next = prepared.layout();
    assert_eq!(next.section(&section("first")).unwrap().extent_priority, 4);
    assert_eq!(next.section(&section("second")).unwrap().extent_priority, 1);
    assert_eq!(next.instance(&id("closed")).unwrap().memory.extent_priority, 1);
    assert_eq!(next.section(&section("zero")).unwrap().extent_priority, 0);
    let mut right = before.layout.section(&section("right")).unwrap().clone();
    right.extent_priority = 3; // Global compaction preserves its tier tie with the native return.
    assert_eq!(next.section(&section("right")).unwrap(), &right);
    let SectionLocation::Window { return_to, .. } = &next.section(&section("window")).unwrap().location else {
        panic!("window");
    };
    assert_eq!((return_to.preferred_extent, return_to.extent_priority), (Some(260.0), 3));
    let SectionLocation::Window { return_to, .. } = &next.instance(&id("native")).unwrap().memory.location else {
        panic!("memory");
    };
    assert_eq!(return_to.extent_priority, 3);
    assert_eq!(next.sections.iter().map(|s| s.weight).collect::<Vec<_>>(), before.layout.sections.iter().map(|s| s.weight).collect::<Vec<_>>());
    service.commit(prepared).unwrap();
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("second"), size: size(280.0, 300.0) }])).unwrap();
    assert_eq!(service.layout().section(&section("second")).unwrap().extent_priority, 5);
}

#[test]
fn ranked_allocated_pairs_represent_explicit_underfill_compression_and_repeated_overlap() {
    for height in [1000.0, 400.0] {
        let mut service = fixture_service(false).unwrap();
        for (panel, group) in [("one", "first"), ("two", "second"), ("three", "third")] {
            open(&mut service, panel, group);
        }
        let mut layout = service.layout().clone();
        for group in &mut layout.sections {
            group.preferred_extent = Some(200.0);
        }
        for panel in &mut layout.instances {
            panel.memory.preferred_extent = Some(200.0);
        }
        let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
        let mut env = allocated_pair_environment(&service, height);
        env.gap = 4.0;
        let initial = allocate_layout(service.layout(), service.registry(), &env).unwrap();
        let expected = if height == 1000.0 { 200.0 } else { 392.0 / 3.0 };
        for group in &initial.sections {
            assert!((group.outer.size.height - expected).abs() < 0.002);
        }
        let first = if height == 1000.0 { 180.0 } else { 120.0 };
        let second = initial.sections[0].outer.size.height + initial.sections[1].outer.size.height - first;
        let action = transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, first),
            second_size: size(280.0, second),
            first_weight: 0.1,
            second_weight: 0.2,
            other_weight_scale: 1.0,
            prefer_allocated: true,
        }]);
        let before = service.inspect();
        let prepared = service.prepare(action).unwrap();
        assert_eq!(service.inspect(), before);
        let shown = allocate_layout(prepared.layout(), service.registry(), &env).unwrap();
        assert!((shown.sections[0].outer.size.height - first).abs() < 0.002);
        assert!((shown.sections[1].outer.size.height - second).abs() < 0.002);
        assert!((shown.sections[2].outer.size.height - initial.sections[2].outer.size.height).abs() < 0.002);
        assert!((shown.sections[2].outer.origin.y - initial.sections[2].outer.origin.y).abs() < 0.002);
        assert_eq!(prepared.layout().sections.iter().map(|s| s.extent_priority).collect::<Vec<_>>(), vec![1, 1, 0]);
        service.commit(prepared).unwrap();
        assert_eq!(allocate_layout(service.layout(), service.registry(), &env).unwrap(), shown);
        let saved = service.layout().clone();
        service
            .dispatch(transaction(vec![
                Mutation::Collapse { section: section("first"), collapsed: true },
                Mutation::Collapse { section: section("first"), collapsed: false },
            ]))
            .unwrap();
        assert_eq!(service.layout(), &saved);
        let overlap_first = if height == 1000.0 { 210.0 } else { 130.0 };
        let overlap_second = shown.sections[1].outer.size.height + shown.sections[2].outer.size.height - overlap_first;
        service
            .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
                first: section("second"),
                second: section("third"),
                first_size: size(280.0, overlap_first),
                second_size: size(280.0, overlap_second),
                first_weight: 0.2,
                second_weight: 0.3,
                other_weight_scale: 1.0,
                prefer_allocated: true,
            }]))
            .unwrap();
        let overlap = allocate_layout(service.layout(), service.registry(), &env).unwrap();
        assert!((overlap.sections[0].outer.size.height - first).abs() < 0.002);
        assert!((overlap.sections[1].outer.size.height - overlap_first).abs() < 0.002);
        assert!((overlap.sections[2].outer.size.height - overlap_second).abs() < 0.002);
        assert_eq!(service.layout().sections.iter().map(|s| s.extent_priority).collect::<Vec<_>>(), vec![1, 2, 2]);
        let mut expanded = env.clone();
        expanded.capacity.size.height = 1400.0;
        let roomy = allocate_layout(service.layout(), service.registry(), &expanded).unwrap();
        for (actual, saved) in roomy.sections.iter().zip(&service.layout().sections) {
            assert!((actual.outer.size.height - saved.preferred_extent.unwrap()).abs() < 0.002);
        }
    }
    let mut service = fixture_service(false).unwrap();
    for (panel, group) in [("one", "first"), ("two", "second"), ("three", "third")] {
        open(&mut service, panel, group);
    }
    let mut layout = service.layout().clone();
    for (group, extent) in layout.sections.iter_mut().zip([1200.0, 120.0, 200.0]) {
        group.preferred_extent = Some(extent);
    }
    for panel in &mut layout.instances {
        panel.memory.preferred_extent = layout.sections.iter().find(|s| s.tabs.contains(&panel.id)).unwrap().preferred_extent;
    }
    let mut service = LayoutService::new(service.registry().clone(), layout, HostCapabilities::default()).unwrap();
    let mut env = allocated_pair_environment(&service, 1528.0);
    env.gap = 4.0;
    let initial = allocate_layout(service.layout(), service.registry(), &env).unwrap();
    for (first, second) in [(120.0, 1200.0), (1200.0, 120.0)] {
        service
            .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
                first: section("first"),
                second: section("second"),
                first_size: size(280.0, first),
                second_size: size(280.0, second),
                first_weight: 0.0,
                second_weight: 1.0,
                other_weight_scale: 1.0,
                prefer_allocated: true,
            }]))
            .unwrap();
        let actual = allocate_layout(service.layout(), service.registry(), &env).unwrap();
        assert!((actual.sections[0].outer.size.height - first).abs() < 0.002);
        assert!((actual.sections[1].outer.size.height - second).abs() < 0.002);
        assert!((actual.sections[2].outer.origin.y - initial.sections[2].outer.origin.y).abs() < 0.002);
        assert_eq!(actual.sections[2].outer.size, initial.sections[2].outer.size);
    }
}

#[test]
fn ranked_pair_intent_roundtrips_and_false_clears_rank_without_snapshot_resurrection() {
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 180.0),
            second_size: size(280.0, 220.0),
            first_weight: 0.4,
            second_weight: 0.6,
            other_weight_scale: 1.0,
            prefer_allocated: true,
        }]))
        .unwrap();
    let mut envelope = LayoutEnvelope::new(service.layout().clone());
    for (index, group) in service.layout().sections.iter().enumerate() {
        let mut data = serde_json::to_value(group).unwrap();
        data["future"] = serde_json::json!({"extent_priority":"opaque payload"});
        envelope.section_snapshots.insert(group.id.clone(), SectionSnapshot { data, index, projected: Some(group.clone()) });
    }
    let loaded = decode(&encode(&envelope, service.registry()).unwrap(), service.registry(), &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    assert_eq!(loaded.envelope.layout, *service.layout());
    for group in &loaded.envelope.layout.sections {
        assert_eq!(group.extent_priority, 1);
    }
    service
        .dispatch(transaction(vec![Mutation::ResizeAllocatedPair {
            first: section("first"),
            second: section("second"),
            first_size: size(280.0, 120.0),
            second_size: size(280.0, 280.0),
            first_weight: 0.0,
            second_weight: 1.0,
            other_weight_scale: 1.0,
            prefer_allocated: false,
        }]))
        .unwrap();
    let staged = with_live_layout(&loaded.envelope, service.layout().clone(), service.registry()).unwrap();
    let raw: serde_json::Value = serde_json::from_slice(&encode(&staged, service.registry()).unwrap()).unwrap();
    for name in ["first", "second"] {
        assert!(raw["section_snapshots"][name]["data"].get("extent_priority").is_none());
        assert!(raw["section_snapshots"][name]["data"].get("preferred_extent").is_none());
        assert_eq!(raw["section_snapshots"][name]["data"]["future"]["extent_priority"], "opaque payload");
    }
}

#[test]
fn bottom_height_updates_the_common_row_and_preserves_width_intent_rank_and_weight() {
    let mut registry = synthetic_registry().unwrap();
    let mut constrained = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    constrained.kind = PanelKind::new("fixture.short-height").unwrap();
    constrained.min_size.height = 140.0;
    constrained.max_size.height = 300.0;
    registry.register(constrained).unwrap();
    let mut service =
        LayoutService::new(registry.clone(), Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    service
        .dispatch(transaction(vec![
            fixture_open("fixture.log", "one", "first", Zone::Bottom, 0).unwrap(),
            fixture_open("fixture.log", "two", "second", Zone::Bottom, 1).unwrap(),
            Mutation::Open {
                kind: PanelKind::new("fixture.short-height").unwrap(),
                instance: id("inactive"),
                destination: Some(Destination::JoinTab { section: section("first"), index: 1 }),
            },
        ]))
        .unwrap();
    service
        .dispatch(transaction(vec![
            Mutation::SetSize { section: section("first"), size: size(320.0, 240.0) },
            Mutation::SetSize { section: section("second"), size: size(450.0, 500.0) },
        ]))
        .unwrap();
    let mut imported = service.layout().clone();
    let memory = &mut imported.instances.iter_mut().find(|p| p.id == id("one")).unwrap().memory;
    memory.size.width = 800.0;
    memory.preferred_extent = Some(600.0);
    memory.extent_priority = 77;
    memory.weight = 3.0;
    let mut service = LayoutService::new(registry.clone(), imported, HostCapabilities::default()).unwrap();
    assert_eq!(bottom_height_bounds(service.layout(), &registry).unwrap(), (140.0, 300.0));
    let before = service.layout().clone();
    let prepared = service.prepare(transaction(vec![Mutation::SetBottomHeight { section: section("first"), height: 180.0 }])).unwrap();
    assert_eq!(service.layout(), &before);
    for (old, new) in before.sections.iter().zip(&prepared.layout().sections) {
        assert_eq!(new.size.height, 180.0);
        assert_eq!(
            (new.size.width, new.preferred_extent, new.extent_priority, new.weight),
            (old.size.width, old.preferred_extent, old.extent_priority, old.weight)
        );
        assert_eq!(new.active, old.active);
    }
    for (old, new) in before.instances.iter().zip(&prepared.layout().instances) {
        let mut expected = old.memory.clone();
        expected.size.height = 180.0;
        assert_eq!(new.memory, expected, "height-only edit preserves divergent remembered width intent");
    }
    service.commit(prepared).unwrap();
    assert!(service.layout().instances.iter().all(|p| p.memory.size.height == 180.0));
    for height in [0.0, -1.0, f32::NAN, f32::INFINITY, 139.0, 301.0, MAX_LOGICAL + 1.0] {
        reject(&mut service, vec![Mutation::SetBottomHeight { section: section("second"), height }], Rule::Bounds);
    }
    service.dispatch(transaction(vec![fixture_open("fixture.log", "side", "side", Zone::Left, 2).unwrap()])).unwrap();
    reject(&mut service, vec![Mutation::SetBottomHeight { section: section("side"), height: 180.0 }], Rule::Placement);
    reject(&mut service, vec![Mutation::SetBottomHeight { section: section("missing"), height: 180.0 }], Rule::MissingTarget);
    assert_eq!(
        commands::parse_command("panel.set_bottom_height", &serde_json::json!({"section":"first","height":180})).unwrap_err().rule,
        Rule::MissingTarget
    );
}

#[test]
fn bottom_height_refuses_disjoint_registered_card_intervals_atomically() {
    let mut registry = synthetic_registry().unwrap();
    for (kind, min, max, default) in [("fixture.low", 120.0, 200.0, 180.0), ("fixture.high", 400.0, 600.0, 450.0)] {
        let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
        spec.kind = PanelKind::new(kind).unwrap();
        spec.min_size.height = min;
        spec.max_size.height = max;
        spec.default_size.height = default;
        registry.register(spec).unwrap();
    }
    let mut service =
        LayoutService::new(registry, Layout::empty(WorkspaceId::new("fixture.workspace").unwrap()), HostCapabilities::default()).unwrap();
    for (kind, group, height) in [("fixture.low", "low", 180.0), ("fixture.high", "high", 450.0)] {
        let index = service.layout().sections.len();
        service
            .dispatch(transaction(vec![Mutation::Open {
                kind: PanelKind::new(kind).unwrap(),
                instance: id(group),
                destination: Some(Destination::NewSection {
                    section: section(group),
                    index,
                    size: size(280.0, height),
                    location: SectionLocation::Dock { zone: Zone::Bottom },
                }),
            }]))
            .unwrap();
    }
    assert_eq!(bottom_height_bounds(service.layout(), service.registry()).unwrap_err().rule, Rule::IncompatibleTabs);
    reject(&mut service, vec![Mutation::SetBottomHeight { section: section("low"), height: 180.0 }], Rule::IncompatibleTabs);
}

#[test]
fn raw_quarantine_priority_and_nested_opaque_fields_survive_known_compaction_then_native_reappearance() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    let mut service = fixture_service(true).unwrap();
    open(&mut service, "one", "first");
    open(&mut service, "two", "second");
    let mut layout = service.layout().clone();
    for (group, rank) in layout.sections.iter_mut().zip([u16::MAX, 17]) {
        group.preferred_extent = Some(300.0);
        group.extent_priority = rank;
    }
    for panel in &mut layout.instances {
        let group = layout.sections.iter().find(|s| s.tabs.contains(&panel.id)).unwrap();
        panel.memory.preferred_extent = group.preferred_extent;
        panel.memory.extent_priority = group.extent_priority;
    }
    let registry = service.registry().clone();
    let column = layout.sections[0].column.clone().unwrap();
    let mut raw = serde_json::to_value(LayoutEnvelope::new(layout)).unwrap();
    raw["quarantine"] = json!([{"instance":"extension","kind":"extension.panel","section":null,"instance_data":{
        "id":"extension","kind":"extension.panel","requested_open":false,"future":{"extent_priority":"opaque rank payload"},"memory":{
            "section":"native-memory","section_index":2,"tab_index":0,"size":{"width":280,"height":400},"collapsed":false,"weight":240,
            "preferred_extent":400,"extent_priority":41,"location":{"type":"window","host":"extension-host","origin":{"x":20,"y":30},
                "future_monitor":{"extent_priority":"monitor metadata"},"return_to":{
                    "section":"extension-return","zone":"left","index":2,"column":column,"column_index":0,"column_width":280,
                    "size":{"width":280,"height":350},"collapsed":false,"weight":240,"preferred_extent":350,"extent_priority":42
                }
            }
        }
    }}]);
    let pending: PanelInstance = serde_json::from_value(raw["quarantine"][0]["instance_data"].clone()).unwrap();
    let snapshot = Section {
        id: pending.memory.section.clone(),
        location: pending.memory.location.clone(),
        tabs: vec![pending.id.clone()],
        active: pending.id,
        size: pending.memory.size,
        collapsed: false,
        column: None,
        weight: pending.memory.weight,
        preferred_extent: pending.memory.preferred_extent,
        extent_priority: pending.memory.extent_priority,
    };
    let snapshot_data = serde_json::to_value(snapshot).unwrap();
    raw["section_snapshots"]["native-memory"] = json!({"index":2,"data":snapshot_data});
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    let opaque = loaded.envelope.quarantine[0].instance_data.clone();
    let mut service = LayoutService::new(
        registry.clone(),
        loaded.envelope.layout.clone(),
        HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true },
    )
    .unwrap();
    service.dispatch(transaction(vec![Mutation::SetSize { section: section("first"), size: size(280.0, 350.0) }])).unwrap();
    assert_eq!(service.layout().section(&section("first")).unwrap().extent_priority, 5);
    let mut staged = with_live_layout(&loaded.envelope, service.layout().clone(), &registry).unwrap();
    assert_eq!(staged.quarantine[0].instance_data, opaque);
    let shadow = staged.extent_priority_overrides.iter().find(|record| record.target == ExtentPriorityTarget::MemoryReturn(id("extension"))).unwrap();
    assert_eq!((shadow.stored_priority, shadow.priority), (42, 3));
    let shared = staged
        .extent_priority_overrides
        .iter()
        .find(|record| record.target == ExtentPriorityTarget::SectionReturn(section("native-memory")))
        .unwrap();
    assert_eq!((shared.stored_priority, shared.priority), (42, 3));
    assert_eq!(staged.section_snapshots[&section("native-memory")].data, snapshot_data);
    let roundtrip = decode(&encode(&staged, &registry).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(roundtrip.notice.is_none());
    assert_eq!(roundtrip.envelope.quarantine[0].instance_data, opaque);
    let mut preset = LayoutEnvelope::new(staged.layout.clone());
    preset.layout.extent_priority_context = ExtentPriorityContext::default();
    let mut catalogue = commands::InMemoryCatalogue::default();
    catalogue.register_builtin("Without native peer", &preset, &registry).unwrap();
    catalogue.remember(&staged, None, &registry).unwrap();
    let mut backend = commands::InMemoryPanelBackend::new(
        LayoutService::new(registry.clone(), staged.layout.clone(), HostCapabilities::default()).unwrap(),
        catalogue,
    );
    backend.execute("layout.apply", &json!({"name":"Without native peer"})).unwrap();
    staged = backend.catalogue.state().remembered(&staged.layout.workspace).unwrap().project_envelope(&registry, &staged.layout.workspace).unwrap();
    assert_eq!(staged.quarantine[0].instance_data, opaque);
    assert_eq!(staged.section_snapshots[&section("native-memory")].data, snapshot_data);
    for target in [ExtentPriorityTarget::MemoryReturn(id("extension")), ExtentPriorityTarget::SectionReturn(section("native-memory"))] {
        let shadow = staged.extent_priority_overrides.iter().find(|record| record.target == target).unwrap();
        assert_eq!((shadow.stored_priority, shadow.priority), (42, 3), "preset merge retains native recovery precedence");
    }
    let mut grown = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.panel").unwrap();
    grown.register(spec).unwrap();
    restore_quarantine(&mut staged, &grown).unwrap();
    let mut owner =
        LayoutService::new(grown.clone(), staged.layout.clone(), HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true })
            .unwrap();
    owner
        .dispatch(transaction(vec![Mutation::Open {
            kind: PanelKind::new("extension.panel").unwrap(),
            instance: id("extension"),
            destination: None,
        }]))
        .unwrap();
    let window = owner.layout().containing(&id("extension")).unwrap();
    assert_eq!((window.preferred_extent, window.extent_priority), (Some(400.0), 2));
    let opened = with_live_layout(&staged, owner.layout().clone(), &grown).unwrap();
    assert_eq!(opened.section_snapshots[&section("native-memory")].data, snapshot_data);
    owner.dispatch(transaction(vec![Mutation::NativeClose { host: HostId::new("extension-host").unwrap() }])).unwrap();
    let dock = owner.layout().containing(&id("extension")).unwrap();
    assert_eq!((dock.preferred_extent, dock.extent_priority), (Some(350.0), 3));
    let mut environment = allocated_pair_environment(&owner, 400.0);
    environment.gap = 4.0;
    let geometry = allocate_layout(owner.layout(), owner.registry(), &environment).unwrap();
    for (name, expected) in [("first", 152.0), ("second", 120.0), ("extension-return", 120.0)] {
        let actual = geometry.sections.iter().find(|s| s.section == section(name)).unwrap();
        assert!((actual.outer.size.height - expected).abs() < 0.002, "new authored precedence survives opaque native recovery");
    }
    let saved = with_live_layout(&opened, owner.layout().clone(), &grown).unwrap();
    assert_eq!(saved.retained_data[&id("extension")]["instance"]["future"]["extent_priority"], "opaque rank payload");
    assert_eq!(saved.retained_data[&id("extension")]["instance"]["memory"]["location"]["future_monitor"]["extent_priority"], "monitor metadata");
}

fn priority_recovery_envelope(first_rank: u16, snapshot: bool) -> (PanelRegistry, LayoutEnvelope) {
    use serde_json::json;
    let mut service = fixture_service(false).unwrap();
    open(&mut service, "a", "a");
    open(&mut service, "b", "b");
    let mut layout = service.layout().clone();
    for (group, rank) in layout.sections.iter_mut().zip([first_rank, 17]) {
        group.preferred_extent = Some(300.0);
        group.extent_priority = rank;
    }
    for panel in &mut layout.instances {
        let group = layout.sections.iter().find(|s| s.id == panel.memory.section).unwrap();
        panel.memory.preferred_extent = group.preferred_extent;
        panel.memory.extent_priority = group.extent_priority;
    }
    let column = layout.columns[0].id.clone();
    let mut raw = serde_json::to_value(LayoutEnvelope::new(layout)).unwrap();
    raw["quarantine"] = json!([{"instance":"q","kind":"extension.priority","section":if snapshot { json!("q") } else { serde_json::Value::Null },
        "instance_data":{"id":"q","kind":"extension.priority","requested_open":snapshot,"memory":{
            "section":"q","location":{"type":"dock","zone":"left"},"column":column,"column_width":280,
            "section_index":2,"tab_index":0,"size":{"width":280,"height":350},"collapsed":false,"weight":240,
            "preferred_extent":350,"extent_priority":42},"future":{"draft":"keep Q raw","extent_priority":"opaque"}}}]);
    if snapshot {
        raw["section_snapshots"]["q"] = json!({"index":2,"data":{"id":"q","location":{"type":"dock","zone":"left"},
            "column":column,"tabs":["q"],"active":"q","size":{"width":280,"height":350},"collapsed":false,
            "weight":240,"preferred_extent":350,"extent_priority":42,"future_surface":{"keep":true}}});
        raw["section_order"].as_array_mut().unwrap().push(json!("q"));
    }
    let registry = service.registry().clone();
    let loaded = decode(&serde_json::to_vec(&raw).unwrap(), &registry, &persistence_fallback()).unwrap();
    assert!(loaded.notice.is_none());
    (registry, loaded.envelope)
}

fn restored_priority_owner(mut envelope: LayoutEnvelope, registry: &PanelRegistry) -> (PanelRegistry, LayoutEnvelope, LayoutService) {
    let mut grown = registry.clone();
    let mut spec = registry.get(&PanelKind::new("fixture.log").unwrap()).unwrap().clone();
    spec.kind = PanelKind::new("extension.priority").unwrap();
    grown.register(spec).unwrap();
    restore_quarantine(&mut envelope, &grown).unwrap();
    let mut owner = LayoutService::new(grown.clone(), envelope.layout.clone(), HostCapabilities::default()).unwrap();
    owner
        .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("extension.priority").unwrap(), instance: id("q"), destination: None }]))
        .unwrap();
    (grown, envelope, owner)
}

#[test]
fn recovery_priority_context_preserves_latest_resize_with_and_without_exhaustion_and_shared_snapshots() {
    for initial in [u16::MAX, 10] {
        for snapshot in [false, true] {
            let (registry, envelope) = priority_recovery_envelope(initial, snapshot);
            let raw = envelope.quarantine[0].instance_data.clone();
            let original_snapshot = envelope.section_snapshots.get(&section("q")).map(|s| s.data.clone());
            let context = preparation_context(&envelope).unwrap();
            assert!(context.extent_priorities.complete);
            assert_eq!(envelope.layout.instances.len(), 2);
            assert_eq!(envelope.layout.sections.len(), 2, "priority metadata creates no placed surrogates");
            let mut service = LayoutService::new(registry.clone(), envelope.layout.clone(), HostCapabilities::default()).unwrap();
            let action = transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }]);
            let before = service.inspect();
            let projected = service.project_with_context(action.clone(), &context).unwrap();
            let prepared = service.prepare_with_context(action.clone(), &AvailabilityMap::new(), &context).unwrap();
            assert_eq!(prepared.layout(), &projected);
            assert_eq!(service.prepare(action.clone()).unwrap().layout(), &projected, "full-envelope hydration seeds bare preparation too");
            assert_eq!(service.inspect(), before);
            let mut incomplete = context.clone();
            incomplete.extent_priorities.records.clear();
            assert_eq!(service.prepare_with_context(action.clone(), &AvailabilityMap::new(), &incomplete).err().unwrap().rule, Rule::Capability);
            assert_eq!(service.prepare_with_reservations(action, &context.sections, &context.columns).err().unwrap().rule, Rule::Capability);
            let staged = with_live_layout(&envelope, prepared.layout().clone(), &registry).unwrap();
            assert_eq!(staged.quarantine[0].instance_data, raw);
            assert_eq!(staged.section_snapshots.get(&section("q")).map(|s| s.data.clone()), original_snapshot);
            let authored = if initial == u16::MAX { 4 } else { 43 };
            assert_eq!(prepared.layout().section(&section("a")).unwrap().extent_priority, authored);
            if initial == u16::MAX {
                assert!(
                    staged
                        .extent_priority_overrides
                        .iter()
                        .any(|r| r.target == ExtentPriorityTarget::Memory(id("q")) && r.stored_priority == 42 && r.priority == 2)
                );
            } else {
                assert!(staged.extent_priority_overrides.is_empty());
            }
            let encoded = encode(&staged, &registry).unwrap();
            let loaded = decode(&encoded, &registry, &persistence_fallback()).unwrap();
            assert!(loaded.notice.is_none());
            let (grown, recovered, owner) = restored_priority_owner(loaded.envelope, &registry);
            let effective_q = if initial == u16::MAX { 2 } else { 42 };
            assert_eq!(owner.layout().containing(&id("q")).unwrap().extent_priority, effective_q);
            let mut environment = allocated_pair_environment(&owner, 400.0);
            environment.gap = 4.0;
            let allocated = allocate_layout(owner.layout(), &grown, &environment).unwrap();
            for (name, expected) in [("a", 152.0), ("b", 120.0), ("q", 120.0)] {
                assert!((allocated.sections.iter().find(|s| s.section == section(name)).unwrap().outer.size.height - expected).abs() < 0.002);
            }
            let saved = with_live_layout(&recovered, owner.layout().clone(), &grown).unwrap();
            assert_eq!(saved.retained_data[&id("q")]["instance"], raw);
            if snapshot {
                assert_eq!(saved.section_snapshots[&section("q")].data["extent_priority"], 42);
            }
            service.commit(prepared).unwrap();
            if initial == u16::MAX {
                assert_eq!(
                    service
                        .prepare_with_context(
                            transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 360.0) }]),
                            &AvailabilityMap::new(),
                            &context
                        )
                        .err()
                        .unwrap()
                        .rule,
                    Rule::Revision
                );
            }
        }
    }
}

#[test]
fn catalogue_only_recovery_priorities_reach_headless_single_and_atomic_batch_preparation() {
    use commands::LayoutCatalogue;
    for batch in [false, true] {
        let (registry, envelope) = priority_recovery_envelope(u16::MAX, false);
        let raw = envelope.quarantine[0].instance_data.clone();
        let mut bare = envelope.layout.clone();
        bare.extent_priority_context = ExtentPriorityContext::default();
        let service = LayoutService::new(registry.clone(), bare, HostCapabilities::default()).unwrap();
        let mut catalogue = commands::InMemoryCatalogue::default();
        catalogue.remember(&envelope, None, &registry).unwrap();
        let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
        let params = serde_json::json!({"target":{"type":"section","id":"a"},"size":{"width":280,"height":350}});
        if batch {
            backend
                .batch(&[
                    serde_json::json!({"command":"panel.resize","params":params}),
                    serde_json::json!({"command":"panel.resize","params":{"target":{"type":"section","id":"a"},"size":{"width":280,"height":360}}}),
                ])
                .unwrap();
        } else {
            backend.execute("panel.resize", &params).unwrap();
        }
        let stored = backend.catalogue.state().remembered(&backend.service.layout().workspace).unwrap();
        let remembered = stored.project_envelope(&registry, &backend.service.layout().workspace).unwrap();
        assert_eq!(remembered.quarantine[0].instance_data, raw);
        let (grown, _, owner) = restored_priority_owner(remembered, &registry);
        let mut environment = allocated_pair_environment(&owner, 400.0);
        environment.gap = 4.0;
        let allocated = allocate_layout(owner.layout(), &grown, &environment).unwrap();
        assert!((allocated.sections.iter().find(|s| s.section == section("a")).unwrap().outer.size.height - 152.0).abs() < 0.002);
        assert!((allocated.sections.iter().find(|s| s.section == section("q")).unwrap().outer.size.height - 120.0).abs() < 0.002);
    }
}

#[test]
fn incomplete_priority_proposals_and_hostile_shadow_metadata_cannot_publish_wrong_order() {
    let (registry, envelope) = priority_recovery_envelope(u16::MAX, false);
    let original = encode(&envelope, &registry).unwrap();
    let mut bare = envelope.layout.clone();
    bare.extent_priority_context = ExtentPriorityContext::default();
    let service = LayoutService::new(registry.clone(), bare, HostCapabilities::default()).unwrap();
    let bad = service.prepare(transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }])).unwrap();
    assert_eq!(with_live_layout(&envelope, bad.layout().clone(), &registry).unwrap_err().rule, Rule::Capability);
    assert_eq!(encode(&envelope, &registry).unwrap(), original);
    let full = preparation_context(&envelope).unwrap();
    let good = service
        .prepare_with_context(
            transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }]),
            &AvailabilityMap::new(),
            &full,
        )
        .unwrap();
    let staged = with_live_layout(&envelope, good.layout().clone(), &registry).unwrap();
    let mut bare = staged.layout.clone();
    bare.extent_priority_context = ExtentPriorityContext::default();
    let bare = LayoutService::new(registry.clone(), bare, HostCapabilities::default()).unwrap();
    let stale = bare
        .prepare_with_context(
            transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 360.0) }]),
            &AvailabilityMap::new(),
            &full,
        )
        .unwrap();
    assert_eq!(with_live_layout(&staged, stale.layout().clone(), &registry).unwrap_err().rule, Rule::Revision);
    let mut wrong = envelope.clone();
    wrong.extent_priority_overrides.push(ExtentPriorityRecord {
        target: ExtentPriorityTarget::Memory(id("q")),
        zone: Zone::Left,
        column: wrong.layout.columns.first().map(|c| c.id.clone()),
        stored_priority: 41,
        priority: 2,
    });
    assert_eq!(encode(&wrong, &registry).unwrap_err().rule, Rule::Schema);
    wrong.extent_priority_overrides[0].stored_priority = 42;
    wrong.extent_priority_overrides.push(wrong.extent_priority_overrides[0].clone());
    assert_eq!(encode(&wrong, &registry).unwrap_err().rule, Rule::DuplicateId);
    let mut too_many = ExtentPriorityContext { complete: true, records: Vec::new() };
    for n in 0..=MAX_EXTENT_PRIORITY_RECORDS {
        too_many.records.push(ExtentPriorityRecord {
            target: ExtentPriorityTarget::Memory(id(&format!("q-{n}"))),
            zone: Zone::Bottom,
            column: None,
            stored_priority: 42,
            priority: 2,
        });
    }
    assert_eq!(too_many.validate().unwrap_err().rule, Rule::CollectionLimit);
}

#[test]
fn preset_apply_and_reset_retain_opaque_effective_priorities_order_and_rejected_bytes() {
    use commands::LayoutCatalogue;
    use serde_json::json;
    for initial in [u16::MAX, 10] {
        for snapshot in [false, true] {
            for operation in ["layout.apply", "layout.reset"] {
                let (registry, original) = priority_recovery_envelope(initial, snapshot);
                let service = LayoutService::new(registry.clone(), original.layout.clone(), HostCapabilities::default()).unwrap();
                let prepared = service.prepare(transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }])).unwrap();
                let current = with_live_layout(&original, prepared.layout().clone(), &registry).unwrap();
                let raw_q = current.quarantine[0].instance_data.clone();
                let raw_snapshot = current.section_snapshots.get(&section("q")).map(|s| s.data.clone());
                let mut preset = LayoutEnvelope::new(current.layout.clone());
                preset.layout.extent_priority_context = ExtentPriorityContext::default();
                assert!(preset.quarantine.is_empty() && preset.section_snapshots.is_empty());
                let mut catalogue = commands::InMemoryCatalogue::default();
                catalogue.register_builtin("Without Q", &preset, &registry).unwrap();
                catalogue
                    .register_workspace(commands::WorkspaceDescriptor {
                        id: current.layout.workspace.clone(),
                        title_key: "ui.panel".into(),
                        default_preset: "Without Q".into(),
                    })
                    .unwrap();
                let rejected = [0, 255, 128, 17];
                catalogue.remember(&current, Some(&rejected), &registry).unwrap();
                let preset_bytes = catalogue.state().preset("Without Q").unwrap().bytes().to_vec();
                let service = LayoutService::new(registry.clone(), current.layout.clone(), HostCapabilities::default()).unwrap();
                let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
                backend.execute(operation, &if operation == "layout.apply" { json!({"name":"Without Q"}) } else { json!({}) }).unwrap();
                assert_eq!(backend.catalogue.state().preset("Without Q").unwrap().bytes(), preset_bytes);
                let stored = backend.catalogue.state().remembered(&current.layout.workspace).unwrap();
                assert_eq!(stored.rejected(), Some(rejected.as_slice()));
                let merged = stored.project_envelope(&registry, &current.layout.workspace).unwrap();
                assert_eq!(merged.quarantine[0].instance_data, raw_q);
                assert_eq!(merged.section_snapshots.get(&section("q")).map(|s| s.data.clone()), raw_snapshot);
                assert_eq!(merged.section_order, current.section_order);
                let effective_q = if initial == u16::MAX { 2 } else { 42 };
                let context = preparation_context(&merged).unwrap();
                let q = context.extent_priorities.records.iter().find(|r| r.target == ExtentPriorityTarget::Memory(id("q"))).unwrap();
                assert_eq!((q.stored_priority, q.priority), (42, effective_q));
                if initial == u16::MAX {
                    assert_eq!(merged.layout.section(&section("a")).unwrap().extent_priority, 4);
                    assert_eq!(merged.layout.section(&section("b")).unwrap().extent_priority, 1);
                    assert!(merged.extent_priority_overrides.windows(2).all(|pair| pair[0].target < pair[1].target));
                }
                let (grown, recovered, owner) = restored_priority_owner(merged, &registry);
                assert_eq!(owner.layout().containing(&id("q")).unwrap().extent_priority, effective_q);
                assert_eq!(owner.layout().sections.iter().map(|s| s.id.clone()).collect::<Vec<_>>(), vec![section("a"), section("b"), section("q")]);
                assert_eq!(recovered.retained_data[&id("q")]["instance"], raw_q);
                let mut environment = allocated_pair_environment(&owner, 400.0);
                environment.gap = 4.0;
                let geometry = allocate_layout(owner.layout(), &grown, &environment).unwrap();
                for (name, expected) in [("a", 152.0), ("b", 120.0), ("q", 120.0)] {
                    assert!((geometry.sections.iter().find(|s| s.section == section(name)).unwrap().outer.size.height - expected).abs() < 0.002);
                }
            }
        }
    }
}

#[test]
fn preset_merge_refuses_conflicting_effective_tiers_for_identical_raw_recovery_sources() {
    use commands::LayoutCatalogue;
    for snapshot in [false, true] {
        for incoming_wins in [false, true] {
            let (registry, original) = priority_recovery_envelope(u16::MAX, snapshot);
            let service = LayoutService::new(registry.clone(), original.layout.clone(), HostCapabilities::default()).unwrap();
            let prepared = service.prepare(transaction(vec![Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }])).unwrap();
            let mut current = with_live_layout(&original, prepared.layout().clone(), &registry).unwrap();
            let mut incoming = current.clone();
            let changed = if incoming_wins { &mut incoming } else { &mut current };
            for record in &mut changed.extent_priority_overrides {
                if matches!(&record.target, ExtentPriorityTarget::Memory(owner) if *owner == id("q"))
                    || matches!(&record.target, ExtentPriorityTarget::Section(owner) if *owner == section("q"))
                {
                    record.priority = 3;
                }
            }
            changed.layout.extent_priority_context = preparation_context(changed).unwrap().extent_priorities;
            assert_eq!(current.quarantine, incoming.quarantine);
            assert_eq!(current.section_snapshots, incoming.section_snapshots);
            let mut catalogue = commands::InMemoryCatalogue::default();
            catalogue.register_builtin("Same recovery", &current, &registry).unwrap();
            catalogue.register_builtin("Conflicting recovery", &incoming, &registry).unwrap();
            catalogue.remember(&current, None, &registry).unwrap();
            let service = LayoutService::new(registry.clone(), current.layout.clone(), HostCapabilities::default()).unwrap();
            let mut backend = commands::InMemoryPanelBackend::new(service, catalogue);
            backend.execute("layout.apply", &serde_json::json!({"name":"Same recovery"})).unwrap();
            let before = backend.service.inspect();
            let catalogue = backend.catalogue.state().export().unwrap();
            let failure = backend.execute("layout.apply", &serde_json::json!({"name":"Conflicting recovery"})).unwrap_err();
            assert_eq!(failure.rule, Rule::DuplicateId);
            assert_eq!(failure.detail, "retained effective priority collision");
            assert_eq!(backend.service.inspect(), before);
            assert_eq!(backend.catalogue.state().export().unwrap(), catalogue);
        }
    }
}

#[test]
fn staging_new_authored_sections_and_closed_memories_requires_complete_recovery_context() {
    for was_closed in [false, true] {
        for close_after in [false, true] {
            for exhausted in [false, true] {
                let (registry, mut envelope) = priority_recovery_envelope(10, false);
                envelope.layout.sections.retain(|s| s.id != section("a"));
                if was_closed {
                    envelope.layout.instances.iter_mut().find(|p| p.id == id("a")).unwrap().requested_open = false;
                } else {
                    envelope.layout.instances.retain(|p| p.id != id("a"));
                }
                if exhausted {
                    envelope.layout.sections.iter_mut().find(|s| s.id == section("b")).unwrap().extent_priority = u16::MAX;
                    envelope.layout.instances.iter_mut().find(|p| p.id == id("b")).unwrap().memory.extent_priority = u16::MAX;
                }
                envelope.section_snapshots.remove(&section("a"));
                envelope.section_order.retain(|id| *id != section("a"));
                envelope.layout.extent_priority_context = ExtentPriorityContext::default();
                let original = encode(&envelope, &registry).unwrap();
                let loaded = decode(&original, &registry, &persistence_fallback()).unwrap();
                assert!(loaded.notice.is_none());
                let envelope = loaded.envelope;
                let raw_q = envelope.quarantine[0].instance_data.clone();
                let mut bare = envelope.layout.clone();
                bare.extent_priority_context = ExtentPriorityContext::default();
                let service = LayoutService::new(registry.clone(), bare, HostCapabilities::default()).unwrap();
                let action = transaction(vec![
                    if was_closed {
                        Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("a"), destination: None }
                    } else {
                        fixture_open("fixture.log", "a", "a", Zone::Left, 0).unwrap()
                    },
                    Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) },
                ]);
                let close = |layout: &Layout| {
                    if close_after {
                        let mut closer = LayoutService::new(registry.clone(), layout.clone(), HostCapabilities::default()).unwrap();
                        closer.dispatch(transaction(vec![Mutation::Close { instance: id("a") }])).unwrap();
                        assert!(closer.layout().section(&section("a")).is_err());
                        closer.layout().clone()
                    } else {
                        layout.clone()
                    }
                };
                let before = service.inspect();
                let bad = close(service.prepare(action.clone()).unwrap().layout());
                assert_eq!(with_live_layout(&envelope, bad, &registry).unwrap_err().rule, Rule::Capability);
                assert_eq!(service.inspect(), before);
                assert_eq!(encode(&envelope, &registry).unwrap(), original);
                let context = preparation_context(&envelope).unwrap();
                let good = service.prepare_with_context(action, &AvailabilityMap::new(), &context).unwrap();
                let staged = with_live_layout(&envelope, close(good.layout()), &registry).unwrap();
                assert_eq!(staged.quarantine[0].instance_data, raw_q);
                let (grown, recovered, mut owner) = restored_priority_owner(staged, &registry);
                if close_after {
                    owner
                        .dispatch(transaction(vec![Mutation::Open {
                            kind: PanelKind::new("fixture.log").unwrap(),
                            instance: id("a"),
                            destination: None,
                        }]))
                        .unwrap();
                }
                assert!(owner.layout().containing(&id("a")).unwrap().extent_priority > owner.layout().containing(&id("q")).unwrap().extent_priority);
                let mut environment = allocated_pair_environment(&owner, 400.0);
                environment.gap = 4.0;
                let geometry = allocate_layout(owner.layout(), &grown, &environment).unwrap();
                for (name, expected) in [("a", 152.0), ("b", 120.0), ("q", 120.0)] {
                    assert!((geometry.sections.iter().find(|s| s.section == section(name)).unwrap().outer.size.height - expected).abs() < 0.002);
                }
                let saved = with_live_layout(&recovered, owner.layout().clone(), &grown).unwrap();
                assert_eq!(saved.retained_data[&id("q")]["instance"], raw_q);
            }
        }
    }
}

#[test]
fn inherited_closed_reopen_and_native_return_tiers_remain_stageable_without_authored_intent() {
    for native in [false, true] {
        let (registry, envelope) = priority_recovery_envelope(60, false);
        let caps = HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true };
        let mut service = LayoutService::new(registry.clone(), envelope.layout.clone(), caps.clone()).unwrap();
        if native {
            service
                .dispatch(transaction(vec![Mutation::Move {
                    instance: id("a"),
                    destination: Destination::NewSection {
                        section: section("native-a"),
                        index: 0,
                        size: size(280.0, 300.0),
                        location: SectionLocation::Window {
                            host: HostId::new("inherited-tier").unwrap(),
                            origin: Point { x: 0.0, y: 0.0 },
                            return_to: DockReturn {
                                zone: Zone::Left,
                                index: 0,
                                section: None,
                                column: None,
                                column_index: 0,
                                column_width: None,
                                size: size(280.0, 300.0),
                                collapsed: false,
                                join: None,
                                weight: 1.0,
                                preferred_extent: None,
                                extent_priority: 0,
                            },
                        },
                    },
                }]))
                .unwrap();
        } else {
            service.dispatch(transaction(vec![Mutation::Close { instance: id("a") }])).unwrap();
        }
        let saved = with_live_layout(&envelope, service.layout().clone(), &registry).unwrap();
        let mut bare = saved.layout.clone();
        bare.extent_priority_context = ExtentPriorityContext::default();
        let service = LayoutService::new(registry.clone(), bare, caps).unwrap();
        let mutation = if native {
            Mutation::NativeClose { host: HostId::new("inherited-tier").unwrap() }
        } else {
            Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("a"), destination: None }
        };
        let prepared = service.prepare(transaction(vec![mutation])).unwrap();
        let staged = with_live_layout(&saved, prepared.layout().clone(), &registry).unwrap();
        assert_eq!(staged.layout.containing(&id("a")).unwrap().extent_priority, 60);
        assert_eq!(staged.quarantine[0].instance_data, envelope.quarantine[0].instance_data);
        let (grown, _, owner) = restored_priority_owner(staged, &registry);
        let mut environment = allocated_pair_environment(&owner, 400.0);
        environment.gap = 4.0;
        let geometry = allocate_layout(owner.layout(), &grown, &environment).unwrap();
        for (panel, expected) in [("a", 152.0), ("b", 120.0), ("q", 120.0)] {
            let group = owner.layout().containing(&id(panel)).unwrap();
            assert!((geometry.sections.iter().find(|s| s.section == group.id).unwrap().outer.size.height - expected).abs() < 0.002);
        }
    }
}

#[test]
fn native_close_then_resize_cannot_inherit_a_reused_window_primary_priority() {
    for close_after in [false, true] {
        let (registry, mut envelope) = priority_recovery_envelope(18, false);
        let host = HostId::new("rank-reuse").unwrap();
        let original_a = envelope.layout.section(&section("a")).unwrap().clone();
        let native = SectionLocation::Window {
            host: host.clone(),
            origin: Point { x: 20.0, y: 30.0 },
            return_to: DockReturn {
                zone: Zone::Left,
                section: Some(section("a")),
                index: 0,
                join: None,
                column: original_a.column.clone(),
                column_index: 0,
                column_width: Some(280.0),
                size: size(280.0, 300.0),
                collapsed: false,
                weight: original_a.weight,
                preferred_extent: Some(300.0),
                extent_priority: 18,
            },
        };
        let group = envelope.layout.sections.iter_mut().find(|s| s.id == section("a")).unwrap();
        group.id = section("window-a");
        group.location = native.clone();
        group.column = None;
        group.size.height = 350.0;
        group.preferred_extent = Some(350.0);
        group.extent_priority = 19;
        let memory = &mut envelope.layout.instances.iter_mut().find(|p| p.id == id("a")).unwrap().memory;
        memory.section = section("window-a");
        memory.location = native;
        memory.column = None;
        memory.column_width = None;
        memory.column_index = 0;
        memory.size.height = 350.0;
        memory.preferred_extent = Some(350.0);
        memory.extent_priority = 19;
        for group in &mut envelope.section_order {
            if *group == section("a") {
                *group = section("window-a");
            }
        }
        envelope.layout.extent_priority_context = ExtentPriorityContext::default();
        let bytes = encode(&envelope, &registry).unwrap();
        let loaded = decode(&bytes, &registry, &persistence_fallback()).unwrap();
        assert!(loaded.notice.is_none());
        let envelope = loaded.envelope;
        let raw_q = envelope.quarantine[0].instance_data.clone();
        let caps = HostCapabilities { native_windows: true, max_native_hosts: 8, persistence: true };
        let mut bare = envelope.layout.clone();
        bare.extent_priority_context = ExtentPriorityContext::default();
        let service = LayoutService::new(registry.clone(), bare, caps.clone()).unwrap();
        let before = service.inspect();
        // Matching the actual return descriptor remains an inherited operation
        // even though its primary Window extent and tier differ.
        let returned = service.prepare(transaction(vec![Mutation::NativeClose { host: host.clone() }])).unwrap();
        let inherited = with_live_layout(&envelope, returned.layout().clone(), &registry).unwrap();
        let a = inherited.layout.section(&section("a")).unwrap();
        assert_eq!((a.preferred_extent, a.extent_priority, &a.column), (Some(300.0), 18, &original_a.column));
        assert_eq!(inherited.quarantine[0].instance_data, raw_q);

        let action = transaction(vec![Mutation::NativeClose { host }, Mutation::SetSize { section: section("a"), size: size(280.0, 350.0) }]);
        let close = |layout: &Layout| {
            if close_after {
                let mut closer = LayoutService::new(registry.clone(), layout.clone(), caps.clone()).unwrap();
                closer.dispatch(transaction(vec![Mutation::Close { instance: id("a") }])).unwrap();
                assert!(closer.layout().section(&section("a")).is_err());
                closer.layout().clone()
            } else {
                layout.clone()
            }
        };
        let bad = service.prepare(action.clone()).unwrap();
        let a = bad.layout().section(&section("a")).unwrap();
        assert_eq!((a.preferred_extent, a.extent_priority), (Some(350.0), 19), "bare redock removes the former primary tier before reusing 19");
        assert_eq!(with_live_layout(&envelope, close(bad.layout()), &registry).unwrap_err().rule, Rule::Capability);
        assert_eq!(service.inspect(), before);
        assert_eq!(encode(&envelope, &registry).unwrap(), bytes);

        let context = preparation_context(&envelope).unwrap();
        let good = service.prepare_with_context(action, &AvailabilityMap::new(), &context).unwrap();
        assert_eq!(good.layout().section(&section("a")).unwrap().extent_priority, 43);
        let staged = with_live_layout(&envelope, close(good.layout()), &registry).unwrap();
        assert_eq!(staged.quarantine[0].instance_data, raw_q);
        let (grown, recovered, mut owner) = restored_priority_owner(staged, &registry);
        if close_after {
            owner
                .dispatch(transaction(vec![Mutation::Open { kind: PanelKind::new("fixture.log").unwrap(), instance: id("a"), destination: None }]))
                .unwrap();
        }
        let mut environment = allocated_pair_environment(&owner, 400.0);
        environment.gap = 4.0;
        let geometry = allocate_layout(owner.layout(), &grown, &environment).unwrap();
        for (name, expected) in [("a", 152.0), ("b", 120.0), ("q", 120.0)] {
            assert!((geometry.sections.iter().find(|s| s.section == section(name)).unwrap().outer.size.height - expected).abs() < 0.002);
        }
        let saved = with_live_layout(&recovered, owner.layout().clone(), &grown).unwrap();
        assert_eq!(saved.retained_data[&id("q")]["instance"], raw_q);
    }
}
