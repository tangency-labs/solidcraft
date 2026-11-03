use solidcraft_engine::panels::*;

use super::*;

fn id(value: &str) -> InstanceId {
    InstanceId::new(value).unwrap()
}
fn section(value: &str) -> SectionId {
    SectionId::new(value).unwrap()
}
fn capacity() -> LogicalRect {
    LogicalRect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 1400.0, height: 1000.0 } }
}
fn editor_key(h: &PanelMountHarness) -> ControllerKey {
    ControllerKey { instance: id("editor"), owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()) }
}
fn open_editor(h: &mut PanelMountHarness) {
    h.dispatch(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap();
}
fn move_editor(zone: Zone) -> Mutation {
    Mutation::Move {
        instance: id("editor"),
        destination: Destination::NewSection {
            section: section("editor-moved"),
            index: 0,
            location: if zone == Zone::Floating {
                SectionLocation::Floating { origin: Point { x: 50.0, y: 50.0 } }
            } else {
                SectionLocation::Dock { zone }
            },
            size: Size { width: 300.0, height: 280.0 },
        },
    }
}

#[test]
fn mount_harness_uses_real_controller_slots_and_usable_presentation_context() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let resolved = h.mount(capacity()).unwrap();
    assert_eq!(resolved.panels.len(), 1);
    assert!(h.host.slot_calls >= 3);
    assert_eq!(h.host.slot_calls % 3, 0);
    let record = h.host.mounts.back().unwrap();
    assert_eq!(record.context.usable_body.size.height, 192.0);
    assert_eq!(record.context.flow, Flow::Tall);
    assert_eq!(record.presentation.actions.len(), 3);
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
    assert!(h.runtime.state(&editor_key(&h)).unwrap().presented);
}

#[test]
fn hiding_tabbing_collapsing_and_reparenting_retain_editor_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "retained draft".into()).unwrap();
    h.dispatch(vec![
        Mutation::Open {
            kind: PanelKind::new("fixture.log").unwrap(),
            instance: id("log"),
            destination: Some(Destination::JoinTab { section: section("editor-section"), index: 1 }),
        },
        Mutation::Activate { instance: id("log") },
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert!(!h.runtime.state(&key).unwrap().presented);
    let updates = h.probe.state(&key).unwrap().updates;
    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    h.tick().unwrap();
    h.dispatch(vec![move_editor(Zone::Floating)]).unwrap();
    h.tick().unwrap();
    let state = h.probe.state(&key).unwrap();
    assert!(state.transaction_live);
    assert_eq!(state.draft, "retained draft");
    assert_eq!((state.cancels, state.applies, state.finishes), (0, 0, 0));
    assert!(state.updates > updates);
    assert_eq!(h.commands.calls, 0);
    assert_eq!(state.reparents, 1);
}

#[test]
fn explicit_close_cancel_preflight_failure_preserves_whole_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let before = h.runtime.layout().inspect();
    let count = h.runtime.controller_count();
    let commits = h.native.commits;
    let result = h.dispatch(vec![
        Mutation::SetSize { section: section("editor-section"), size: Size { width: 400.0, height: 300.0 } },
        Mutation::Close { instance: id("editor") },
    ]);
    assert_eq!(result.unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.native.commits, commits);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    h.probe.edit(&key, |s| s.reject_cancel = false).unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    h.dispatch(vec![Mutation::Open { kind: PanelKind::new("fixture.editor").unwrap(), instance: id("editor"), destination: None }]).unwrap();
    assert!(h.probe.state(&key).unwrap().transaction_live);
}

#[test]
fn saved_open_layout_reopens_an_explicitly_closed_editor_before_mount() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "closed editor draft".into()).unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 1);
    h.apply_layout(saved.clone(), None).unwrap();
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().reopens, 1);
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&key).unwrap().presented);
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&key).unwrap().draft, "closed editor draft");
    // Committed Reopen clears the lifecycle flag; repeated snapshots cannot reset it.
    h.probe.edit(&key, |s| s.reject_reopen = true).unwrap();
    h.apply_layout(saved, None).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().reopens, 1);
}

#[test]
fn snapshot_closed_entries_hide_live_drafts_but_start_initially_dormant_editors_on_open() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    h.probe
        .edit(&key, |s| {
            s.draft = "hidden live draft".into();
            s.reject_reopen = true;
        })
        .unwrap();
    let mut hidden = saved.clone();
    hidden.sections.clear();
    hidden.instances.first_mut().unwrap().requested_open = false;
    h.apply_layout(hidden.clone(), None).unwrap();
    h.tick().unwrap();
    assert!(!h.runtime.state(&key).unwrap().requested_open);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    h.apply_layout(saved.clone(), None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.probe.state(&key).unwrap().reopens, 0);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    assert_eq!(h.probe.state(&key).unwrap().draft, "hidden live draft");

    let mut dormant = PanelMountHarness::new(false).unwrap();
    dormant.apply_layout(hidden, None).unwrap();
    dormant.tick().unwrap();
    let dormant_key = editor_key(&dormant);
    assert!(!dormant.probe.state(&dormant_key).unwrap().transaction_live);
    dormant.apply_layout(saved, None).unwrap();
    dormant.mount(capacity()).unwrap();
    assert!(dormant.probe.state(&dormant_key).unwrap().transaction_live);
    assert_eq!(dormant.probe.state(&dormant_key).unwrap().reopens, 1);
}

