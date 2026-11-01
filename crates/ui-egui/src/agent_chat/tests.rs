use super::*;
use std::sync::mpsc::channel;

fn chat() -> AgentChat {
    let mut c = AgentChat::default();
    c.state = Some(State::Ready);
    c
}

#[cfg(not(target_arch = "wasm32"))]
fn app_with_discovered_login() -> SolveApp {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.detection = Some(discover::Detection {
        found: vec![discover::Found {
            id: "qwen",
            name: "Qwen Code",
            argv: vec!["qwen".into(), "--acp".into()],
            version: None,
            login: discover::Login::LooksLoggedIn,
            problem: None,
            install: "npm install -g @qwen-code/qwen-code",
        }],
        ..Default::default()
    });
    app.agent_chat.running_id = "qwen".into();
    app
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn agent_login_file_does_not_claim_a_verified_sign_in() {
    let app = app_with_discovered_login();
    assert_eq!(agents::current(&app).unwrap().login, "login_found");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn agent_login_follows_chat_authentication_results() {
    let mut app = app_with_discovered_login();
    app.preferences.agents.push(agents::AgentConfig { id: "other".into(), name: "Other agent".into(), ..Default::default() });
    app.agent_chat.on_event(Event::AuthRequired { methods: vec![] });
    assert_eq!(agents::current(&app).unwrap().login, "signed_out");
    assert_eq!(agents::entries(&app).iter().find(|e| e.id == "other").unwrap().login, "");
    app.agent_chat.settings.tests.insert("qwen".into(), settings::Test::Done(Err(backend::TestError::AuthRequired)));
    app.agent_chat.on_event(Event::Ready { agent: "Qwen Code".into(), images: false });
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");
    assert!(!app.agent_chat.settings.tests.contains_key("qwen"), "a newer chat clears the stale test error");
    app.agent_chat.on_event(Event::AuthRequired { methods: vec![] });
    assert_eq!(agents::current(&app).unwrap().login, "signed_out");
}

#[cfg(unix)]
#[test]
fn agent_login_test_results_override_discovery() {
    // A real local ACP subprocess: initialize succeeds, session/new rejects the expired login.
    let script = r#"
read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake","version":"1"}}}'
read -r session
printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"credentials expired"}}'
while IFS= read -r line; do :; done
"#;
    let argv = vec!["sh".into(), "-c".into(), script.into()];
    let result = acp_client::test_agent(&argv, &std::env::temp_dir());
    assert_eq!(result, Err(backend::TestError::AuthRequired), "keep the protocol error, not translated text");
    let mut app = app_with_discovered_login();
    let complete = |app: &mut SolveApp, result| {
        let (tx, rx) = channel();
        app.agent_chat.settings.tests.insert("qwen".into(), settings::Test::Running(rx));
        tx.send(result).unwrap();
        frame(app);
    };
    complete(&mut app, result);
    assert_eq!(agents::current(&app).unwrap().login, "signed_out");
    complete(&mut app, Err(backend::TestError::Other("connection failed".into())));
    assert_eq!(agents::current(&app).unwrap().login, "signed_out", "transport errors aren't login evidence");
    let success = script.replace(r#""error":{"code":-32000,"message":"credentials expired"}"#, r#""result":{"sessionId":"test"}"#);
    let result = acp_client::test_agent(&["sh".into(), "-c".into(), success], &std::env::temp_dir());
    assert!(result.is_ok(), "{result:?}");
    complete(&mut app, result);
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");
    complete(&mut app, Err(backend::TestError::Other("connection failed".into())));
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");
    app.agent_chat.on_event(Event::AuthRequired { methods: vec![] });
    frame(&mut app);
    assert_eq!(agents::current(&app).unwrap().login, "signed_out", "an old test cannot overwrite newer chat evidence");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn agent_login_refresh_returns_to_unverified_discovery() {
    let mut app = app_with_discovered_login();
    app.agent_chat.on_event(Event::AuthRequired { methods: vec![] });
    app.agent_chat.settings.tests.insert("qwen".into(), settings::Test::Done(Err(backend::TestError::AuthRequired)));
    let (tx, rx) = channel();
    app.agent_chat.detecting = Some(rx);
    tx.send(app.agent_chat.detection.clone().unwrap()).unwrap();
    frame(&mut app);
    assert_eq!(agents::current(&app).unwrap().login, "login_found");
    assert!(!app.agent_chat.settings.tests.contains_key("qwen"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_sign_in_proven_in_one_tab_holds_in_every_tab() {
    // #9293: discovery found no login file before the user signed in from the panel. The chat's
    // proof of the sign-in belongs to the window, so a new tab doesn't ask for it again.
    let mut app = app_with_discovered_login();
    if let Some(d) = app.agent_chat.detection.as_mut() {
        d.found[0].login = discover::Login::NotLoggedIn;
    }
    assert_eq!(assistant_mark(&app), AssistantMark::NeedsAuth);
    app.agent_chat.on_event(Event::Ready { agent: "Qwen Code".into(), images: false });
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");

    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert_eq!(app.agent_chat.state(), State::Off, "a new tab starts its own conversation");
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");
    assert_eq!(assistant_mark(&app), AssistantMark::Ready, "no sign-in mark in the new tab");

    // Evidence from the new tab is the newest, also back in the first one.
    app.agent_chat.running_id = "qwen".into();
    app.agent_chat.on_event(Event::AuthRequired { methods: vec![] });
    crate::documents::switch(&mut app, 0);
    assert_eq!(agents::current(&app).unwrap().login, "signed_out");
    app.agent_chat.on_event(Event::Ready { agent: "Qwen Code".into(), images: false });

    // A replacement design starts a fresh conversation, not a fresh sign-in.
    crate::documents::reset_chat(&mut app);
    assert_eq!(agents::current(&app).unwrap().login, "signed_in");
    crate::documents::close(&mut app, 0, true);
    assert_eq!(agents::current(&app).unwrap().login, "signed_in", "closing a tab keeps the window's evidence");
}

#[test]
fn streamed_text_joins_and_tool_updates_change_their_call() {
    let mut c = chat();
    for e in [
        Event::Text { thought: false, text: "Mak".into() },
        Event::Text { thought: false, text: "ing a box".into() },
        Event::Tool {
            id: "t".into(),
            title: Some("batch".into()),
            kind: Some("other".into()),
            status: Some(ToolStatus::Pending),
            input: None,
            output: None,
        },
        Event::Tool { id: "t".into(), title: None, kind: None, status: Some(ToolStatus::Completed), input: None, output: Some("ok".into()) },
    ] {
        c.on_event(e);
    }
    assert_eq!(c.items.len(), 2);
    assert!(matches!(&c.items[0], Item::Agent { text, .. } if text == "Making a box"));
    assert!(matches!(&c.items[1], Item::Tool { title, status: Some(ToolStatus::Completed), output: Some(o), .. } if title == "batch" && o == "ok"));
}

#[test]
fn a_sign_in_request_keeps_the_question_for_after_the_login() {
    let mut c = chat();
    c.last = Some(("make a cube".into(), false, None, vec![attach::Attachment::imported("motor.step".into())]));
    c.state = Some(State::Busy);
    assert!(c.on_event(Event::TurnEnded { reason: "auth".into() }).is_some());
    c.on_event(Event::AuthRequired { methods: vec![] });
    assert_eq!(c.state(), State::NeedsAuth);
    let (text, image, _, files) = c.queued.clone().expect("queued for after the login");
    assert_eq!((text.as_str(), image), ("make a cube", false));
    assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["motor.step"], "its files go again too");
}

#[test]
fn a_plan_update_replaces_the_turns_plan() {
    let mut c = chat();
    c.push(Item::User { text: "go".into(), image: false, pictures: Vec::new(), files: Vec::new(), context: Context::default() });
    c.on_event(Event::Plan(vec![("a".into(), "pending".into())]));
    c.on_event(Event::Text { thought: false, text: "working".into() });
    c.on_event(Event::Plan(vec![("a".into(), "completed".into())]));
    let plans: Vec<_> = c.items.iter().filter(|i| matches!(i, Item::Plan(_))).collect();
    assert_eq!(plans.len(), 1);
    assert!(matches!(plans[0], Item::Plan(p) if p[0].1 == "completed"));
}

#[test]
fn a_permission_is_answered_once() {
    let mut c = chat();
    let opts = vec![PermissionOption { id: "allow".into(), name: "Allow".into(), kind: "allow_once".into() }];
    c.push(Item::Permission { key: 5, title: "batch".into(), options: opts, answer: None });
    assert_eq!(c.waiting(), 1);
    c.answer(0, Some("allow"));
    c.answer(0, None);
    assert!(matches!(&c.items[0], Item::Permission { answer: Some(a), .. } if a == "Allow"));
    assert_eq!(c.waiting(), 0);
}

/// An app with the chat's endpoint wired to a channel the test feeds, and a message in progress.
fn app_in_a_turn() -> (SolveApp, Sender<ControlRequest>) {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    // The default mode: modelling runs, files ask.
    assert_eq!(app.preferences.ai_permissions, agents::Mode::Edits);
    let (tx, rx) = channel();
    app.agent_chat.tools_rx = Some(rx);
    app.agent_chat.state = Some(State::Busy);
    start_turn(&mut app, "two boxes".into(), false, None, Vec::new());
    (app, tx)
}

fn ask(tx: &Sender<ControlRequest>, method: &str, params: Value) -> Receiver<Value> {
    let (req, rx) = ControlRequest::new(method, params);
    tx.send(req).unwrap();
    rx
}

fn frame(app: &mut SolveApp) {
    let ctx = crate::fonts::test_context();
    poll(app, &ctx);
    app.drain_control(&ctx);
}

fn boxed(x: f64) -> Value {
    json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}})
}

/// Everything the agent changed in one message undoes as one step, and the conversation offers
/// that undo while nothing else has happened since.
#[test]
fn a_messages_changes_undo_as_one_step() {
    let (mut app, tx) = app_in_a_turn();
    let before = app.session.doc.clone();
    let depth = app.session.undo.len();
    let replies: Vec<_> = [0.0, 20.0, 40.0].into_iter().map(|x| ask(&tx, "engine.execute", boxed(x))).collect();
    frame(&mut app);
    for r in replies {
        assert_eq!(r.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    }
    assert_eq!(app.session.undo.len(), depth + 3);
    assert_eq!(app.agent_chat.turn.as_ref().map(|t| t.changes), Some(3));
    app.agent_chat.on_event(Event::TurnEnded { reason: String::new() });
    end_turn(&mut app);
    assert_eq!(app.session.undo.len(), depth + 1, "one step");
    assert!(app.session.undo.last().is_some_and(|s| s.label.contains("two boxes")));
    let n = app.agent_chat.items.iter().rposition(|i| matches!(i, Item::Changes { steps: 3, .. })).expect("the changes are listed");
    undo_changes(&mut app, n);
    assert!(Arc::ptr_eq(&app.session.doc, &before), "back to the design before the message");
    assert!(matches!(app.agent_chat.items[n], Item::Changes { undone: true, .. }));
}

/// Saving waits for Allow or Deny; Deny answers the agent without running it, Allow runs it.
#[test]
fn file_actions_wait_for_the_user() {
    let (mut app, tx) = app_in_a_turn();
    let dir = solidcraft_engine::io::vfs::temp_dir().join(format!("sc-agent-chat-approve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.solidcraft");
    let save = json!({"command": "file.save", "params": {"path": path.to_string_lossy()}, "allow_paths": [dir.to_string_lossy()]});
    let denied = ask(&tx, "engine.execute", save.clone());
    frame(&mut app);
    assert_eq!(app.agent_chat.waiting(), 1);
    assert!(denied.try_recv().is_err(), "nothing answered before the user decides");
    let id = app.agent_chat.held[0].id;
    approve(&mut app, id, false);
    let r = denied.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(r["error"].as_str().is_some_and(|e| e.contains("declined")), "{r}");
    assert!(!path.exists());
    let allowed = ask(&tx, "engine.execute", save);
    frame(&mut app);
    let id = app.agent_chat.held[0].id;
    approve(&mut app, id, true);
    frame(&mut app);
    let r = allowed.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert!(path.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Crash reports and app methods never run for the agent, requests outside a message don't
/// either, and a message stops taking changes at the limit.
#[test]
fn the_gate_refuses_and_limits() {
    let (mut app, tx) = app_in_a_turn();
    assert_eq!(app.preferences.agent_chat_turn_limit, 0, "no limit by default");
    app.preferences.agent_chat_turn_limit = 2;
    let crash = ask(&tx, "engine.execute", json!({"command": "diagnostics.crash_reports", "params": {}}));
    let quit = ask(&tx, "app.quit", json!({"force": true}));
    let ok: Vec<_> = (0..2).map(|i| ask(&tx, "engine.execute", boxed(f64::from(i) * 20.0))).collect();
    let over = ask(&tx, "engine.execute", boxed(80.0));
    let read = ask(&tx, "document.inspect", json!({}));
    frame(&mut app);
    let get = |r: Receiver<Value>| r.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert!(get(crash)["error"].as_str().is_some_and(|e| e.contains("crash reports")));
    assert!(get(quit)["error"].as_str().is_some_and(|e| e.contains("not available")));
    for r in ok {
        assert_eq!(get(r)["ok"], true);
    }
    assert!(get(over)["error"].as_str().is_some_and(|e| e.contains("limit of 2")));
    assert_eq!(get(read)["ok"], true, "reads go on past the limit");
    end_turn(&mut app);
    let late = ask(&tx, "engine.execute", boxed(100.0));
    frame(&mut app);
    assert!(get(late)["error"].as_str().is_some_and(|e| e.contains("no message in progress")));
}

#[test]
fn merging_finds_the_turns_start_when_the_history_was_trimmed() {
    // Distinct designs: what counts is which `Arc` a snapshot holds.
    let doc = |_: usize| Arc::new(solidcraft_engine::doc::Document::default());
    let snap = |d: &Arc<solidcraft_engine::doc::Document>| solidcraft_engine::Snapshot {
        label: String::new(),
        doc: d.clone(),
        active_sketch: None,
        pending_moves: Default::default(),
        visibility: Default::default(),
        section: None,
    };
    let docs: Vec<_> = (0..6).map(doc).collect();
    // The turn began at depth 3 with docs[3]; the history then lost its oldest step.
    let start = UndoMark { depth: 3, top: Some(Box::new(snap(&docs[2]))) };
    let mut undo: Vec<_> = docs[1..6].iter().map(snap).collect();
    assert_eq!(merge_undo(&mut undo, &docs[3], &start, "Agent"), 3);
    assert_eq!(undo.len(), 3);
    assert!(Arc::ptr_eq(&undo[2].doc, &docs[3]) && undo[2].label == "Agent");
    // The step on top at the start is gone too: the first step is found by its design.
    let mut undo: Vec<_> = docs[3..6].iter().map(snap).collect();
    assert_eq!(merge_undo(&mut undo, &docs[3], &start, "Agent"), 3);
    // A section step in the turn: the merged step brings back the section before it.
    let mut undo: Vec<_> = docs[1..6].iter().map(snap).collect();
    let before = solidcraft_engine::SectionState { hatch: true, ..Default::default() };
    undo[3].section = Some(before.clone());
    merge_undo(&mut undo, &docs[3], &start, "Agent");
    assert_eq!(undo[2].section, Some(before.clone()));
    // A start that is no longer in the history: nothing is merged.
    let mut undo: Vec<_> = docs[1..6].iter().map(snap).collect();
    let gone = UndoMark { depth: 3, top: Some(Box::new(snap(&doc(8)))) };
    assert_eq!(merge_undo(&mut undo, &doc(9), &gone, "Agent"), 0);
    assert_eq!(undo.len(), 5);
    // A turn that made no step leaves the user's own last step alone, though a section step
    // keeps the design it began with.
    let mut undo: Vec<_> = docs[1..4].iter().map(snap).collect();
    undo[2].section = Some(before.clone());
    undo[2].doc = docs[3].clone();
    let start = UndoMark::of(&undo);
    assert_eq!(merge_undo(&mut undo, &docs[3], &start, "Agent"), 0);
    assert_eq!((undo.len(), undo[2].label.as_str()), (3, ""));
    // A full history of steps that all kept one design (section steps): the turn's steps follow
    // the step on top at its start.
    let step = |i: usize| solidcraft_engine::Snapshot { label: i.to_string(), ..snap(&docs[3]) };
    let start = UndoMark::of(&[step(1), step(2), step(3)]);
    let mut undo = vec![step(3), step(4), step(5)];
    assert_eq!(merge_undo(&mut undo, &docs[3], &start, "Agent"), 2);
}

/// The user's own section step is theirs: a message that changed nothing does not take it over,
/// and the chat's Undo of an agent's changes stops once the user has sectioned since.
#[test]
fn section_steps_stay_the_users() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    app.run("inspect.section", json!({"plane": "XY", "at": 5})).unwrap();
    let (_tx, rx) = channel::<ControlRequest>();
    app.agent_chat.tools_rx = Some(rx);
    app.agent_chat.state = Some(State::Busy);
    start_turn(&mut app, "look".into(), false, None, Vec::new());
    end_turn(&mut app);
    assert_eq!(app.session.undo.len(), 2);
    assert_eq!(app.session.undo.last().map(|s| s.label.as_str()), Some("Section Analysis"));
    assert!(!app.agent_chat.items.iter().any(|i| matches!(i, Item::Changes { .. })));

    let (mut app, tx) = app_in_a_turn();
    let reply = ask(&tx, "engine.execute", boxed(0.0));
    frame(&mut app);
    assert_eq!(reply.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    end_turn(&mut app);
    let n = app.agent_chat.items.iter().rposition(|i| matches!(i, Item::Changes { .. })).expect("the changes are listed");
    app.run("inspect.section", json!({"plane": "XY", "at": 5})).unwrap();
    undo_changes(&mut app, n);
    assert_eq!(app.session.section.len(), 1, "the user's section stays");
    assert_eq!(app.session.doc.features.len(), 1, "and so does the agent's box");
    assert!(matches!(app.agent_chat.items[n], Item::Changes { undone: false, .. }));
    app.run("edit.undo", json!({})).unwrap();
    undo_changes(&mut app, n);
    assert!(app.session.doc.features.is_empty() && matches!(app.agent_chat.items[n], Item::Changes { undone: true, .. }));
}

/// People who don't want AI never see it: with "Show AI features" off there is no button, the
/// shortcut does nothing, no panel draws, nothing is looked for and nothing starts, even when
/// asked over the control channel. Turned on, the button is there; discovery waits for the panel.
#[test]
fn with_ai_features_off_nothing_of_the_chat_shows_or_runs() {
    use crate::scenario::{handle_at, rect_of};
    let mut h = crate::scenario::Harness::new();
    let shortcut = json!({"key": "A", "shift": true, "cmd": true});
    h.app.preferences.ai_features = false;
    h.frames(3);
    assert!(handle_at("titlebar:assistant").is_none(), "no Assistant button");
    h.call("ui.key", shortcut.clone());
    h.frames(3);
    h.call("ui.agent_chat", json!({"open": true, "send": "hello", "connect": true}));
    h.frames(3);
    assert!(!h.app.agent_chat.open && rect_of("agent_chat:panel").is_none(), "no panel");
    assert!(h.app.agent_chat.detection.is_none() && h.app.agent_chat.detecting.is_none(), "nothing looked for");
    assert!(h.app.agent_chat.backend.is_none() && h.app.agent_chat.tools_port.is_none(), "nothing started");
    // The View settings menu has no chat entry either.
    if let Some(view) = handle_at("nav:view") {
        h.call("ui.click", json!({"x": view.x, "y": view.y}));
        h.frames(3);
        assert!(handle_at("view:agent_chat").is_none(), "no menu entry");
        h.call("ui.click", json!({"x": view.x, "y": view.y}));
    }
    // On (the default): the button shows; discovery waits until the panel opens.
    h.app.preferences.ai_features = true;
    h.frames(3);
    assert!(handle_at("titlebar:assistant").is_some(), "the Assistant button");
    assert!(h.app.agent_chat.detection.is_none() && h.app.agent_chat.detecting.is_none(), "nothing looked for yet");
    h.call("ui.key", shortcut.clone());
    h.frames(3);
    assert!(h.app.agent_chat.open && rect_of("agent_chat:panel").is_some(), "Ctrl+Shift+A opens the panel");
    assert!(h.app.agent_chat.detection.is_some() || h.app.agent_chat.detecting.is_some(), "the panel looks for agents");
    assert!(h.app.agent_chat.backend.is_none(), "but starts none until a chat begins");
    // Without the toolbar button the shortcut still works.
    h.app.preferences.ai_toolbar_button = false;
    h.call("ui.key", shortcut);
    h.frames(3);
    assert!(handle_at("titlebar:assistant").is_none() && !h.app.agent_chat.open);
}

#[test]
fn ai_features_and_the_toolbar_button_are_on_by_default() {
    let p = crate::prefs::Prefs::default();
    assert!(p.ai_features && p.ai_toolbar_button);
}

/// Pop out puts the chat in its own window (a native window on the desktop, which can go to
/// another screen; embedded here, where the test context has one viewport), and Dock puts it back
/// in the side panel. The control channel does the same.
#[test]
fn the_chat_pops_out_into_its_own_window_and_docks_back() {
    use crate::scenario::{handle_at, rect_of};
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    assert!(rect_of("agent_chat:panel").is_some() && rect_of("agent_chat:window").is_none(), "docked by default");
    assert!(!h.app.preferences.agent_chat_popped);
    let pop = handle_at("agent_chat:pop_out").expect("a pop-out button in the header");
    h.call("ui.click", json!({"x": pop.x, "y": pop.y}));
    h.frames(3);
    assert!(h.app.preferences.agent_chat_popped);
    assert!(rect_of("agent_chat:panel").is_none(), "the side panel is gone");
    assert!(rect_of("agent_chat:window").is_some(), "the chat is in its own window");
    assert_eq!(status(&h.app)["popped"], json!(true));
    // A window has its own minimise; the strip and the side drag are the docked panel's.
    assert!(handle_at("agent_chat:minimize").is_none() && handle_at("agent_chat:grip").is_none());
    let dock = handle_at("agent_chat:dock").expect("a dock button in the popped-out header");
    h.call("ui.click", json!({"x": dock.x, "y": dock.y}));
    h.frames(3);
    assert!(!h.app.preferences.agent_chat_popped);
    assert!(rect_of("agent_chat:panel").is_some() && rect_of("agent_chat:window").is_none(), "back in the side panel");
    h.call("ui.agent_chat", json!({"popped": true}));
    h.frames(3);
    assert!(rect_of("agent_chat:window").is_some() && rect_of("agent_chat:panel").is_none());
    // Closing the chat keeps where it lives: it opens popped out again.
    h.call("ui.agent_chat", json!({"open": false}));
    h.frames(2);
    assert!(rect_of("agent_chat:window").is_none());
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    assert!(rect_of("agent_chat:window").is_some());
}

/// The saved place is given to the window once, when it opens; after that the system owns it.
/// Given every frame, it chased the system's own rounding: a window that shook a pixel up and
/// down (686, 687, 686…) on a Retina screen.
#[test]
fn the_window_place_is_given_once_when_it_opens() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.preferences.agent_chat_window = Some([1967.0, 160.0, 569.0, 686.0]);
    let first = panel::window_builder(&app);
    assert_eq!(first.position, Some(egui::pos2(1967.0, 160.0)));
    assert_eq!(first.inner_size, Some(egui::vec2(569.0, 686.0)));
    app.agent_chat.window_placed = true;
    app.preferences.agent_chat_window = Some([1967.0, 160.0, 569.0, 687.0]);
    let later = panel::window_builder(&app);
    assert_eq!((later.position, later.inner_size), (None, None), "nothing to move or resize once it is open");
    assert!(later.title.is_some());
    // Docked or closed, the next pop-out opens at the saved place again.
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true, "popped": true}));
    h.frames(2);
    assert!(h.app.agent_chat.window_placed);
    h.call("ui.agent_chat", json!({"popped": false}));
    h.frames(2);
    assert!(!h.app.agent_chat.window_placed);
    h.call("ui.agent_chat", json!({"popped": true}));
    h.frames(2);
    h.call("ui.agent_chat", json!({"open": false}));
    h.frames(2);
    assert!(!h.app.agent_chat.window_placed);
}

/// macOS shows the text cursor only in the active window: the chat's own window becomes active
/// when the pointer comes over its text box from the app's main window, and never takes the focus
/// from another app.
#[test]
fn the_chat_window_takes_the_focus_for_its_text_box_from_the_main_window_only() {
    assert!(panel::take_focus_for_text(true, true, Some(false), Some(true)));
    assert!(!panel::take_focus_for_text(true, true, Some(false), Some(false)), "another app is active");
    assert!(!panel::take_focus_for_text(true, true, Some(true), Some(false)), "already active");
    assert!(!panel::take_focus_for_text(true, false, Some(false), Some(true)), "not over the text box");
    assert!(!panel::take_focus_for_text(false, true, Some(false), Some(true)), "docked: the main window shows it");
    assert!(!panel::take_focus_for_text(true, true, None, Some(true)), "unknown: leave it");
}

/// Whether the chat is popped out, and its window's place and size, are preferences; ones saved
/// before they existed load docked, and an unusable saved place is ignored.
#[test]
fn the_popped_out_window_and_its_place_are_remembered() {
    use crate::prefs::Prefs;
    let mut p = Prefs::default();
    assert!(!p.agent_chat_popped && p.agent_chat_window.is_none());
    p.agent_chat_popped = true;
    p.agent_chat_window = Some([-1600.0, 80.0, 420.0, 700.0]);
    let back: Prefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert!(back.agent_chat_popped);
    assert_eq!(back.agent_chat_window, Some([-1600.0, 80.0, 420.0, 700.0]), "a place on a screen left of the main one is kept");
    let mut old = serde_json::to_value(Prefs::default()).unwrap();
    old.as_object_mut().unwrap().remove("agent_chat_popped");
    old.as_object_mut().unwrap().remove("agent_chat_window");
    let old: Prefs = serde_json::from_value(old).unwrap();
    assert!(!old.agent_chat_popped && old.agent_chat_window.is_none());
    assert_eq!(panel::window_place(Some([10.0, 20.0, 400.0, 600.0])), Some(([10.0, 20.0], [400.0, 600.0])));
    for bad in [[f32::NAN, 0.0, 400.0, 600.0], [0.0, 0.0, 10.0, 600.0], [0.0, 0.0, 400.0, f32::INFINITY], [1e9, 0.0, 400.0, 600.0]] {
        assert_eq!(panel::window_place(Some(bad)), None, "{bad:?}");
    }
}

/// "Ask every time": a modelling command waits for Allow; "Always allow" lets the same command
/// through from then on for this agent. "Allow everything": even a save runs without a question.
#[test]
fn permission_modes_decide_what_asks() {
    let (mut app, tx) = app_in_a_turn();
    app.agent_chat.running_id = "claude".into();
    app.preferences.ai_permissions = agents::Mode::Ask;
    let first = ask(&tx, "engine.execute", boxed(0.0));
    frame(&mut app);
    assert_eq!(app.agent_chat.waiting(), 1, "a modelling command asks");
    let id = app.agent_chat.held[0].id;
    approve_always(&mut app, id);
    frame(&mut app);
    assert_eq!(first.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    assert!(agents::always_allowed(&app.preferences, "claude", "solid.box"));
    let second = ask(&tx, "engine.execute", boxed(20.0));
    frame(&mut app);
    assert_eq!(app.agent_chat.waiting(), 0, "remembered");
    assert_eq!(second.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    // Another agent keeps asking.
    app.agent_chat.running_id = "codex".into();
    let _third = ask(&tx, "engine.execute", boxed(40.0));
    frame(&mut app);
    assert_eq!(app.agent_chat.waiting(), 1);
    decline_all(&mut app, "test");
    // Allow everything, set for this agent only.
    app.preferences.agent_permissions.insert("codex".into(), agents::Mode::All);
    let dir = solidcraft_engine::io::vfs::temp_dir().join(format!("sc-agent-chat-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b.solidcraft");
    let save = ask(
        &tx,
        "engine.execute",
        json!({"command": "file.save", "params": {"path": path.to_string_lossy()}, "allow_paths": [dir.to_string_lossy()]}),
    );
    frame(&mut app);
    frame(&mut app);
    assert_eq!(app.agent_chat.waiting(), 0);
    assert_eq!(save.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    // Crash reports stay off limits whatever the mode.
    let crash = ask(&tx, "engine.execute", json!({"command": "diagnostics.crash_reports"}));
    frame(&mut app);
    assert_eq!(crash.recv_timeout(std::time::Duration::from_secs(5)).unwrap()["ok"], false);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The agent's own questions: SolidCraft's tools are allowed (its gate checks them), an earlier
/// "Always allow" answers by itself, and both leave a note; anything else is asked.
#[test]
fn the_agents_own_questions_are_answered_by_the_mode_or_asked() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.running_id = "claude".into();
    let opts = || {
        vec![
            PermissionOption { id: "allow".into(), name: "Allow".into(), kind: "allow_once".into() },
            PermissionOption { id: "always".into(), name: "Always".into(), kind: "allow_always".into() },
            PermissionOption { id: "no".into(), name: "Reject".into(), kind: "reject_once".into() },
        ]
    };
    on_permission(&mut app, 1, "mcp__solidcraft__batch".into(), opts());
    assert!(matches!(app.agent_chat.items.last(), Some(Item::Notice { text, .. }) if text.contains("mcp__solidcraft__batch")));
    on_permission(&mut app, 2, "Run ls".into(), opts());
    assert!(matches!(app.agent_chat.items.last(), Some(Item::Permission { answer: None, .. })));
    let n = app.agent_chat.items.len() - 1;
    answer_permission(&mut app, n, Some("always"));
    assert!(agents::always_allowed(&app.preferences, "claude", "Run ls"));
    on_permission(&mut app, 3, "Run ls".into(), opts());
    assert!(matches!(app.agent_chat.items.last(), Some(Item::Notice { .. })), "remembered");
}

// ---- Messages typed while the agent works ----

use std::sync::Mutex;

/// A backend that records what the panel asks of it, and reports what the test pushes.
#[derive(Clone, Default)]
struct Script {
    calls: Arc<Mutex<Vec<String>>>,
    events: Arc<Mutex<Vec<Event>>>,
    /// Every prompt in full (its text and pictures).
    prompts: Arc<Mutex<Vec<Vec<PromptPart>>>>,
}

impl Script {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn prompts(&self) -> Vec<Vec<PromptPart>> {
        self.prompts.lock().unwrap().clone()
    }
    fn report(&self, e: Event) {
        self.events.lock().unwrap().push(e);
    }
}

impl ChatBackend for Script {
    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
    fn prompt(&mut self, parts: &[PromptPart]) {
        let text = parts.iter().find_map(|p| if let PromptPart::Text(t) = p { Some(t.clone()) } else { None }).unwrap_or_default();
        self.calls.lock().unwrap().push(format!("prompt: {text}"));
        self.prompts.lock().unwrap().push(parts.to_vec());
    }
    fn cancel(&mut self) {
        self.calls.lock().unwrap().push("cancel".into());
    }
    fn new_session(&mut self) {
        self.calls.lock().unwrap().push("new_session".into());
    }
    fn set_model(&mut self, model: &str) {
        self.calls.lock().unwrap().push(format!("set_model: {model}"));
    }
    fn answer_permission(&mut self, key: u64, option: Option<&str>) {
        self.calls.lock().unwrap().push(format!("permission: {key} {option:?}"));
    }
    fn set_config_option(&mut self, id: &str, value: &str) {
        self.calls.lock().unwrap().push(format!("set_config_option: {id}={value}"));
    }
    fn authenticate(&mut self, _: &str) {}
    fn shutdown(&mut self, _: bool) {}
}

/// An app whose agent is scripted and ready.
fn app_with_script() -> (SolveApp, Script, egui::Context) {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    let script = Script::default();
    app.agent_chat.backend = Some(Box::new(script.clone()));
    app.agent_chat.state = Some(State::Ready);
    (app, script, crate::fonts::test_context())
}

fn ended(script: &Script, reason: &str) {
    script.report(Event::TurnEnded { reason: reason.into() });
}

#[test]
fn document_tabs_keep_their_conversations_and_backend_context() {
    let (mut app, first, ctx) = app_with_script();
    app.agent_chat.open = true;
    app.agent_chat.control_starter = Some(Box::new(|_| Some(12345)));
    app.agent_chat.tools_port = Some(12345);
    app.agent_chat.settings.add = Some(settings::AddState::default());
    app.agent_chat.agent_id = Some("first-agent".into());
    app.agent_chat.input = "first draft".into();
    app.agent_chat.attach_view = true;
    app.agent_chat.add_file(attach::Attachment::imported("sample.step".into()));
    send(&mut app, &ctx, "first request".into());
    send(&mut app, &ctx, "first follow-up".into());

    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert!(app.agent_chat.items.is_empty() && app.agent_chat.input.is_empty());
    assert!(app.agent_chat.pending.is_empty() && app.agent_chat.attachments.is_empty());
    assert!(app.agent_chat.backend.is_none() && app.agent_chat.agent_id.is_none());
    assert!(app.agent_chat.tools_port.is_none() && app.agent_chat.tools_rx.is_none());
    assert!(!app.agent_chat.attach_view);
    assert!(app.agent_chat.open && app.agent_chat.control_starter.is_some());
    assert!(app.agent_chat.settings.add.is_some(), "settings belong to the window");
    assert!(first.calls().iter().any(|c| c == "cancel"));

    let second = Script::default();
    app.agent_chat.backend = Some(Box::new(second.clone()));
    app.agent_chat.state = Some(State::Ready);
    app.agent_chat.input = "second draft".into();
    send(&mut app, &ctx, "second request".into());
    ended(&second, "");
    poll(&mut app, &ctx);
    ended(&first, "cancelled");
    crate::documents::switch(&mut app, 0);
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.input, "first draft");
    assert_eq!(app.agent_chat.agent_id.as_deref(), Some("first-agent"));
    assert_eq!(app.agent_chat.tools_port, Some(12345), "the endpoint belongs to its conversation");
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::User { files, .. } if files.iter().any(|name| name == "sample.step"))));
    assert_eq!(texts(&app), ["first follow-up"]);
    assert!(app.agent_chat.stopped, "returning must not send the stopped queue");
    assert_eq!(first.prompts().len(), 1);
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::User { text, .. } if text == "first request")));
    assert!(!app.agent_chat.items.iter().any(|i| matches!(i, Item::User { text, .. } if text == "second request")));
    assert!(app.agent_chat.attach_view);

    // Closing an earlier tab shifts indices, but the surviving backend and draft stay together.
    crate::documents::close(&mut app, 0, true);
    assert_eq!(app.docs.active, 0);
    assert_eq!(app.agent_chat.input, "second draft");
    assert!(!app.agent_chat.stopped, "leaving an idle conversation does not stop a turn");
    send(&mut app, &ctx, "second continuation".into());
    assert_eq!(second.prompts().len(), 2, "the same backend retains this tab's context");
    crate::documents::close(&mut app, 0, true);
    assert!(app.agent_chat.items.is_empty() && app.agent_chat.backend.is_none());
    assert!(app.agent_chat.open && app.agent_chat.control_starter.is_some());
}

