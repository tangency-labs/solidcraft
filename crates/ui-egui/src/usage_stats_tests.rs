use std::sync::{Arc, Mutex};

use serde_json::json;
use solidcraft_engine::Session;

use super::*;
use crate::agent_chat::backend::HttpRequest;

/// The engine's counts are one per process: these tests take turns.
static TURN: Mutex<()> = Mutex::new(());

type Requests = Arc<Mutex<Vec<HttpRequest>>>;

/// 2023-11-14, 22:13:20 UTC.
const T0: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

/// An app whose reports go to a server answering `status` (None: offline), the notice shown.
fn app_with_server(status: Option<u16>) -> (SolveApp, Requests) {
    let requests: Requests = Arc::default();
    let seen = requests.clone();
    let http: crate::tickets::Http = Arc::new(move |r: HttpRequest, done: std::sync::mpsc::Sender<crate::tickets::Reply>| {
        seen.lock().unwrap().push(r);
        let _ = done.send(status.map(|status| (status, Vec::new())).ok_or_else(|| "network unreachable".to_string()));
    });
    let services = crate::Services { usage_http: Some(http), ..Default::default() };
    let mut app = SolveApp::new(Session::default(), services);
    app.usage = UsageStats::default();
    app.usage.store.notice_shown = true;
    let _ = solidcraft_engine::usage::take();
    (app, requests)
}

/// An operation: a command that changes the design (an undo step).
fn make_box(app: &mut SolveApp) {
    app.session.execute("solid.box", &json!({"length": 2, "width": 2, "height": 2})).unwrap();
}

/// A day of counting: a tick turns counting on, an operation runs, a tick takes it in.
fn use_app(app: &mut SolveApp, unix: i64, now: f64) {
    tick_at(app, unix, now, true);
    make_box(app);
    tick_at(app, unix, now, true);
}

/// The day's counts of `bucket` from `source` (`ui`, `agent`, `script`).
fn counts<'a>(app: &'a SolveApp, day: &str, tools: bool, source: &str) -> Option<&'a BTreeMap<String, u32>> {
    let day = app.usage.store.days.get(day)?;
    (if tools { &day.tools } else { &day.operations }).get(source)
}

#[test]
fn a_finished_day_is_sent_once_to_the_endpoint_with_exactly_the_reported_fields() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(200));
    use_app(&mut app, T0, 0.0);
    // Today is never sent; tomorrow, after the first minute, yesterday is.
    tick_at(&mut app, T0, FIRST_TRY_SECONDS + 1.0, false);
    assert!(requests.lock().unwrap().is_empty(), "the day in progress is not sent");
    tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + 1.0, false);
    tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + 2.0, false);
    let sent = requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "one report for one finished day");
    let request = &sent[0];
    assert_eq!((request.method, request.url.as_str()), ("POST", ENDPOINT));
    let body: serde_json::Value = serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
    let keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    let mut allowed = ["v", "install_id", "day", "version", "os", "arch", "platform", "active_minutes", "operations", "tools"];
    allowed.sort_unstable();
    assert_eq!(keys, allowed, "these fields and nothing else");
    assert_eq!(body["v"], SCHEMA);
    assert_eq!(body["day"], "2023-11-14");
    assert_eq!(body["platform"], "desktop");
    assert_eq!(body["active_minutes"], 1, "input in one minute is one active minute");
    assert!(valid_id(body["install_id"].as_str().unwrap()));
    assert_eq!(body["operations"], json!({"ui": {"solid.box": 1}}));
    for bucket in ["operations", "tools"] {
        for (source, ids) in body[bucket].as_object().unwrap() {
            assert!(["ui", "agent", "script"].contains(&source.as_str()), "{source}");
            for (id, count) in ids.as_object().unwrap() {
                assert!(id == "plugin" || solidcraft_engine::find_command(id).is_some(), "only registered command ids: {id}");
                assert!(count.is_u64());
            }
        }
    }
    assert!(!app.usage.store.days.contains_key("2023-11-14"), "a sent day is not kept");
    // No more tries for it, however many frames follow.
    for frame in 0..1000 {
        tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + 3.0 + f64::from(frame), false);
    }
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn off_by_policy_environment_or_preference_counts_keeps_and_sends_nothing() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for off_by in [Off::Policy, Off::Environment, Off::Preference] {
        let (mut app, requests) = app_with_server(Some(200));
        // A day kept from before it was turned off is deleted.
        use_app(&mut app, T0, 0.0);
        assert!(!app.usage.store.days.is_empty());
        match off_by {
            Off::Policy => app.policy = crate::policy::Policy::from_json(r#"{"UsageStatistics": 0}"#),
            Off::Environment => app.usage.env_off = true,
            Off::Preference => app.preferences.usage_statistics = false,
        }
        assert_eq!(off(&app), Some(off_by));
        for day in 0..10 {
            use_app(&mut app, T0 + day * DAY, 1000.0 + day as f64 * RETRY_HOURS * 3600.0);
        }
        assert!(!app.session.count_usage, "{off_by:?}: the session counts nothing");
        assert!(app.usage.store.days.is_empty(), "{off_by:?}: nothing is kept or queued");
        assert!(requests.lock().unwrap().is_empty(), "{off_by:?}: nothing is sent");
        assert_eq!(next_report_json(&app), None, "{off_by:?}: nothing to show");
        assert!(solidcraft_engine::usage::take().is_empty());
    }
}