#[test]
fn snapshot_reopen_refusal_is_atomic_across_retained_documents_and_workspace_switches() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 1).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Bottom, 2).unwrap(),
    ])
    .unwrap();
    h.tick().unwrap();
    let first = editor_key(&h);
    let mut next = h.runtime.environment().clone();
    next.document = Some(DocumentId::new("second-document").unwrap());
    h.runtime.set_environment(next, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    let second = editor_key(&h);
    h.probe.edit(&first, |s| s.draft = "first canceled draft".into()).unwrap();
    h.probe.edit(&second, |s| s.draft = "second canceled draft".into()).unwrap();
    let mut saved = h.runtime.layout().layout().clone();
    saved.workspace = WorkspaceId::new("reopened-workspace").unwrap();
    h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap();
    h.mount(capacity()).unwrap();
    for key in [&first, &second] {
        assert!(!h.probe.state(key).unwrap().transaction_live);
        assert_eq!(h.probe.state(key).unwrap().cancels, 1);
    }
    let scope = h.local.scopes[&id("application")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("application"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let count = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let hosts = h.native.hosts.clone();
    h.probe.edit(&second, |s| s.reject_reopen = true).unwrap();
    assert_eq!(h.apply_layout(saved.clone(), None).unwrap_err().rule, Rule::Lifecycle);
    h.probe.edit(&second, |s| s.reject_reopen = false).unwrap();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(saved.clone(), None).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    assert_eq!(h.native.hosts, hosts);
    for key in [&first, &second] {
        assert_eq!(h.probe.state(key).unwrap().reopens, 0);
        assert!(!h.probe.state(key).unwrap().transaction_live);
    }
    h.native.fail_prepare = false;
    h.apply_layout(saved, None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), count + 1, "only the destination workspace log is new");
    for (key, draft) in [(&first, "first canceled draft"), (&second, "second canceled draft")] {
        let state = h.probe.state(key).unwrap();
        assert!(state.transaction_live);
        assert_eq!(state.reopens, 1);
        assert_eq!(state.cancels, 1);
        assert_eq!(state.draft, draft);
    }
}

#[test]
fn snapshot_reopen_targets_the_destination_workspace_owner() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![fixture_open("fixture.log", "log", "log-section", Zone::Left, 0).unwrap()]).unwrap();
    let first_layout = h.runtime.layout().layout().clone();
    let first = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(first_layout.workspace.clone()) };
    h.dispatch(vec![Mutation::Close { instance: id("log") }]).unwrap();
    let mut second_layout = first_layout.clone();
    second_layout.workspace = WorkspaceId::new("second-workspace").unwrap();
    h.apply_layout(second_layout.clone(), None).unwrap();
    let second = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(second_layout.workspace.clone()) };
    h.dispatch(vec![Mutation::Close { instance: id("log") }]).unwrap();
    h.apply_layout(first_layout, None).unwrap();
    assert_eq!(h.probe.state(&first).unwrap().reopens, 1);
    assert_eq!(h.probe.state(&second).unwrap().reopens, 0);
    h.apply_layout(second_layout, None).unwrap();
    assert_eq!(h.probe.state(&first).unwrap().reopens, 1);
    assert_eq!(h.probe.state(&second).unwrap().reopens, 1);
}

#[test]
fn contextual_unavailability_retains_dormant_editor_and_background_updates() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    let before = h.runtime.layout().layout().clone();
    h.models.editable = false;
    h.tick().unwrap();
    assert_eq!(h.runtime.layout().layout(), &before);
    assert!(matches!(h.runtime.state(&key).unwrap().available, Availability::Unavailable { .. }));
    assert!(!h.runtime.state(&key).unwrap().presented);
    assert_eq!(h.dispatch(vec![Mutation::Focus { instance: id("editor") }]).unwrap_err().rule, Rule::Availability);
    let updates = h.probe.state(&key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&key).unwrap().updates > updates);
    h.models.editable = true;
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&key).unwrap().presented);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
}

#[test]
fn unavailable_new_editor_does_not_commit_layout_or_controller() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.models.editable = false;
    let before = h.runtime.layout().inspect();
    assert_eq!(
        h.dispatch(vec![fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap_err().rule,
        Rule::Availability
    );
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), 0);
}

#[test]
fn document_switch_retains_independent_editors_and_does_not_duplicate_layout() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let first_key = editor_key(&h);
    h.probe.edit(&first_key, |s| s.draft = "document one draft".into()).unwrap();
    let original = h.runtime.environment().clone();
    let layout = h.runtime.layout().layout().clone();
    let mut next = original.clone();
    next.document = Some(DocumentId::new("another-document").unwrap());
    h.runtime.set_environment(next, &mut h.local).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    assert_eq!(h.runtime.layout().layout(), &layout);
    let second_key = editor_key(&h);
    h.probe.edit(&second_key, |s| s.draft = "document two draft".into()).unwrap();
    let updates = h.probe.state(&first_key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&first_key).unwrap().updates > updates);
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    assert_eq!(h.probe.state(&first_key).unwrap().draft, "document one draft");
    assert_eq!(h.probe.state(&second_key).unwrap().draft, "document two draft");
    assert_eq!(h.probe.state(&first_key).unwrap().cancels, 0);
}

fn document_editor_harness() -> PanelMountHarness {
    let mut h = PanelMountHarness::without_content(false).unwrap();
    for (kind, editor, ownership) in [
        ("fixture.application", false, Ownership::Application),
        ("fixture.log", true, Ownership::Document),
        ("fixture.pin", false, Ownership::Workspace),
    ] {
        h.runtime.register(PanelKind::new(kind).unwrap(), fixture_controller_binding(&h.probe, editor, ownership)).unwrap();
    }
    h
}