#[test]
fn leaving_during_startup_or_sign_in_keeps_the_unsent_message_for_send() {
    for state in [State::Starting, State::NeedsAuth] {
        let (mut app, script, ctx) = app_with_script();
        app.agent_chat.state = Some(state);
        app.agent_chat.add_file(attach::Attachment::imported("sample.step".into()));
        send(&mut app, &ctx, "waiting request".into());
        assert!(script.prompts().is_empty());
        assert!(app.agent_chat.queued.is_some());
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        // Both events may have been buffered while the original tab was hidden.
        script.report(Event::AuthRequired { methods: Vec::new() });
        script.report(Event::Ready { agent: "scripted".into(), images: false });
        crate::documents::switch(&mut app, 0);
        poll(&mut app, &ctx);
        assert!(script.prompts().is_empty(), "returning must not send a startup or sign-in retry");
        assert!(app.agent_chat.queued.is_none());
        assert_eq!(texts(&app), ["waiting request"]);
        assert_eq!(app.agent_chat.pending[0].files[0].name, "sample.step");
        send_now(&mut app, &ctx);
        assert_eq!(script.prompts().len(), 1, "an explicit Send resumes the message");
        assert_eq!(app.agent_chat.items.iter().filter(|i| matches!(i, Item::User { text, .. } if text == "waiting request")).count(), 1);
    }
}

#[test]
fn a_late_sign_in_retry_or_permission_cannot_restart_a_departed_turn() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "first request".into());
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    script.report(Event::Permission {
        key: 7,
        title: "mcp__solidcraft__batch".into(),
        options: vec![PermissionOption { id: "allow".into(), name: "Allow".into(), kind: "allow_once".into() }],
    });
    ended(&script, "auth");
    script.report(Event::AuthRequired { methods: Vec::new() });
    script.report(Event::Ready { agent: "scripted".into(), images: false });
    crate::documents::switch(&mut app, 0);
    poll(&mut app, &ctx);
    assert!(script.calls().iter().any(|c| c == "permission: 7 None"), "late permission is cancelled even for our own tools");
    assert_eq!(app.agent_chat.waiting(), 0);
    assert_eq!(script.prompts().len(), 1, "a cancelled turn must not retry after sign-in");
    assert!(app.agent_chat.turn.is_none() && app.agent_chat.queued.is_none());
}

/// Exercise the chat endpoint's gate and the real dispatch paths, including the animated
/// pointer and asynchronous file loads. The scripted backend never needs a model account.
fn chat_tool(app: &mut SolveApp, tx: &Sender<ControlRequest>, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let reply = ask(tx, method, params);
    pump_tools(app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        app.drain_control(ctx);
        app.now += 1.0;
        crate::agent_cursor::step(app, ctx);
        crate::file_load::tick(app, ctx);
        app.poll_control_jobs(ctx);
        match reply.try_recv() {
            Ok(response) => return response,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => panic!("the chat's tool reply was dropped"),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        assert!(std::time::Instant::now() < deadline, "the chat's tool did not finish");
        std::thread::yield_now();
    }
}

#[test]
fn an_agents_new_design_keeps_its_conversation_and_continues() {
    // #7526: the old reset disconnected the backend inside its own file.new request.
    for (method, params, animate) in [
        ("engine.execute", json!({"command": "file.new"}), false),
        ("engine.execute", json!({"command": "FILE.NEW"}), false),
        ("engine.execute", json!({"command": "file.new"}), true),
        ("engine.batch", json!({"commands": [{"command": "file.new"}, boxed(0.0)]}), false),
        ("engine.batch", json!({"commands": [{"command": "file.new"}, boxed(0.0)]}), true),
    ] {
        let (mut app, script, ctx) = app_with_script();
        let (tx, rx) = channel();
        app.agent_chat.tools_rx = Some(rx);
        app.preferences.ai_permissions = agents::Mode::All;
        app.run("solid.box", boxed(0.0)["params"].clone()).unwrap();
        send(&mut app, &ctx, "create another design".into());
        let active = app.agent_chat.turn.as_ref().unwrap().active.clone();
        app.ui.agent_cursor.show = animate;
        crate::scenario::publish_handle("toolbar:file.new", egui::pos2(80.0, 100.0));
        crate::scenario::publish_handle("toolbar:solid.box", egui::pos2(100.0, 100.0));
        let response = chat_tool(&mut app, &tx, &ctx, method, params);
        assert_eq!(response["ok"], true, "{response}");
        assert!(app.agent_chat.backend.is_some(), "the agent must receive its own replacement's reply");
        assert!(active.load(std::sync::atomic::Ordering::Relaxed));
        assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::User { text, .. } if text == "create another design")));
        assert!(!script.calls().iter().any(|call| matches!(call.as_str(), "cancel" | "new_session")));
        let response = chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(30.0));
        assert_eq!(response["ok"], true, "the same agent continues: {response}");
        assert_eq!(app.agent_chat.turn.as_ref().unwrap().changes, 2, "a replacement does not reset the turn's change budget");
        let count = if method == "engine.batch" { 2 } else { 1 };
        assert_eq!(app.session.doc.features.len(), count);
        ended(&script, "");
        poll(&mut app, &ctx);
        assert_eq!(app.session.undo.len(), 1, "only the new design's edits form this turn's undo step");
        app.run("edit.undo", json!({})).unwrap();
        assert!(app.session.doc.features.is_empty(), "undo must not resurrect the previous design");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_agents_opened_design_keeps_its_conversation_and_undo_ownership() {
    for blank in [true, false] {
        let dir = std::env::temp_dir().join(format!("solidcraft-chat-open-{}-{blank}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("next.solidcraft");
        let mut next = solidcraft_engine::Session::default();
        next.execute("solid.box", &boxed(100.0)["params"]).unwrap();
        next.execute("file.save_as", &json!({"path": path})).unwrap();
        let (mut app, script, ctx) = app_with_script();
        let (tx, rx) = channel();
        app.agent_chat.tools_rx = Some(rx);
        app.preferences.ai_permissions = agents::Mode::All;
        if !blank {
            app.run("solid.box", boxed(0.0)["params"].clone()).unwrap();
            app.run("file.save_as", json!({"path": dir.join("first.solidcraft")})).unwrap();
        }
        send(&mut app, &ctx, "open another design and add a box".into());
        let active = app.agent_chat.turn.as_ref().unwrap().active.clone();
        if !blank {
            assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(20.0))["ok"], true);
            assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(40.0))["ok"], true);
        }
        let response =
            chat_tool(&mut app, &tx, &ctx, "engine.execute", json!({"command": "doc.open", "params": {"path": path}, "allow_paths": [dir]}));
        assert_eq!(response["ok"], true, "{response}");
        assert!(app.agent_chat.backend.is_some(), "opening the file must carry the initiating chat");
        assert!(active.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(app.docs.count(), if blank { 1 } else { 2 });
        if !blank {
            let response = chat_tool(
                &mut app,
                &tx,
                &ctx,
                "engine.execute",
                json!({
                    "command": "doc.open", "params": {"path": dir.join("first.solidcraft")}, "allow_paths": [dir]
                }),
            );
            assert_eq!(response["ok"], false, "another tab must keep its own conversation: {response}");
            assert!(response["error"].as_str().unwrap().contains("already open in another tab"));
            assert_eq!(app.docs.active, 1);
        }
        assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(130.0))["ok"], true);
        assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(150.0))["ok"], true);
        assert_eq!(app.session.doc.features.len(), 3);
        ended(&script, "");
        poll(&mut app, &ctx);
        assert!(!script.calls().iter().any(|call| matches!(call.as_str(), "cancel" | "new_session")));
        assert_eq!(app.session.undo.len(), 1);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.doc.features.len(), 1, "the opened design's existing body survives undo");
        if !blank {
            crate::documents::switch(&mut app, 0);
            assert_eq!(app.session.doc.features.len(), 3);
            assert_eq!(app.session.undo.len(), 2, "the first design keeps its own grouped agent edits");
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(app.session.doc.features.len(), 1);
            assert!(app.agent_chat.backend.is_none(), "the endpoint moved with the conversation");
            crate::documents::switch(&mut app, 1);
            assert!(app.agent_chat.backend.is_some());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn an_agents_failed_new_design_batch_restores_its_undo_boundary() {
    let (mut app, script, ctx) = app_with_script();
    let (tx, rx) = channel();
    app.agent_chat.tools_rx = Some(rx);
    app.preferences.ai_permissions = agents::Mode::All;
    app.run("solid.box", boxed(0.0)["params"].clone()).unwrap();
    send(&mut app, &ctx, "try another design".into());
    assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(20.0))["ok"], true);
    let before = app.session.doc.clone();
    let response = chat_tool(
        &mut app,
        &tx,
        &ctx,
        "engine.batch",
        json!({"commands": [
            {"command": "file.new"},
            {"command": "sketch.line", "params": {"points": [[0, 0], [10, 10]]}}
        ]}),
    );
    assert_eq!(response["result"]["completed"], 1, "the new design ran before the failure: {response}");
    assert_eq!(response["result"]["rolled_back"], true);
    assert!(Arc::ptr_eq(&app.session.doc, &before));
    assert!(app.agent_chat.backend.is_some());
    assert_eq!(chat_tool(&mut app, &tx, &ctx, "engine.execute", boxed(40.0))["ok"], true);
    ended(&script, "");
    poll(&mut app, &ctx);
    assert_eq!(app.session.undo.len(), 2, "edits before and after the failed batch share one turn");
    app.run("edit.undo", json!({})).unwrap();
    assert_eq!(app.session.doc.features.len(), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_agents_cancelled_open_cannot_replace_the_design_later() {
    let dir = std::env::temp_dir().join(format!("solidcraft-chat-cancel-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("next.solidcraft");
    solidcraft_engine::Session::default().execute("file.save_as", &json!({"path": path})).unwrap();
    let (mut app, script, ctx) = app_with_script();
    let (tx, rx) = channel();
    app.agent_chat.tools_rx = Some(rx);
    app.preferences.ai_permissions = agents::Mode::All;
    app.run("solid.box", boxed(0.0)["params"].clone()).unwrap();
    let original = app.session.doc.clone();
    send(&mut app, &ctx, "open another design".into());
    let (started_tx, started_rx) = channel();
    let (release_tx, release_rx) = channel();
    app.file_load.worker_gate = Some((started_tx, release_rx));
    let reply = ask(&tx, "engine.execute", json!({"command": "doc.open", "params": {"path": path}, "allow_paths": [dir]}));
    pump_tools(&mut app);
    app.drain_control(&ctx);
    started_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    cancel(&mut app);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.file_load.idle() && std::time::Instant::now() < deadline {
        crate::file_load::tick(&mut app, &ctx);
        std::thread::yield_now();
    }
    assert!(app.file_load.idle());
    assert_eq!(reply.try_recv().unwrap()["ok"], false, "a stopped turn cannot finish its pending open");
    assert!(Arc::ptr_eq(&app.session.doc, &original));
    assert_eq!(app.docs.count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn programmatic_replacement_clears_the_conversation_and_invalidates_its_requests() {
    for command in ["file.new", "FILE.NEW"] {
        let (mut app, script, ctx) = app_with_script();
        let (tx, rx) = channel();
        app.agent_chat.tools_rx = Some(rx);
        send(&mut app, &ctx, "first request".into());
        let queued = ask(&tx, "engine.execute", boxed(0.0));
        pump_tools(&mut app);
        app.run(command, json!({})).unwrap();
        app.drain_control(&ctx);
        assert_eq!(queued.try_recv().unwrap()["ok"], false);
        assert!(app.agent_chat.items.is_empty() && app.agent_chat.backend.is_none());
        assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
        assert!(script.calls().iter().any(|c| c == "cancel"));
    }
}

#[test]
fn a_script_resets_chat_only_after_a_replacement_succeeds() {
    for (steps, replaced) in [
        (json!([{"command": "missing.command"}, {"command": "file.new"}]), false),
        (json!([{"command": "file.new"}]), true),
        (json!([{"id": "FILE.NEW"}]), true),
        (json!([{"command": "file.new"}, {"command": "missing.command"}]), true),
    ] {
        for params in [steps.clone(), json!({"commands": steps})] {
            let (mut app, _script, ctx) = app_with_script();
            send(&mut app, &ctx, "first request".into());
            let (req, reply) = ControlRequest::new("engine.script", params);
            app.run_control(&ctx, req);
            assert!(reply.try_recv().is_ok());
            assert_eq!(app.agent_chat.backend.is_none(), replaced);
            assert_eq!(app.agent_chat.items.is_empty(), replaced);
            assert_eq!(app.agent_chat.turn.is_none(), replaced);
        }
    }
}

#[test]
fn leaving_a_tab_refuses_computation_and_export_replies() {
    // The export deliberately omits its path: even a worker's error must be replaced by
    // cancellation after leaving, without writing a file or echoing into another document.
    for params in [boxed(0.0), json!({"command": "file.export", "params": {}})] {
        let (mut app, tx) = app_in_a_turn();
        let ctx = crate::fonts::test_context();
        app.apply_off_thread = Some(std::time::Duration::ZERO);
        app.preferences.agent_off_thread = true;
        app.preferences.ai_permissions = agents::Mode::All;
        let reply = ask(&tx, "engine.execute", params);
        pump_tools(&mut app);
        app.drain_control(&ctx);
        assert!(app.control_job.is_some() || !app.control_answering.is_empty(), "the worker result waits for a later frame");
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        let incoming = app.session.doc.clone();
        let log = app.session.log.clone();
        app.poll_control_jobs(&ctx);
        let response = reply.try_recv().expect("cancellation must not wait for the worker");
        assert_eq!(response["ok"], false);
        assert!(response["error"].as_str().is_some_and(|e| e.contains("document tab was left")));
        assert!(app.control_job.is_none() && app.control_answering.is_empty());
        assert!(Arc::ptr_eq(&app.session.doc, &incoming));
        assert_eq!(app.session.log, log);
        assert!(app.session.undo.is_empty());
        crate::documents::switch(&mut app, 0);
        assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
    }
}

#[test]
fn leaving_a_tab_refuses_admitted_commands_animations_and_screenshots() {
    for animate in [false, true] {
        let (mut app, tx) = app_in_a_turn();
        let ctx = crate::fonts::test_context();
        app.ui.agent_cursor.show = animate;
        crate::scenario::publish_handle("toolbar:solid.box", egui::pos2(100.0, 100.0));
        app.ui.agent_cursor.speed = crate::agent_cursor::Speed::Times(1.0);
        let change = ask(&tx, "engine.execute", boxed(0.0));
        let shot = ask(&tx, "ui.screenshot", json!({}));
        pump_tools(&mut app);
        // Capture a request already waiting for a frame, as well as a queued command.
        let screenshot = app.control_held.pop_back().unwrap();
        app.run_control(&ctx, screenshot);
        assert_eq!(app.pending_shots.len(), 1);
        if animate {
            app.drain_control(&ctx);
            assert!(app.agent.busy(), "the command is waiting on cursor animation");
        }
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        let other = app.session.doc.clone();
        app.drain_control(&ctx);
        crate::agent_cursor::step(&mut app, &ctx);
        app.collect_screenshots(&ctx);
        for reply in [change, shot] {
            let response = reply.try_recv().expect("cancelled requests answer without running");
            assert_eq!(response["ok"], false);
            assert!(response["error"].as_str().is_some_and(|e| e.contains("document tab was left")));
        }
        assert!(Arc::ptr_eq(&app.session.doc, &other));
        assert!(app.session.undo.is_empty() && app.pending_shots.is_empty());
        assert!(app.queued_shots.is_empty(), "a cancelled screenshot must not capture the incoming tab");
        crate::documents::switch(&mut app, 0);
        assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
        // Adopting into a blank tab replaces the document and clears its old conversation.
        app.agent_chat.input = "discarded draft".into();
        crate::documents::adopt(&mut app, solidcraft_engine::Session::default());
        assert!(app.agent_chat.input.is_empty() && app.agent_chat.items.is_empty());
    }
}

#[test]
fn late_events_approvals_and_undo_stay_with_the_originating_document() {
    let (mut app, first, ctx) = app_with_script();
    let (tx, rx) = channel();
    app.agent_chat.tools_rx = Some(rx);
    send(&mut app, &ctx, "make two blocks".into());
    for x in [0.0, 20.0] {
        let reply = ask(&tx, "engine.execute", boxed(x));
        frame(&mut app);
        assert_eq!(reply.try_recv().unwrap()["ok"], true);
    }
    assert_eq!(app.session.undo.len(), 2);
    app.preferences.ai_permissions = agents::Mode::Ask;
    let held = ask(&tx, "engine.execute", boxed(40.0));
    pump_tools(&mut app);
    assert_eq!(app.agent_chat.waiting(), 1);
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert_eq!(held.try_recv().unwrap()["ok"], false);
    assert_eq!(app.agent_chat.waiting(), 0);
    first.report(Event::Text { thought: false, text: "Stopped in the first design".into() });
    ended(&first, "cancelled");
    let late = ask(&tx, "engine.execute", boxed(60.0));
    frame(&mut app);
    assert!(app.agent_chat.items.is_empty());
    assert!(app.session.doc.features.is_empty() && app.session.undo.is_empty());
    assert!(matches!(late.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));

    crate::documents::switch(&mut app, 0);
    frame(&mut app);
    assert_eq!(late.try_recv().unwrap()["ok"], false);
    assert_eq!(app.session.doc.features.len(), 2);
    assert_eq!(app.session.undo.len(), 1, "the outgoing turn merged only its own undo");
    let changes = app.agent_chat.items.iter().position(|i| matches!(i, Item::Changes { steps: 2, .. })).unwrap();
    assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Agent { text, .. } if text == "Stopped in the first design")));
    undo_changes(&mut app, changes);
    assert!(app.session.doc.features.is_empty());
    crate::documents::switch(&mut app, 1);
    app.preferences.ai_features = false;
    poll(&mut app, &ctx);
    crate::documents::switch(&mut app, 0);
    assert!(app.agent_chat.backend.is_none(), "disabling AI stops hidden conversations too");
}

fn texts(app: &SolveApp) -> Vec<String> {
    app.agent_chat.pending.iter().map(|p| p.text.clone()).collect()
}

/// While the agent works a message is queued, not dropped and not sent; it is not in the
/// transcript as a sent message, but waits as a pending one that can be edited and removed.
#[test]
fn a_message_typed_while_busy_is_queued() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "make a box".into());
    assert_eq!(app.agent_chat.state(), State::Busy);
    send(&mut app, &ctx, "then a cylinder".into());
    send(&mut app, &ctx, "   ".into());
    send(&mut app, &ctx, "and a sphere".into());
    assert_eq!(texts(&app), ["then a cylinder", "and a sphere"]);
    assert_eq!(script.calls(), ["prompt: make a box"], "nothing else was sent");
    let users = app.agent_chat.items.iter().filter(|i| matches!(i, Item::User { .. })).count();
    assert_eq!(users, 1, "queued messages are not sent messages yet");
    // Remove one; edit the other.
    remove_pending(&mut app, 0);
    assert_eq!(texts(&app), ["and a sphere"]);
    app.agent_chat.pending[0].text = "and a cone".into();
    assert_eq!(app.agent_chat.waiting(), 0);
    assert!(status(&app)["pending"][0]["text"] == "and a cone");
}