#[test]
fn allowed_by_policy_leaves_the_persons_choice() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    app.policy = crate::policy::Policy::from_json(r#"{"UsageStatistics": 1}"#);
    assert_eq!(off(&app), None);
    app.preferences.usage_statistics = false;
    assert_eq!(off(&app), Some(Off::Preference));
}

#[test]
fn nothing_is_counted_before_the_notice_was_shown() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(200));
    app.usage.store.notice_shown = false;
    use_app(&mut app, T0, 0.0);
    use_app(&mut app, T0 + DAY, 10_000.0);
    assert!(!app.session.count_usage);
    assert!(app.usage.store.days.is_empty());
    assert!(app.usage.store.install_id.is_empty(), "not even an id");
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn the_environment_turns_usage_statistics_off() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string());
    assert!(!env_off(env(&[])));
    assert!(env_off(env(&[("DO_NOT_TRACK", "1")])));
    assert!(env_off(env(&[("SOLIDCRAFT_NO_TELEMETRY", "1")])));
    assert!(env_off(env(&[("SOLIDCRAFT_NO_TELEMETRY", "yes")])));
    assert!(!env_off(env(&[("DO_NOT_TRACK", "0")])));
    assert!(!env_off(env(&[("DO_NOT_TRACK", "")])));
    assert!(!env_off(env(&[("DO_NOT_TRACK", "false")])));
}

#[test]
fn offline_a_day_waits_with_one_try_a_day_and_is_dropped_after_seven_days() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(None);
    use_app(&mut app, T0, 0.0);
    // A day of frames, a frame a second, the next day: one try at start, none after.
    for second in 0..3600 {
        tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + f64::from(second), false);
    }
    assert_eq!(requests.lock().unwrap().len(), 1, "one try, no storm");
    assert!(app.usage.store.days.contains_key("2023-11-14"), "kept to try again");
    tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + RETRY_HOURS * 3600.0 + 1.0, false);
    assert_eq!(requests.lock().unwrap().len(), 2, "the next try a day later");
    tick_at(&mut app, T0 + KEEP_DAYS * DAY, 1.0e9, false);
    assert!(!app.usage.store.days.contains_key("2023-11-14"), "dropped after {KEEP_DAYS} days");
}

#[test]
fn a_server_error_keeps_the_day_and_a_refusal_drops_it() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (status, kept) in [(500, true), (429, true), (400, false), (204, false)] {
        let (mut app, requests) = app_with_server(Some(status));
        use_app(&mut app, T0, 0.0);
        tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + 1.0, false);
        tick_at(&mut app, T0 + DAY, FIRST_TRY_SECONDS + 2.0, false);
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(app.usage.store.days.contains_key("2023-11-14"), kept, "{status}");
    }
}

#[test]
fn several_days_kept_offline_are_sent_one_after_another() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(200));
    app.services.usage_http = None;
    for day in 0..3 {
        use_app(&mut app, T0 + day * DAY, 0.0);
    }
    let (_, server) = app_with_server(Some(200));
    let seen = server.clone();
    app.services.usage_http = Some(Arc::new(move |r: HttpRequest, done: std::sync::mpsc::Sender<crate::tickets::Reply>| {
        seen.lock().unwrap().push(r);
        let _ = done.send(Ok((200, Vec::new())));
    }));
    for frame in 0..10 {
        tick_at(&mut app, T0 + 3 * DAY, FIRST_TRY_SECONDS + f64::from(frame), false);
    }
    let days: Vec<String> = server
        .lock()
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<serde_json::Value>(r.body.as_deref().unwrap()).unwrap()["day"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(days, ["2023-11-14", "2023-11-15", "2023-11-16"]);
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn active_minutes_count_minutes_with_input_not_frames() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    let start = T0 - T0 % 60;
    for second in 0..120 {
        tick_at(&mut app, start + second, 0.0, true);
    }
    // Two minutes without input count nothing.
    for second in 120..240 {
        tick_at(&mut app, start + second, 0.0, false);
    }
    tick_at(&mut app, start + 300, 0.0, true);
    assert_eq!(app.usage.store.days["2023-11-14"].active_minutes, 3);
}