#[test]
fn all_open_documents_retain_four_editors_plus_application_and_workspace_services() {
    let mut h = document_editor_harness();
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 0).unwrap(),
        fixture_open("fixture.pin", "workspace", "workspace-section", Zone::Left, 1).unwrap(),
        fixture_open("fixture.log", "document-0", "document-section-0", Zone::Floating, 2).unwrap(),
        fixture_open("fixture.log", "document-1", "document-section-1", Zone::Floating, 3).unwrap(),
        fixture_open("fixture.log", "document-2", "document-section-2", Zone::Floating, 4).unwrap(),
        fixture_open("fixture.log", "document-3", "document-section-3", Zone::Floating, 5).unwrap(),
    ])
    .unwrap();
    let saved = h.runtime.layout().layout().clone();
    let original = h.runtime.environment().clone();
    let app_key = ControllerKey { instance: id("application"), owner: ContentOwner::Application };
    let workspace_key = ControllerKey { instance: id("workspace"), owner: ContentOwner::Workspace(saved.workspace.clone()) };
    let mut editor_keys = Vec::new();
    assert_eq!(MAX_RETAINED_DOCUMENT_OWNERS, crate::documents::MAX_OPEN);
    for document in 0..crate::documents::MAX_OPEN {
        let mut environment = original.clone();
        environment.document = Some(DocumentId::new(format!("retained-document-{document}")).unwrap());
        // The initial fixture owner counts toward the same 32-document budget.
        if document == 0 {
            environment = original.clone();
        }
        h.runtime.set_environment(environment, &mut h.local).unwrap();
        h.tick().unwrap();
        for panel in 0..4 {
            let key = ControllerKey {
                instance: id(&format!("document-{panel}")),
                owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()),
            };
            h.probe
                .edit(&key, |s| {
                    s.draft = format!("document {document}, panel {panel}");
                    s.reject_cancel = true;
                    s.reject_reopen = true;
                })
                .unwrap();
            editor_keys.push(key);
        }
    }
    assert_eq!(h.runtime.controller_count(), crate::documents::MAX_OPEN * 4 + 2);
    assert!(h.runtime.controller_count() > MAX_INSTANCES);
    assert_eq!(h.runtime.layout().layout(), &saved);
    h.apply_layout(Layout::empty(saved.workspace.clone()), None).unwrap();
    let keys: Vec<_> = editor_keys.iter().cloned().chain([app_key, workspace_key]).collect();
    let updates: Vec<_> = keys.iter().map(|key| h.probe.state(key).unwrap().updates).collect();
    for _ in 0..3 {
        h.tick().unwrap();
    }
    for (key, before) in keys.iter().zip(updates) {
        assert_eq!(h.probe.state(key).unwrap().updates, before + 3);
        assert!(!h.runtime.state(key).unwrap().presented);
    }
    h.apply_layout(saved.clone(), None).unwrap();
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.mount(capacity()).unwrap();
    for (index, key) in editor_keys.iter().enumerate() {
        let state = h.probe.state(key).unwrap();
        assert_eq!(state.draft, format!("document {}, panel {}", index / 4, index % 4));
        assert!(state.transaction_live);
        assert_eq!((state.cancels, state.reopens, state.destroys), (0, 0, 0));
    }
    assert_eq!(h.runtime.controller_count(), crate::documents::MAX_OPEN * 4 + 2);
    assert_eq!(h.commands.calls, 0);
}

#[test]
fn retained_document_owner_capacity_refusal_preserves_environment_layout_and_local_input() {
    let mut h = document_editor_harness();
    h.dispatch(vec![fixture_open("fixture.log", "editor", "editor-section", Zone::Left, 0).unwrap()]).unwrap();
    let original = h.runtime.environment().clone();
    for document in 1..MAX_RETAINED_DOCUMENT_OWNERS {
        let mut environment = original.clone();
        environment.document = Some(DocumentId::new(format!("document-{document}")).unwrap());
        h.runtime.set_environment(environment, &mut h.local).unwrap();
    }
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.draft = "at document capacity".into()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let count = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut excess = environment.clone();
    excess.document = Some(DocumentId::new("excess-document").unwrap());
    assert_eq!(h.runtime.set_environment(excess, &mut h.local).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), count);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.tick().unwrap();
    assert!(h.runtime.update_errors().is_empty());
    assert_eq!(h.probe.state(&key).unwrap().draft, "at document capacity");
    assert!(h.probe.state(&key).unwrap().transaction_live);
    // Returning to a retained open owner succeeds at capacity, without eviction.
    h.runtime.set_environment(original, &mut h.local).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_DOCUMENT_OWNERS);
}

#[test]
fn retained_workspace_owner_capacity_refusal_is_an_atomic_snapshot_failure() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    let saved = h.runtime.layout().layout().clone();
    // The application consumes one non-document owner; workspaces share the rest.
    for workspace in 1..MAX_RETAINED_OTHER_OWNERS - 1 {
        let mut layout = saved.clone();
        layout.workspace = WorkspaceId::new(format!("retained-workspace-{workspace}")).unwrap();
        h.apply_layout(layout, None).unwrap();
    }
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
    let scope = h.local.scopes[&id("application")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("application"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut excess = saved.clone();
    excess.workspace = WorkspaceId::new("excess-workspace").unwrap();
    assert_eq!(h.apply_layout(excess, None).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.apply_layout(saved, None).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_RETAINED_OTHER_OWNERS);
}

#[test]
fn retained_controller_capacity_per_owner_does_not_increase_the_layout_instance_limit() {
    let mut h = document_editor_harness();
    let mutations: Vec<_> = (0..MAX_INSTANCES)
        .map(|index| {
            let group = index / MAX_TABS;
            let instance = id(&format!("editor-{index}"));
            let section_id = section(&format!("editors-{group}"));
            if index % MAX_TABS == 0 {
                fixture_open("fixture.log", &instance.0, &section_id.0, Zone::Left, group).unwrap()
            } else {
                Mutation::Open {
                    kind: PanelKind::new("fixture.log").unwrap(),
                    instance,
                    destination: Some(Destination::JoinTab { section: section_id, index: index % MAX_TABS }),
                }
            }
        })
        .collect();
    for batch in mutations.chunks(MAX_MUTATIONS) {
        h.dispatch(batch.to_vec()).unwrap();
    }
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
    let first = ControllerKey { instance: id("editor-0"), owner: ContentOwner::Document(h.runtime.environment().document.clone().unwrap()) };
    h.probe.edit(&first, |s| s.draft = "full owner draft".into()).unwrap();
    let before = h.runtime.layout().inspect();
    let commits = h.native.commits;
    let releases = h.local.releases;
    // One visible layout instance still cannot introduce a 129th retained
    // identity/controller alongside this owner's hidden drafts. The engine's
    // identity cap and runtime's per-owner bound stay aligned.
    let mut source = fixture_service(false).unwrap();
    source.dispatch(transaction(vec![fixture_open("fixture.log", "excess-editor", "excess-section", Zone::Left, 0).unwrap()])).unwrap();
    assert_eq!(h.apply_layout(source.layout().clone(), None).unwrap_err().rule, Rule::CollectionLimit);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
    assert_eq!(h.native.commits, commits);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.probe.state(&first).unwrap().draft, "full owner draft");
    assert!(h.probe.state(&first).unwrap().transaction_live);
    assert_eq!(h.probe.state(&first).unwrap().destroys, 0);
    assert_eq!(
        h.dispatch(vec![fixture_open("fixture.log", "excess-editor", "excess-section", Zone::Left, 4).unwrap()]).unwrap_err().rule,
        Rule::CollectionLimit
    );
    h.apply_layout(before.layout, None).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), MAX_INSTANCES);
}

