//! Completed distances must reach the viewport before a continuously moving pointer stops.
use super::*;
use serde_json::json;

fn colors() -> Colors {
    Colors { body: [1, 1, 1, 255], added: [2, 2, 2, 255], new_body: [2, 2, 2, 255], removed: [3, 3, 3, 100], edge: [0, 0, 0, 255], ghost: true }
}

fn fixture() -> Session {
    let mut s = Session::new(solidcraft_engine::doc::Document::new("Preview drag"));
    s.execute("sketch.create", &json!({"plane":"XY", "name":"Profile"})).unwrap();
    s.execute("sketch.rectangle.two_point", &json!({"p0":[0,0], "p1":[10,10]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    s
}

fn extrude(distance: i32) -> Vec<(String, Value)> {
    vec![("solid.extrude".into(), json!({"sketch":"Profile", "distance":distance, "operation":"new"}))]
}

fn check_drag(s: &Session, commands: impl Fn(i32) -> Vec<(String, Value)>, base_z: f64) {
    let original = serde_json::to_value(&*s.doc).unwrap();
    let undo = s.history.len();
    let mut pv = PreviewState::default();
    for (finished, wanted) in [(10, 20), (20, 30), (30, 30)] {
        let old = commands(finished);
        let new = commands(wanted);
        let old_key = job_key(&old, s.revision, colors());
        let key = job_key(&new, s.revision, colors());
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((old_key, rx));
        pv.running_tool = as_new_body(&old).is_some();
        pv.running_stream_key = stream_key(&old, s.revision, colors());
        let built = compute_tool(s, &old, colors()).unwrap();
        tx.send(Ok(built)).unwrap();
        pv.poll(key, stream_key(&new, s.revision, colors()));
        assert!(pv.active && pv.error.is_none(), "completed geometry must show during the drag: {:?}", pv.error);
        assert!((pv.bounds.max.z - base_z - f64::from(finished)).abs() < 1e-8);
        assert_eq!(pv.shown_key, Some(old_key));
        assert_eq!(pv.shown_key == Some(key), finished == wanted, "only the final distance satisfies the latest request");
        assert!(pv.running.is_none());
    }
    assert_eq!(serde_json::to_value(&*s.doc).unwrap(), original);
    assert_eq!(s.history.len(), undo);
}

#[test]
fn new_body_extrude_keeps_moving_during_a_drag() {
    check_drag(&fixture(), extrude, 0.0);
}

#[test]
fn edited_new_body_extrude_keeps_moving_during_a_drag() {
    let mut s = fixture();
    let feature = s.execute("solid.extrude", &extrude(5)[0].1).unwrap()["feature"].as_u64().unwrap();
    check_drag(
        &s,
        |distance| vec![("timeline.redefine".into(), json!({"feature":feature,"command":"solid.extrude","params":extrude(distance)[0].1}))],
        0.0,
    );
}

#[test]
fn face_offset_keeps_moving_during_a_drag() {
    let mut s = Session::new(solidcraft_engine::doc::Document::new("Face offset drag"));
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    check_drag(&s, |distance| vec![("solid.offset_face".into(), json!({"faces":[[5,5,10]],"distance":distance}))], 10.0);
}

#[test]
fn join_tool_keeps_moving_without_waiting_for_its_boolean() {
    let mut s = fixture();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":5})).unwrap();
    check_drag(&s, |distance| vec![("solid.extrude".into(), json!({"sketch":"Profile","distance":distance,"operation":"join"}))], 0.0);
}

#[test]
fn distance_stream_keeps_feature_selection_and_extent_context() {
    let old = extrude(10);
    let key = stream_key(&old, 7, colors());
    assert!(key.is_some());
    assert_eq!(key, stream_key(&extrude(20), 7, colors()));
    assert_ne!(key, stream_key(&old, 8, colors()));
    let mut recolored = colors();
    recolored.added = [40, 50, 60, 255];
    assert_ne!(key, stream_key(&old, 7, recolored));
    for (field, value) in [
        ("sketch", json!("Other")),
        ("profiles", json!([1])),
        ("face", json!([0, 0, 5])),
        ("operation", json!("cut")),
        ("direction", json!("negative")),
        ("distance2", json!(5)),
        ("to", json!([0, 0, 50])),
        ("taper", json!(5)),
        ("start_offset", json!(3)),
        ("participants", json!(["Other"])),
    ] {
        let mut changed = old.clone();
        changed[0].1[field] = value;
        assert_ne!(key, stream_key(&changed, 7, colors()), "changed {field} cannot reuse the old result");
    }
    for field in ["distance2", "to_offset"] {
        let mut from = old.clone();
        from[0].1[field] = json!(5);
        let mut to = from.clone();
        to[0].1[field] = json!(9);
        assert_eq!(stream_key(&from, 7, colors()), stream_key(&to, 7, colors()));
    }
    let edit =
        |feature, distance| vec![("timeline.redefine".into(), json!({"feature":feature,"command":"solid.extrude","params":extrude(distance)[0].1}))];
    assert_eq!(stream_key(&edit(2, 10), 7, colors()), stream_key(&edit(2, 20), 7, colors()));
    assert_ne!(stream_key(&edit(2, 10), 7, colors()), stream_key(&edit(3, 20), 7, colors()));
    assert_ne!(key, stream_key(&edit(2, 10), 7, colors()));
    let offset = |distance| vec![("solid.offset_face".into(), json!({"faces":[[5,5,10]],"distance":distance}))];
    assert!(stream_key(&offset(1), 7, colors()).is_some());
    assert_eq!(stream_key(&offset(1), 7, colors()), stream_key(&offset(5), 7, colors()));
    assert!(stream_key(&[], 7, colors()).is_none());
    let fillet = |radius| vec![("solid.fillet".into(), json!({"edges":[[5,0,10]],"radius":radius}))];
    assert_eq!(stream_key(&fillet(1), 7, colors()), stream_key(&fillet(2), 7, colors()));
    let other_edge = vec![("solid.fillet".into(), json!({"edges":[[0,5,10]],"radius":1}))];
    assert_ne!(stream_key(&fillet(1), 7, colors()), stream_key(&other_edge, 7, colors()), "other edges are another setup");
    assert!(stream_key(&[("solid.shell".into(), json!({"thickness":1}))], 7, colors()).is_none());
    let mut mixed = old;
    mixed.push(("select.clear".into(), json!({})));
    assert!(stream_key(&mixed, 7, colors()).is_none());
}

#[test]
fn distance_results_reject_other_setups_and_obsolete_errors() {
    let s = fixture();
    let old = extrude(10);
    let current = extrude(20);
    for (revision, failed) in [(s.revision + 1, false), (s.revision, true)] {
        let mut built = compute_tool(&s, &old, colors()).unwrap();
        if failed {
            built.error = Some("old distance failed".into());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let mut pv = PreviewState {
            running: Some((job_key(&old, s.revision, colors()), rx)),
            running_stream_key: stream_key(&old, s.revision, colors()),
            ..Default::default()
        };
        tx.send(Ok(built)).unwrap();
        pv.poll(job_key(&current, revision, colors()), stream_key(&current, revision, colors()));
        assert!(!pv.active && pv.error.is_none() && pv.shown_key.is_none());
    }
    let mut join = current.clone();
    join[0].1["operation"] = json!("join");
    assert!(as_new_body(&join).is_some());
    assert!(stream_key(&join, s.revision, colors()).is_none(), "tool previews retain their upstream streaming path");
}

/// A fillet's radius dragged (or typed): each finished preview shows while the next radius
/// computes, as a distance does; before, every one was dropped until the radius held still.
#[test]
fn fillet_preview_keeps_moving_while_its_radius_changes() {
    let mut s = Session::new(solidcraft_engine::doc::Document::new("Fillet drag"));
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let fillet = |radius: i32| vec![("solid.fillet".into(), json!({"edges":[[5,0,10]],"radius":f64::from(radius) / 4.0}))];
    let mut pv = PreviewState::default();
    for (finished, wanted) in [(4, 6), (6, 8), (8, 8)] {
        let (old, new) = (fillet(finished), fillet(wanted));
        let old_key = job_key(&old, s.revision, colors());
        let key = job_key(&new, s.revision, colors());
        let (tx, rx) = std::sync::mpsc::channel();
        pv.running = Some((old_key, rx));
        pv.running_tool = false;
        pv.running_stream_key = stream_key(&old, s.revision, colors());
        tx.send(compute(&s, &old, colors())).unwrap();
        pv.poll(key, stream_key(&new, s.revision, colors()));
        assert!(pv.active && pv.error.is_none(), "the finished radius shows during the drag: {:?}", pv.error);
        assert_eq!(pv.shown_key, Some(old_key));
        assert!(pv.running.is_none());
    }
}

/// Edges picked one after another (#7167): the preview of the previous selection, still
/// running, is left behind and asked to stop, so the current selection's starts at once instead
/// of after it. `DETACHED` jobs are left behind at most: while they run, the next obsolete one is
/// waited for. A radius changing on the same edges still streams (above).
#[test]
fn a_new_selection_does_not_wait_for_the_previous_selections_preview() {
    let s = Session::default();
    let picks: Vec<Vec<(String, Value)>> = (1..=DETACHED + 2)
        .map(|n| {
            let edges: Vec<Value> = (0..n).map(|i| json!([5, i, 10])).collect();
            vec![("solid.fillet".into(), json!({"edges":edges,"radius":1.0}))]
        })
        .collect();
    let keys = |c: &[(String, Value)]| (job_key(c, s.revision, colors()), stream_key(c, s.revision, colors()));
    let running = |pv: &mut PreviewState, c: &[(String, Value)]| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let progress = Arc::new(Progress::default());
        pv.running = Some((keys(c).0, rx));
        pv.running_progress = Some(progress.clone());
        pv.running_stream_key = keys(c).1;
        (tx, progress)
    };
    let mut pv = PreviewState::default();
    let mut left = Vec::new();
    for (n, next) in picks[1..=DETACHED].iter().enumerate() {
        let job = running(&mut pv, &picks[n]);
        let (key, stream) = keys(next);
        pv.poll(key, stream);
        assert!(pv.running.is_none(), "selection {} can start at once", n + 2);
        assert!(job.1.is_cancelled() && pv.detached.len() == n + 1, "the obsolete job is asked to stop and counted");
        left.push(job);
    }
    let (_tx, waited) = running(&mut pv, &picks[DETACHED]);
    let (key, stream) = keys(&picks[DETACHED + 1]);
    pv.poll(key, stream);
    assert!(pv.running.is_some() && !waited.is_cancelled(), "with {DETACHED} jobs left behind, the next obsolete one is waited for");

    // One that was left behind ends: its result is dropped and its place is free again.
    left[0].0.send(Err("stopped".into())).unwrap();
    pv.poll(key, stream);
    assert!(pv.detached.len() == DETACHED && waited.is_cancelled(), "the waited one is left behind in turn");
    assert!(pv.running.is_none() && pv.shown_key.is_none() && pv.error.is_none(), "no obsolete result is shown");

    // A radius changing on the same edges is the same setup: the running job is kept.
    let mut pv = PreviewState::default();
    let (_tx, progress) = running(&mut pv, &picks[0]);
    let mut bigger = picks[0].clone();
    bigger[0].1["radius"] = json!(2.0);
    let (key, stream) = keys(&bigger);
    pv.poll(key, stream);
    assert!(pv.running.is_some() && !progress.is_cancelled() && pv.detached.is_empty());

    // A tool, and inputs that don't stream (no stream key), are waited for as before.
    let (key, _) = keys(&picks[1]);
    for (tool, stream) in [(true, keys(&picks[1]).1), (false, None)] {
        let mut pv = PreviewState::default();
        let (_tx, progress) = running(&mut pv, &picks[0]);
        pv.running_tool = tool;
        pv.poll(key, stream);
        assert!(pv.running.is_some() && !progress.is_cancelled() && pv.detached.is_empty(), "tool {tool}, stream {stream:?}");
    }

    // A dialog closed with jobs left behind still counts the one it leaves (it keeps running),
    // so no further one is left behind until they end; one that ends without a result too.
    let mut pv = PreviewState::default();
    let mut senders = Vec::new();
    for n in 0..DETACHED {
        let (tx, _) = running(&mut pv, &picks[n]);
        let (key, stream) = keys(&picks[n + 1]);
        pv.poll(key, stream);
        senders.push(tx);
    }
    let (tx, progress) = running(&mut pv, &picks[DETACHED]);
    pv.clear();
    assert!(progress.is_cancelled() && pv.detached.len() == DETACHED + 1, "the closed dialog's job is counted");
    let (_tx, waited) = running(&mut pv, &picks[0]);
    let (key, stream) = keys(&picks[1]);
    pv.poll(key, stream);
    assert!(pv.running.is_some() && !waited.is_cancelled(), "over the limit, the next obsolete job is waited for");
    drop(senders);
    drop(tx);
    pv.poll(key, stream);
    assert!(waited.is_cancelled() && pv.detached.len() == 1, "jobs that ended without a result free their places");
}

/// The job `spawn` starts is the one a newer selection asks to stop (#7167): held before its
/// feature, it is left behind, skips it once cancelled and ends; its result is never shown.
#[test]
fn a_preview_left_behind_stops_before_its_next_feature() {
    let mut s = Session::new(solidcraft_engine::doc::Document::new("Left behind"));
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let fillet = s.execute("solid.fillet", &json!({"edges":[[5,0,10]],"radius":1.0})).unwrap()["feature"].as_u64().unwrap();
    let redefine = |edges: Value| {
        vec![("timeline.redefine".to_string(), json!({"feature":fillet,"command":"solid.fillet","params":{"edges":edges,"radius":2.0}}))]
    };
    let (first, second) = (redefine(json!([[5, 0, 10]])), redefine(json!([[5, 0, 10], [0, 5, 10]])));
    let keys = |c: &[(String, Value)]| (job_key(c, s.revision, colors()), stream_key(c, s.revision, colors()));
    assert!(keys(&first).1.is_some() && keys(&first).1 != keys(&second).1, "another selection is another setup");
    let mut pv = PreviewState::default();
    let progress = Arc::new(Progress::default());
    // The job waits before the fillet (feature 1) until it is cancelled.
    progress.hold_at(1);
    pv.running_stream_key = keys(&first).1;
    spawn(&mut pv, keys(&first).0, s.scratch(), first, colors(), false, progress.clone());
    let (key, stream) = keys(&second);
    pv.poll(key, stream);
    assert!(progress.is_cancelled() && pv.running.is_none() && pv.detached.len() == 1, "left behind and asked to stop");
    let t0 = std::time::Instant::now();
    while !pv.detached.is_empty() {
        assert!(t0.elapsed() < std::time::Duration::from_secs(30), "the held job did not stop");
        std::thread::sleep(std::time::Duration::from_millis(5));
        pv.poll(key, stream);
    }
    assert!(pv.shown_key.is_none() && !pv.active && pv.error.is_none(), "its result is never shown");
}