#[test]
fn only_what_the_person_or_an_agent_did_is_counted_never_a_replay() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    tick_at(&mut app, T0, 0.0, false);
    make_box(&mut app);
    make_box(&mut app);
    // Undo and redo replay what was done; a failure changes nothing; opening a design starts a
    // fresh history; selection, visibility and views are background.
    app.session.execute("edit.undo", &json!({})).unwrap();
    app.session.execute("edit.redo", &json!({})).unwrap();
    let _ = app.session.execute("no.such_command", &json!({}));
    let _ = app.session.execute("solid.box", &json!({"length": -1}));
    app.session.execute("file.new", &json!({})).unwrap();
    // A scratch copy (previews, thumbnails) counts nothing.
    let mut scratch = app.session.scratch();
    scratch.execute("solid.box", &json!({"length": 2, "width": 2, "height": 2})).unwrap();
    // An agent's action is counted under `agent`.
    {
        let _agent = solidcraft_engine::usage::from(solidcraft_engine::usage::Source::Agent);
        make_box(&mut app);
    }
    tick_at(&mut app, T0, 0.0, false);
    let day = &app.usage.store.days["2023-11-14"];
    assert_eq!(
        day.operations,
        [("agent".to_string(), [("solid.box".to_string(), 1)].into()), ("ui".to_string(), [("solid.box".to_string(), 2)].into())].into()
    );
    assert!(day.tools.is_empty(), "nothing was started from the UI");
}

#[test]
fn a_tool_started_from_the_ui_counts_even_when_cancelled() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    tick_at(&mut app, T0, 0.0, false);
    app.start("solid.box");
    crate::dialogs::cancel(&mut app);
    tick_at(&mut app, T0, 0.0, false);
    assert_eq!(counts(&app, "2023-11-14", true, "ui"), Some(&[("solid.box".to_string(), 1)].into()));
    assert!(counts(&app, "2023-11-14", false, "ui").is_none(), "a cancelled tool changed nothing");
}

#[test]
fn background_commands_are_never_counted_and_plugins_count_as_one_script_id() {
    use solidcraft_engine::usage::{Bucket, counted};
    for background in ["select.set", "select.clear", "view.fit", "view.home", "browser.visibility", "camera.orbit"] {
        assert!(!counted(background), "{background}");
    }
    for action in ["solid.extrude", "sketch.line", "feature.edit", "plugin.run"] {
        assert!(counted(action), "{action}");
    }
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    tick_at(&mut app, T0, 0.0, false);
    solidcraft_engine::usage::note(Bucket::Operations, "plugin.run");
    solidcraft_engine::usage::note(Bucket::Operations, "select.set");
    tick_at(&mut app, T0, 0.0, false);
    let day = &app.usage.store.days["2023-11-14"];
    assert_eq!(day.operations, [("script".to_string(), [("plugin".to_string(), 1)].into())].into());
}

#[test]
fn reset_id_makes_a_new_id_and_deletes_what_the_old_one_kept() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    use_app(&mut app, T0, 0.0);
    let old = app.usage.store.install_id.clone();
    reset_id(&mut app);
    assert!(valid_id(&app.usage.store.install_id));
    assert_ne!(app.usage.store.install_id, old);
    assert!(app.usage.store.days.is_empty());
}

#[test]
fn show_whats_sent_is_the_next_report() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    use_app(&mut app, T0, 0.0);
    let shown: serde_json::Value = serde_json::from_str(&next_report_json(&app).unwrap()).unwrap();
    let day = &app.usage.store.days["2023-11-14"];
    let sent = serde_json::to_value(report(&app.usage.store, "2023-11-14", day)).unwrap();
    assert_eq!(shown, sent);
}

#[test]
fn a_stored_store_comes_back_and_a_hostile_one_is_a_new_one() {
    let mut store = Store { install_id: new_id().unwrap(), notice_shown: true, days: BTreeMap::new() };
    let ui: Counts = [("ui".to_string(), [("solid.box".to_string(), 2)].into())].into();
    store.days.insert("2023-11-14".into(), Day { active_minutes: 5, operations: ui.clone(), tools: ui });
    assert_eq!(Store::from_stored(Some(&store.to_stored())), store);
    for hostile in ["", "[]", "null", "{", r#"{"install_id": 7}"#] {
        assert_eq!(Store::from_stored(Some(hostile)), Store::default(), "{hostile}");
    }
    let odd = Store::from_stored(Some(r#"{"install_id": "../../etc/passwd", "days": {"yesterday": {}, "2023-02-30": {}, "2023-11-14": {}}}"#));
    assert_eq!(odd.install_id, "", "an id that isn't a random one is made again");
    assert_eq!(odd.days.keys().collect::<Vec<_>>(), ["2023-11-14"]);
}

#[test]
fn days_are_utc_calendar_dates() {
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(T0.div_euclid(DAY)), "2023-11-14");
    for days in [-1000, 0, 59, 60, 10_957, 19_675, 20_000, 30_000] {
        assert_eq!(day_number(&date(days)), Some(days), "{days}");
    }
    for not_a_day in ["2023-13-01", "2023-02-29", "2023-1-01", "20231114", "2023-11-14-1", "abcd-ef-gh", ""] {
        assert_eq!(day_number(not_a_day), None, "{not_a_day}");
    }
    assert_eq!(day_number("2024-02-29"), Some(19_782));
}

#[test]
fn ids_are_random_version_4_uuids() {
    let a = new_id().unwrap();
    let b = new_id().unwrap();
    assert_ne!(a, b);
    assert!(valid_id(&a) && valid_id(&b));
    assert!(!valid_id("00000000-0000-0000-0000-000000000000"));
    assert!(!valid_id("ABCDEF00-0000-4000-8000-000000000000"));
}