/// When the turn ends normally the queue goes as the next prompt, all messages in order in one.
#[test]
fn the_queue_is_sent_when_the_turn_ends() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "first".into());
    send(&mut app, &ctx, "second".into());
    send(&mut app, &ctx, "third".into());
    ended(&script, "");
    poll(&mut app, &ctx);
    assert_eq!(script.calls(), ["prompt: first", "prompt: second\n\nthird"]);
    assert!(app.agent_chat.pending.is_empty());
    assert_eq!(app.agent_chat.state(), State::Busy, "the next turn is under way");
    assert!(matches!(app.agent_chat.items.last(), Some(Item::User { text, .. }) if text == "second\n\nthird"));
    // Nothing queued: the end of the turn just ends it.
    ended(&script, "");
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert_eq!(script.calls().len(), 2);
}

/// Stop does not send the queue by itself; it stays, with a Send that sends it later.
#[test]
fn stop_keeps_the_queue() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "first".into());
    send(&mut app, &ctx, "later".into());
    cancel(&mut app);
    assert_eq!(script.calls(), ["prompt: first", "cancel"]);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert_eq!(texts(&app), ["later"], "still queued");
    assert_eq!(script.calls().len(), 2, "not sent");
    // Send (the bubble's button) sends it, without the interruption note.
    send_now(&mut app, &ctx);
    assert_eq!(script.calls(), ["prompt: first", "cancel", "prompt: later"]);
    assert!(app.agent_chat.pending.is_empty());
}

/// Send now: cancel first, the prompt only after the agent has ended the cancelled turn, with a
/// note that the previous step was interrupted (shown without it in the conversation).
#[test]
fn send_now_cancels_then_sends_with_a_note() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "build a gearbox".into());
    send(&mut app, &ctx, "use 12 teeth instead".into());
    send_now(&mut app, &ctx);
    assert_eq!(script.calls(), ["prompt: build a gearbox", "cancel"], "cancel alone, the prompt waits for the end");
    poll(&mut app, &ctx);
    assert_eq!(script.calls().len(), 2);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    assert_eq!(script.calls(), ["prompt: build a gearbox".to_string(), "cancel".into(), format!("prompt: {INTERRUPT_NOTE}\n\nuse 12 teeth instead")]);
    assert!(matches!(app.agent_chat.items.last(), Some(Item::User { text, .. }) if text == "use 12 teeth instead"));
    // Stop after Send now wins: nothing more is sent by itself.
    send(&mut app, &ctx, "one more".into());
    send_now(&mut app, &ctx);
    cancel(&mut app);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    assert_eq!(texts(&app), ["one more"]);
}

/// The steered message also goes after a turn that ended by itself just before the cancel arrived.
#[test]
fn send_now_after_the_turn_already_ended_still_sends() {
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "a".into());
    send(&mut app, &ctx, "b".into());
    send_now(&mut app, &ctx);
    ended(&script, "");
    poll(&mut app, &ctx);
    assert_eq!(script.calls().last().map(String::as_str), Some("prompt: b"), "no note: nothing was interrupted");
}

/// What an interrupted turn changed undoes as one step, and the steered message starts its own.
#[test]
fn an_interrupted_turns_changes_undo_as_one_step() {
    let (mut app, script, ctx) = app_with_script();
    let (tx, rx) = channel();
    app.agent_chat.tools_rx = Some(rx);
    let before = app.session.doc.clone();
    let depth = app.session.undo.len();
    send(&mut app, &ctx, "two boxes".into());
    let replies: Vec<_> = [0.0, 20.0].into_iter().map(|x| ask(&tx, "engine.execute", boxed(x))).collect();
    frame(&mut app);
    for r in replies {
        assert_eq!(r.recv_timeout(std::time::Duration::from_secs(30)).unwrap()["ok"], true);
    }
    send(&mut app, &ctx, "stop, make it one".into());
    send_now(&mut app, &ctx);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    assert_eq!(app.session.undo.len(), depth + 1, "the interrupted message is one step");
    let n = app.agent_chat.items.iter().rposition(|i| matches!(i, Item::Changes { steps: 2, .. })).expect("its changes are listed");
    // The steered message is on its own turn.
    assert!(app.agent_chat.turn.as_ref().is_some_and(|t| t.label.contains("stop, make it one") && t.changes == 0));
    undo_changes(&mut app, n);
    assert!(Arc::ptr_eq(&app.session.doc, &before));
}

/// The same with a real agent process speaking ACP: `session/cancel` goes out, the agent answers
/// the held prompt with the cancelled stop reason, and only then does the queued text go as a new
/// `session/prompt`.
#[cfg(unix)]
#[test]
fn send_now_with_an_acp_agent_sends_cancel_then_the_prompt() {
    let dir = std::env::temp_dir().join(format!("sc-agent-chat-steer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("received.log");
    // A fake agent: answers initialize and session/new; holds a prompt whose text says "slow"
    // until it gets session/cancel (then answers it "cancelled"); answers other prompts at once.
    let script = r#"
log="$1"; held=""
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      case "$line" in
        *slow*) held="$id" ;;
        *) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
      esac ;;
    *'"method":"session/cancel"'*)
      if [ -n "$held" ]; then printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$held"; held=""; fi ;;
  esac
done
"#;
    let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string(), "fake-agent".to_string(), log.to_string_lossy().into_owned()];
    let start = backend::StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.state = Some(State::Starting);
    let ctx = crate::fonts::test_context();
    let until = |app: &mut SolveApp, what: &str, done: &dyn Fn(&SolveApp) -> bool| {
        let t = std::time::Instant::now();
        while !done(app) {
            assert!(t.elapsed() < std::time::Duration::from_secs(20), "timed out waiting for {what}");
            poll(app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    until(&mut app, "the agent to be ready", &|a| a.agent_chat.state() == State::Ready);
    send(&mut app, &ctx, "slow job".into());
    send(&mut app, &ctx, "do this instead".into());
    assert_eq!(texts(&app), ["do this instead"]);
    send_now(&mut app, &ctx);
    until(&mut app, "the steered turn to finish", &|a| a.agent_chat.state() == State::Ready && a.agent_chat.pending.is_empty());
    let methods: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            let m = v["method"].as_str()?.to_string();
            let text = v.pointer("/params/prompt/0/text").and_then(Value::as_str).unwrap_or("");
            Some(if text.is_empty() { m } else { format!("{m}: {text}") })
        })
        .collect();
    assert_eq!(
        methods,
        [
            "initialize".to_string(),
            "session/new".into(),
            "session/prompt: slow job".into(),
            "session/cancel".into(),
            format!("session/prompt: {INTERRUPT_NOTE}\n\ndo this instead"),
        ]
    );
    disconnect(&mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

/// New chat: the running message is stopped, the transcript and the queue are cleared, a fresh
/// session starts on the same agent, the mode stays, and the design's undo history is untouched.
#[test]
fn new_chat_clears_everything_and_keeps_the_agent_and_mode() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.modes = vec![("plan".into(), "Plan".into()), ("code".into(), "Code".into())];
    app.agent_chat.mode = "plan".into();
    app.agent_chat.agent_id = Some("claude".into());
    let depth = app.session.undo.len();
    assert!(!new_chat_needs_confirm(&app));
    send(&mut app, &ctx, "first".into());
    send(&mut app, &ctx, "queued".into());
    assert!(new_chat_needs_confirm(&app), "a running message and a queue ask first");
    new_chat(&mut app);
    assert_eq!(script.calls(), ["prompt: first", "cancel", "new_session"]);
    assert!(app.agent_chat.items.is_empty() && app.agent_chat.pending.is_empty());
    assert_eq!(app.agent_chat.state(), State::Starting);
    assert_eq!(app.agent_chat.agent_id.as_deref(), Some("claude"));
    // The cancelled message's late end is not reported; the fresh session is ready again.
    ended(&script, "cancelled");
    script.report(Event::Modes { current: "code".into(), available: vec![] });
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Ready);
    assert!(app.agent_chat.items.is_empty(), "no notice about the old message");
    assert_eq!(app.agent_chat.mode, "plan", "the mode is kept");
    assert_eq!(app.session.undo.len(), depth);
    assert!(!new_chat_needs_confirm(&app));
}

