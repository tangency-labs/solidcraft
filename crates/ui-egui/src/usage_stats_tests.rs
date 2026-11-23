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
    app.usage = UsageStats { hosted: true, ..UsageStats::default() };
    app.usage.store.notice_ok = true;
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
    let mut allowed =
        ["v", "install_id", "day", "version", "os", "arch", "platform", "active_minutes", "operations", "tools", "failed", "compute_ms", "dialog_s"];
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
    app.usage.store.notice_ok = false;
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
    tick_at(&mut app, T0, 1.0, false);
    crate::dialogs::cancel(&mut app);
    tick_at(&mut app, T0, 40.0, false);
    assert_eq!(counts(&app, "2023-11-14", true, "ui"), Some(&[("solid.box".to_string(), 1)].into()));
    assert!(counts(&app, "2023-11-14", false, "ui").is_none(), "a cancelled tool changed nothing");
    let timed = &app.usage.store.days["2023-11-14"].dialog_s;
    assert_eq!(timed["solid.box"]["cancelled"], [("30s-2m".to_string(), 1)].into(), "open for 39 s, then cancelled: {timed:?}");
}

#[test]
fn a_tool_applied_is_timed_as_applied() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = solidcraft_engine::usage::take();
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.usage = UsageStats { hosted: true, ..UsageStats::default() };
    h.app.usage.store.notice_ok = true;
    h.frames(2);
    h.app.start("solid.box");
    h.frames(3);
    assert!(h.app.dialog.is_some(), "Box opens its dialog");
    crate::dialogs::confirm(&mut h.app);
    // The apply may run off the window thread: the dialog counts as open until its result is in.
    for _ in 0..200 {
        h.frames(1);
        if h.app.applying.is_none() && h.app.dialog.is_none() && h.app.usage.open_tool.is_none() {
            break;
        }
    }
    let timed: Vec<&Dialogs> = h.app.usage.store.days.values().map(|day| &day.dialog_s).collect();
    let applied = timed.iter().find_map(|d| d.get("solid.box")).expect("Box was timed");
    assert_eq!(applied.get("applied").map(|b| b.values().sum::<u32>()), Some(1), "{applied:?}");
    assert!(!applied.contains_key("cancelled"));
    for (bucket, expected) in [(0.0, "0-5s"), (4.99, "0-5s"), (5.0, "5-30s"), (29.9, "5-30s"), (30.0, "30s-2m"), (119.0, "30s-2m"), (120.0, "2m+")] {
        assert_eq!(dialog_bucket(bucket), expected, "{bucket}");
    }
}

