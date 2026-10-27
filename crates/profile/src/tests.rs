use super::*;

/// The profiler is global: tests that switch it on and off run one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn spin(ms: u64) {
    let t = web_time::Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {}
}

fn row(r: &Report, name: &str) -> Row {
    r.rows.iter().find(|x| x.name == name).copied().unwrap_or_else(|| panic!("no row {name}: {r:?}"))
}

#[test]
fn nothing_is_recorded_while_off() {
    let _s = serial();
    set_enabled(false);
    reset();
    {
        let _a = span("test", "off");
        spin(1);
    }
    record("test", "off_record", Duration::from_millis(3));
    assert!(report().rows.is_empty());
}

#[test]
fn nested_spans_have_self_and_total_time() {
    let _s = serial();
    reset();
    set_enabled(true);
    {
        let _outer = span("test", "outer");
        spin(4);
        {
            let _inner = span("test", "inner");
            spin(4);
        }
        // Timed by someone else (SMLib's C API): a child of `outer`.
        record("smlib", "call", Duration::from_millis(2));
    }
    set_enabled(false);
    let r = report();
    let (outer, inner, call) = (row(&r, "outer"), row(&r, "inner"), row(&r, "call"));
    assert_eq!((outer.count, inner.count, call.count), (1, 1, 1));
    assert!(outer.total >= Duration::from_millis(8), "{outer:?}");
    assert!(inner.self_time >= Duration::from_millis(4), "{inner:?}");
    assert_eq!(call.self_time, Duration::from_millis(2));
    // The outer span's self time leaves out both children.
    assert_eq!(outer.self_time, outer.total - inner.total - call.total, "{r:?}");
    assert_eq!(r.events, 3);
    let cats = r.by_category();
    assert_eq!(cats.iter().map(|c| c.0).collect::<Vec<_>>().len(), 2);
    let t = r.table(10);
    assert!(t.contains("test outer") && t.contains("smlib call") && t.contains("self time by category"), "{t}");
}

#[test]
fn a_span_inside_one_of_the_same_name_counts_once_in_the_total() {
    let _s = serial();
    reset();
    set_enabled(true);
    {
        let _a = span("command", "x");
        spin(2);
        let _b = span("command", "x");
        spin(2);
    }
    set_enabled(false);
    let x = row(&report(), "x");
    assert_eq!(x.count, 2);
    assert!(x.total >= x.max && x.total <= x.max + Duration::from_micros(1), "only the outer one: {x:?}");
}

#[test]
fn a_mark_reports_only_what_came_after_it() {
    let _s = serial();
    reset();
    set_enabled(true);
    drop(span("test", "before"));
    let m = mark();
    drop(span("test", "after"));
    drop(span("test", "after"));
    set_enabled(false);
    let r = m.report();
    assert_eq!(r.rows.len(), 1, "{r:?}");
    assert_eq!(row(&r, "after").count, 2);
}

#[test]
fn spans_on_other_threads_get_their_own_track() {
    let _s = serial();
    reset();
    set_enabled(true);
    let _main = span("test", "main");
    std::thread::Builder::new().name("worker".into()).spawn(|| drop(span("test", "on_worker"))).unwrap().join().unwrap();
    drop(_main);
    set_enabled(false);
    let mut out = Vec::new();
    write_chrome_trace(&mut out).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let events = v["traceEvents"].as_array().unwrap();
    let tid = |name: &str| events.iter().find(|e| e["name"] == name).map(|e| e["tid"].clone()).unwrap();
    assert_ne!(tid("main"), tid("on_worker"));
    assert!(events.iter().any(|e| e["name"] == "thread_name" && e["args"]["name"] == "worker"), "{v}");
    let main = events.iter().find(|e| e["name"] == "main").unwrap();
    assert_eq!((main["ph"].as_str(), main["cat"].as_str()), (Some("X"), Some("test")));
    assert!(main["dur"].as_f64().is_some_and(|d| d >= 0.0));
}

#[test]
fn the_trace_is_capped_and_the_aggregate_is_not() {
    let _s = serial();
    reset();
    set_enabled(true);
    for _ in 0..MAX_EVENTS + 10 {
        drop(span("test", "many"));
    }
    set_enabled(false);
    let r = report();
    assert_eq!((r.events, r.dropped, row(&r, "many").count), (MAX_EVENTS, 10, MAX_EVENTS as u64 + 10));
    reset();
    assert_eq!(report().events, 0);
}

#[test]
fn a_panic_inside_a_span_still_closes_it() {
    let _s = serial();
    reset();
    set_enabled(true);
    let _ = std::panic::catch_unwind(|| {
        let _outer = span("test", "unwound");
        let _inner = span("test", "unwound_inner");
        std::panic::panic_any("kernel");
    });
    drop(span("test", "after_panic"));
    set_enabled(false);
    let r = report();
    assert_eq!(row(&r, "unwound").count, 1);
    // The stack is clean again: the next span is a root, its total its own.
    let after = row(&r, "after_panic");
    assert_eq!(after.total, after.self_time);
}

#[test]
fn interned_names_are_kept_and_bounded() {
    let a = intern("smlib_boolean");
    assert!(std::ptr::eq(a, intern("smlib_boolean")));
    assert_eq!(a, "smlib_boolean");
}