/// With a real ACP agent, New chat sends `session/new` on the same process (no restart).
#[cfg(unix)]
#[test]
fn new_chat_with_an_acp_agent_opens_a_new_session_on_the_same_process() {
    let dir = std::env::temp_dir().join(format!("sc-agent-chat-new-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("received.log");
    let script = r#"
log="$1"; n=0
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"session/new"'*) n=$((n+1)); printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s%s"}}\n' "$id" "$n" ;;
    *'"method":"session/prompt"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
"#;
    let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string(), "fake-agent".to_string(), log.to_string_lossy().into_owned()];
    let start = backend::StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.state = Some(State::Starting);
    let ctx = crate::fonts::test_context();
    let until = |app: &mut SolveApp, done: &dyn Fn(&SolveApp) -> bool| {
        let t = std::time::Instant::now();
        while !done(app) {
            assert!(t.elapsed() < std::time::Duration::from_secs(20), "timed out");
            poll(app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    send(&mut app, &ctx, "hello".into());
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    assert!(!app.agent_chat.items.is_empty());
    let pid = app.agent_chat.backend.as_ref().and_then(|b| b.pid());
    new_chat(&mut app);
    assert!(app.agent_chat.items.is_empty());
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    assert_eq!(app.agent_chat.backend.as_ref().and_then(|b| b.pid()), pid, "same process");
    send(&mut app, &ctx, "again".into());
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    let log_text = std::fs::read_to_string(&log).unwrap();
    let methods: Vec<String> =
        log_text.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| v["method"].as_str().map(String::from)).collect();
    assert_eq!(methods, ["initialize", "session/new", "session/prompt", "session/new", "session/prompt"]);
    assert!(log_text.lines().last().is_some_and(|l| l.contains("\"s2\"")), "the prompt goes to the new session: {log_text}");
    disconnect(&mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #9811: an agent that offers the standard `systemPrompt` field gets the note in it, on the first
/// session and on the one New chat opens.
#[cfg(unix)]
#[test]
fn an_acp_agent_offering_the_standard_field_gets_the_system_note_in_every_session() {
    let dir = std::env::temp_dir().join(format!("sc-agent-chat-note-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("received.log");
    let script = r#"
log="$1"; n=0
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentCapabilities":{"sessionCapabilities":{"systemPrompt":{}}},"agentInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"session/new"'*) n=$((n+1)); printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s%s"}}\n' "$id" "$n" ;;
  esac
done
"#;
    let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string(), "fake-agent".to_string(), log.to_string_lossy().into_owned()];
    let start = backend::StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.state = Some(State::Starting);
    let ctx = crate::fonts::test_context();
    let until = |app: &mut SolveApp, done: &dyn Fn(&SolveApp) -> bool| {
        let t = std::time::Instant::now();
        while !done(app) {
            assert!(t.elapsed() < std::time::Duration::from_secs(20), "timed out");
            poll(app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    new_chat(&mut app);
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    let sessions: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["method"] == "session/new")
        .collect();
    assert_eq!(sessions.len(), 2);
    for session in &sessions {
        assert_eq!(session.pointer("/params/systemPrompt/content/0/text").and_then(Value::as_str), Some(acp::SYSTEM_NOTE));
    }
    disconnect(&mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

fn models_reply(current: &str) -> Event {
    let m = |id: &str| acp::ModelInfo { id: id.into(), name: id.to_uppercase(), description: String::new() };
    Event::Models { current: current.into(), available: vec![m("fast"), m("smart")] }
}

/// An endpoint's listing whose ids read `vendor/model` and whose names (the descriptions here) read
/// `Vendor: Model`, as OpenRouter's, names its providers; the rows then read the model's name with
/// the id as the tooltip. One vendor is one flat list.
#[test]
fn an_endpoints_vendor_named_listing_names_its_providers() {
    let m = |id: &str, description: &str| acp::ModelInfo { id: id.into(), name: id.into(), description: description.into() };
    assert_eq!(model_provider(&m("openai/gpt-6.1-terra", "OpenAI: GPT-6.1 Terra")), Some(("OpenAI".into(), "GPT-6.1 Terra".into())));
    assert_eq!(model_provider(&m("openai/gpt-6.1-terra", "OpenAI: GPT-6.1 Terra\nmore")), Some(("OpenAI".into(), "GPT-6.1 Terra".into())));
    assert_eq!(model_provider(&m("gpt-6.1-terra", "OpenAI: GPT-6.1 Terra")), None, "no vendor in the id");
    assert_eq!(model_provider(&m("openai/gpt-6.1-terra", "OpenAI: ")), None, "no name");
    assert_eq!(model_provider(&m("openai/gpt-6.1-terra", "")), None);
    assert_eq!(
        model_provider(&m("openai/gpt-6.1-terra", "Provider: Host • current")),
        Some(("Host".into(), "openai/gpt-6.1-terra".into())),
        "an agent's own word first"
    );
    let rows = model_rows(&[m("openai/gpt-6.1-terra", "OpenAI: GPT-6.1 Terra"), m("qwen/qwen3.8-flash", "Qwen: Qwen3.8 Flash"), m("free-model", "")]);
    assert_eq!(
        rows,
        [
            ("GPT-6.1 Terra".to_string(), Some("OpenAI".to_string()), "openai/gpt-6.1-terra".to_string()),
            ("Qwen3.8 Flash".to_string(), Some("Qwen".to_string()), "qwen/qwen3.8-flash".to_string()),
            ("free-model".to_string(), Some(tr("ui.agent_chat_provider_other").to_string()), String::new()),
        ]
    );
    let rows = model_rows(&[m("openai/gpt-6.1-terra", "OpenAI: GPT-6.1 Terra"), m("openai/gpt-5.6-luna", "OpenAI: GPT-5.6 Luna")]);
    assert_eq!(rows[0], ("openai/gpt-6.1-terra".to_string(), None, "OpenAI: GPT-6.1 Terra".to_string()), "one vendor is one flat list");
}

/// Models from several providers fold under their providers inside the picker: the current model's
/// provider is open and the others folded; a header opens or folds its group; a typed search opens
/// every group with a match but one folded by hand; the provider asked for over the control channel
/// is open at the next opening.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_header_groups_models_by_provider_and_folds_them() {
    use crate::scenario::handle_at;
    let mut h = narrow_chat();
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    h.app.agent_chat.running_id = "claude".into();
    let m = |id: &str, name: &str| acp::ModelInfo { id: id.into(), name: name.into(), description: String::new() };
    script.report(Event::Models {
        current: "a".into(),
        available: vec![m("a", "Host · Alpha"), m("c", "Nous Portal · Gamma"), m("b", "Host · Beta"), m("d", "Nous Portal · Delta")],
    });
    h.frames(4);
    assert!(handle_at("agent_chat:provider").is_none(), "no provider drop-down of its own");
    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:group:Host").is_some() && handle_at("agent_chat:model:group:Nous Portal").is_some());
    assert!(handle_at("agent_chat:model:a").is_some() && handle_at("agent_chat:model:b").is_some(), "the current model's provider is open");
    assert!(handle_at("agent_chat:model:c").is_none() && handle_at("agent_chat:model:d").is_none(), "the other provider is folded");
    h.step(&json!({"click": {"handle": "agent_chat:model:group:Nous Portal"}})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:c").is_some(), "a header opens its group");
    h.step(&json!({"click": {"handle": "agent_chat:model:group:Host"}})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:a").is_none(), "and folds it");
    h.step(&json!({"text": "ta"})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:b").is_none(), "a fold made by hand stays while searching, Beta or not");
    assert!(handle_at("agent_chat:model:d").is_some() && handle_at("agent_chat:model:c").is_none(), "Delta matches, Gamma does not");
    h.step(&json!({"click": {"handle": "agent_chat:model:d"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_model: d"]);
    assert_eq!(h.app.agent_chat.model, "d");
    assert!(handle_at("agent_chat:model:search").is_none(), "a choice closes it");

    control(&mut h.app, &h.ctx, &json!({"provider": "Host"})).unwrap();
    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:a").is_some(), "the provider asked for is open");
    assert!(handle_at("agent_chat:model:d").is_some(), "and the current model's");
}

/// Choosing a model tells the agent, shows it as current and remembers it for this agent; a model
/// the agent did not offer is ignored.
#[test]
fn choosing_a_model_sets_it_and_remembers_it() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "claude".into();
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    assert!(models_shown(&app));
    assert_eq!(app.agent_chat.model, "fast");
    set_model(&mut app, "smart");
    set_model(&mut app, "nonsense");
    assert_eq!(script.calls(), ["set_model: smart"]);
    assert_eq!(app.agent_chat.model, "smart");
    assert_eq!(app.preferences.agent_models.get("claude").map(String::as_str), Some("smart"));
    assert_eq!(status(&app)["model"], "smart");
    // The agent switching its own model is shown too.
    script.report(Event::Models { current: "fast".into(), available: vec![] });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(app.agent_chat.models.len(), 2, "the list stays");
}

/// A scripted endpoint: its agent is configured, and it is running with these models.
#[cfg(not(target_arch = "wasm32"))]
fn endpoint_with_script() -> (SolveApp, Script, egui::Context) {
    let (mut app, script, ctx) = app_with_script();
    let endpoint = "http://localhost:11434/v1";
    app.agent_chat.running_id = "api-1".into();
    app.agent_chat.running_endpoint = endpoint.into();
    app.preferences.agents.push(agents::AgentConfig {
        id: "api-1".into(),
        kind: agents::Kind::Api,
        base_url: endpoint.into(),
        model: "fast".into(),
        ..Default::default()
    });
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    (app, script, ctx)
}

/// A typed model goes straight to an endpoint, joins the list for the next connection and becomes
/// the agent's default.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_typed_model_goes_to_an_endpoint_and_is_remembered() {
    let (mut app, script, _ctx) = endpoint_with_script();
    assert!(custom_models_accepted(&app));
    assert!(set_custom_model(&mut app, "  openai/gpt-x "));
    assert_eq!(script.calls(), ["set_model: openai/gpt-x"]);
    assert_eq!(app.agent_chat.models.first().map(|m| m.id.as_str()), Some("openai/gpt-x"));
    assert_eq!(app.agent_chat.models_cache.get("http://localhost:11434/v1").and_then(|l| l.first()).map(|m| m.id.as_str()), Some("openai/gpt-x"));
    assert_eq!(app.preferences.agents.iter().find(|a| a.id == "api-1").unwrap().model, "openai/gpt-x");
    assert_eq!(app.preferences.agent_models.get("api-1").map(String::as_str), Some("openai/gpt-x"));
    assert_eq!(status(&app)["model"], "openai/gpt-x");
    assert_eq!(status(&app)["customModels"], true);
    let listed = app.agent_chat.models.len();
    assert!(set_custom_model(&mut app, "smart"));
    assert_eq!(app.agent_chat.models.len(), listed, "a listed id goes the usual way");
    assert_eq!(app.agent_chat.model, "smart");
}

/// A typed model is only sent when it could be a model at all, and the list stays within its cap.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_typed_model_that_cannot_be_one_is_not_sent() {
    let (mut app, script, _ctx) = endpoint_with_script();
    for text in ["", "   ", "x".repeat(MODEL_ID_CAP + 1).as_str(), "bell\u{7}", "two\nlines"] {
        assert!(!set_custom_model(&mut app, text), "{text:?}");
    }
    assert!(script.calls().is_empty());
    assert!(set_custom_model(&mut app, &"x".repeat(MODEL_ID_CAP)));
    app.agent_chat.models =
        (0..MODEL_LIST_CAP).map(|i| acp::ModelInfo { id: format!("m{i}"), name: String::new(), description: String::new() }).collect();
    assert!(set_custom_model(&mut app, "one-more"));
    assert_eq!(app.agent_chat.models.len(), MODEL_LIST_CAP);
    assert_eq!(app.agent_chat.models.first().map(|m| m.id.as_str()), Some("one-more"));
}

/// An agent with a model option gets a typed model through it. Nothing changes until it answers,
/// and a refusal leaves the old model with the agent's reason in the chat.
#[test]
fn a_typed_model_is_sent_to_a_modern_agent_and_a_rejection_keeps_the_old_one() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "modern".into();
    script.report(Event::ConfigOptions(modern_options("fast")));
    poll(&mut app, &ctx);
    assert!(custom_models_accepted(&app));
    assert!(set_custom_model(&mut app, "claude-x"));
    assert_eq!(script.calls(), ["set_config_option: some.provider.model=claude-x"]);
    assert!(app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "fast");
    assert!(!set_custom_model(&mut app, "claude-y"), "one request at a time");

    script.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Err("no such model".into()) });
    poll(&mut app, &ctx);
    assert!(!app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "fast");
    assert!(
        app.agent_chat
            .items
            .iter()
            .any(|i| matches!(i, Item::Notice { text, error: true } if text.contains("claude-x") && text.contains("no such model"))),
        "the notice names the model and the reason"
    );
    assert!(!app.preferences.agent_models.contains_key("modern"));
    assert_eq!(app.agent_chat.typed_model, None);

    // An agent that takes it says so in its snapshot, and that is what is remembered.
    assert!(set_custom_model(&mut app, "claude-x"));
    script.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("claude-x")) });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "claude-x");
    assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("claude-x"));
}

/// A legacy agent shows a typed model at once; a refusal puts everything back and says why.
#[test]
fn a_typed_model_is_sent_to_a_legacy_agent_and_a_rejection_reverts_it() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "claude".into();
    app.preferences.agent_models.insert("claude".into(), "fast".into());
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    assert!(set_custom_model(&mut app, "zzz"));
    assert_eq!(script.calls(), ["set_model: zzz"]);
    assert_eq!(app.agent_chat.model, "zzz");
    assert_eq!(app.preferences.agent_models.get("claude").map(String::as_str), Some("zzz"));

    script.report(Event::ModelRejected("no such model".into()));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(app.agent_chat.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["fast", "smart"]);
    assert_eq!(app.preferences.agent_models.get("claude").map(String::as_str), Some("fast"));
    assert!(
        app.agent_chat
            .items
            .iter()
            .any(|i| matches!(i, Item::Notice { text, error: true } if text.contains("zzz") && text.contains("no such model"))),
        "the notice names the model and the reason"
    );

    // A listed model the agent refuses goes back too, and nothing was remembered before it here.
    app.preferences.agent_models.remove("claude");
    set_model(&mut app, "smart");
    script.report(Event::ModelRejected("x".repeat(MODELS_NOTE_CAP * 2)));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(app.agent_chat.models.len(), 2);
    assert!(!app.preferences.agent_models.contains_key("claude"));
    let Some(Item::Notice { text, .. }) = app.agent_chat.items.last() else { panic!("no notice") };
    assert_eq!(text.chars().count(), MODELS_NOTE_CAP);

    // The agent confirming its model ends the wait: a later refusal has nothing to put back.
    assert!(set_custom_model(&mut app, "zzz"));
    script.report(Event::Models { current: "zzz".into(), available: vec![] });
    script.report(Event::ModelRejected("late".into()));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "zzz");
}

/// An agent that lists no models and has no model option cannot be sent one; a stopped one neither.
#[test]
fn an_agent_without_models_takes_no_typed_model() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "other".into();
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    assert!(!custom_models_accepted(&app));
    assert_eq!(status(&app)["customModels"], false);
    assert!(!set_custom_model(&mut app, "zzz"));
    assert!(script.calls().is_empty());
    script.report(Event::ConfigOptions(vec![]));
    poll(&mut app, &ctx);
    assert!(!custom_models_accepted(&app), "its options name no model");

    let (mut app, script, ctx) = app_with_script();
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    assert!(custom_models_accepted(&app));
    app.agent_chat.backend = None;
    assert!(!custom_models_accepted(&app));
    assert!(!set_custom_model(&mut app, "zzz"));
}

/// The control channel's `model`: a listed one as before, an unlisted one when the agent takes
/// typed models, otherwise ignored as before.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_control_channel_sends_an_unlisted_model_where_it_can_go() {
    let (mut app, script, ctx) = endpoint_with_script();
    let reply = control(&mut app, &ctx, &json!({"model": "smart"})).unwrap();
    assert_eq!(reply["model"], "smart");
    let reply = control(&mut app, &ctx, &json!({"model": "unlisted"})).unwrap();
    assert_eq!(reply["model"], "unlisted");
    assert_eq!(reply["customModels"], true);
    assert_eq!(reply["modelsNote"], Value::Null);
    assert_eq!(script.calls(), ["set_model: smart", "set_model: unlisted"]);

    let (mut app, script, ctx) = app_with_script();
    let reply = control(&mut app, &ctx, &json!({"model": "unlisted"})).unwrap();
    assert_eq!(reply["model"], "");
    assert_eq!(reply["customModels"], false);
    assert!(script.calls().is_empty());
}

/// The header's picker on an agent that lists models: it searches, a row chooses, and what was typed
/// can be chosen too. The control and the popup stay in a narrow chat's window.
#[test]
fn the_header_model_picker_searches_and_takes_a_typed_model() {
    use crate::scenario::{handle_at, rect_of};
    let mut h = narrow_chat();
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    h.app.agent_chat.running_id = "claude".into();
    script.report(models_reply("fast"));
    h.frames(4);
    let panel = rect_of("agent_chat:panel").expect("the panel");
    let control = rect_of("agent_chat:model").expect("the picker");
    assert!(control.left() >= panel.left() && control.right() <= panel.right(), "{control:?} in {panel:?}");
    assert!(handle_at("agent_chat:model:search").is_none(), "closed");

    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    for name in ["search", "fast", "smart"] {
        assert!(handle_at(&format!("agent_chat:model:{name}")).is_some(), "{name}");
    }
    assert!(handle_at("agent_chat:model:custom").is_none(), "nothing typed");
    let search = rect_of("agent_chat:model:search").expect("the search box");
    assert!(search.left() >= 0.0 && search.right() <= 1600.0, "inside the harness's window: {search:?}");

    h.step(&json!({"text": "SMA"})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:fast").is_none() && handle_at("agent_chat:model:smart").is_some());
    assert!(handle_at("agent_chat:model:custom").is_some(), "the typed text is not a listed model");
    h.step(&json!({"click": {"handle": "agent_chat:model:smart"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_model: smart"]);
    assert_eq!(h.app.agent_chat.model, "smart");
    assert!(handle_at("agent_chat:model:search").is_none(), "a choice closes it");

    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    h.step(&json!({"text": "zzz"})).unwrap();
    h.frames(3);
    h.step(&json!({"click": {"handle": "agent_chat:model:custom"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_model: smart", "set_model: zzz"]);
    assert_eq!(h.app.agent_chat.model, "zzz");
    disconnect(&mut h.app);
}

/// The picker on an agent with a model option goes by the same names. A row and a typed model both
/// go through the option, and other options stay plain dropdowns.
#[test]
fn the_header_model_option_searches_and_takes_a_typed_model() {
    use crate::scenario::handle_at;
    let mut h = narrow_chat();
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    script.report(Event::ConfigOptions(modern_options("fast")));
    h.frames(4);
    assert_eq!(handle_at("agent_chat:model"), handle_at("agent_chat:config:some.provider.model"));

    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    for name in ["search", "fast", "smart"] {
        assert!(handle_at(&format!("agent_chat:model:{name}")).is_some(), "{name}");
        assert_eq!(handle_at(&format!("agent_chat:model:{name}")), handle_at(&format!("agent_chat:config:some.provider.model:{name}")));
    }
    h.step(&json!({"click": {"handle": "agent_chat:model:smart"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_config_option: some.provider.model=smart"]);
    script.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    h.frames(3);
    assert_eq!(h.app.agent_chat.model, "smart");

    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    h.step(&json!({"text": "zzz"})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:custom").is_some());
    h.step(&json!({"click": {"handle": "agent_chat:model:custom"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls().last().map(String::as_str), Some("set_config_option: some.provider.model=zzz"));
    assert_eq!(h.app.agent_chat.model, "smart", "not until the agent says so");
    assert!(h.app.agent_chat.config_pending);
    disconnect(&mut h.app);
}

/// Nothing typed can be chosen where it could not be sent, and the reason a listing failed shows
/// in the popup.
#[test]
fn the_header_model_picker_offers_no_typed_model_it_cannot_send_and_shows_why_listing_failed() {
    use crate::scenario::handle_at;
    let mut h = narrow_chat();
    h.app.agent_chat.state = Some(State::Ready);
    h.app.agent_chat.models = vec![acp::ModelInfo { id: "fast".into(), name: "fast".into(), description: String::new() }];
    h.app.agent_chat.model = "fast".into();
    h.app.agent_chat.models_note = Some("Couldn't list models: offline".into());
    h.frames(4);
    assert!(!custom_models_accepted(&h.app));
    h.step(&json!({"click": {"handle": "agent_chat:model"}})).unwrap();
    h.frames(3);
    assert!(h.widget("Couldn't list models: offline").is_some());
    h.step(&json!({"text": "zzz"})).unwrap();
    h.frames(3);
    assert!(handle_at("agent_chat:model:search").is_some());
    assert!(handle_at("agent_chat:model:custom").is_none());
}

/// A running endpoint's card uses the chat's list and changes the chat's model.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_card_of_a_running_endpoint_changes_the_chats_model() {
    use crate::scenario::handle_at;
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.preferences.ai_features = true;
    h.app.agent_chat.detection = Some(Default::default());
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    h.app.agent_chat.running_id = "api-1".into();
    h.app.agent_chat.running_endpoint = "http://localhost:1/v1".into();
    h.app.preferences.agents.push(agents::AgentConfig {
        id: "api-1".into(),
        name: "Endpoint".into(),
        kind: agents::Kind::Api,
        base_url: "http://localhost:1/v1".into(),
        model: "fast".into(),
        ..Default::default()
    });
    script.report(models_reply("fast"));
    h.step(&json!({"call": "ui.prefs", "params": {"open": "AI"}})).unwrap();
    h.frames(4);
    h.step(&json!({"click": {"handle": "prefs:ai_model:api-1"}})).unwrap();
    h.frames(3);
    assert!(handle_at("prefs:ai_model:api-1:smart").is_some());
    h.step(&json!({"click": {"handle": "prefs:ai_model:api-1:smart"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_model: smart"]);
    assert_eq!(h.app.agent_chat.model, "smart");
    h.step(&json!({"click": {"handle": "prefs:ai_model:api-1"}})).unwrap();
    h.frames(3);
    h.step(&json!({"text": "zzz"})).unwrap();
    h.frames(3);
    h.step(&json!({"click": {"handle": "prefs:ai_model:api-1:custom"}})).unwrap();
    h.frames(3);
    assert_eq!(script.calls(), ["set_model: smart", "set_model: zzz"]);
    assert_eq!(h.app.preferences.agents.iter().find(|a| a.id == "api-1").map(|a| a.model.as_str()), Some("zzz"));
    disconnect(&mut h.app);
}

/// A listing that fails says why and leaves last time's list, with the configured model in it; the
/// next connection starts without the note.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_listing_that_fails_says_why_and_keeps_the_list_of_last_time() {
    let (mut app, script, ctx) = app_with_script();
    let endpoint = "http://localhost:11434/v1";
    app.agent_chat.running_endpoint = endpoint.into();
    script.report(Event::ModelsNote(None));
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.models_note, None);
    let ids = |app: &SolveApp| app.agent_chat.models.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&app), ["fast", "smart"]);

    let only = |id: &str| acp::ModelInfo { id: id.into(), name: id.into(), description: String::new() };
    script.report(Event::ModelsNote(Some("x".repeat(MODELS_NOTE_CAP + 10))));
    script.report(Event::Models { current: "local".into(), available: vec![only("local")] });
    poll(&mut app, &ctx);
    let note = app.agent_chat.models_note.clone().unwrap();
    assert!(note.starts_with(&trf("ui.agent_chat_models_failed", &[("error", "x")])), "{note}");
    assert_eq!(note.chars().count(), MODELS_NOTE_CAP);
    assert_eq!(ids(&app), ["local", "fast", "smart"]);
    assert_eq!(app.agent_chat.model, "local");
    assert_eq!(app.agent_chat.models_cache.get(endpoint).map(Vec::len), Some(2), "the failed listing is not remembered");

    script.report(Event::ModelsNote(Some("again".into())));
    script.report(Event::Models { current: "fast".into(), available: vec![only("fast")] });
    poll(&mut app, &ctx);
    assert_eq!(ids(&app), ["local", "fast", "smart"], "a model already listed is not listed twice");

    // A real listing on an endpoint that cannot be reached.
    let mut app = app_with_endpoints();
    app.agent_chat.models_note = Some("from before".into());
    app.agent_chat.agent_id = Some("api-1".into());
    connect(&mut app, &ctx);
    assert_eq!(app.agent_chat.models_note, None);
    let start = std::time::Instant::now();
    while app.agent_chat.models.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
        poll(&mut app, &ctx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.agent_chat.models_note.as_deref().is_some_and(|note| note.contains("offline")), "{:?}", app.agent_chat.models_note);
    assert_eq!(ids(&app), ["m"]);
    assert!(models_shown(&app));
    disconnect(&mut app);
}

/// The next session puts back the model chosen last time, when the agent offers it.
#[test]
fn the_remembered_model_is_set_on_the_next_session() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "claude".into();
    app.preferences.agent_models.insert("claude".into(), "smart".into());
    script.report(models_reply("fast"));
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    assert_eq!(script.calls(), ["set_model: smart"]);
    assert_eq!(app.agent_chat.model, "smart");
    // Already on it, or no longer offered: nothing is sent.
    script.report(models_reply("smart"));
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    app.preferences.agent_models.insert("claude".into(), "gone".into());
    script.report(models_reply("fast"));
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    assert_eq!(script.calls().len(), 1);
}

fn provider_models() -> Vec<acp::ModelInfo> {
    let m = |id: &str, name: &str, description: &str| acp::ModelInfo { id: id.into(), name: name.into(), description: description.into() };
    vec![
        m("nous:anthropic/opus", "Nous Portal · anthropic/opus", "Provider: Nous Portal"),
        m("nous:deepseek/v4", "Nous Portal · deepseek/v4", "Provider: Nous Portal"),
        m("custom:host:deepseek-v4.1-flash", "deepseek-v4.1-flash", "Provider: Host • current"),
        m("custom:host:haiku", "haiku", "Provider: Host"),
        m("plain", "plain", ""),
    ]
}

/// Models from several providers: the header picks a provider, then one of its models, listed
/// without the provider in front. Picking a provider alone leaves the agent's model as it is.
#[test]
fn models_from_several_providers_are_picked_by_provider_first() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "hermes".into();
    script.report(Event::Models { current: "custom:host:deepseek-v4.1-flash".into(), available: provider_models() });
    poll(&mut app, &ctx);
    let providers = model_providers(&app);
    assert_eq!(providers, [("Nous Portal".to_string(), 2), ("Host".to_string(), 2), ("Other".to_string(), 1)]);
    assert_eq!(shown_provider(&app).as_deref(), Some("Host"), "the current model's provider");
    let names = |app: &SolveApp| listed_models(app).into_iter().map(|(_, n)| n).collect::<Vec<_>>();
    assert_eq!(names(&app), ["deepseek-v4.1-flash", "haiku"]);
    set_model_provider(&mut app, "Nous Portal");
    set_model_provider(&mut app, "nonsense");
    assert_eq!(names(&app), ["anthropic/opus", "deepseek/v4"]);
    assert!(script.calls().is_empty(), "picking a provider sends nothing");
    assert_eq!(app.agent_chat.model, "custom:host:deepseek-v4.1-flash");
    let s = status(&app);
    assert_eq!(s["provider"], "Nous Portal");
    assert_eq!(s["providers"][0], json!({"name": "Nous Portal", "count": 2}));
    set_model(&mut app, "nous:deepseek/v4");
    assert_eq!(script.calls(), ["set_model: nous:deepseek/v4"]);
    assert_eq!(shown_provider(&app).as_deref(), Some("Nous Portal"));
    // The agent switching to another provider's model shows that provider.
    set_model_provider(&mut app, "Host");
    script.report(Event::Models { current: "plain".into(), available: vec![] });
    poll(&mut app, &ctx);
    assert_eq!(shown_provider(&app).as_deref(), Some("Other"));
    assert_eq!(names(&app), ["plain"]);
}

/// Models from one provider, or with none named: one list, every model as the agent names it.
#[test]
fn models_from_one_provider_keep_one_list() {
    let (mut app, script, ctx) = app_with_script();
    script.report(models_reply("fast"));
    poll(&mut app, &ctx);
    assert!(model_providers(&app).is_empty());
    assert_eq!(shown_provider(&app), None);
    assert_eq!(listed_models(&app).into_iter().map(|(_, n)| n).collect::<Vec<_>>(), ["FAST", "SMART"]);
    assert_eq!(status(&app)["provider"], Value::Null);
    let one = |name: &str| acp::ModelInfo { id: name.into(), name: format!("Only · {name}"), description: String::new() };
    script.report(Event::Models { current: "a".into(), available: vec![one("a"), one("b")] });
    poll(&mut app, &ctx);
    assert!(model_providers(&app).is_empty(), "a single provider needs no provider picker");
    assert_eq!(listed_models(&app)[0].1, "Only · a");
}

/// A provider name from the agent is bounded, and `·` or `Provider:` without a name is no provider.
#[test]
fn hostile_provider_names_are_bounded() {
    let m = |name: String, description: &str| acp::ModelInfo { id: "x".into(), name, description: description.into() };
    let long = "p".repeat(10_000);
    let (provider, name) = model_provider(&m(format!("{long} · model"), "")).unwrap();
    assert_eq!((provider.chars().count(), name.as_str()), (128, "model"));
    assert_eq!(model_provider(&m(" · model".into(), "")), None);
    assert_eq!(model_provider(&m("model".into(), "Provider:  ")), None);
    assert_eq!(model_provider(&m("model".into(), "Provider: A\nsecond line • current")).map(|p| p.0), Some("A".into()));
}

fn modern_options(model: &str) -> Vec<acp::ConfigOption> {
    vec![
        acp::ConfigOption {
            id: "some.provider.model".into(),
            name: "Model".into(),
            description: String::new(),
            category: "model".into(),
            current_value: model.into(),
            options: ["fast", "smart"]
                .into_iter()
                .map(|v| acp::ConfigChoice { value: v.into(), name: v.into(), description: String::new(), group: None })
                .collect(),
        },
        acp::ConfigOption {
            id: "custom-mode-key".into(),
            name: "Mode".into(),
            description: String::new(),
            category: "mode".into(),
            current_value: "ask".into(),
            options: ["ask", "plan"]
                .into_iter()
                .map(|v| acp::ConfigChoice { value: v.into(), name: v.into(), description: String::new(), group: None })
                .collect(),
        },
    ]
}

#[test]
fn modern_snapshots_replace_provider_choices_and_ignore_legacy_updates() {
    let (mut app, script, ctx) = app_with_script();
    script.report(Event::Models { current: "plain".into(), available: provider_models() });
    poll(&mut app, &ctx);
    set_model_provider(&mut app, "Host");
    script.report(Event::ConfigOptions(modern_options("smart")));
    script.report(Event::Models { current: "plain".into(), available: provider_models() });
    script.report(Event::Modes { current: "old-mode".into(), available: vec![("old-mode".into(), "Old mode".into())] });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "smart");
    assert_eq!(app.agent_chat.mode, "ask");
    assert!(app.agent_chat.model_provider.is_none());
    assert!(model_providers(&app).is_empty());
    assert_eq!(listed_models(&app).into_iter().map(|(m, _)| m.id).collect::<Vec<_>>(), ["fast", "smart"]);
    script.report(Event::ConfigOptions(vec![]));
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.config_options, Some(vec![]));
    assert!(app.agent_chat.models.is_empty() && app.agent_chat.modes.is_empty());
    set_model(&mut app, "plain");
    set_mode(&mut app, "old-mode");
    assert!(script.calls().is_empty(), "an empty modern snapshot never falls back to stale legacy choices");
}

#[test]
fn modern_acknowledgements_stay_with_their_document_without_resuming_its_queue() {
    let (mut app, first, ctx) = app_with_script();
    app.agent_chat.running_id = "first-modern".into();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    app.preferences.agent_models.insert("first-modern".into(), "smart".into());
    new_chat(&mut app);
    send(&mut app, &ctx, "first request".into());
    first.report(Event::ConfigOptions(modern_options("fast")));
    first.report(Event::Ready { agent: "first-modern".into(), images: false });
    poll(&mut app, &ctx);
    assert!(app.agent_chat.config_pending && app.agent_chat.queued.is_some());

    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    assert!(app.agent_chat.config_options.is_none() && !app.agent_chat.config_pending);
    let second = Script::default();
    app.agent_chat.backend = Some(Box::new(second.clone()));
    app.agent_chat.state = Some(State::Ready);
    app.agent_chat.running_id = "second-modern".into();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    set_model(&mut app, "smart");
    first.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    poll(&mut app, &ctx);
    assert!(app.agent_chat.config_pending, "the hidden tab's acknowledgement cannot unlock this one");
    assert_eq!(app.agent_chat.model, "fast");
    assert!(!app.preferences.agent_models.contains_key("second-modern"));
    second.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    poll(&mut app, &ctx);
    assert_eq!(app.preferences.agent_models.get("second-modern").map(String::as_str), Some("smart"));

    crate::documents::switch(&mut app, 0);
    assert!(app.agent_chat.config_pending, "pending configuration belongs to the conversation");
    poll(&mut app, &ctx);
    assert!(!app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "smart");
    assert_eq!(app.preferences.agent_models.get("first-modern").map(String::as_str), Some("smart"));
    assert_eq!(texts(&app), ["first request"]);
    assert!(app.agent_chat.queued.is_none() && app.agent_chat.stopped);
    assert!(first.prompts().is_empty() && second.prompts().is_empty(), "a late configuration reply cannot resume a departed tab's prompt");
}

#[test]
fn closing_a_pending_modern_chat_preserves_the_surviving_api_endpoint_and_saved_models() {
    for close_active in [false, true] {
        let (mut app, modern, ctx) = app_with_script();
        app.agent_chat.running_id = "modern".into();
        app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
        app.preferences.agent_models.insert("modern".into(), "fast".into());
        set_model(&mut app, "smart");
        assert!(app.agent_chat.config_pending);

        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        let endpoint = "http://127.0.0.1:12345/v1";
        let api = Script::default();
        app.agent_chat.backend = Some(Box::new(api.clone()));
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.running_id = "endpoint-agent".into();
        app.agent_chat.running_endpoint = endpoint.into();
        app.preferences.agents.push(agents::AgentConfig {
            id: "endpoint-agent".into(),
            kind: agents::Kind::Api,
            base_url: endpoint.into(),
            model: "fast".into(),
            ..Default::default()
        });
        api.report(models_reply("fast"));
        poll(&mut app, &ctx);
        set_model(&mut app, "smart");
        let cached = app.agent_chat.models_cache.get(endpoint).cloned().unwrap();
        modern.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
        modern.report(Event::ConfigOptions(vec![]));
        if close_active {
            crate::documents::switch(&mut app, 0);
            assert!(app.agent_chat.config_pending);
        }
        crate::documents::close(&mut app, 0, true);
        poll(&mut app, &ctx);
        assert_eq!(app.agent_chat.running_id, "endpoint-agent");
        assert_eq!(app.agent_chat.running_endpoint, endpoint);
        assert!(app.agent_chat.config_options.is_none() && !app.agent_chat.config_pending);
        assert_eq!(app.agent_chat.model, "smart");
        assert_eq!(app.agent_chat.models_cache.get(endpoint), Some(&cached));
        assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("fast"));
        assert_eq!(app.preferences.agent_models.get("endpoint-agent").map(String::as_str), Some("smart"));
        assert_eq!(app.preferences.agents.iter().find(|a| a.id == "endpoint-agent").unwrap().model, "smart");
        assert_eq!(api.calls(), ["set_model: smart"], "the closed chat's events cannot reach the surviving backend");

        // Reusing the same agent in a fresh design must restore only the last confirmed model.
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        assert!(app.agent_chat.config_options.is_none() && !app.agent_chat.config_pending);
        let fresh = Script::default();
        app.agent_chat.backend = Some(Box::new(fresh.clone()));
        app.agent_chat.state = Some(State::Starting);
        app.agent_chat.running_id = "modern".into();
        fresh.report(Event::ConfigOptions(modern_options("fast")));
        fresh.report(Event::Ready { agent: "modern".into(), images: false });
        poll(&mut app, &ctx);
        assert_eq!(app.agent_chat.model, "fast");
        assert!(!app.agent_chat.config_pending);
        assert!(fresh.calls().is_empty(), "an unacknowledged choice from the closed chat is not restored");
        assert_eq!(app.agent_chat.models_cache.get(endpoint), Some(&cached));
    }
}

#[test]
fn document_disconnect_paths_clear_modern_configuration_and_pending_requests() {
    let assert_cleared = |app: &SolveApp| {
        assert!(app.agent_chat.backend.is_none());
        assert!(app.agent_chat.config_options.is_none() && !app.agent_chat.config_pending);
        assert!(app.agent_chat.models.is_empty() && app.agent_chat.modes.is_empty());
        assert!(app.agent_chat.model.is_empty() && app.agent_chat.mode.is_empty());
        assert!(app.agent_chat.model_provider.is_none());
        assert_eq!(app.agent_chat.state(), State::Off);
    };
    let (mut app, _, _) = app_with_script();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    set_model(&mut app, "smart");
    disconnect(&mut app);
    assert_cleared(&app);

    for quitting in [false, true] {
        let (mut app, first, ctx) = app_with_script();
        app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
        set_model(&mut app, "smart");
        crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
        app.agent_chat.backend = Some(Box::new(Script::default()));
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
        set_model(&mut app, "smart");
        first.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
        if quitting {
            shutdown(&mut app);
        } else {
            app.preferences.ai_features = false;
            poll(&mut app, &ctx);
        }
        assert_cleared(&app);
        crate::documents::switch(&mut app, 0);
        assert_cleared(&app);
        assert!(app.preferences.agent_models.is_empty(), "an unpolled acknowledgement is discarded with its backend");
    }
}

#[cfg(unix)]
#[test]
fn modern_config_requests_roundtrip_success_rejection_malformed_and_retry() {
    let dir = std::env::temp_dir().join(format!("sc-chat-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("requests.jsonl");
    let script = r#"
log="$1"; n=0
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentInfo":{"name":"fake"}}}\n' "$id" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1","configOptions":[{"id":"vendor.model-key","name":"Model","category":"model","type":"select","currentValue":"fast","options":[{"value":"fast","name":"Fast"},{"value":"smart","name":"Smart"}]}]}}\n' "$id" ;;
    *'"method":"session/set_config_option"'*)
      n=$((n+1))
      case "$n" in
        1|4) v=smart; [ "$n" = 4 ] && v=fast; printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"id":"vendor.model-key","name":"Model","category":"model","type":"select","currentValue":"%s","options":[{"value":"fast","name":"Fast"},{"value":"smart","name":"Smart"}]}]}}\n' "$id" "$v" ;;
        2) printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32001,"message":"rejected"}}\n' "$id" ;;
        3) printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"type":"select"}]}}\n' "$id" ;;
      esac ;;
  esac