#[test]
fn failures_count_the_kind_of_error_only_and_operations_their_time() {
    use solidcraft_engine::usage::{compute_bucket, error_kind};
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, _) = app_with_server(Some(200));
    tick_at(&mut app, T0, 0.0, false);
    make_box(&mut app);
    let secret = "Secret Bracket C:\\Users\\someone";
    // Whichever of these fails first (each refers to what isn't there); its parameters carry
    // words that must never reach a report.
    let tries = [
        ("solid.fillet", json!({"edges": [secret], "radius": 1})),
        ("body.delete", json!({"bodies": [secret]})),
        ("solid.extrude", json!({"profile": secret, "distance": 5})),
    ];
    let (failed_id, error) =
        tries.iter().find_map(|(id, params)| app.session.execute(id, params).err().map(|e| (*id, e))).expect("one of them fails");
    tick_at(&mut app, T0, 0.0, false);
    let day = &app.usage.store.days["2023-11-14"];
    let kinds = &day.failed["ui"][failed_id];
    assert_eq!(kinds.values().sum::<u32>(), 1, "{kinds:?}");
    let kind = kinds.keys().next().unwrap();
    assert_eq!(Some(kind.clone()), error_kind(&error));
    assert!(kind.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'), "{kind}");
    let all = serde_json::to_string(&report(&app.usage.store, "2023-11-14", day)).unwrap();
    assert!(!all.contains("Secret") && !all.contains("Users"), "no message, no parameter: {all}");
    assert_eq!(day.compute_ms["solid.box"].values().sum::<u32>(), 1, "the operation is timed: {:?}", day.compute_ms);
    assert_eq!(day.compute_ms[failed_id].values().sum::<u32>(), 1, "and the failure: {:?}", day.compute_ms);
    for (ms, expected) in [(0.0, "0-100ms"), (99.9, "0-100ms"), (100.0, "100ms-1s"), (999.0, "100ms-1s"), (1000.0, "1s-10s"), (10_000.0, "10s+")] {
        assert_eq!(compute_bucket(ms), expected, "{ms}");
    }
    assert_eq!(error_kind(&solidcraft_engine::EngineError::Cancelled("x".into())), None, "a cancel is no failure");
    let feature = solidcraft_engine::EngineError::Feature(Box::new(solidcraft_engine::FeatureFailure {
        error: "the fillet radius is too large for the Secret Bracket".into(),
        code: solidcraft_engine::kernel::ErrorCode { call: "Blend".into(), kind: 7, operation: "smlib::blend::rolling".into() },
    }));
    assert_eq!(error_kind(&feature).as_deref(), Some("kernel.blend.7"), "the call and the kernel's number only");
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
    let mut store = Store { install_id: new_id().unwrap(), notice_ok: true, ..Store::default() };
    let ui: Counts = [("ui".to_string(), [("solid.box".to_string(), 2)].into())].into();
    store.days.insert("2023-11-14".into(), Day { active_minutes: 5, operations: ui.clone(), tools: ui, ..Day::default() });
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

#[test]
fn nothing_is_counted_or_sent_before_ok_and_ok_starts_it() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(200));
    app.usage.store.notice_ok = false;
    assert!(notice_pending(&app), "the first start shows it");
    // Days of use, sends due: the counter doesn't run at all, and nothing is queued or sent.
    for day in 0..3 {
        use_app(&mut app, T0 + day * DAY, 100.0 + day as f64 * RETRY_HOURS * 3600.0);
    }
    assert!(!app.session.count_usage);
    assert!(app.usage.store.days.is_empty(), "nothing queued before OK");
    assert!(requests.lock().unwrap().is_empty(), "nothing sent before OK");
    assert!(solidcraft_engine::usage::take().is_empty());
    // × hides it for this run only, and still nothing counts.
    act(&mut app, NoticeAction::Close);
    assert!(!notice_pending(&app));
    use_app(&mut app, T0, 0.0);
    assert!(app.usage.store.days.is_empty(), "closing is no OK");
    let store = app.usage.store.clone();
    let (mut again, _) = app_with_server(Some(200));
    again.usage.store = store;
    assert!(notice_pending(&again), "shown again at the next start");
    act(&mut again, NoticeAction::Ok);
    assert!(!notice_pending(&again));
    use_app(&mut again, T0, 0.0);
    assert!(again.session.count_usage && !again.usage.store.days.is_empty(), "OK starts counting");
}

#[test]
fn a_0_46_installation_sees_the_new_notice_and_counting_pauses_until_ok() {
    // 0.46.0 kept `notice_shown`, 0.46.1 also `notice_dismissed`; neither was an OK.
    for stored in [
        r#"{"install_id": "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed", "notice_shown": true, "days": {"2023-11-14": {"active_minutes": 3}}}"#,
        r#"{"install_id": "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed", "notice_shown": true, "notice_dismissed": true}"#,
    ] {
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        app.load_usage_stats(Some(stored));
        assert!(!app.usage.store.notice_ok);
        assert!(notice_pending(&app), "the new notice shows");
        assert!(!active(&app), "counting pauses until OK");
        tick_at(&mut app, T0, 0.0, true);
        assert!(app.usage.store.days.is_empty(), "and what 0.46 queued isn't sent");
    }
    // Someone who turned sharing off stays off, with no notice.
    let mut off_before = SolveApp::new(Session::default(), crate::Services::default());
    off_before.preferences.usage_statistics = false;
    off_before.load_usage_stats(Some(r#"{"notice_shown": true}"#));
    assert!(!notice_pending(&off_before) && !active(&off_before));
}

#[test]
fn no_notice_when_the_organization_or_the_environment_turned_statistics_off() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for policy in [true, false] {
        let (mut app, _) = app_with_server(Some(200));
        app.usage.store.notice_ok = false;
        if policy {
            app.policy = crate::policy::Policy::from_json(r#"{"UsageStatistics": 0}"#);
        } else {
            app.usage.env_off = true;
        }
        assert!(!notice_pending(&app));
        use_app(&mut app, T0, 0.0);
        assert!(app.usage.store.days.is_empty(), "nothing starts counting");
    }
}

#[test]
fn settings_alone_doesnt_start_counting_and_off_there_ends_the_notice() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(200));
    app.usage.store.notice_ok = false;
    act(&mut app, NoticeAction::Settings);
    assert!(app.prefs_window.shell.open);
    assert_eq!(app.prefs_window.shell.section, crate::prefs::PAGE_PRIVACY);
    assert!(notice_pending(&app), "Settings is no OK: the notice stays");
    use_app(&mut app, T0, 0.0);
    assert!(app.usage.store.days.is_empty() && !app.session.count_usage, "Settings alone counts nothing");
    // Off in Preferences ends the notice, with no counting.
    app.preferences.usage_statistics = false;
    assert!(!notice_pending(&app));
    use_app(&mut app, T0 + DAY, 1.0e6);
    assert!(app.usage.store.days.is_empty());
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn the_privacy_page_shows_the_organizations_choice_locked() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.usage.hosted = true;
    h.app.usage.store.notice_ok = true;
    h.app.policy = crate::policy::Policy::from_json(r#"{"UsageStatistics": 0}"#);
    crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_PRIVACY);
    h.frames(3);
    assert!(h.widget("Set by your organization").is_some());
    h.step(&json!({"click":{"handle":"prefs:usage_statistics"}})).unwrap();
    h.frames(1);
    assert!(h.app.preferences.usage_statistics, "the person's own choice is kept and can't be changed here");
    assert_eq!(off(&h.app), Some(Off::Policy));
    h.step(&json!({"click":{"handle":"prefs:usage_statistics:show"}})).unwrap();
    h.frames(2);
    assert!(h.widget("Nothing is sent while usage statistics are off.").is_some());
}