#[test]
fn multi_instance_controllers_have_independent_identity_and_service_lifetimes() {
    let mut h = PanelMountHarness::new(false).unwrap();
    h.dispatch(vec![
        fixture_open("fixture.log", "log-one", "logs-one", Zone::Left, 0).unwrap(),
        fixture_open("fixture.log", "log-two", "logs-two", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    let first = ControllerKey { instance: id("log-one"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    let second = ControllerKey { instance: id("log-two"), owner: first.owner.clone() };
    h.dispatch(vec![Mutation::Close { instance: id("log-one") }]).unwrap();
    h.tick().unwrap();
    assert!(!h.runtime.state(&first).unwrap().requested_open);
    assert!(h.runtime.state(&second).unwrap().requested_open);
    assert!(h.probe.state(&first).unwrap().updates > 0);
    h.dispatch(vec![Mutation::Destroy { instance: id("log-one") }]).unwrap();
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&first).unwrap().destroys, 1);
}

#[test]
fn native_preflight_failure_and_close_redock_preserve_controller_identity() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    let before = h.runtime.layout().inspect();
    let mutation = Mutation::Move {
        instance: id("editor"),
        destination: Destination::NewSection {
            section: section("native-editor"),
            index: 0,
            location: SectionLocation::Window {
                host: HostId::new("native-host").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn { zone: Zone::Left, index: 0, size: Size { width: 280.0, height: 240.0 }, collapsed: false, join: None },
            },
            size: Size { width: 400.0, height: 300.0 },
        },
    };
    h.native.fail_prepare = true;
    assert_eq!(h.dispatch(vec![mutation.clone()]).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.probe.state(&key).unwrap().reparents, 0);
    h.native.fail_prepare = false;
    h.dispatch(vec![mutation]).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    h.dispatch(vec![Mutation::NativeClose { host: HostId::new("native-host").unwrap() }]).unwrap();
    assert!(h.native.hosts.is_empty());
    assert_eq!(h.runtime.controller_count(), 1);
    let state = h.probe.state(&key).unwrap();
    assert!(state.transaction_live);
    assert_eq!(state.cancels, 0);
    assert_eq!(state.reparents, 2);
}

#[test]
fn content_apply_finish_and_cancel_are_separate_from_layout_actions() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    h.commands.fail = true;
    assert!(
        h.runtime
            .content_action(
                &id("editor"),
                &ControlId::new("apply").unwrap(),
                &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local }
            )
            .is_err()
    );
    assert_eq!(h.probe.state(&key).unwrap().applies, 0);
    h.commands.fail = false;
    h.runtime
        .content_action(
            &id("editor"),
            &ControlId::new("apply").unwrap(),
            &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
        )
        .unwrap();
    assert_eq!(h.commands.calls, 1);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    h.runtime
        .content_action(
            &id("editor"),
            &ControlId::new("finish").unwrap(),
            &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local },
        )
        .unwrap();
    assert_eq!(h.commands.calls, 2);
    assert!(!h.probe.state(&key).unwrap().transaction_live);
    assert!(h.runtime.layout().layout().instance(&id("editor")).unwrap().requested_open);
}

#[test]
fn local_keyboard_drops_overlays_clipboard_and_picking_reject_wrong_scope() {
    let mut local = FakeLocalServices::default();
    let scope = LocalScope {
        workspace: WorkspaceId::new("workspace").unwrap(),
        document: Some(DocumentId::new("document").unwrap()),
        viewport: ViewportId::new("viewport").unwrap(),
    };
    local.bind(id("one"), scope.clone()).unwrap();
    local.bind(id("two"), scope.clone()).unwrap();
    let keyboard = LocalRequest::KeyboardOwner { instance: id("one"), control: ControlId::new("editor").unwrap() };
    local.request(&scope, keyboard).unwrap();
    assert_eq!(
        local.request(&scope, LocalRequest::KeyboardOwner { instance: id("two"), control: ControlId::new("editor").unwrap() }).unwrap_err().rule,
        Rule::Lifecycle
    );
    let mut other = scope.clone();
    other.viewport = ViewportId::new("other").unwrap();
    let requests = [
        LocalRequest::FileDrop { instance: id("one"), name: "part.sc".into(), bytes: vec![0; 10] },
        LocalRequest::PointerDrop { instance: id("one"), at: Point { x: 1.0, y: 2.0 }, payload: "fixture".into() },
        LocalRequest::Overlay {
            instance: id("one"),
            region: InputRegion {
                control: ControlId::new("overlay").unwrap(),
                role: InputRole::Overlay,
                bounds: capacity(),
                keyboard_focusable: true,
                accessible_name: "Specimen".into(),
            },
        },
        LocalRequest::Clipboard { instance: id("one"), text: "fixture".into() },
        LocalRequest::Pick { instance: id("one"), at: Point { x: 5.0, y: 5.0 } },
    ];
    for request in requests {
        assert_eq!(local.request(&other, request.clone()).unwrap_err().rule, Rule::Placement);
        let mut wrong_document = scope.clone();
        wrong_document.document = Some(DocumentId::new("wrong-document").unwrap());
        assert_eq!(local.request(&wrong_document, request.clone()).unwrap_err().rule, Rule::Placement);
        local.request(&scope, request).unwrap();
    }
    local.release(&scope, &id("one"));
    assert!(local.keyboard.is_empty());
    assert!(local.requests.is_empty());
}

#[test]
fn reparent_releases_host_local_state_and_retains_transaction() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes.get(&id("editor")).unwrap().clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    h.dispatch(vec![move_editor(Zone::Right)]).unwrap();
    assert!(h.local.keyboard.is_empty());
    assert!(!h.local.scopes.contains_key(&id("editor")));
    assert!(h.probe.state(&editor_key(&h)).unwrap().transaction_live);
}