done
"#;
    let argv = vec!["sh".into(), "-c".into(), script.into(), "fake".into(), log.to_string_lossy().into_owned()];
    let (wake_tx, wake_rx) = channel();
    let start = backend::StartContext {
        cwd: dir.clone(),
        bridge: None,
        tools: None,
        wake: Box::new(move || {
            let _ = wake_tx.send(());
        }),
    };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.running_id = "modern".into();
    app.agent_chat.state = Some(State::Starting);
    let ctx = crate::fonts::test_context();
    let until = |app: &mut SolveApp, done: &dyn Fn(&SolveApp) -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            poll(app, &ctx);
            if done(app) {
                return;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!left.is_zero(), "timed out; status={}", status(app));
            wake_rx.recv_timeout(left).expect("backend wake");
        }
    };
    until(&mut app, &|a| a.agent_chat.state() == State::Ready);
    assert_eq!(app.agent_chat.model, "fast");
    for (value, expected, saved) in [("smart", "smart", "smart"), ("fast", "smart", "smart"), ("smart", "smart", "smart"), ("fast", "fast", "fast")] {
        set_model(&mut app, value);
        assert!(app.agent_chat.config_pending);
        until(&mut app, &|a| !a.agent_chat.config_pending);
        assert_eq!(app.agent_chat.model, expected);
        assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some(saved));
    }
    disconnect(&mut app);
    let requests: Vec<Value> = std::fs::read_to_string(&log).unwrap().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let configs: Vec<_> = requests.iter().filter(|v| v["method"] == "session/set_config_option").collect();
    assert_eq!(configs.len(), 4);
    assert_eq!(
        configs.iter().map(|v| v["params"].clone()).collect::<Vec<_>>(),
        ["smart", "fast", "smart", "fast"].map(|value| json!({"sessionId":"s1","configId":"vendor.model-key","value":value}))
    );
    assert!(!requests.iter().any(|v| v["method"] == "session/set_model"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modern_controls_route_by_category_and_wait_for_acknowledgement() {
    let (mut app, script, _) = app_with_script();
    app.agent_chat.running_id = "modern".into();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    set_model(&mut app, "smart");
    assert!(app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "fast");
    assert!(!app.preferences.agent_models.contains_key("modern"));
    assert_eq!(script.calls(), ["set_config_option: some.provider.model=smart"]);
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    assert!(app.agent_chat.config_pending, "a notification does not acknowledge the request");
    app.agent_chat.on_event(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    assert!(!app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "smart");
    app.preferences.agent_models.insert("modern".into(), app.agent_chat.model.clone());
    set_mode(&mut app, "ask");
    assert_eq!(script.calls().last().map(String::as_str), Some("set_config_option: custom-mode-key=ask"));
}

#[test]
fn modern_rejection_and_malformed_success_unlock_without_changing_confirmed_value() {
    let (mut app, script, _) = app_with_script();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("fast")));
    set_model(&mut app, "smart");
    app.agent_chat.on_event(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Err("rejected".into()) });
    assert!(!app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "fast");
    set_model(&mut app, "smart");
    app.agent_chat.on_event(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Err("malformed snapshot".into()) });
    assert!(!app.agent_chat.config_pending);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(script.calls().len(), 2);
}

#[test]
fn modern_saved_model_restore_waits_for_ack_and_manual_confirmation_persists() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "modern".into();
    app.preferences.agent_models.insert("modern".into(), "smart".into());
    script.report(Event::ConfigOptions(modern_options("fast")));
    script.report(Event::Ready { agent: "modern".into(), images: false });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("smart"));
    assert_eq!(script.calls(), ["set_config_option: some.provider.model=smart"]);
    script.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.model, "smart");
    assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("smart"));
}

#[test]
fn modern_model_restore_then_mode_restore_are_acknowledged_before_queued_prompt() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "modern".into();
    app.preferences.agent_models.insert("modern".into(), "smart".into());
    app.agent_chat.restore_mode = Some("plan".into());
    app.agent_chat.queued = Some(("describe the model".into(), false, None, Vec::new()));
    script.report(Event::ConfigOptions(modern_options("fast")));
    script.report(Event::Ready { agent: "modern".into(), images: false });
    poll(&mut app, &ctx);
    assert_eq!(script.calls(), ["set_config_option: some.provider.model=smart"]);
    assert_eq!(app.agent_chat.model, "fast");
    assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("smart"));
    script.report(Event::ConfigOptionFinished { id: "some.provider.model".into(), result: Ok(modern_options("smart")) });
    poll(&mut app, &ctx);
    assert_eq!(script.calls().get(1).map(String::as_str), Some("set_config_option: custom-mode-key=plan"));
    assert!(app.agent_chat.queued.is_some(), "prompt waits for mode confirmation");
    let mut confirmed = modern_options("smart");
    confirmed[1].current_value = "plan".into();
    script.report(Event::ConfigOptionFinished { id: "custom-mode-key".into(), result: Ok(confirmed) });
    poll(&mut app, &ctx);
    assert!(script.calls().iter().any(|s| s == "prompt: describe the model"));
    assert_eq!(app.preferences.agent_models.get("modern").map(String::as_str), Some("smart"));
}

#[test]
fn invalidated_modern_sessions_do_not_expose_stale_legacy_choices() {
    for event in [Event::AuthRequired { methods: vec![] }, Event::Exited { tail: vec![] }] {
        let (mut app, _, _) = app_with_script();
        app.agent_chat.on_event(Event::ConfigOptions(modern_options("smart")));
        app.agent_chat.config_pending = true;
        app.agent_chat.on_event(event);
        assert!(app.agent_chat.config_options.is_none() && !app.agent_chat.config_pending);
        assert!(app.agent_chat.models.is_empty() && app.agent_chat.modes.is_empty());
        assert!(app.agent_chat.model.is_empty() && app.agent_chat.mode.is_empty());
    }
    let (mut app, _, _) = app_with_script();
    app.agent_chat.on_event(Event::ConfigOptions(modern_options("smart")));
    new_chat(&mut app);
    assert_eq!(app.agent_chat.restore_mode.as_deref(), Some("ask"));
    assert!(app.agent_chat.config_options.is_none() && app.agent_chat.models.is_empty());
}

/// An agent that reports no models gets no picker; another agent's choice is not applied to it.
#[test]
fn an_agent_without_models_shows_no_picker() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.running_id = "other".into();
    app.preferences.agent_models.insert("claude".into(), "smart".into());
    script.report(Event::Ready { agent: "fake".into(), images: false });
    poll(&mut app, &ctx);
    assert!(!models_shown(&app));
    assert!(script.calls().is_empty());
    assert_eq!(status(&app)["models"], json!([]));
}