#[test]
fn the_privacy_page_turns_statistics_off_and_shows_the_next_report() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.usage.hosted = true;
    h.app.usage.store.notice_ok = true;
    crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_PRIVACY);
    h.frames(3);
    h.step(&json!({"click":{"handle":"prefs:usage_statistics:show"}})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("prefs:usage_statistics:report").is_some(), "the next report's JSON is shown");
    h.step(&json!({"click":{"handle":"prefs:usage_statistics"}})).unwrap();
    h.frames(2);
    assert!(!h.app.preferences.usage_statistics);
    assert!(h.app.usage.store.days.is_empty());
}

#[test]
fn the_notice_shows_on_a_real_first_start_and_never_in_an_app_that_keeps_nothing() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Tests, scenarios and hosts that keep nothing between runs: no notice, nothing counted.
    let mut kept_nothing = SolveApp::new(Session::default(), crate::Services::default());
    assert!(!notice_pending(&kept_nothing));
    tick_at(&mut kept_nothing, T0, 0.0, true);
    assert!(!kept_nothing.session.count_usage);
    assert!(kept_nothing.usage.store.days.is_empty());
    let mut h = crate::scenario::Harness::new();
    h.frames(5);
    assert!(crate::scenario::rect_of("toast:usage_stats").is_none(), "the harness never shows it");
    // The app's first start: the host loads what it kept, which is nothing yet.
    let mut first = SolveApp::new(Session::default(), crate::Services::default());
    first.load_usage_stats(None);
    assert!(notice_pending(&first), "the first start shows the notice");
    act(&mut first, NoticeAction::Ok);
    // The next start loads what this one kept: answered, no second notice.
    let kept = first.usage_stats_for_storage();
    let mut next = SolveApp::new(Session::default(), crate::Services::default());
    next.load_usage_stats(Some(&kept));
    assert!(!notice_pending(&next));
    // And the real first start shows it as the shared toast.
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.load_usage_stats(None);
    h.frames(5);
    assert!(crate::scenario::rect_of("toast:usage_stats").is_some(), "the toast is drawn on a real first start");
}

#[test]
fn delete_my_data_asks_the_server_to_delete_the_old_id_and_uses_a_new_one() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(Some(204));
    use_app(&mut app, T0, 0.0);
    let old = app.usage.store.install_id.clone();
    // With statistics turned off it still goes: what was sent earlier stays until deleted.
    app.preferences.usage_statistics = false;
    assert!(delete_my_data(&mut app));
    assert_ne!(app.usage.store.install_id, old);
    assert!(app.usage.store.days.is_empty());
    tick_at(&mut app, T0, 1.0, false);
    tick_at(&mut app, T0, 2.0, false);
    let sent = requests.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!((sent[0].method, sent[0].url.as_str()), ("DELETE", format!("{INSTALL_ENDPOINT}{old}").as_str()));
    assert!(sent[0].body.is_none(), "nothing but the id");
    assert!(app.usage.store.forget.is_empty(), "done once the server answered");
    // Nothing more goes, however many frames follow.
    for frame in 0..100 {
        tick_at(&mut app, T0, 3.0 + f64::from(frame), false);
    }
    assert_eq!(requests.lock().unwrap().len(), 1);
}