#[test]
fn reduced_motion_rtl_and_hidden_updates_are_independent_of_paint() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    let mut environment = h.runtime.environment().clone();
    environment.direction = Direction::Rtl;
    h.runtime.set_environment(environment, &mut h.local).unwrap();
    let resolved = h.mount(capacity()).unwrap();
    assert!(resolved.panels.first().unwrap().outer.origin.x > 0.0);
    let context = &h.host.mounts.back().unwrap().context;
    assert_eq!(context.direction, Direction::Rtl);
    assert_eq!(context.motion.duration_ms(250), 0);
    assert_eq!(Motion::Animated.duration_ms(600), 500);
    let renders = h.probe.state(&editor_key(&h)).unwrap().renders;
    h.dispatch(vec![Mutation::Collapse { section: section("editor-section"), collapsed: true }]).unwrap();
    for _ in 0..5 {
        h.tick().unwrap();
    }
    assert_eq!(h.probe.state(&editor_key(&h)).unwrap().renders, renders);
    assert!(h.probe.state(&editor_key(&h)).unwrap().updates >= 6);
}

#[test]
fn mount_rejects_duplicate_inactive_or_invalid_regions() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let mut resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    resolved.panels.push(resolved.panels.first().unwrap().clone());
    assert_eq!(
        h.runtime
            .mount(&resolved, &mut h.host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            .unwrap_err()
            .rule,
        Rule::ExactlyOnce
    );
    assert_eq!(h.host.slot_calls, 0);
    resolved.panels.pop();
    resolved.panels.first_mut().unwrap().body.size.height = f32::INFINITY;
    assert_eq!(
        h.runtime
            .mount(&resolved, &mut h.host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            .unwrap_err()
            .rule,
        Rule::Bounds
    );
}

#[test]
fn shutdown_is_atomic_and_explicit_owner_destruction_does_not_reset_layout() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    let layout = h.runtime.layout().layout().clone();
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    assert_eq!(h.runtime.shutdown(&mut h.local).unwrap_err().rule, Rule::Lifecycle);
    assert_eq!(h.runtime.controller_count(), 1);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 0);
    h.probe.edit(&key, |s| s.reject_cancel = false).unwrap();
    h.runtime.destroy_owner(&key.owner, &mut h.local).unwrap();
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.probe.state(&key).unwrap().destroys, 1);
    assert_eq!(h.runtime.layout().layout(), &layout);
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 0, "destroyed owners cannot silently return");
    h.runtime.revive_owner(&key.owner).unwrap();
    h.tick().unwrap();
    assert_eq!(h.runtime.controller_count(), 1);
}

#[test]
fn runtime_action_sink_and_native_service_scopes_use_the_shared_interfaces() {
    let mut h = PanelMountHarness::new(true).unwrap();
    {
        let mut services = ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local };
        let mut adapter = RuntimeActionSink { runtime: &mut h.runtime, native: &mut h.native, services: &mut services };
        let sink: &mut dyn ActionSink = &mut adapter;
        sink.dispatch(transaction(vec![fixture_open("fixture.log", "one", "first", Zone::Left, 0).unwrap()])).unwrap();
        assert_eq!(sink.inspect().layout.instances.len(), 1);
    }
    h.dispatch(vec![Mutation::Move {
        instance: id("one"),
        destination: Destination::NewSection {
            section: section("native"),
            index: 0,
            location: SectionLocation::Window {
                host: HostId::new("native-viewport").unwrap(),
                origin: Point { x: 0.0, y: 0.0 },
                return_to: DockReturn { zone: Zone::Left, index: 0, size: Size { width: 280.0, height: 240.0 }, collapsed: false, join: None },
            },
            size: Size { width: 280.0, height: 240.0 },
        },
    }])
    .unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.host.mounts.back().unwrap().context.viewport, ViewportId::new("native-viewport").unwrap());
    let scope = h.local.scopes.get(&id("one")).unwrap().clone();
    assert_eq!(scope.viewport, ViewportId::new("native-viewport").unwrap());
    let mut root = scope.clone();
    root.viewport = h.runtime.environment().viewport.clone();
    assert_eq!(h.local.request(&root, LocalRequest::Clipboard { instance: id("one"), text: "fixture".into() }).unwrap_err().rule, Rule::Placement);
}

#[test]
fn egui_host_uses_logical_bounds_at_two_hundred_percent_scale() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    h.dispatch(vec![Mutation::SetSize { section: section("editor-section"), size: Size { width: 160.0, height: 120.0 } }]).unwrap();
    let resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(2.0);
    let mut failure = None;
    let _output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
        crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
            let mut host = EguiPanelHost { ui };
            if let Err(reason) =
                h.runtime.mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
            {
                failure = Some(reason);
            }
        });
    });
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(resolved.panels.first().unwrap().outer.size, Size { width: 160.0, height: 120.0 });
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
}

struct AppFixtureServices {
    models: FakeModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
    refreshed: u64,
}
impl AppPanelServices for AppFixtureServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        self.refreshed += 1;
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

#[test]
fn solve_app_integration_is_opt_in_and_cannot_dirty_or_mutate_document() {
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    assert!(app.panel_runtime().is_none());
    let doc = app.session.doc.clone();
    let model = app.session.model.state();
    let revision = app.session.revision;
    let undo = app.session.undo.len();
    let dirty = app.session.is_dirty();
    let recomputed = app.session.model.recomputed;
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    app.enable_panel_runtime(
        h.runtime,
        Box::new(AppFixtureServices {
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            refreshed: 0,
        }),
    )
    .unwrap();
    super::app::tick_app(&mut app);
    assert_eq!(app.panel_runtime().unwrap().controller_count(), 2, "the synthetic and real document retain separate controller owners");
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
    assert_eq!(app.session.revision, revision);
    assert_eq!(app.session.undo.len(), undo);
    assert_eq!(app.session.is_dirty(), dirty);
    assert_eq!(app.session.model.recomputed, recomputed);
    app.disable_panel_runtime().unwrap();
    assert!(app.panel_runtime().is_none());
}