/// With a real ACP agent the choice goes out as `session/set_model` with the session and model.
#[cfg(unix)]
#[test]
fn set_model_goes_to_an_acp_agent() {
    let dir = std::env::temp_dir().join(format!("sc-agent-chat-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("received.log");
    let script = r#"
log="$1"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1","models":{"currentModelId":"fast","availableModels":[{"modelId":"fast","name":"Fast"},{"modelId":"smart","name":"Smart"}]}}}\n' "$id" ;;
    *'"method":"session/set_model"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id" ;;
  esac
done
"#;
    let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string(), "fake-agent".to_string(), log.to_string_lossy().into_owned()];
    let start = backend::StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.state = Some(State::Starting);
    app.agent_chat.running_id = "fake".into();
    app.preferences.agent_models.insert("fake".into(), "smart".into());
    let ctx = crate::fonts::test_context();
    let t = std::time::Instant::now();
    loop {
        poll(&mut app, &ctx);
        let logged = std::fs::read_to_string(&log).unwrap_or_default();
        if logged.contains("session/set_model") {
            break;
        }
        assert!(t.elapsed() < std::time::Duration::from_secs(20), "no set_model: {logged}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(models_shown(&app) && app.agent_chat.model == "smart", "the remembered model was put back");
    let logged = std::fs::read_to_string(&log).unwrap();
    let set: Value = logged.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["method"] == "session/set_model").unwrap();
    assert_eq!(set["params"], json!({"sessionId": "s1", "modelId": "smart"}));
    // A choice made in the picker goes out too.
    set_model(&mut app, "fast");
    let t = std::time::Instant::now();
    while std::fs::read_to_string(&log).unwrap_or_default().matches("session/set_model").count() < 2 {
        assert!(t.elapsed() < std::time::Duration::from_secs(20));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    disconnect(&mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An agent that lists `models` but does not serve the unstable `session/set_model` answers
/// "method not found": the choice stays and no error shows (#9970). Other errors still show, as
/// the refusal of the model they were about.
#[cfg(unix)]
#[test]
fn set_model_method_not_found_shows_no_error() {
    let dir = std::env::temp_dir().join(format!("sc-agent-chat-model-unserved-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("received.log");
    let script = r#"
log="$1"; n=0
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"agentInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1","models":{"currentModelId":"fast","availableModels":[{"modelId":"fast","name":"Fast"},{"modelId":"smart","name":"Smart"}]}}}\n' "$id" ;;
    *'"method":"session/set_model"'*)
      n=$((n+1))
      if [ "$n" = 1 ]; then printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"\\"Method not found\\": session/set_model"}}\n' "$id"
      else printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32001,"message":"model unavailable"}}\n' "$id"; fi ;;
  esac
done
"#;
    let argv = vec!["sh".to_string(), "-c".to_string(), script.to_string(), "fake-agent".to_string(), log.to_string_lossy().into_owned()];
    let start = backend::StartContext { cwd: dir.clone(), bridge: None, tools: None, wake: Box::new(|| {}) };
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.backend = Some(Box::new(acp_client::AcpBackend::start(&argv, start).unwrap()));
    app.agent_chat.state = Some(State::Starting);
    app.agent_chat.running_id = "fake".into();
    let ctx = crate::fonts::test_context();
    let errors = |app: &SolveApp| -> Vec<String> {
        app.agent_chat.items.iter().filter_map(|i| if let Item::Notice { text, error: true } = i { Some(text.clone()) } else { None }).collect()
    };
    let t = std::time::Instant::now();
    while !(app.agent_chat.state() == State::Ready && models_shown(&app)) {
        poll(&mut app, &ctx);
        assert!(t.elapsed() < std::time::Duration::from_secs(20), "agent not ready: {}", status(&app));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    set_model(&mut app, "smart");
    set_model(&mut app, "fast");
    // Replies come in order: once the second one's error shows, the first reply was handled too.
    let t = std::time::Instant::now();
    let refused = trf("ui.agent_chat_model_rejected", &[("model", "fast"), ("error", "model unavailable")]);
    while !errors(&app).contains(&refused) {
        poll(&mut app, &ctx);
        assert!(t.elapsed() < std::time::Duration::from_secs(20), "no error for the second choice: {:?}", errors(&app));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(errors(&app), [refused], "method not found is no error to show");
    assert!(app.agent_chat.state().running(), "the chat goes on");
    assert_eq!(std::fs::read_to_string(&log).unwrap().matches("session/set_model").count(), 2);
    disconnect(&mut app);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- Files attached to a message ----

/// A folder of files to attach: a picture and a text file.
fn attachable(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("solidcraft-chat-files-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    image::RgbaImage::from_pixel(40, 30, image::Rgba([250, 250, 250, 255])).save(d.join("target.png")).unwrap();
    std::fs::write(d.join("brief.md"), "wall 2 mm, Pi 5 inside\n").unwrap();
    d
}

fn at(d: &std::path::Path, f: &str) -> String {
    d.join(f).to_string_lossy().to_string()
}

/// Wait for the files read off the window thread to join the message.
fn settle_files(app: &mut SolveApp, ctx: &egui::Context) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !app.agent_chat.loading.is_empty() && std::time::Instant::now() < deadline {
        take_loaded(app, ctx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    take_loaded(app, ctx);
}

/// The same with the whole app running frames.
fn settle_chat(h: &mut crate::scenario::Harness) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !h.app.agent_chat.loading.is_empty() && std::time::Instant::now() < deadline {
        h.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.frames(2);
}

fn text_of(parts: &[PromptPart]) -> String {
    parts.iter().filter_map(|p| if let PromptPart::Text(t) = p { Some(t.clone()) } else { None }).collect()
}

fn pictures_in(parts: &[PromptPart]) -> usize {
    parts.iter().filter(|p| matches!(p, PromptPart::Image { .. })).count()
}

/// Files attached to a message go with it: the pictures as images, the text files in its text,
/// and every file by name; the message lists them in the conversation.
#[test]
fn a_message_carries_its_files() {
    let d = attachable("carry");
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.images = true;
    control(&mut app, &ctx, &json!({"attach": at(&d, "target.png")})).unwrap();
    control(&mut app, &ctx, &json!({"attach": at(&d, "brief.md")})).unwrap();
    settle_files(&mut app, &ctx);
    assert_eq!(status(&app)["attachments"], json!(["target.png", "brief.md"]));
    assert!(control(&mut app, &ctx, &json!({"attach": at(&d, "missing.png")})).is_err());
    send(&mut app, &ctx, "make the case like this".into());
    let sent = script.prompts();
    assert_eq!(sent.len(), 1);
    let text = text_of(&sent[0]);
    assert!(text.starts_with("make the case like this"), "{text}");
    assert!(text.contains("Attached pictures: target.png.") && text.contains("wall 2 mm, Pi 5 inside"), "{text}");
    assert_eq!(sent[0].iter().filter(|p| matches!(p, PromptPart::Image { mime, .. } if mime == "image/png")).count(), 1);
    assert!(app.agent_chat.attachments.is_empty(), "the files went with the message");
    // The picture shows as a thumbnail with the viewer (its name is the note); the text file by name.
    assert_eq!(status(&app)["items"][0]["files"], json!(["brief.md"]));
    assert_eq!(status(&app)["items"][0]["pictures"], 1);
    assert!(matches!(&app.agent_chat.items[0], Item::User { pictures, .. } if pictures.first().is_some_and(|p| p.note == "target.png" && !p.view)));
    let _ = std::fs::remove_dir_all(&d);
}

/// Review of #1744: a picture was decoded on the window thread, so a large one froze the window.
/// It is read off it now: the message waits for it (Send waits too), the files keep the order they
/// were attached in, and a refused picture says why later, in the app's language.
#[test]
fn pictures_are_read_off_the_window_thread() {
    let d = attachable("worker");
    std::fs::write(d.join("fake.png"), b"not a picture at all").unwrap();
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.images = true;
    control(&mut app, &ctx, &json!({"attach": at(&d, "target.png")})).unwrap();
    control(&mut app, &ctx, &json!({"attach": at(&d, "brief.md")})).unwrap();
    assert_eq!(status(&app)["loading"], json!(["target.png"]), "the picture is still being read");
    send(&mut app, &ctx, "like this".into());
    assert!(script.prompts().is_empty(), "Send waits for the files");
    assert_eq!(app.agent_chat.input, "like this", "and keeps the words");
    settle_files(&mut app, &ctx);
    assert_eq!(status(&app)["attachments"], json!(["target.png", "brief.md"]), "in the order they were attached");
    let words = std::mem::take(&mut app.agent_chat.input);
    send(&mut app, &ctx, words);
    assert_eq!(script.prompts().len(), 1);
    {
        let _ja = crate::i18n::scope("ja");
        control(&mut app, &ctx, &json!({"attach": at(&d, "fake.png")})).unwrap();
    }
    settle_files(&mut app, &ctx);
    assert!(
        matches!(app.agent_chat.items.last(), Some(Item::Notice { text, error: true }) if text == "fake.png: 読み込めない画像です"),
        "a refused picture says why, in the language it was attached in"
    );
    assert!(app.agent_chat.attachments.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// Review of #1744: the prompt cap applied to the typed text only, so eight 256 KB text files
/// took a message to about 2 MB. The whole message stays within it now, every file still named.
#[test]
fn a_message_with_large_text_files_stays_within_the_prompt_cap() {
    let d = attachable("cap");
    let (mut app, script, ctx) = app_with_script();
    for i in 0..attach::MAX_FILES {
        let path = d.join(format!("notes{i}.txt"));
        std::fs::write(&path, "y".repeat(250 * 1024)).unwrap();
        control(&mut app, &ctx, &json!({"attach": path.to_string_lossy()})).unwrap();
    }
    send(&mut app, &ctx, "read these".into());
    let text = text_of(&script.prompts()[0]);
    assert!(text.chars().count() <= MAX_PROMPT, "{} characters", text.chars().count());
    assert!((0..attach::MAX_FILES).all(|i| text.contains(&format!("`notes{i}.txt`"))), "every file is named");
    let _ = std::fs::remove_dir_all(&d);
}

/// A message sent before the agent is ready keeps its files until it goes; an agent that takes
/// no pictures gets none, and the message says which were left out. Files alone make a message,
/// and its undo step is named after the first file.
#[test]
fn files_wait_with_their_message_and_pictures_skip_an_agent_that_takes_none() {
    let d = attachable("wait");
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.state = Some(State::Starting);
    attach_path(&mut app, &at(&d, "target.png")).unwrap();
    settle_files(&mut app, &ctx);
    send(&mut app, &ctx, "  ".into());
    assert!(script.prompts().is_empty(), "not sent before the agent is ready");
    assert_eq!(status(&app)["items"][0]["user"], "", "no words, just the file");
    script.report(Event::Ready { agent: "Test".into(), images: false });
    poll(&mut app, &ctx);
    let sent = script.prompts();
    assert_eq!(sent.len(), 1, "sent once the agent is ready");
    assert_eq!(pictures_in(&sent[0]), 0, "no pictures to an agent that takes none");
    assert!(text_of(&sent[0]).contains("not sent"), "{}", text_of(&sent[0]));
    assert_eq!(app.agent_chat.turn.as_ref().map(|t| t.label.as_str()), Some("target.png"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn at_most_eight_files_on_a_message() {
    let d = attachable("eight");
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    for _ in 0..attach::MAX_FILES {
        attach_path(&mut app, &at(&d, "brief.md")).unwrap();
    }
    assert!(attach_path(&mut app, &at(&d, "brief.md")).unwrap_err().contains('8'));
    assert!(attach_rgba(&mut app, 2, 2, &[9; 16]).is_err());
    assert_eq!(app.agent_chat.attachments.len(), attach::MAX_FILES);
    let ctx = egui::Context::default();
    assert!(control(&mut app, &ctx, &json!({"remove_file": 8})).is_err(), "no ninth file to take off");
    control(&mut app, &ctx, &json!({"remove_file": 0})).unwrap();
    assert_eq!(app.agent_chat.attachments.len(), attach::MAX_FILES - 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// While the agent works, a message queues with its files: the queue shows them, Stop keeps
/// them, and they go with the queue.
#[test]
fn a_queued_message_keeps_its_files_through_stop_and_sends_them() {
    let d = attachable("queue");
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.images = true;
    send(&mut app, &ctx, "make a box".into());
    attach_path(&mut app, &at(&d, "target.png")).unwrap();
    settle_files(&mut app, &ctx);
    send(&mut app, &ctx, "like this".into());
    attach_path(&mut app, &at(&d, "brief.md")).unwrap();
    send(&mut app, &ctx, String::new());
    assert!(app.agent_chat.attachments.is_empty(), "the files moved to the queue");
    assert_eq!(status(&app)["pending"][0]["files"], json!(["target.png"]));
    assert_eq!(status(&app)["pending"][1]["files"], json!(["brief.md"]));
    cancel(&mut app);
    ended(&script, "cancelled");
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.pending.len(), 2, "Stop keeps the queue and its files");
    send_now(&mut app, &ctx);
    let sent = script.prompts();
    let last = sent.last().expect("the queue went");
    let text = text_of(last);
    assert!(text.starts_with("like this") && !text.contains("\n\n\n\n"), "a message of files alone adds no empty paragraph: {text:?}");
    assert!(text.contains("Attached pictures: target.png.") && text.contains("wall 2 mm"), "{text}");
    assert_eq!(pictures_in(last), 1);
    let sent = status(&app)["items"].as_array().and_then(|i| i.last()).cloned().unwrap_or_default();
    assert_eq!((sent["files"].clone(), sent["pictures"].clone()), (json!(["brief.md"]), json!(1)));
    let _ = std::fs::remove_dir_all(&d);
}

/// Queued messages go as one prompt with at most eight files; the rest are named, not sent.
#[test]
fn the_queue_sends_at_most_eight_files_and_names_the_rest() {
    let d = attachable("queue-cap");
    let (mut app, script, ctx) = app_with_script();
    send(&mut app, &ctx, "first".into());
    for n in 0..3 {
        for _ in 0..4 {
            attach_path(&mut app, &at(&d, "brief.md")).unwrap();
        }
        send(&mut app, &ctx, format!("batch {n}"));
    }
    ended(&script, "");
    poll(&mut app, &ctx);
    let sent = script.prompts();
    assert_eq!(sent.len(), 2);
    assert_eq!(text_of(&sent[1]).matches("Attached file `brief.md`").count(), attach::MAX_FILES);
    let told = app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, .. } if text.contains("brief.md") && text.contains('8')));
    assert!(told, "the files left out are named");
    let _ = std::fs::remove_dir_all(&d);
}

/// New chat clears the files waiting on the chips too, and asks first when there are some.
#[test]
fn new_chat_clears_the_files_waiting_and_asks_first() {
    let d = attachable("new");
    let (mut app, _script, _ctx) = app_with_script();
    assert!(!new_chat_needs_confirm(&app));
    attach_path(&mut app, &at(&d, "brief.md")).unwrap();
    assert!(new_chat_needs_confirm(&app), "files would be lost");
    new_chat(&mut app);
    assert!(app.agent_chat.attachments.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// A harness with the chat open at its narrowest and nothing to look for.
fn narrow_chat() -> crate::scenario::Harness {
    let mut h = crate::scenario::Harness::new();
    h.app.preferences.agent_chat_width = panel::MIN_WIDTH;
    h.app.agent_chat.open = true;
    h.app.agent_chat.detection = Some(Default::default());
    h
}

/// #10172: before a connection, what the panel draws under the agent switcher (the Agent row
/// with Add agent and Refresh) keeps the panel's side margin, docked and floating: nothing starts
/// at the panel's edge or is cut off at the other one.
#[test]
fn the_agent_list_keeps_the_panels_side_margin() {
    use crate::shell::LayoutMode;
    for layout in [LayoutMode::Docked, LayoutMode::Floating] {
        let mut h = narrow_chat();
        h.app.ui.layout = layout;
        h.frames(3);
        h.frame();
        let panel = crate::scenario::rect_of("agent_chat:panel").expect("the panel");
        let mut texts = Vec::new();
        for clipped in &h.shapes {
            crate::test_support::each_shape(&clipped.shape, &mut |s| {
                if let egui::Shape::Text(t) = s {
                    texts.push((t.galley.text().to_string(), egui::Rect::from_min_size(t.pos, t.galley.rect.size())));
                }
            });
        }
        let at = |label: &str| texts.iter().find(|(text, r)| text == label && panel.contains(r.center())).map(|(_, r)| *r);
        let margin = crate::theme::Tokens::get().space_12 - 0.5;
        let agent = at(tr("ui.agent_chat_pick")).expect("the Agent row");
        assert!(agent.left() >= panel.left() + margin, "{layout:?}: Agent at {agent:?} in {panel:?}");
        let refresh = at(tr("ui.agent_chat_refresh")).expect("Refresh");
        assert!(refresh.right() <= panel.right() - margin, "{layout:?}: Refresh at {refresh:?} in {panel:?}");
    }
}

/// Every chip and the text box sit inside the panel.
fn assert_inside_the_panel(chips: usize) {
    use crate::scenario::rect_of;
    let panel = rect_of("agent_chat:panel").expect("the panel");
    let mut rects: Vec<(String, egui::Rect)> =
        (0..chips).map(|i| (format!("file:{i}"), rect_of(&format!("agent_chat:file:{i}")).expect("a chip"))).collect();
    rects.push(("input".into(), rect_of("agent_chat:input").expect("the text box")));
    for (name, r) in rects {
        assert!(r.left() >= panel.left() && r.right() <= panel.right(), "{name} {r:?} in {panel:?}");
    }
}

/// Files for the next message show as chips above the text box (a picture's thumbnail, the name,
/// a × that takes it off), inside the chat, and Send sends files with no text.
#[test]
fn attached_files_show_as_chips_that_can_be_taken_off_and_sent_alone() {
    use crate::scenario::handle_at;
    let d = attachable("chips");
    let mut h = narrow_chat();
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    h.app.agent_chat.images = true;
    h.frames(2);
    h.call("ui.agent_chat", json!({"attach": at(&d, "target.png")}));
    h.call("ui.agent_chat", json!({"attach": at(&d, "brief.md")}));
    settle_chat(&mut h);
    assert!(handle_at("agent_chat:file:0").is_some() && handle_at("agent_chat:file:1").is_some(), "a chip per file");
    assert_inside_the_panel(2);
    let x = handle_at("agent_chat:file_remove:0").expect("a × on the chip");
    h.call("ui.click", json!({"x": x.x, "y": x.y}));
    h.frames(2);
    assert_eq!(status(&h.app)["attachments"], json!(["brief.md"]));
    let send = handle_at("agent_chat:send").expect("Send");
    h.call("ui.click", json!({"x": send.x, "y": send.y}));
    h.frames(2);
    assert_eq!(script.prompts().len(), 1, "files alone make a message");
    assert!(h.app.agent_chat.attachments.is_empty() && handle_at("agent_chat:file:0").is_none());
    let _ = std::fs::remove_dir_all(&d);
}

/// Review of #1744: chips' thumbnails were cached by the picture's address, so a new picture at a
/// freed address could show an old one. Each file has an id of its own, never reused, and the
/// cache follows the files on the message.
#[test]
fn each_file_has_its_own_id_and_thumbnail() {
    let d = attachable("ids");
    image::RgbaImage::from_pixel(20, 20, image::Rgba([10, 200, 10, 255])).save(d.join("green.png")).unwrap();
    let mut h = narrow_chat();
    h.frames(2);
    h.call("ui.agent_chat", json!({"attach": at(&d, "target.png")}));
    h.call("ui.agent_chat", json!({"attach": at(&d, "green.png")}));
    settle_chat(&mut h);
    let first: Vec<u64> = h.app.agent_chat.attachments.iter().map(|f| f.id).collect();
    assert!(first.len() == 2 && first[0] != first[1], "{first:?}");
    let mut cached: Vec<u64> = h.app.agent_chat.thumbs.keys().copied().collect();
    cached.sort_unstable();
    assert_eq!(cached, first, "a thumbnail per file, by its id");
    h.call("ui.agent_chat", json!({"remove_file": 0}));
    h.call("ui.agent_chat", json!({"attach": at(&d, "target.png")}));
    settle_chat(&mut h);
    let now: Vec<u64> = h.app.agent_chat.attachments.iter().map(|f| f.id).collect();
    assert!(!now.contains(&first[0]), "a removed file's id is not used again: {now:?}");
    let mut cached: Vec<u64> = h.app.agent_chat.thumbs.keys().copied().collect();
    cached.sort_unstable();
    let mut want = now.clone();
    want.sort_unstable();
    assert_eq!(cached, want, "the cache follows the files on the message");
    let _ = std::fs::remove_dir_all(&d);
}

/// The + button: "Choose a file…" asks the app's file picker for the types that can be attached
/// (not the design types Open shows) and attaches what it answers; "Paste a picture" attaches
/// the clipboard's picture (here given as RGBA, as the system does).
#[test]
fn the_plus_button_attaches_a_chosen_file_or_a_pasted_picture() {
    use crate::scenario::handle_at;
    let d = attachable("plus");
    let mut h = narrow_chat();
    let chosen = at(&d, "brief.md");
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let seen = asked.clone();
    h.app.services.pick_file = Some(Box::new(move |_label: &str, exts: &[&str]| {
        *seen.borrow_mut() = exts.iter().map(|e| e.to_string()).collect();
        let p = chosen.clone();
        Box::pin(async move { Some(p) })
    }));
    h.frames(2);
    let plus = handle_at("agent_chat:add").expect("a + button");
    h.call("ui.click", json!({"x": plus.x, "y": plus.y}));
    h.frames(2);
    let choose = handle_at("agent_chat:add_file").expect("Choose a file… in its menu");
    assert!(handle_at("agent_chat:paste_picture").is_some(), "Paste a picture in its menu");
    h.call("ui.click", json!({"x": choose.x, "y": choose.y}));
    h.frames(4);
    assert_eq!(status(&h.app)["attachments"], json!(["brief.md"]));
    let exts = asked.borrow().clone();
    for ext in ["png", "jpg", "gif", "webp", "svg", "md", "step", "stl"] {
        assert!(exts.iter().any(|e| e == ext), "{ext} in {exts:?}");
    }
    assert!(!exts.iter().any(|e| e == "solidcraft"));
    attach_rgba(&mut h.app, 4, 3, &[200; 48]).unwrap();
    attach_rgba(&mut h.app, 4, 3, &[200; 47]).unwrap();
    settle_chat(&mut h);
    assert!(
        h.app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, error: true } if text.starts_with("pasted picture 2.png:"))),
        "not a picture: it says so once it has been read"
    );
    assert_eq!(status(&h.app)["attachments"], json!(["brief.md", "pasted picture 1.png"]));
    let _ = std::fs::remove_dir_all(&d);
}

#[derive(Debug)]
struct DroppedPath(std::path::PathBuf);

impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_files(h: &mut crate::scenario::Harness, at: egui::Pos2, files: &[std::path::PathBuf]) {
    h.drop_files(at, files.iter().map(|p| Arc::new(DroppedPath(p.clone())) as egui::DroppedFileHandle).collect());
}

/// A tetrahedron, as a small ASCII STL.
const TETRA_STL: &str = "solid t
facet normal 0 0 -1
 outer loop
  vertex 0 0 0
  vertex 0 10 0
  vertex 10 0 0
 endloop
endfacet
facet normal 0 -1 0
 outer loop
  vertex 0 0 0
  vertex 10 0 0
  vertex 0 0 10
 endloop
endfacet
facet normal -1 0 0
 outer loop
  vertex 0 0 0
  vertex 0 0 10
  vertex 0 10 0
 endloop
endfacet
facet normal 1 1 1
 outer loop
  vertex 10 0 0
  vertex 0 10 0
  vertex 0 0 10
 endloop
endfacet
endsolid t
";

/// Review of #1744: a CAD file's import wasn't waited on, so a failed or queued-out import still
/// told the agent "I added x to the design". The file joins the message once its import is in;
/// a failed import says why and the agent is told nothing of it.
#[test]
fn a_cad_file_joins_the_message_once_its_import_is_in() {
    let d = attachable("cad");
    std::fs::write(d.join("block.stl"), TETRA_STL).unwrap();
    std::fs::write(d.join("broken.stl"), [0u8, 1, 2]).unwrap();
    let mut h = narrow_chat();
    let script = Script::default();
    h.app.agent_chat.backend = Some(Box::new(script.clone()));
    h.app.agent_chat.state = Some(State::Ready);
    h.frames(2);
    attach_path(&mut h.app, &at(&d, "block.stl")).unwrap();
    assert_eq!(status(&h.app)["loading"], json!(["block.stl"]), "importing, not yet on the message");
    assert_eq!(status(&h.app)["attachments"], json!([]));
    settle_chat(&mut h);
    assert_eq!(status(&h.app)["attachments"], json!(["block.stl"]), "in, and on the message");
    h.call("ui.agent_chat", json!({"attach": at(&d, "broken.stl")}));
    settle_chat(&mut h);
    assert!(
        h.app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, error: true } if text.starts_with("broken.stl:"))),
        "a failed import says why"
    );
    assert_eq!(status(&h.app)["attachments"], json!(["block.stl"]), "and is not on the message");
    h.call("ui.agent_chat", json!({"send": "fit around this"}));
    h.frames(2);
    let text = text_of(&script.prompts()[0]);
    assert!(text.contains("`block.stl` to the design") && !text.contains("broken.stl"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Review of #1744: taking an imported part's chip off leaves the part in the design, and its ×
/// says so; any other file's × just takes it off.
#[test]
fn an_imported_parts_chip_says_the_part_stays() {
    assert_eq!(panel::remove_tip(&attach::Attachment::imported("motor.step".into())), tr("ui.agent_chat_remove_imported"));
    assert!(tr("ui.agent_chat_remove_imported").contains("stays in the design"));
    let picture = attach::from_rgba("a.png".into(), 2, 2, &[9; 16]).unwrap();
    assert_eq!(panel::remove_tip(&picture), tr("ui.agent_chat_remove_file"));
}

/// Files dropped on the chat attach to the next message, and the app doesn't open them as well
/// (while they are dragged over it, the chat says so); dropped on the 3D view they are not the
/// chat's (the app handles them as before), nor when the chat is closed.
#[test]
fn files_dropped_on_the_chat_attach_and_the_app_does_not_open_them() {
    use crate::scenario::rect_of;
    let d = attachable("drop");
    let mut h = narrow_chat();
    h.call("ui.resize", json!({"width": 1600, "height": 1000}));
    h.frames(3);
    let panel = rect_of("agent_chat:panel").expect("docked");
    // While files are dragged over the chat, it says they will attach; over the 3D view, not.
    let hovered = || vec![egui::HoveredFile { path: Some(d.join("target.png")), mime: String::new() }];
    h.drag_files(panel.center(), hovered(), Vec::new());
    assert!(rect_of("agent_chat:drop_hint").is_some_and(|r| panel.contains(r.center())), "the hint over the chat");
    h.drag_files(egui::pos2(300.0, 500.0), hovered(), Vec::new());
    assert!(rect_of("agent_chat:drop_hint").is_none(), "no hint over the 3D view");
    drop_files(&mut h, panel.center(), &[d.join("target.png"), d.join("brief.md")]);
    settle_chat(&mut h);
    assert_eq!(status(&h.app)["attachments"], json!(["target.png", "brief.md"]));
    assert!(!h.app.file_load.busy(), "the app doesn't open them as well");
    drop_files(&mut h, egui::pos2(300.0, 500.0), &[d.join("brief.md")]);
    assert_eq!(status(&h.app)["attachments"].as_array().map(Vec::len), Some(2), "over the 3D view: not the chat's");
    // Folded to its strip, a drop on the strip is the chat's too.
    h.call("ui.agent_chat", json!({"minimized": true}));
    h.frames(3);
    let strip = h.app.agent_chat.drop_rect.expect("the strip takes drops");
    assert!(strip.width() < 60.0, "{strip:?}");
    drop_files(&mut h, strip.center(), &[d.join("brief.md")]);
    assert_eq!(status(&h.app)["attachments"].as_array().map(Vec::len), Some(3));
    // Closed: nothing is the chat's, wherever the pointer was.
    h.app.agent_chat.open = false;
    h.frames(2);
    assert!(h.app.agent_chat.drop_rect.is_none());
    drop_files(&mut h, strip.center(), &[d.join("brief.md")]);
    assert_eq!(status(&h.app)["attachments"].as_array().map(Vec::len), Some(3));
    let _ = std::fs::remove_dir_all(&d);
}

/// The chat popped out into its own window takes drops too, and says so while files are dragged
/// over it (#2812). Here the window floats inside the app, as it does where the system can't open
/// one: a drop on the window is the chat's, one elsewhere is not.
#[test]
fn files_dropped_on_the_popped_out_chat_attach() {
    use crate::scenario::rect_of;
    let d = attachable("popdrop");
    let mut h = narrow_chat();
    h.call("ui.resize", json!({"width": 1600, "height": 1000}));
    h.app.preferences.agent_chat_popped = true;
    h.frames(4);
    let window = rect_of("agent_chat:window").expect("popped out");
    let hovered = || vec![egui::HoveredFile { path: Some(d.join("target.png")), mime: String::new() }];
    h.drag_files(window.center(), hovered(), Vec::new());
    assert!(rect_of("agent_chat:drop_hint").is_some_and(|r| window.contains(r.center())), "the hint over the window");
    let outside = egui::pos2(5.0, 500.0);
    assert!(!window.contains(outside));
    h.drag_files(outside, hovered(), Vec::new());
    assert!(rect_of("agent_chat:drop_hint").is_none(), "no hint away from it");
    drop_files(&mut h, window.center(), &[d.join("target.png"), d.join("brief.md")]);
    settle_chat(&mut h);
    assert_eq!(status(&h.app)["attachments"], json!(["target.png", "brief.md"]));
    assert!(!h.app.file_load.busy(), "the app doesn't open them as well");
    drop_files(&mut h, outside, &[d.join("brief.md")]);
    assert_eq!(status(&h.app)["attachments"].as_array().map(Vec::len), Some(2), "away from the window: not the chat's");
    let _ = std::fs::remove_dir_all(&d);
}

/// Many files and a long message in a narrow chat: the chips wrap onto more rows and the text box
/// keeps to the chat (a row of chips wider than the chat would widen everything under it).
#[test]
fn chips_and_a_long_message_wrap_inside_a_narrow_chat() {
    use crate::scenario::handle_at;
    let d = attachable("wrap");
    let mut h = narrow_chat();
    for _ in 0..6 {
        h.call("ui.agent_chat", json!({"attach": at(&d, "target.png")}));
    }
    h.app.agent_chat.input = "a separate design next to the robot body, the main version, with these measures for the second one".into();
    settle_chat(&mut h);
    assert_inside_the_panel(6);
    let (first, second) = (handle_at("agent_chat:file:0").unwrap(), handle_at("agent_chat:file:1").unwrap());
    assert!(second.y > first.y + 10.0, "at 280 px the second chip goes on the next row: {first:?} {second:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// An agent's text is Markdown: a code block shows with its Copy button and a link is a button
/// of its own, instead of the raw markers.
#[test]
fn an_agents_markdown_is_drawn_as_blocks() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    h.app.agent_chat.items.push(Item::Agent {
        thought: false,
        text: "# Result\n\n- a **bold** point\n- another\n\n```\nlet x = 1;\n```\n\nSee [the docs](https://example.com/a).".into(),
    });
    h.frames(3);
    assert!(h.widget("Copy").is_some(), "the code block has its Copy button");
    assert!(h.widget("↗ example.com").is_some(), "the link is a button that names where it goes");
    // A hostile reply draws too (no panic, bounded work).
    h.app.agent_chat.items.push(Item::Agent { thought: false, text: "*".repeat(50_000) + &"[".repeat(5_000) + &"`a".repeat(5_000) });
    h.frames(3);
}

// ---- Attach selection (#2286) ----

/// A 40 x 30 x 20 block, its first face and first edge as the selection would hold them, its body
/// and its feature.
fn box_selection(app: &mut SolveApp) -> Vec<solidcraft_engine::Sel> {
    use solidcraft_engine::Sel;
    app.run("solid.box", json!({"length": 40, "width": 30, "height": 20})).expect("a box");
    let face = app.session.execute("model.faces", &json!({"body": "Body1"})).expect("faces")["faces"][0].clone();
    let edge = app.session.execute("model.edges", &json!({"body": "Body1"})).expect("edges")["edges"][0].clone();
    let at = |v: &Value| solidcraft_engine::geom::Vec3::new(v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0), v[2].as_f64().unwrap_or(0.0));
    vec![
        Sel::Face { body: "Body1".into(), index: face["index"].as_u64().unwrap_or(0) as usize, point: at(&face["point"]) },
        Sel::Edge { body: "Body1".into(), index: edge["index"].as_u64().unwrap_or(0) as usize, point: at(&edge["mid"]) },
        Sel::Feature { id: app.session.doc.features.first().map(|f| f.id).expect("a feature") },
        Sel::Body { name: "Body1".into() },
    ]
}

/// With Attach selection on, the message goes with what is selected: the agent gets it as text
/// after the message, the conversation shows the message as typed with the items as chips.
#[test]
fn an_attached_selection_goes_as_text_for_the_agent_and_as_chips_in_the_conversation() {
    let (mut app, script, ctx) = app_with_script();
    app.session.selection = box_selection(&mut app);
    // Off: the message is just the message.
    send(&mut app, &ctx, "plain".into());
    assert_eq!(script.calls(), ["prompt: plain"]);
    ended(&script, "");
    poll(&mut app, &ctx);

    app.agent_chat.attach_selection = true;
    send(&mut app, &ctx, "round this".into());
    let prompt = script.calls().pop().expect("a prompt");
    assert!(prompt.starts_with("prompt: round this\n\n[Selected in SolidCraft"), "{prompt}");
    for want in ["Elements:", "- Flat face ", "planar", "area ", "- Edge ", "length ", "name ", "Features:", "feature id", "Bodies:", "- Body1"] {
        assert!(prompt.contains(want), "{want:?} missing from {prompt}");
    }
    let Some(Item::User { text, context, .. }) = app.agent_chat.items.last() else { panic!("a sent message") };
    assert_eq!(text, "round this", "the conversation shows what was typed");
    assert_eq!(context.items.len(), 4);
    let kinds: Vec<_> = context.items.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [context::Kind::Element, context::Kind::Element, context::Kind::Feature, context::Kind::Body]);
    // Nothing selected: nothing is added, and the box stays ticked.
    ended(&script, "");
    poll(&mut app, &ctx);
    app.session.selection.clear();
    send(&mut app, &ctx, "nothing".into());
    assert_eq!(script.calls().pop().as_deref(), Some("prompt: nothing"));
}

/// A queued message keeps the selection of the moment it was typed; queued messages go as one
/// prompt with all their items.
#[test]
fn queued_messages_keep_the_selection_they_were_typed_with() {
    use solidcraft_engine::Sel;
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.attach_selection = true;
    send(&mut app, &ctx, "first".into());
    app.session.selection = vec![Sel::Body { name: "Alpha".into() }];
    send(&mut app, &ctx, "second".into());
    app.session.selection = vec![Sel::Body { name: "Beta".into() }];
    send(&mut app, &ctx, "third".into());
    assert_eq!(app.agent_chat.pending.iter().map(|p| p.context.total()).collect::<Vec<_>>(), [1, 1]);
    ended(&script, "");
    poll(&mut app, &ctx);
    let prompt = script.calls().pop().expect("the queue went");
    assert!(prompt.starts_with("prompt: second\n\nthird\n\n[Selected"), "{prompt}");
    assert!(prompt.contains("- Alpha") && prompt.contains("- Beta"), "{prompt}");
    let Some(Item::User { context, .. }) = app.agent_chat.items.last() else { panic!("a sent message") };
    assert_eq!(context.items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
}

/// A large selection is capped and says how much is left out; nothing in it can break the prompt.
#[test]
fn a_huge_selection_is_counted_not_dropped_silently() {
    use solidcraft_engine::Sel;
    let mut session = solidcraft_engine::Session::default();
    let selection: Vec<Sel> = (0..100).map(|i| Sel::Body { name: format!("Body{i}") }).collect();
    let context = context::describe(&mut session, &selection);
    assert_eq!((context.items.len(), context.omitted, context.total()), (context::MAX_ITEMS, 100 - context::MAX_ITEMS, 100));
    assert!(context.prompt_block().contains("and 36 more, not listed"));
    // Two such messages together stay capped.
    let mut both = context.clone();
    both.merge(context);
    assert_eq!((both.items.len(), both.total()), (context::MAX_ITEMS, 200));
    // Unknown bodies, faces and features are described by what is known, without panicking.
    let odd = [
        Sel::Face { body: "Nope".into(), index: usize::MAX, point: solidcraft_engine::geom::Vec3::new(f64::NAN, f64::INFINITY, 0.0) },
        Sel::Edge { body: "Nope".into(), index: 7, point: solidcraft_engine::geom::Vec3::new(0.0, 0.0, 0.0) },
        Sel::Feature { id: u64::MAX },
        Sel::Plane { name: "XY".into() },
        Sel::SketchCurve { id: "c1".into(), sketch: None },
    ];
    let described = context::describe(&mut session, &odd);
    assert_eq!(described.items.len(), 5);
    assert!(described.prompt_block().contains("Other:"));
}

/// The checkbox shows the number selected, and the chips of a sent message are drawn grouped.
#[test]
fn the_selection_checkbox_and_the_chips_are_drawn() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    assert!(h.widget("Attach selection").is_some(), "the checkbox is there, with nothing selected");
    h.app.session.selection = box_selection(&mut h.app);
    h.frames(3);
    assert!(h.widget("Attach selection (4)").is_some(), "it counts what is selected");
    let context = context::describe_selection(&mut h.app.session);
    h.app.agent_chat.items.push(Item::User { text: "look".into(), image: false, pictures: Vec::new(), context, files: Vec::new() });
    h.frames(3);
    for heading in ["Elements 2", "Features 1", "Bodies 1"] {
        assert!(h.widget(heading).is_some(), "{heading} is drawn");
    }
}

#[test]
fn assistant_marks_follow_chat_login_turns_and_approvals() {
    use AssistantMark::*;
    let mark = |state, login, configured| assistant_mark_for(state, login, configured, 0, false, false);
    assert_eq!(mark(State::Off, "", false), Quiet);
    assert_eq!(mark(State::Off, "signed_in", false), Quiet);
    for login in ["", "unknown"] {
        assert_eq!(mark(State::Off, login, true), Quiet);
    }
    for login in ["signed_in", "key"] {
        assert_eq!(mark(State::Off, login, true), Ready);
    }
    assert_eq!(mark(State::Off, "signed_out", true), NeedsAuth);
    for login in ["", "signed_out", "unknown", "signed_in"] {
        for (state, expected) in [
            (State::Ready, Ready),
            (State::Busy, Busy),
            (State::Starting, Busy),
            (State::NeedsAuth, NeedsAuth),
            (State::Exited, Stopped),
            (State::Failed, Stopped),
        ] {
            assert_eq!(mark(state, login, true), expected);
            assert_eq!(assistant_mark_for(state, login, true, 12, false, false), Waiting(12));
        }
    }
    assert_eq!(assistant_mark_for(State::Ready, "signed_in", true, 0, true, false), Stopped);
    assert_eq!(assistant_mark_for(State::Ready, "signed_in", true, 0, false, true), Stopped);
}

/// An app whose endpoint agents answer at once (offline), with `api-1` and `api-2` added.
#[cfg(not(target_arch = "wasm32"))]
fn app_with_endpoints() -> SolveApp {
    let http: backend::HttpFn = Arc::new(|_| Err("offline".into()));
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services { agent_http: Some(http), ..Default::default() });
    for id in ["api-1", "api-2"] {
        app.preferences.agents.push(agents::AgentConfig {
            id: id.into(),
            kind: agents::Kind::Api,
            base_url: "http://localhost:11434/v1".into(),
            model: "m".into(),
            ..Default::default()
        });
    }
    app
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_agent_started_last_connects_at_start_once_the_agents_are_known() {
    let ctx = crate::fonts::test_context();
    let mut app = app_with_endpoints();
    // Off by default: a launch starts nothing and leaves the panel closed.
    begin(&mut app);
    poll(&mut app, &ctx);
    assert!(!app.agent_chat.open && app.agent_chat.backend.is_none() && app.agent_chat.detecting.is_none());

    app.preferences.agent_connect_at_start = true;
    app.preferences.agent_chat_open_at_start = true;
    app.preferences.agent_last = "api-2".into();
    begin(&mut app);
    assert!(app.agent_chat.open);
    // Nothing waits for discovery: the agent starts when it is in, in whichever tab is shown.
    let (found, detecting) = channel();
    app.agent_chat.detecting = Some(detecting);
    poll(&mut app, &ctx);
    assert!(app.agent_chat.backend.is_none());
    crate::documents::open_tab(&mut app, solidcraft_engine::Session::default()).unwrap();
    found.send(discover::Detection::default()).unwrap();
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.agent_id.as_deref(), Some("api-2"), "the agent started last, not the default");
    assert_eq!(app.agent_chat.state(), State::Starting);
    assert!(app.agent_chat.backend.is_some());
    // A design opened over the launch's blank one keeps the agent: nothing was asked yet.
    crate::documents::adopt(&mut app, solidcraft_engine::Session::default());
    assert!(app.agent_chat.backend.is_some() && app.agent_chat.items.is_empty());
    assert_eq!(app.docs.count(), 2);
    // Once per launch.
    disconnect(&mut app);
    poll(&mut app, &ctx);
    assert!(app.agent_chat.backend.is_none());
    // Connecting another agent makes it the one started last.
    app.agent_chat.agent_id = Some("api-1".into());
    connect(&mut app, &ctx);
    assert_eq!(app.preferences.agent_last, "api-1");
    assert!(status(&app)["lastAgent"] == "api-1" && status(&app)["connectAtStart"] == true);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_connection_that_never_gets_ready_says_connection_failed_and_offers_retry() {
    let ctx = crate::fonts::test_context();
    // The agent started last is gone: the chat says so, and no other agent starts by itself.
    let mut app = app_with_endpoints();
    app.preferences.agent_connect_at_start = true;
    app.preferences.agent_last = "claude".into();
    app.agent_chat.detection = Some(discover::Detection::default());
    begin(&mut app);
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Failed);
    assert!(app.agent_chat.backend.is_none() && app.agent_chat.agent_id.is_none());
    assert!(matches!(app.agent_chat.items.last(), Some(Item::Notice { text, error: true }) if text.contains("claude")));
    assert!(!State::Failed.running(), "the Connect button reads Retry, not Stop");
    assert_eq!(status(&app)["state"], "failed");

    // An agent that cannot start.
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.preferences.agents.push(agents::AgentConfig { id: "custom-1".into(), command: "solidcraft-no-such-agent-7009".into(), ..Default::default() });
    connect(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Failed);
    assert!(app.preferences.agent_last.is_empty(), "only an agent that started is remembered");

    // A found agent that failed its version check (#9034): the picker says it doesn't start, and
    // connecting says why and how to install it again, besides the system's own message.
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    let reason = "This version of opencode.exe is not compatible with the version of Windows you're running.";
    app.agent_chat.detection = Some(discover::Detection {
        found: vec![discover::Found {
            id: "opencode",
            name: "OpenCode",
            argv: vec!["solidcraft-no-such-agent-9034".into(), "acp".into()],
            version: None,
            login: discover::Login::Unknown,
            problem: Some(reason.into()),
            install: "npm install -g opencode-ai",
        }],
        ..Default::default()
    });
    let entry = agents::current(&app).unwrap();
    assert!(entry.detail().contains(reason), "{}", entry.detail());
    connect(&mut app, &ctx);
    assert_eq!(app.agent_chat.state(), State::Failed);
    let told = |needle: &str| app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, error: true } if text.contains(needle)));
    assert!(told(reason) && told("npm install -g opencode-ai") && told("OpenCode"), "{:?}", app.agent_chat.items);
    assert_eq!(status(&app)["agents"][0]["problem"], reason);
    // One that answered has nothing to say.
    let mut fine = entry.clone();
    fine.problem = None;
    assert_eq!(fine.problem_hint(), None);

    // No agent at all: the picker opens on what to install.
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.agent_chat.detection = Some(discover::Detection::default());
    connect(&mut app, &ctx);
    assert_eq!((app.agent_chat.state(), app.agent_chat.picking), (State::Failed, Some(true)));

    // An agent that ends while starting failed to connect; one that ends later stopped.
    let mut c = chat();
    c.state = Some(State::Starting);
    c.on_event(Event::Error("initialize failed".into()));
    c.on_event(Event::Exited { tail: Vec::new() });
    assert_eq!(c.state(), State::Failed);
    let mut c = chat();
    c.on_event(Event::Exited { tail: Vec::new() });
    assert_eq!(c.state(), State::Exited);
}

#[test]
fn assistant_click_and_shortcut_toggle_the_same_panel_without_a_connection() {
    let mut h = crate::scenario::Harness::new();
    h.frames(3);
    assert_eq!(assistant_mark(&h.app), AssistantMark::Quiet);
    assert!(crate::scenario::handle_at("toolbar:ai").is_none());
    h.step(&json!({"click":{"handle":"titlebar:assistant"}})).unwrap();
    assert!(h.app.agent_chat.open);
    h.step(&json!({"key":"A", "shift":true,"cmd":true})).unwrap();
    assert!(!h.app.agent_chat.open);
    assert!(h.app.agent_chat.backend.is_none());
}

/// A long message scrolls inside its box: the buttons under the box stay in the window (#2224).
/// Measured against the panel's bottom once the agents on this machine have been looked for:
/// what that finds (and the status it reports) arrives in its own time and may change what is
/// around the panel, which is not what this checks.
#[test]
fn a_long_message_keeps_the_buttons_in_the_window() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.resize", json!({"width": 1600.0, "height": 1000.0}));
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(8);
    let began = std::time::Instant::now();
    while h.app.agent_chat.detecting.is_some() && began.elapsed() < std::time::Duration::from_secs(30) {
        h.frame();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.frames(4);
    let bottom = |h: &crate::scenario::Harness| f64::from(h.app.agent_chat.panel_rect.map(|r| r.bottom()).expect("the panel is drawn"));
    let resting: Vec<_> = ["Send", "Attach view"]
        .into_iter()
        .map(|name| {
            let [_, y] = h.widget(name).unwrap_or_else(|| panic!("{name} is drawn"));
            (name, bottom(&h) - y)
        })
        .collect();
    h.app.agent_chat.input = "Please make the plate thicker and then add a fillet on every outside edge.\n".repeat(60);
    h.frames(8);
    let window = 1000.0;
    for (name, rest) in resting {
        let [_, y] = h.widget(name).unwrap_or_else(|| panic!("{name} is drawn"));
        assert!(y < window, "{name} is at {y}, below the window's bottom at {window}");
        // The box is pinned to the bottom and grows upwards: the buttons stay where they were
        // above the panel's bottom.
        let above = bottom(&h) - y;
        assert!((above - rest).abs() < 1.0, "{name} moved from {rest} to {above} above the panel's bottom");
    }
}

fn test_png(colour: [u8; 4], width: u32) -> Vec<u8> {
    let mut png = Vec::new();
    let image = image::RgbaImage::from_pixel(width, 4, image::Rgba(colour));
    image::DynamicImage::ImageRgba8(image).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).expect("a PNG");
    png
}

/// A message's pictures show as thumbnails; a click opens the viewer on that one, where Next, the
/// arrow keys and Esc work (#2224). A message whose picture is not taken yet keeps a plain caption.
#[test]
fn a_messages_pictures_open_in_a_viewer() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.app.agent_chat.items.push(Item::User {
        text: "waiting".into(),
        image: true,
        pictures: Vec::new(),
        files: Vec::new(),
        context: Context::default(),
    });
    h.frames(3);
    let [x, y] = h.widget("View attached").expect("the plain caption is drawn");
    h.call("ui.click", json!({"x": x, "y": y}));
    h.frames(3);
    assert!(h.app.agent_chat.gallery.is_none(), "no picture yet: nothing opens");

    h.app.agent_chat.items.clear();
    let pictures = vec![Picture::new(test_png([200, 30, 30, 255], 8), "first view"), Picture::new(test_png([30, 30, 200, 255], 16), "")];
    let ours = pictures.clone();
    h.app.agent_chat.items.push(Item::User { text: "look".into(), image: true, pictures, files: Vec::new(), context: Context::default() });
    h.frames(2);
    // Decoded in the background: let both finish before looking for thumbnails.
    for picture in &ours {
        assert!(until_decoded(&mut h, picture), "the picture was decoded");
    }
    assert!(h.widget("View attached").is_none(), "thumbnails replace the caption");
    let point = |name: &str| {
        let p = crate::scenario::handle_at(name).unwrap_or_else(|| panic!("{name} is drawn"));
        json!({"x": p.x, "y": p.y})
    };
    // Hovering is not clicking.
    let second = point("agent_chat:picture:0:1");
    h.call("ui.move", second.clone());
    h.frames(3);
    assert!(h.app.agent_chat.gallery.is_none());
    h.call("ui.click", second);
    h.frames(4);
    assert_eq!(h.app.agent_chat.gallery.as_ref().map(|g| g.at), Some(1), "the click opens the viewer on that picture");
    assert!(crate::scenario::rect_of("agent_chat:gallery_picture").is_some(), "the picture is drawn big");
    // Next wraps round; the arrow keys step too.
    let next = point("agent_chat:gallery_next");
    h.call("ui.click", next);
    h.frames(3);
    assert_eq!(h.app.agent_chat.gallery.as_ref().map(|g| g.at), Some(0));
    h.call("ui.key", json!({"key": "ArrowRight"}));
    h.frames(3);
    assert_eq!(h.app.agent_chat.gallery.as_ref().map(|g| g.at), Some(1));
    let state = h.call("ui.agent_chat", json!({}));
    assert_eq!(state["result"]["items"][0]["pictures"], 2);
    h.call("ui.key", json!({"key": "Escape"}));
    h.frames(3);
    assert!(h.app.agent_chat.gallery.is_none(), "Esc closes the viewer");
    // New chat closes it with the conversation.
    h.app.agent_chat.gallery = Some(Gallery { pictures: vec![Picture::new(test_png([0, 0, 0, 255], 4), "")], at: 0 });
    super::new_chat(&mut h.app);
    assert!(h.app.agent_chat.gallery.is_none());
}