#[test]
fn a_deletion_the_server_didnt_take_waits_an_hour_and_survives_a_restart() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, requests) = app_with_server(None);
    use_app(&mut app, T0, 0.0);
    let old = app.usage.store.install_id.clone();
    assert!(delete_my_data(&mut app));
    for second in 0..600 {
        tick_at(&mut app, T0, 1.0 + f64::from(second), false);
    }
    assert_eq!(requests.lock().unwrap().len(), 1, "one try, no storm");
    assert_eq!(app.usage.store.forget, std::slice::from_ref(&old));
    let kept = Store::from_stored(Some(&app.usage.store.to_stored()));
    assert_eq!(kept.forget, [old], "it waits across starts");
    tick_at(&mut app, T0, 3700.0, false);
    assert_eq!(requests.lock().unwrap().len(), 2, "tried again an hour later");
}

#[test]
fn no_deletion_goes_when_the_organization_or_the_environment_turned_statistics_off() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for policy in [true, false] {
        let (mut app, requests) = app_with_server(Some(204));
        use_app(&mut app, T0, 0.0);
        if policy {
            app.policy = crate::policy::Policy::from_json(r#"{"UsageStatistics": 0}"#);
        } else {
            app.usage.env_off = true;
        }
        assert!(!delete_my_data(&mut app));
        tick_at(&mut app, T0, 1.0, false);
        assert!(requests.lock().unwrap().is_empty());
    }
}

#[test]
fn the_host_reaches_only_the_report_and_delete_addresses() {
    let id = new_id().unwrap();
    assert!(allowed_url(ENDPOINT));
    assert!(allowed_url(&format!("{INSTALL_ENDPOINT}{id}")));
    for not in [
        format!("{INSTALL_ENDPOINT}{id}/x"),
        format!("{INSTALL_ENDPOINT}../report"),
        INSTALL_ENDPOINT.to_string(),
        "https://metrics.solidcraft.io/v1/summary".to_string(),
        "https://example.com/v1/report".to_string(),
        format!("{ENDPOINT}?x=1"),
    ] {
        assert!(!allowed_url(&not), "{not}");
    }
}

#[test]
fn the_toast_stays_until_ok_and_ok_starts_counting() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.load_usage_stats(None);
    // A minute of frames: still there, nothing counted.
    h.frames(3600);
    assert!(crate::scenario::rect_of("toast:usage_stats").is_some(), "it never goes by itself");
    assert!(!h.app.session.count_usage);
    // Its buttons: Settings, then OK (the main one, right-most).
    let ok = crate::scenario::rect_of("toast:usage_stats:button:1").expect("OK");
    h.step(&json!({"click": [ok.center().x, ok.center().y]})).unwrap();
    h.frames(2);
    assert!(h.app.usage.store.notice_ok);
    assert!(crate::scenario::rect_of("toast:usage_stats").is_none());
    assert!(h.app.session.count_usage, "OK starts counting");
}

#[test]
fn the_x_hides_the_notice_until_the_next_start_and_counts_nothing() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.load_usage_stats(None);
    h.frames(3);
    h.step(&json!({"click": {"handle": "toast:usage_stats:close"}})).unwrap();
    h.frames(2);
    assert!(crate::scenario::rect_of("toast:usage_stats").is_none());
    assert!(!h.app.usage.store.notice_ok && !h.app.session.count_usage);
    let kept = h.app.usage_stats_for_storage();
    let mut next = SolveApp::new(Session::default(), crate::Services::default());
    next.load_usage_stats(Some(&kept));
    assert!(notice_pending(&next), "back at the next start");
}

#[test]
fn delete_my_data_says_it_was_requested_and_shows_the_new_id() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.load_usage_stats(None);
    agree(&mut h.app);
    h.app.usage.store.install_id = new_id().unwrap();
    crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_PRIVACY);
    h.frames(3);
    h.step(&json!({"click":{"handle":"prefs:usage_statistics:delete"}})).unwrap();
    h.frames(2);
    let new = h.app.usage.store.install_id.clone();
    let said = format!("Deletion requested. New installation ID: {new}");
    assert!(h.widget(&said).is_some(), "said on the page");
    assert!(h.app.status.as_ref().is_some_and(|(text, _, _)| *text == said), "and as the status");
}

#[test]
fn delete_my_data_waits_for_the_one_on_its_way() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut h = crate::scenario::Harness::new();
    h.app.home.open = false;
    h.app.load_usage_stats(None);
    agree(&mut h.app);
    h.app.usage.store.install_id = new_id().unwrap();
    crate::prefs::open_at(&mut h.app, crate::prefs::PAGE_PRIVACY);
    h.frames(3);
    // No server in the harness: the first deletion stays on its way.
    for _ in 0..20 {
        let _ = h.step(&json!({"click":{"handle":"prefs:usage_statistics:delete"}}));
        h.frames(1);
    }
    assert_eq!(h.app.usage.store.forget.len(), 1, "twenty clicks, one deletion: {:?}", h.app.usage.store.forget);
}