#[test]
fn saved_layout_hiding_and_workspace_switches_retain_all_content_owners() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![
        fixture_open("fixture.application", "application", "application-section", Zone::Right, 1).unwrap(),
        fixture_open("fixture.log", "log", "log-section", Zone::Bottom, 2).unwrap(),
    ])
    .unwrap();
    h.mount(capacity()).unwrap();
    let saved = h.runtime.layout().layout().clone();
    let key = editor_key(&h);
    let app_key = ControllerKey { instance: id("application"), owner: ContentOwner::Application };
    let log_key = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(saved.workspace.clone()) };
    for (key, draft) in [(&key, "active editor draft"), (&app_key, "application conversation"), (&log_key, "workspace conversation")] {
        h.probe.edit(key, |s| s.draft = draft.into()).unwrap();
    }
    h.probe
        .edit(&key, |s| {
            s.reject_cancel = true;
            s.reject_reopen = true;
        })
        .unwrap();
    let updates = h.probe.state(&key).unwrap().updates;
    let hidden = Layout::empty(saved.workspace.clone());
    h.apply_layout(hidden, None).unwrap();
    let resolved = h.mount(capacity()).unwrap();
    assert!(resolved.panels.is_empty());
    assert!(resolved.rails.is_empty());
    assert!(!h.runtime.state(&key).unwrap().presented);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert!(h.probe.state(&key).unwrap().updates > updates);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    let mut other = saved.clone();
    other.workspace = WorkspaceId::new("workspace-two").unwrap();
    h.apply_layout(other.clone(), None).unwrap();
    h.tick().unwrap();
    let other_log = ControllerKey { instance: id("log"), owner: ContentOwner::Workspace(other.workspace.clone()) };
    assert_eq!(h.runtime.controller_count(), 4);
    assert!(h.runtime.state(&other_log).is_some());
    assert!(!h.runtime.state(&log_key).unwrap().presented);
    let hidden_updates = h.probe.state(&log_key).unwrap().updates;
    h.tick().unwrap();
    assert!(h.probe.state(&log_key).unwrap().updates > hidden_updates);
    h.probe.edit(&other_log, |s| s.draft = "other workspace draft".into()).unwrap();
    h.apply_layout(saved.clone(), None).unwrap();
    h.mount(capacity()).unwrap();
    assert_eq!(h.runtime.layout().layout(), &saved);
    assert_eq!(h.runtime.environment().workspace, saved.workspace);
    assert_eq!(h.runtime.controller_count(), 4);
    assert!(h.runtime.state(&key).unwrap().presented);
    for (key, draft) in [
        (&key, "active editor draft"),
        (&app_key, "application conversation"),
        (&log_key, "workspace conversation"),
        (&other_log, "other workspace draft"),
    ] {
        assert_eq!(h.probe.state(key).unwrap().draft, draft);
    }
    assert_eq!((h.probe.state(&key).unwrap().cancels, h.commands.calls), (0, 0));
    assert_eq!(h.probe.state(&key).unwrap().reopens, 0);
}

#[test]
fn failed_layout_application_preserves_environment_controllers_native_and_local_state() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let before = h.runtime.layout().inspect();
    let environment = h.runtime.environment().clone();
    let controllers = h.runtime.controller_count();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    let mut other = before.layout.clone();
    other.workspace = WorkspaceId::new("other-workspace").unwrap();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(other.clone(), Some(before.revision)).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.runtime.environment(), &environment);
    assert_eq!(h.runtime.controller_count(), controllers);
    assert_eq!(h.local.keyboard, keyboard);
    assert_eq!(h.local.scopes, scopes);
    assert_eq!(h.local.releases, releases);
    assert_eq!(h.native.commits, commits);
    h.native.fail_prepare = false;
    assert_eq!(h.apply_layout(other.clone(), Some(before.revision + 1)).unwrap_err().rule, Rule::Revision);
    other.instances.first_mut().unwrap().kind = PanelKind::new("fixture.log").unwrap();
    assert_eq!(h.apply_layout(other, None).unwrap_err().rule, Rule::DuplicateId);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.local.keyboard, keyboard);
}

#[test]
fn snapshot_application_keeps_missing_bindings_and_unavailable_content_dormant() {
    let mut source = PanelMountHarness::new(false).unwrap();
    open_editor(&mut source);
    let saved = source.runtime.layout().layout().clone();
    let mut h = PanelMountHarness::without_content(false).unwrap();
    h.apply_layout(saved.clone(), None).unwrap();
    assert!(matches!(h.runtime.layout().availability(&id("editor")), Availability::Unavailable { .. }));
    assert_eq!(h.runtime.controller_count(), 0);
    assert_eq!(h.runtime.layout().layout(), &saved);
    assert_eq!(h.dispatch(vec![Mutation::Focus { instance: id("editor") }]).unwrap_err().rule, Rule::Availability);
    let mut contextual = PanelMountHarness::new(false).unwrap();
    contextual.models.editable = false;
    contextual.apply_layout(saved.clone(), None).unwrap();
    contextual.tick().unwrap();
    assert_eq!(contextual.runtime.layout().layout(), &saved);
    assert!(!contextual.runtime.state(&editor_key(&contextual)).unwrap().presented);
    contextual.models.editable = true;
    contextual.mount(capacity()).unwrap();
    assert!(contextual.runtime.state(&editor_key(&contextual)).unwrap().presented);
}