/// A message's picture is kept on exactly the message it was taken for: not an earlier one that
/// also wanted a picture, not one that has it already, and nothing that isn't such a message.
#[test]
fn the_picture_goes_to_the_message_it_was_taken_for() {
    let mut chat = AgentChat::default();
    chat.push(Item::User { text: "no picture".into(), image: false, pictures: Vec::new(), files: Vec::new(), context: Context::default() });
    chat.push(Item::User {
        text: "first".into(),
        image: true,
        pictures: vec![Picture::new(vec![1], "")],
        files: Vec::new(),
        context: Context::default(),
    });
    chat.push(Item::User { text: "second".into(), image: true, pictures: Vec::new(), files: Vec::new(), context: Context::default() });
    chat.push(Item::User { text: "third".into(), image: true, pictures: Vec::new(), files: Vec::new(), context: Context::default() });
    let sizes = |chat: &AgentChat| -> Vec<usize> {
        chat.items
            .iter()
            .filter_map(|i| match i {
                Item::User { pictures, .. } => Some(pictures.iter().map(|p| p.bytes.len()).sum()),
                _ => None,
            })
            .collect()
    };
    // The picture of the third message goes to the third, though the second still waits for one.
    chat.keep_picture(Some(3), vec![2, 3]);
    assert_eq!(sizes(&chat), [0, 1, 0, 2]);
    // The same message again (it was sent again): nothing is added, nor moved to an older message.
    chat.keep_picture(Some(3), vec![9; 5]);
    assert_eq!(sizes(&chat), [0, 1, 0, 2]);
    // A message that did not want one, no message, or a place that is gone.
    chat.keep_picture(Some(0), vec![1]);
    chat.keep_picture(None, vec![1]);
    chat.keep_picture(Some(99), vec![1]);
    assert_eq!(sizes(&chat), [0, 1, 0, 2]);
    chat.keep_picture(Some(2), vec![0; super::MAX_PICTURE_BYTES + 1]);
    assert_eq!(sizes(&chat), [0, 1, 0, 2], "an oversized picture is not kept");
}

/// A message sent again after a sign-in is still the message it was: its turn points at its own
/// place in the conversation, so its picture is kept there, once, and never on an older message that
/// wanted one (#2224 review). The picture itself can't be drawn without a renderer in a test, so the
/// place a turn is started for is what is checked.
#[test]
fn a_message_sent_again_after_sign_in_stays_the_message_it_was() {
    let (mut app, script, ctx) = app_with_script();
    app.agent_chat.images = true;
    // An older message that wanted a picture and has none.
    app.agent_chat.push(Item::User { text: "older".into(), image: true, pictures: Vec::new(), files: Vec::new(), context: Context::default() });
    app.agent_chat.attach_view = true;
    send(&mut app, &ctx, "the new one".into());
    let place = app.agent_chat.items.len() - 1;
    assert_eq!(app.agent_chat.last, Some(("the new one".into(), true, Some(place), Vec::new())), "the turn knows its message");
    // The agent asked for a sign-in; after it the message goes again, as it is done on a sign-in.
    ended(&script, "");
    poll(&mut app, &ctx);
    app.agent_chat.queued = app.agent_chat.last.take();
    script.report(Event::Ready { agent: "fake".into(), images: true });
    poll(&mut app, &ctx);
    assert_eq!(app.agent_chat.last, Some(("the new one".into(), true, Some(place), Vec::new())), "sent again, still for the same message");
    assert_eq!(app.agent_chat.items.iter().filter(|i| matches!(i, Item::User { .. })).count(), 2, "not a new message");
    // Its picture goes there, once; the older message gets none.
    app.agent_chat.keep_picture(Some(place), test_png([1, 2, 3, 255], 4));
    app.agent_chat.keep_picture(Some(place), test_png([1, 2, 3, 255], 4));
    let counts: Vec<usize> =
        app.agent_chat.items.iter().filter_map(|i| if let Item::User { pictures, .. } = i { Some(pictures.len()) } else { None }).collect();
    assert_eq!(counts, [0, 1]);
}

/// The picture viewer opens over the chat's own window too, not only over the docked panel (#1745).
#[test]
fn the_viewer_opens_in_the_popped_out_window() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.app.preferences.agent_chat_popped = true;
    h.app.agent_chat.items.push(Item::User {
        text: "look".into(),
        image: true,
        pictures: vec![Picture::new(test_png([200, 30, 30, 255], 8), "first view")],
        context: Context::default(),
        files: Vec::new(),
    });
    h.frames(2);
    let Item::User { pictures, .. } = &h.app.agent_chat.items[0] else { panic!("the message") };
    let first = pictures[0].clone();
    assert!(until_decoded(&mut h, &first), "the picture was decoded");
    let at = crate::scenario::handle_at("agent_chat:picture:0:0").expect("the thumbnail is drawn in the window");
    h.call("ui.click", json!({"x": at.x, "y": at.y}));
    h.frames(4);
    assert!(h.app.agent_chat.gallery.is_some(), "the click opened the viewer");
    assert!(crate::scenario::rect_of("agent_chat:gallery_picture").is_some(), "and it is drawn");
}

// ---- Pick context (#2286) ----

/// Put `sels` into the open picker's inputs the way clicks would: each in the input that takes it.
fn pick_into_dialog(app: &mut SolveApp, sels: &[solidcraft_engine::Sel]) {
    use solidcraft_engine::Sel;
    let mut d = app.dialog.take().expect("the picker is open");
    for sel in sels {
        d.active = match sel {
            Sel::Face { .. } | Sel::Edge { .. } | Sel::Vertex { .. } => 0,
            Sel::Feature { .. } => 1,
            _ => 2,
        };
        d.pick(&app.session, sel.clone());
    }
    app.dialog = Some(d);
}

/// Pick context opens the picker with Elements, Features and Bodies; OK puts what was picked in the
/// tray above the message box, as chips with an ×; × takes one out; Cancel keeps what was there.
#[test]
fn the_picker_fills_a_tray_of_chips_that_can_be_trimmed() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    let sels = box_selection(&mut h.app);
    h.frames(3);
    let [x, y] = h.widget("Pick context").expect("the button is there");
    h.call("ui.click", json!({"x": x, "y": y}));
    h.frames(3);
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::ChatContext)), "the picker opened");
    assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs.iter().map(|i| i.label).collect::<Vec<_>>()), Some(vec!["Elements", "Features", "Bodies"]));
    pick_into_dialog(&mut h.app, &sels);
    h.frames(3);
    let [x, y] = h.widget("OK").expect("OK is offered");
    h.call("ui.click", json!({"x": x, "y": y}));
    h.frames(4);
    assert!(h.app.dialog.is_none(), "OK closed the picker");
    assert_eq!(h.app.agent_chat.picked.len(), 4);
    assert_eq!(h.app.agent_chat.picked_context.items.len(), 4);
    for heading in ["Elements 2", "Features 1", "Bodies 1"] {
        assert!(h.widget(heading).is_some(), "{heading} is drawn in the tray");
    }
    // × takes the body out: the tray and the picks stay in step.
    let cross = crate::scenario::handle_at("agent_chat:picked_remove:3").expect("every chip has an ×");
    h.call("ui.click", json!({"x": cross.x, "y": cross.y}));
    h.frames(4);
    assert_eq!(h.app.agent_chat.picked.len(), 3);
    assert!(h.app.agent_chat.picked_context.items.iter().all(|i| i.kind != context::Kind::Body));
    // Reopened, the picker holds what is in the tray; Cancel leaves it alone.
    start_pick(&mut h.app);
    assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs.iter().map(|i| i.items.len()).collect::<Vec<_>>()), Some(vec![2, 1, 0]));
    pick_into_dialog(&mut h.app, &[solidcraft_engine::Sel::Body { name: "Body1".into() }]);
    crate::dialogs::cancel(&mut h.app);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.agent_chat.picked.len(), 3, "Cancel keeps what was picked before");
    // The control channel opens it too and reports what is picked.
    let reply = h.call("ui.agent_chat", json!({"pick": true}));
    assert!(matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::ChatContext)));
    assert_eq!(reply["result"]["picked"].as_array().map(Vec::len), Some(3));
    crate::dialogs::cancel(&mut h.app);
    // While another dialog is open, the button does nothing.
    h.app.start("solid.box");
    let before = h.app.dialog.is_some();
    start_pick(&mut h.app);
    assert!(before && !matches!(h.app.dialog.as_ref().map(|d| &d.kind), Some(crate::dialogs::Kind::ChatContext)));
}

/// What was picked goes with the next message, once, together with an attached selection that does
/// not repeat it.
#[test]
fn the_picks_go_with_the_next_message_once() {
    let (mut app, script, ctx) = app_with_script();
    let sels = box_selection(&mut app);
    start_pick(&mut app);
    pick_into_dialog(&mut app, &sels[2..]); // the feature and the body
    let d = app.dialog.take().expect("the picker");
    pick_done(&mut app, &d).expect("OK");
    assert_eq!(app.agent_chat.picked_context.items.len(), 2);

    // The same body is also selected and Attach selection is on: it is listed once.
    app.session.selection = vec![sels[3].clone(), sels[0].clone()];
    app.agent_chat.attach_selection = true;
    send(&mut app, &ctx, "look at these".into());
    let prompt = script.calls().pop().expect("a prompt");
    assert!(prompt.starts_with("prompt: look at these\n\n[Selected in SolidCraft"), "{prompt}");
    assert!(prompt.contains("3 item(s)"), "two picked and one more selected: {prompt}");
    assert_eq!(prompt.matches("- Body1").count(), 1, "{prompt}");
    let Some(Item::User { context, .. }) = app.agent_chat.items.last() else { panic!("a sent message") };
    assert_eq!(context.items.len(), 3);
    assert!(app.agent_chat.picked.is_empty() && app.agent_chat.picked_context.is_empty(), "the tray is empty again");
    // The next message carries nothing of it.
    ended(&script, "");
    poll(&mut app, &ctx);
    app.agent_chat.attach_selection = false;
    send(&mut app, &ctx, "and now?".into());
    assert_eq!(script.calls().pop().as_deref(), Some("prompt: and now?"));
}

/// Two clicks on two rims are two things, not four edges (a click on a rim selects both halves for
/// the commands; the chat counts and describes what was clicked).
#[test]
fn a_rim_clicked_once_is_one_item_not_its_pieces() {
    use solidcraft_engine::Sel;
    let (mut app, script, ctx) = app_with_script();
    app.run("solid.cylinder", json!({"radius": 10, "height": 20})).expect("a cylinder");
    let (name, top, bottom, mid_top, mid_bottom) = {
        let st = app.session.world_state();
        let body = &st.bodies[0];
        let mesh = body.mesh();
        let rim_at = |z: f64| mesh.edges.iter().position(|e| e.len() > 3 && e.iter().all(|p| (p.z - z).abs() < 1e-6)).expect("a rim");
        let (top, bottom) = (rim_at(20.0), rim_at(0.0));
        (body.name.clone(), top, bottom, crate::viewport::polyline_mid(&mesh.edges[top]), crate::viewport::polyline_mid(&mesh.edges[bottom]))
    };
    crate::viewport::select(&mut app, Some(Sel::Edge { body: name.clone(), index: top, point: mid_top }), false, false);
    crate::viewport::select(&mut app, Some(Sel::Edge { body: name, index: bottom, point: mid_bottom }), true, false);
    assert_eq!(app.session.selection.len(), 4, "two clicks selected four halves");
    assert_eq!(selected_count(&mut app), 2, "the checkbox counts the two clicks");

    app.agent_chat.attach_selection = true;
    send(&mut app, &ctx, "these two rims".into());
    let prompt = script.calls().pop().expect("a prompt");
    assert!(prompt.contains("2 item(s)"), "{prompt}");
    assert_eq!(prompt.matches("(2 pieces)").count(), 2, "{prompt}");
    // A rim of radius 10 is about 62.8 mm round (the kernel's own figure), both halves together.
    assert!(prompt.contains("Edge 62.") && prompt.contains("mm (2 pieces)"), "{prompt}");
    let Some(Item::User { context, .. }) = app.agent_chat.items.last() else { panic!("a sent message") };
    assert_eq!(context.items.len(), 2);
    assert_eq!(context.total(), 2);
    // The detail still names the piece indices the tools take.
    assert!(context.items.iter().all(|i| i.detail.contains("indices [")), "{:?}", context.items);
}

/// The same for a surface the kernel splits at a seam: one click on the cylinder's wall is one item.
#[test]
fn a_split_wall_clicked_once_is_one_item() {
    use solidcraft_engine::Sel;
    let (mut app, script, ctx) = app_with_script();
    app.run("solid.cylinder", json!({"radius": 10, "height": 20})).expect("a cylinder");
    let wall = app.session.execute("model.faces", &json!({"body": "Body1"})).expect("faces")["faces"]
        .as_array()
        .and_then(|faces| faces.iter().find(|f| f["cylinder"]["radius"].is_number()).cloned())
        .expect("the wall");
    let at = &wall["point"];
    let point = solidcraft_engine::geom::Vec3::new(at[0].as_f64().unwrap_or(0.0), at[1].as_f64().unwrap_or(0.0), at[2].as_f64().unwrap_or(0.0));
    crate::viewport::select(
        &mut app,
        Some(Sel::Face { body: "Body1".into(), index: wall["index"].as_u64().unwrap_or(0) as usize, point }),
        false,
        false,
    );
    assert!(!app.session.selection.is_empty());
    assert_eq!(selected_count(&mut app), 1, "one click, however many patches it selected: {:?}", app.session.selection);
    app.agent_chat.attach_selection = true;
    send(&mut app, &ctx, "this wall".into());
    let prompt = script.calls().pop().expect("a prompt");
    assert!(prompt.contains("1 item(s)") && prompt.contains("Round face r10"), "{prompt}");
}

// ---- Attached pictures in the shared viewer ----

/// A small picture encoded as `format`.
fn encoded(format: image::ImageFormat, width: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let image = image::RgbaImage::from_pixel(width, 4, image::Rgba([40, 160, 90, 255]));
    let image = if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(image).to_rgb8())
    } else {
        image::DynamicImage::ImageRgba8(image)
    };
    image.write_to(&mut std::io::Cursor::new(&mut out), format).expect("encoded");
    out
}