#[test]
fn explicit_lifecycle_intents_cannot_be_erased_by_a_batch() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.mount(capacity()).unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    let reopen = Mutation::Open { kind: PanelKind::new("fixture.editor").unwrap(), instance: id("editor"), destination: None };
    let before = h.runtime.layout().inspect();
    let keyboard = h.local.keyboard.clone();
    let scopes = h.local.scopes.clone();
    let releases = h.local.releases;
    let commits = h.native.commits;
    for batch in [
        vec![Mutation::Close { instance: id("editor") }, reopen.clone()],
        vec![Mutation::Close { instance: id("editor") }, move_editor(Zone::Right), reopen.clone()],
        vec![Mutation::Destroy { instance: id("editor") }, reopen],
    ] {
        assert_eq!(h.dispatch(batch).unwrap_err().rule, Rule::Lifecycle);
        assert_eq!(h.runtime.layout().inspect(), before);
        assert_eq!(h.local.keyboard, keyboard);
        assert_eq!(h.local.scopes, scopes);
        assert_eq!(h.local.releases, releases);
        assert_eq!(h.native.commits, commits);
        assert!(h.probe.state(&key).unwrap().transaction_live);
        assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
    }
    h.dispatch(vec![
        Mutation::SetSize { section: section("editor-section"), size: Size { width: 300.0, height: 260.0 } },
        fixture_open("fixture.log", "independent", "independent-section", Zone::Right, 1).unwrap(),
    ])
    .unwrap();
    assert_eq!(h.runtime.controller_count(), 2);
    // Noncanceling snapshot hiding is distinct from an explicit Close, including
    // when the saved entry is already marked closed.
    let mut hidden = h.runtime.layout().layout().clone();
    hidden.sections.retain(|s| s.id != section("editor-section"));
    hidden.instances.iter_mut().find(|p| p.id == id("editor")).unwrap().requested_open = false;
    h.apply_layout(hidden, None).unwrap();
    assert_eq!(h.dispatch(vec![Mutation::Close { instance: id("editor") }]).unwrap_err().rule, Rule::Lifecycle);
}

#[test]
fn service_errors_do_not_starve_later_hidden_controllers_or_leave_stale_input() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.dispatch(vec![fixture_open("fixture.log", "zz-log", "log-section", Zone::Right, 1).unwrap()]).unwrap();
    h.mount(capacity()).unwrap();
    let editor = editor_key(&h);
    let log = ControllerKey { instance: id("zz-log"), owner: ContentOwner::Workspace(h.runtime.environment().workspace.clone()) };
    let mut layout = h.runtime.layout().layout().clone();
    layout.sections.retain(|s| s.id != section("log-section"));
    layout.instances.retain(|p| p.id != id("zz-log"));
    h.apply_layout(layout, None).unwrap();
    h.mount(capacity()).unwrap();
    let scope = h.local.scopes[&id("editor")].clone();
    h.local.request(&scope, LocalRequest::KeyboardOwner { instance: id("editor"), control: ControlId::new("draft").unwrap() }).unwrap();
    h.probe.edit(&editor, |s| s.fail_update = true).unwrap();
    let updates = h.probe.state(&log).unwrap().updates;
    for _ in 0..4 {
        assert!(h.tick().is_err());
        assert_eq!(h.runtime.update_errors().len(), 1);
        assert_eq!(h.runtime.update_errors().first().unwrap().controller.as_ref(), Some(&editor));
    }
    assert_eq!(h.probe.state(&log).unwrap().updates, updates + 4);
    assert!(!h.runtime.state(&log).unwrap().presented);
    assert!(!h.runtime.state(&editor).unwrap().presented);
    assert!(h.local.keyboard.is_empty());
    assert!(matches!(h.runtime.layout().availability(&id("editor")), Availability::Unavailable { .. }));
    h.probe
        .edit(&editor, |s| {
            s.fail_update = false;
            s.invalid_availability = true;
        })
        .unwrap();
    assert!(h.tick().is_err());
    assert_eq!(h.runtime.update_errors().first().unwrap().error.rule, Rule::Availability);
    assert_eq!(h.probe.state(&log).unwrap().updates, updates + 5);
    h.probe.edit(&editor, |s| s.invalid_availability = false).unwrap();
    h.tick().unwrap();
    assert!(h.runtime.update_errors().is_empty());
    h.mount(capacity()).unwrap();
    assert!(h.runtime.state(&editor).unwrap().presented);
}

#[test]
fn snapshot_native_transitions_stage_effects_without_canceling_editors() {
    let mut h = PanelMountHarness::new(true).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let key = editor_key(&h);
    h.probe.edit(&key, |s| s.reject_cancel = true).unwrap();
    let mut native_layout = h.runtime.layout().layout().clone();
    native_layout.sections.first_mut().unwrap().location = SectionLocation::Window {
        host: HostId::new("snapshot-host").unwrap(),
        origin: Point { x: 0.0, y: 0.0 },
        return_to: DockReturn { zone: Zone::Left, index: 0, size: Size { width: 280.0, height: 240.0 }, collapsed: false, join: None },
    };
    let reply = h.apply_layout(native_layout.clone(), None).unwrap();
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::CreateNative { .. })));
    assert_eq!(h.native.hosts.len(), 1);
    let away = Layout::empty(WorkspaceId::new("away").unwrap());
    let before = h.runtime.layout().inspect();
    let hosts = h.native.hosts.clone();
    h.native.fail_prepare = true;
    assert_eq!(h.apply_layout(away.clone(), None).unwrap_err().rule, Rule::Capability);
    assert_eq!(h.runtime.layout().inspect(), before);
    assert_eq!(h.native.hosts, hosts);
    h.native.fail_prepare = false;
    let reply = h.apply_layout(away, None).unwrap();
    assert!(reply.effects.iter().any(|e| matches!(e, HostEffect::ReleaseNative { .. })));
    assert!(h.native.hosts.is_empty());
    h.apply_layout(native_layout, None).unwrap();
    assert_eq!(h.native.hosts.len(), 1);
    assert_eq!(h.runtime.controller_count(), 1);
    assert!(h.probe.state(&key).unwrap().transaction_live);
    assert_eq!(h.probe.state(&key).unwrap().cancels, 0);
}

#[test]
fn egui_reference_host_rejects_sparse_or_no_input_geometry_before_widget_work() {
    for mode in 0..3 {
        let mut h = PanelMountHarness::new(false).unwrap();
        open_editor(&mut h);
        h.tick().unwrap();
        let mut resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
        let geometry = resolved.panels.first_mut().unwrap();
        let island = LogicalRect { origin: geometry.outer.origin, size: Size { width: 20.0, height: 20.0 } };
        match mode {
            0 => geometry.input.clear(),
            1 => {
                geometry.painted = vec![island];
                geometry.input.clear();
            }
            _ => geometry.input.first_mut().unwrap().bounds = island,
        }
        resolved.validate().unwrap();
        let ctx = egui::Context::default();
        let mut failure = None;
        let _output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
            crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
                let before = ui.min_rect();
                let mut host = EguiPanelHost { ui };
                failure = h
                    .runtime
                    .mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
                    .err();
                assert_eq!(host.ui.min_rect(), before);
            });
        });
        assert_eq!(failure.unwrap().rule, Rule::Capability);
        assert_eq!(h.probe.state(&editor_key(&h)).unwrap().renders, 0);
    }
}