/// Frames until a picture is decoded (it is, in the background, on first draw), at most two seconds.
fn until_decoded(h: &mut crate::scenario::Harness, picture: &Picture) -> bool {
    for _ in 0..200 {
        h.frames(1);
        if picture.decode.texture.get().is_some() {
            h.frames(2);
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}

/// Pictures in any format the attachments take are drawn: JPEG and GIF as well as PNG, decoded off
/// the window thread (the first frames show a spinner), one that isn't a picture says so.
#[test]
fn attached_pictures_of_any_format_are_decoded_in_the_background() {
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    for (format, width) in [(image::ImageFormat::Png, 8), (image::ImageFormat::Jpeg, 16), (image::ImageFormat::Gif, 12)] {
        let picture = Picture::new(encoded(format, width), format!("photo.{format:?}"));
        h.app.agent_chat.items.clear();
        h.app.agent_chat.items.push(Item::User {
            text: "look".into(),
            image: false,
            pictures: vec![picture.clone()],
            files: Vec::new(),
            context: Context::default(),
        });
        h.frames(2);
        assert!(until_decoded(&mut h, &picture), "{format:?} was decoded");
        let texture = picture.decode.texture.get().and_then(|t| t.as_ref()).unwrap_or_else(|| panic!("{format:?} is a texture"));
        assert_eq!(texture.size()[0], width as usize, "{format:?} keeps its width");
        assert!(crate::scenario::handle_at("agent_chat:picture:0:0").is_some(), "{format:?} has its thumbnail");
    }
    // Bytes that are no picture: the name where the thumbnail would be, and nothing breaks.
    let broken = Picture::new(b"not a picture at all".to_vec(), "fake.png");
    h.app.agent_chat.items.clear();
    h.app.agent_chat.items.push(Item::User {
        text: "look".into(),
        image: false,
        pictures: vec![broken.clone()],
        files: Vec::new(),
        context: Context::default(),
    });
    assert!(until_decoded(&mut h, &broken));
    assert!(matches!(broken.decode.texture.get(), Some(None)), "it could not be read");
    assert!(h.widget("fake.png").is_some(), "its name stands in for the thumbnail");
}

/// A picture attached as a file goes to the viewer like the view's own picture, named in it, and
/// the view's picture is added beside it rather than replacing it, once.
#[test]
fn an_attached_picture_and_the_views_picture_share_the_viewer() {
    let mut chat = AgentChat::default();
    let attached = Picture::shared(Arc::new(encoded(image::ImageFormat::Jpeg, 16)), "photo.jpg", false);
    chat.push(Item::User { text: "see".into(), image: true, pictures: vec![attached], files: Vec::new(), context: Context::default() });
    chat.keep_picture(Some(0), encoded(image::ImageFormat::Png, 8));
    let Some(Item::User { pictures, .. }) = chat.items.last() else { panic!("a sent message") };
    assert_eq!(pictures.iter().map(|p| (p.note.as_str(), p.view)).collect::<Vec<_>>(), [("photo.jpg", false), ("", true)]);
    // A second view picture is not added to the same message.
    chat.keep_picture(Some(0), encoded(image::ImageFormat::Png, 8));
    let Some(Item::User { pictures, .. }) = chat.items.last() else { panic!("a sent message") };
    assert_eq!(pictures.len(), 2);
}

/// Settings › AI › Add agent opens its dialog from the Home tab too (#3483, #3513).
#[test]
fn add_agent_opens_its_dialog_from_home() {
    use crate::scenario::handle_at;
    let mut h = crate::scenario::Harness::new();
    h.call("ui.resize", json!({"width": 1600.0, "height": 1000.0}));
    h.call("ui.home", json!({"open": true}));
    h.call("ui.prefs", json!({"open": "AI"}));
    h.frames(4);
    assert!(h.app.home.open, "Home is showing");
    let add = handle_at("prefs:ai_add").expect("Add agent is drawn");
    h.call("ui.click", json!({"x": add.x, "y": add.y}));
    h.frames(4);
    assert!(h.app.agent_chat.settings.add.is_some(), "Add agent opened its dialog");
    assert!(handle_at("ai_add:add").is_some(), "the dialog is drawn over Home");
}

/// `ui.agent_chat` `add_agent` takes an endpoint's request timeout (rounded and clamped to 30 s
/// ..= 1 h, 300 s when left out) and the state reports it.
#[test]
fn add_agent_takes_a_request_timeout_and_the_state_reports_it() {
    let ctx = egui::Context::default();
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    let timeout = |app: &mut SolveApp, id: &str| {
        let state = control(app, &ctx, &json!({})).unwrap();
        state["agents"].as_array().unwrap().iter().find(|a| a["id"] == id).map(|a| a["request_timeout_s"].clone())
    };
    let add = |app: &mut SolveApp, t: Value| {
        let mut a = json!({"id": "slow", "name": "Slow", "kind": "api", "base_url": "http://localhost:11434/v1", "model": "m"});
        if !t.is_null() {
            a["request_timeout_s"] = t;
        }
        control(app, &ctx, &json!({"add_agent": a}))
    };
    add(&mut app, json!(1200)).unwrap();
    assert_eq!(timeout(&mut app, "slow"), Some(json!(1200)));
    assert_eq!(app.preferences.agents.iter().find(|a| a.id == "slow").unwrap().request_timeout_s, 1200);
    add(&mut app, json!(99999.7)).unwrap();
    assert_eq!(timeout(&mut app, "slow"), Some(json!(3600)));
    add(&mut app, json!(-3)).unwrap();
    assert_eq!(timeout(&mut app, "slow"), Some(json!(30)));
    add(&mut app, Value::Null).unwrap();
    assert_eq!(timeout(&mut app, "slow"), Some(json!(300)));
    assert!(add(&mut app, json!("long")).is_err());
    control(&mut app, &ctx, &json!({"add_agent": {"id": "acp", "name": "A", "command": "x --acp"}})).unwrap();
    assert_eq!(timeout(&mut app, "acp"), Some(Value::Null), "only endpoints have a request timeout");
}

#[test]
fn no_tool_turn_reports_empty_reply_and_unperformed_edit() {
    for (prompt, reply) in [
        ("Hello", ""),
        ("Hello", "   "),
        ("Make a box", "Here is how to make a box."),
        ("Can you rename the body?", "I cannot rename bodies."),
        ("Could you please make a change to the current shape", "Here is the command."),
        ("Sketch a rectangle", r#"{"name":"batch","arguments":{"commands":[]}}"#),
    ] {
        let (mut app, _, _) = app_with_script();
        app.agent_chat.tools_port = Some(12345);
        start_turn(&mut app, prompt.into(), false, None, vec![]);
        if !reply.is_empty() {
            app.agent_chat.on_event(Event::Text { thought: false, text: reply.into() });
        }
        app.agent_chat.on_event(Event::TurnEnded { reason: String::new() });
        end_turn(&mut app);
        let expected = tr("ui.agent_chat_no_tool_call");
        assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, error: false } if text == expected)), "{prompt}");
        assert_eq!(status(&app)["turnWarning"], expected);
        assert!(app.session.undo.is_empty());
        start_turn(&mut app, "What is a fillet?".into(), false, None, vec![]);
        assert!(status(&app)["turnWarning"].is_null());
        assert!(app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, .. } if text == expected)));
        new_chat(&mut app);
        assert!(status(&app)["turnWarning"].is_null());
        assert!(app.agent_chat.items.is_empty());
    }
}

#[test]
fn no_tool_turn_warning_does_not_follow_a_tool_call() {
    let (mut app, _, _) = app_with_script();
    app.agent_chat.tools_port = Some(12345);
    start_turn(&mut app, "Make a box".into(), false, None, vec![]);
    app.agent_chat.on_event(Event::Tool {
        id: "t".into(),
        title: Some("batch".into()),
        kind: None,
        status: Some(ToolStatus::Failed),
        input: None,
        output: None,
    });
    app.agent_chat.on_event(Event::TurnEnded { reason: String::new() });
    end_turn(&mut app);
    assert!(status(&app).get("turnWarning").is_some());
    assert!(status(&app)["turnWarning"].is_null());
    assert!(!app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { .. })));
}

#[test]
fn no_tool_turn_warning_leaves_answers_and_incomplete_turns_quiet() {
    for (prompt, reply, reason, tools) in [
        ("What is a fillet?", "A fillet rounds an edge.", "", true),
        ("How do I make a box?", "Use the Box command.", "", true),
        ("Make a box", "", "cancelled", true),
        ("Make a box", "", "error", true),
        ("Make a box", "", "", false),
    ] {
        let (mut app, _, _) = app_with_script();
        app.agent_chat.tools_port = tools.then_some(12345);
        start_turn(&mut app, prompt.into(), false, None, vec![]);
        app.agent_chat.on_event(Event::Text { thought: false, text: reply.into() });
        app.agent_chat.on_event(Event::TurnEnded { reason: reason.into() });
        end_turn(&mut app);
        assert!(status(&app).get("turnWarning").is_some());
        assert!(status(&app)["turnWarning"].is_null(), "{prompt}: {reason}");
        assert!(!app.agent_chat.items.iter().any(|i| matches!(i, Item::Notice { text, .. } if text == tr("ui.agent_chat_no_tool_call"))));
    }
}

/// A turn that ends with nothing shown because the reply hit its length limit, or the model
/// declined, says so instead of blaming tool support (#9495).
#[test]
fn a_cut_off_or_declined_reply_says_so_instead_of_blaming_tools() {
    for (reason, said) in [("max_tokens", "ui.agent_chat_reply_cut_off"), ("refusal", "ui.agent_chat_reply_declined")] {
        let (mut app, _, _) = app_with_script();
        app.agent_chat.tools_port = Some(12345);
        start_turn(&mut app, "Make a box".into(), false, None, vec![]);
        app.agent_chat.on_event(Event::TurnEnded { reason: reason.into() });
        end_turn(&mut app);
        assert!(status(&app)["turnWarning"].is_null(), "{reason}");
        let notices: Vec<&str> = app
            .agent_chat
            .items
            .iter()
            .filter_map(|i| if let Item::Notice { text, error: false } = i { Some(text.as_str()) } else { None })
            .collect();
        assert_eq!(notices, [tr(said)], "{reason}");
    }
}

// ---- Tool calls in words (#1457) ----

fn in_mm() -> summary::Units<'static> {
    summary::Units { units: "mm", decimals: 2 }
}

fn said(title: &str, input: Value, output: Option<&str>) -> Option<String> {
    summary::summary(title, Some(&input), output, true, in_mm())
}

/// The issue's examples: a card says what the call did, from the command's label, its main
/// parameters and what it returned.
#[test]
fn tool_calls_read_as_what_they_did() {
    let extrude = json!({"command": "solid.extrude", "params": {"sketch": "Sketch1", "distance": 10}});
    assert_eq!(said("execute: solid.extrude", extrude.clone(), None).as_deref(), Some("Extrude Sketch1 by 10 mm"));
    // As an ACP agent names the bridged tool, with the body the result made.
    let made = r#"{"feature": 4, "name": "Extrude1", "created_bodies": ["Body2"], "changed_bodies": []}"#;
    assert_eq!(said("mcp__solidcraft-app__execute", extrude, Some(made)).as_deref(), Some("Extrude Sketch1 by 10 mm → Body2"));
    let edges = json!([[20, 0, 20], [20, 30, 20], [0, 15, 20], [40, 15, 20]]);
    let batch = json!({"commands": [
        {"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}},
        {"command": "solid.fillet", "params": {"edges": edges, "radius": 4}},
    ]});
    assert_eq!(said("batch: solid.box, solid.fillet", batch, None).as_deref(), Some("Box 40 × 30 × 20 mm, then Fillet 4 edges (r 4 mm)"));
    let measured = r#"{"body_count": 1, "bodies": [{"name": "Block", "volume_mm3": 23620.4, "bbox": {"min": [0, 0, 0], "max": [40, 30, 20]}}]}"#;
    assert_eq!(said("measure", json!({"body": "Block"}), Some(measured)).as_deref(), Some("Measured Block: 40 × 30 × 20 mm, 23.6 cm³"));
    assert_eq!(said("save", json!({"path": "C:\\designs\\block.solidcraft"}), None).as_deref(), Some("Saved block.solidcraft"));
    // Still running, or failed: what it does, not what it did.
    let running = summary::summary("save", Some(&json!({"path": "/home/me/block.solidcraft"})), None, false, in_mm());
    assert_eq!(running.as_deref(), Some("Save block.solidcraft"));
    // Lengths in the design's units; an expression as written; one edge is one.
    let inches = summary::Units { units: "in", decimals: 3 };
    let one = json!({"command": "solid.fillet", "params": {"edges": [[0, 0, 0]], "radius": 2.54}});
    assert_eq!(summary::summary("execute: solid.fillet", Some(&one), None, true, inches).as_deref(), Some("Fillet 1 edge (r 0.1 in)"));
    let expr = json!({"command": "solid.extrude", "params": {"sketch": "Sketch1", "distance": "2 * depth"}});
    assert_eq!(said("execute: solid.extrude", expr, None).as_deref(), Some("Extrude Sketch1 by 2 * depth"));
    // A long batch names its first steps and counts the rest.
    let steps: Vec<Value> = (0..6).map(|_| json!({"command": "edit.undo"})).collect();
    assert_eq!(said("batch: edit.undo", json!({"commands": steps}), None).as_deref(), Some("Undo, then Undo, then Undo, then Undo, and 2 more"));
}

/// Paths in a card never show their folders (the owner's "way too much"): the screenshot is just
/// "Screenshot", files are their names, temporary files are not named, and the full text stays in
/// Details and the tooltip.
#[test]
fn tool_cards_show_file_names_never_paths() {
    let shot = r"ui.screenshot: C:\Users\bherbruck\AppData\Local\Temp\solidcraft-mcp-8608-b4c7b64\shot-0.png";
    assert_eq!(said(shot, json!({}), None).as_deref(), Some("Screenshot"));
    assert_eq!(said("mcp__solidcraft-app__ui.screenshot", json!({}), None).as_deref(), Some("Screenshot"));
    // The fallback title (another agent's tools) is cut to file names; temp files drop out.
    assert_eq!(summary::plain_title(shot), "ui.screenshot");
    assert_eq!(summary::plain_title(r"Read C:\Users\Jane Doe\Documents\part.step"), "Read part.step");
    assert_eq!(summary::plain_title(r"Write D:/work/out/block.solidcraft now"), "Write block.solidcraft now");
    assert_eq!(summary::plain_title(r"Open \\nas\share\parts\bracket.step"), "Open bracket.step");
    assert_eq!(summary::plain_title("Read /home/bherbruck/designs/part.step"), "Read part.step");
    assert_eq!(summary::plain_title("Edit ~/designs/part.step."), "Edit part.step.");
    assert_eq!(summary::plain_title("cat \"/home/me/a b.txt\" | head"), "cat \"a b.txt\" | head");
    assert_eq!(summary::plain_title("Read /tmp/claude-1000/scratch/out.txt"), "Read");
    assert_eq!(summary::plain_title("ui.render: /var/folders/xy/T/shot.png"), "ui.render");
    assert_eq!(summary::plain_title("Run mcp__x__y with a/b and /"), "Run mcp__x__y with a/b and /");
    // SolidCraft's own file tools: the name alone; a temp file has none.
    assert_eq!(
        said("export", json!({"path": "/home/me/cad/part.step"}), None).as_deref().map(|s| s.ends_with("part.step") && !s.contains('/')),
        Some(true)
    );
    assert_eq!(
        said("open", json!({"path": r"C:\cad\block.solidcraft"}), None).as_deref().map(|s| s.ends_with(" block.solidcraft") && !s.contains('\\')),
        Some(true)
    );
    assert_eq!(said("save", json!({"path": "/tmp/x/block.solidcraft"}), None).as_deref(), Some("Save"));
    assert_eq!(said("save", json!({"path": r"C:\Users\me\AppData\Local\Temp\a.solidcraft"}), Some("")).as_deref(), Some("Save"));
}

/// Another agent's own tools, an unknown command and malformed or hostile calls keep the card's
/// title (no words), and nothing panics or grows without bound.
#[test]
fn unknown_or_malformed_tool_calls_keep_their_title_and_never_panic() {
    for title in ["Read /etc/hosts", "mcp__github__create_issue", "mcp__github__save", "save /tmp/x.txt", "", "  ", "__", "mcp____"] {
        assert_eq!(said(title, json!({"path": "x.solidcraft", "command": "solid.box"}), None), None, "{title}");
    }
    assert_eq!(said("execute: solid.nope", json!({"command": "solid.nope"}), None), None, "an unknown command");
    assert_eq!(summary::summary("execute: solid.box", None, None, true, in_mm()), None, "arguments that did not parse");
    let long = "x".repeat(100_000);
    let many: Vec<Value> = (0..10_000).map(|i| json!({"command": format!("solid.{i}"), "params": {"distance": i}})).collect();
    let hostile = [
        json!(null),
        json!([1, 2, 3]),
        json!("text"),
        json!({"command": 5, "params": "x"}),
        json!({"command": "solid.box", "params": {"length": "", "width": [], "height": {"a": 1}}}),
        json!({"command": "solid.box", "params": {"length": 1e308, "width": -1e308, "height": 0}}),
        json!({"command": "solid.extrude", "params": {"sketch": long, "distance": long, "edges": [long], "angle": 1e300}}),
        json!({"command": "solid.extrude", "params": {"sketch": "a\nb\u{0}\u{202e}c", "distance": -0.0}}),
        json!({"commands": many}),
        json!({"commands": [null, 1, {"command": "batch", "params": {"commands": [{"command": "solid.box"}]}}, {"id": "save"}]}),
        json!({"body": ["Block"], "path": {"x": 1}, "name": 3, "value": null}),
    ];
    let outputs = [
        None,
        Some(""),
        Some("{"),
        Some("[]"),
        Some(r#"{"bodies": [{"bbox": {"min": [1], "max": "x"}}]}"#),
        Some(r#"{"created_bodies": [1, null, "B"]}"#),
    ];
    for tool in ["execute", "batch", "measure", "save", "export", "open", "new_design", "set_parameter", "mcp__solidcraft__batch"] {
        for input in &hostile {
            for output in outputs {
                for done in [true, false] {
                    if let Some(s) = summary::summary(tool, Some(input), output, done, in_mm()) {
                        assert!(s.chars().count() <= 240 && !s.contains('\n'), "{tool}: {s}");
                    }
                }
            }
        }
    }
    let huge = format!("{{\"bodies\": [{}]}}", "{\"name\": \"B\"},".repeat(20_000));
    assert_eq!(said("measure", json!({}), Some(&huge)).as_deref(), Some("Measure"), "a result too large is not read");
}

/// The card's title is the summary; the raw arguments and result wait behind Details until it is
/// opened. A call with no words keeps its title.
#[test]
fn a_tool_card_shows_its_summary_and_keeps_the_json_behind_details() {
    use crate::scenario::handle_at;
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    let input = json!({"command": "solid.extrude", "params": {"sketch": "Sketch1", "distance": 10}});
    let output = r#"{"feature": 4, "name": "Extrude1", "created_bodies": []}"#;
    let n = h.app.agent_chat.items.len();
    h.app.agent_chat.items.push(Item::Tool {
        id: "t".into(),
        title: "execute: solid.extrude".into(),
        kind: String::new(),
        status: Some(ToolStatus::Completed),
        input: Some(input.clone()),
        output: Some(output.into()),
    });
    h.app.agent_chat.items.push(Item::Tool {
        id: "u".into(),
        title: "mcp__github__create_issue".into(),
        kind: String::new(),
        status: Some(ToolStatus::InProgress),
        input: None,
        output: None,
    });
    h.frames(3);
    assert!(h.widget("Extrude Sketch1 by 10 mm").is_some(), "the summary is the card's title");
    assert!(h.widget("execute: solid.extrude").is_none(), "the raw title is not shown");
    assert!(h.widget("mcp__github__create_issue").is_some(), "a call with no words keeps its title");
    assert!(h.widget("Details").is_some(), "the expander is named");
    let pretty = serde_json::to_string_pretty(&input).unwrap();
    assert!(h.widget("Arguments").is_none() && h.widget(&pretty).is_none() && h.widget(output).is_none(), "the JSON waits behind Details");
    let card = handle_at(&format!("agent_chat:tool:{n}")).expect("the card");
    h.call("ui.click", json!({"x": card.x, "y": card.y}));
    h.frames(3);
    assert!(h.widget("Arguments").is_some() && h.widget(&pretty).is_some(), "the arguments unfold");
    assert!(h.widget("Result").is_some() && h.widget(output).is_some(), "and what it returned");
}

/// The card says "Screenshot"; the full temp path is only under Details.
#[test]
fn a_screenshot_card_hides_the_path_until_details() {
    use crate::scenario::handle_at;
    let mut h = crate::scenario::Harness::new();
    h.call("ui.agent_chat", json!({"open": true}));
    h.frames(3);
    let path = r"C:\Users\bherbruck\AppData\Local\Temp\solidcraft-mcp-8608-b4c7b64\shot-0.png";
    let n = h.app.agent_chat.items.len();
    h.app.agent_chat.items.push(Item::Tool {
        id: "s".into(),
        title: format!("ui.screenshot: {path}"),
        kind: String::new(),
        status: Some(ToolStatus::Completed),
        input: Some(json!({"path": path})),
        output: Some(path.into()),
    });
    h.frames(3);
    assert!(h.widget("Screenshot").is_some(), "the card says what it did");
    assert!(h.widget(path).is_none() && h.widget(&format!("ui.screenshot: {path}")).is_none(), "no path in the card");
    let card = handle_at(&format!("agent_chat:tool:{n}")).expect("the card");
    h.call("ui.click", json!({"x": card.x, "y": card.y}));
    h.frames(3);
    assert!(h.widget("Result").is_some() && h.widget(path).is_some(), "Details holds the full path");
}

/// The composer's input box fills its parent composer area on all four sides, in every layout:
/// the only inset left is the text's own padding inside the box (no second margin around it).
#[test]
fn the_composer_box_fills_its_parent_in_every_layout() {
    use crate::scenario::rect_of;
    for (layout, popped) in [("floating", false), ("docked", false), ("floating", true), ("docked", true)] {
        let mut h = narrow_chat();
        h.call("ui.resize", json!({"width": 1600, "height": 1000}));
        h.call("ui.set", json!({"layout": layout}));
        h.app.preferences.agent_chat_popped = popped;
        h.app.agent_chat.state = Some(State::Ready);
        h.frames(4);
        let footer = rect_of("agent_chat:composer_footer").unwrap_or_else(|| panic!("{layout} popped={popped}: the composer area"));
        let boxed = rect_of("agent_chat:composer_box").expect("the input box");
        let (l, r, t, b) =
            (boxed.left() - footer.left(), footer.right() - boxed.right(), boxed.top() - footer.top(), footer.bottom() - boxed.bottom());
        for (edge, gap) in [("left", l), ("right", r), ("top", t), ("bottom", b)] {
            assert!(gap.abs() <= 1.0, "{layout} popped={popped}: box {edge} edge is {gap} from its parent ({boxed:?} in {footer:?})");
        }
        let input = rect_of("agent_chat:input").expect("the text");
        assert!(input.left() > boxed.left() + 2.0, "{layout} popped={popped}: the text keeps its own padding");
        let send = rect_of("agent_chat:send").expect("Send");
        assert!(boxed.contains_rect(send), "{layout} popped={popped}: Send stays inside the box");
    }
}

/// The composer hint (idle and busy, in every language) stays on one row, even at the narrowest
/// panel, and is cut with an ellipsis instead of wrapping.
#[test]
fn the_composer_hint_stays_on_one_row_at_the_narrowest_width() {
    let mut h = narrow_chat();
    h.app.agent_chat.state = Some(State::Ready);
    h.frames(4);
    let width = crate::scenario::rect_of("agent_chat:input").expect("the text box").width();
    let ctx = crate::fonts::test_context();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/locales");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let language = path.file_stem().unwrap().to_string_lossy().into_owned();
        let _scope = crate::i18n::scope(&language);
        for key in ["ui.agent_chat_placeholder", "ui.agent_chat_placeholder_busy"] {
            let hint = crate::i18n::tr(key).to_owned();
            let mut rows = 0;
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                rows = crate::ui_kit::assistant_hint_galley(ui, &hint, width - 2.0 * crate::theme::Tokens::get().form_inset).rows.len();
            });
            output.textures_delta.clear();
            assert_eq!(rows, 1, "{language} {key} at {width} wide: {hint}");
            checked += 1;
        }
    }
    assert!(checked >= 20, "every language was checked");
}

/// The room tool windows get beside the docked Assistant, in a view that came out inside out
/// (the window of #9492): the Assistant's side gives way instead of panicking in `f32::clamp`.
#[test]
fn tool_room_beside_the_docked_assistant_survives_an_inside_out_view() {
    let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
    app.ui.layout = crate::shell::LayoutMode::Floating;
    app.preferences.ai_features = true;
    app.agent_chat.open = true;
    app.preferences.agent_chat_popped = false;
    app.agent_chat.panel_rect = Some(egui::Rect::from_min_max(egui::pos2(100.0, 40.0), egui::pos2(150.0, 600.0)));
    let inside_out = egui::Rect::from_min_max(egui::pos2(264.1875, 40.0), egui::pos2(159.0, 600.0));
    for side in ["right", "left"] {
        app.preferences.agent_chat_side = side.into();
        let free = tool_space(&app, inside_out);
        assert!(free.min.x.is_finite() && free.max.x.is_finite(), "{side}: {free:?}");
    }
}