#[test]
fn normal_egui_mount_preserves_the_parent_clip() {
    let mut h = PanelMountHarness::new(false).unwrap();
    open_editor(&mut h);
    h.tick().unwrap();
    let resolved = resolve_geometry(h.runtime.layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let ctx = egui::Context::default();
    let parent = egui::Rect::from_min_max(egui::pos2(30.0, 40.0), egui::pos2(150.0, 160.0));
    let mut failure = None;
    let _output = ctx.run_ui(egui::RawInput::default(), |root_ui| {
        crate::ui_kit::Container::new(root_ui.ctx(), crate::ui_kit::Surface::Viewport).central().show(root_ui, |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(parent));
            let mut host = EguiPanelHost { ui };
            failure = h
                .runtime
                .mount(&resolved, &mut host, &mut ContentServices { models: &h.models, commands: &mut h.commands, local: &mut h.local })
                .err();
        });
    });
    assert!(failure.is_none(), "{failure:?}");
    let clip = h.probe.state(&editor_key(&h)).unwrap().last_clip.unwrap();
    assert!(parent.contains_rect(clip));
    assert!(h.probe.state(&editor_key(&h)).unwrap().renders >= 1);
}

#[derive(Default)]
struct ForwardingTrace {
    service_borrows: usize,
    refreshes: usize,
    content_calls: u64,
}
struct ForwardingAppServices {
    models: FakeModels,
    commands: FakeContentCommands,
    local: FakeLocalServices,
    trace: std::rc::Rc<std::cell::RefCell<ForwardingTrace>>,
}
impl AppPanelServices for ForwardingAppServices {
    fn refresh_read_only(&mut self, _session: &solidcraft_engine::Session) -> PanelResult<()> {
        let mut trace = self.trace.try_borrow_mut().map_err(|_| error(Rule::Lifecycle, "trace borrowed"))?;
        trace.refreshes += 1;
        trace.content_calls = self.commands.calls;
        Ok(())
    }
    fn services(&mut self) -> ContentServices<'_> {
        if let Ok(mut trace) = self.trace.try_borrow_mut() {
            trace.service_borrows += 1;
        }
        ContentServices { models: &self.models, commands: &mut self.commands, local: &mut self.local }
    }
}

#[test]
fn solve_app_forwards_dispatch_mount_content_switch_and_ticks_to_its_injected_provider() {
    let PanelMountHarness { runtime, probe, mut native, .. } = PanelMountHarness::new(false).unwrap();
    let trace = std::rc::Rc::new(std::cell::RefCell::new(ForwardingTrace::default()));
    let mut app = crate::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
    let revision = app.session.revision;
    let doc = app.session.doc.clone();
    let model = app.session.model.state();
    let undo = app.session.undo.len();
    let dirty = app.session.is_dirty();
    app.enable_panel_runtime(
        runtime,
        Box::new(ForwardingAppServices {
            models: FakeModels { editable: true },
            commands: FakeContentCommands::default(),
            local: FakeLocalServices::default(),
            trace: trace.clone(),
        }),
    )
    .unwrap();
    app.dispatch_panel_layout(
        transaction(vec![
            fixture_open("fixture.editor", "editor", "editor-section", Zone::Left, 0).unwrap(),
            fixture_open("fixture.log", "log", "log-section", Zone::Right, 1).unwrap(),
        ]),
        &mut native,
    )
    .unwrap();
    app.tick_panel_runtime().unwrap();
    let key = ControllerKey { instance: id("editor"), owner: ContentOwner::Document(DocumentId::new(app.session.document_id.clone()).unwrap()) };
    probe.edit(&key, |state| state.draft = "draft through app".into()).unwrap();
    let saved = app.panel_runtime().unwrap().layout().layout().clone();
    let resolved = resolve_geometry(app.panel_runtime().unwrap().layout(), capacity(), Direction::Ltr, 48.0).unwrap();
    let mut host = FakePanelHost::default();
    app.mount_panels(&resolved, &mut host).unwrap();
    assert!(!host.mounts.is_empty());
    app.panel_content_action(&id("editor"), &ControlId::new("apply").unwrap()).unwrap();
    assert_eq!(probe.state(&key).unwrap().applies, 1);
    let mut other = saved.clone();
    other.workspace = WorkspaceId::new("app-workspace-two").unwrap();
    app.dispatch_panel_layout(
        LayoutAction::Replace { expected_revision: Some(app.panel_runtime().unwrap().layout().inspect().revision), layout: other.clone() },
        &mut native,
    )
    .unwrap();
    app.tick_panel_runtime().unwrap();
    assert_eq!(app.panel_runtime().unwrap().controller_count(), 3);
    app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: Layout::empty(other.workspace) }, &mut native).unwrap();
    let updates = probe.state(&key).unwrap().updates;
    app.tick_panel_runtime().unwrap();
    assert!(probe.state(&key).unwrap().updates > updates);
    app.dispatch_panel_layout(LayoutAction::Replace { expected_revision: None, layout: saved.clone() }, &mut native).unwrap();
    app.tick_panel_runtime().unwrap();
    assert_eq!(probe.state(&key).unwrap().draft, "draft through app");
    assert!(probe.state(&key).unwrap().transaction_live);
    assert_eq!(probe.state(&key).unwrap().cancels, 0);
    assert_eq!(app.panel_runtime().unwrap().layout().layout(), &saved);
    let trace = trace.borrow();
    assert_eq!(trace.content_calls, 1);
    assert!(trace.service_borrows >= 10);
    assert!(trace.refreshes >= 4);
    assert_eq!(app.session.revision, revision);
    assert_eq!(app.session.undo.len(), undo);
    assert_eq!(app.session.is_dirty(), dirty);
    assert!(std::sync::Arc::ptr_eq(&doc, &app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&model, &app.session.model.state()));
}
