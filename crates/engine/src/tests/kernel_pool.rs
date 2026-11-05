//! A design's bodies are made and used on whichever kernel of the pool is free
//! (`solidcraft_smlib`'s `core::Pool`). What it builds must not depend on which: to the last
//! number, on a new kernel or one that has done much, while another is busy, with bodies made on
//! one kernel going into operations on another, through the evaluation cache, undo and redo.

use super::*;

/// A hole, a round and a chamfer, each building on the last.
fn features() -> Vec<(&'static str, Value)> {
    vec![
        ("solid.box", json!({"length": 40, "width": 30, "height": 20})),
        ("solid.cylinder", json!({"base": [12, 15, -1], "axis": [0, 0, 1], "radius": 3.4, "height": 30, "operation": "cut"})),
        ("solid.fillet", json!({"edges": [[0, 0, 10]], "radius": 2})),
        ("solid.chamfer", json!({"edges": [[20, 0, 20]], "distance": 1})),
    ]
}

fn join() -> (&'static str, Value) {
    ("solid.box", json!({"corner": [25, 10, 20], "length": 10, "width": 10, "height": 5, "operation": "join"}))
}

/// Everything a design built, exactly: each body's name, whole B-rep record and triangle count.
fn built(s: &Session) -> Vec<String> {
    let st = s.model.state();
    st.bodies
        .iter()
        .map(|b| {
            let record = serde_json::to_string(b.body.record()).unwrap();
            format!("{} {} {record}", b.name, b.mesh().triangles.len())
        })
        .collect()
}

fn design(features: &[(&'static str, Value)]) -> Session {
    let mut s = Session::default();
    for (id, p) in features {
        run(&mut s, id, p.clone());
    }
    s
}

fn assert_same(what: &str, a: &[String], b: &[String]) {
    assert_eq!(a.len(), b.len(), "{what}: how many bodies");
    assert!(a == b, "{what}: the design built differently");
}

#[test]
fn a_design_rebuilds_the_same_on_any_kernel() {
    let f = features();
    let first = built(&design(&f));
    assert!(!first.is_empty());
    // On a new kernel, the one that built it above busy elsewhere.
    let held = solidcraft_kernel::hold_a_kernel();
    assert_same("a new kernel", &first, &built(&design(&f)));
    // And another new one, with both of those out of use.
    let held_too = solidcraft_kernel::hold_a_kernel();
    assert_same("another new kernel", &first, &built(&design(&f)));
    drop((held, held_too));
    // On one that has built plenty since.
    for _ in 0..3 {
        design(&f);
    }
    assert_same("a kernel that did much before", &first, &built(&design(&f)));
}

/// A curvature round, whose procedural blends take their serials from the kernel: one body made
/// on each of two kernels, the serials of neither mistaken for the other's in the kernel that
/// solves stations for both.
#[test]
fn a_procedural_blend_builds_the_same_on_any_kernel() {
    let f = vec![
        ("solid.box", json!({"length": 40, "width": 30, "height": 20})),
        ("solid.fillet", json!({"edges": [[20, 0, 20]], "radius": 3, "type": "curvature"})),
    ];
    let first = design(&f);
    let held = solidcraft_kernel::hold_a_kernel();
    let second = design(&f);
    let b = built(&second);
    assert_same("a new kernel", &built(&first), &b);
    let v = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    let (mut first, mut second) = (first, second);
    let beside = v(&mut second);
    drop(held);
    assert_eq!(v(&mut first).to_bits(), beside.to_bits(), "one measured right after the other on one kernel");
}

/// A modeled thread, a screw sweep and its boolean, on a short hole.
#[test]
fn a_thread_builds_the_same_on_any_kernel() {
    let f = vec![
        ("solid.box", json!({"length": 20, "width": 20, "height": 4})),
        ("solid.cylinder", json!({"base": [10, 10, -1], "axis": [0, 0, 1], "radius": 3.4, "height": 10, "operation": "cut"})),
        ("solid.thread", json!({"face": [13.4, 10, 2], "modeled": true})),
    ];
    let first = built(&design(&f));
    let held = solidcraft_kernel::hold_a_kernel();
    assert_same("a new kernel", &first, &built(&design(&f)));
    drop(held);
}

/// Bodies from the evaluation cache and the undo stack go on into operations on another kernel:
/// the join's boolean takes a body the held kernel made, undo restores it, redo brings back the
/// join's, and each is what the whole design built in one go on a single kernel gives.
#[test]
fn bodies_from_another_kernel_go_on_through_the_cache_undo_and_redo() {
    let f = features();
    let mut all = f.clone();
    all.push(join());
    let (before, after) = (built(&design(&f)), built(&design(&all)));

    let mut s = design(&f);
    let held = solidcraft_kernel::hold_a_kernel();
    let (id, p) = join();
    run(&mut s, id, p);
    assert_same("joined on another kernel", &after, &built(&s));
    run(&mut s, "edit.undo", json!({}));
    assert_same("undone", &before, &built(&s));
    drop(held);
    let held = solidcraft_kernel::hold_a_kernel();
    run(&mut s, "edit.redo", json!({}));
    assert_same("redone", &after, &built(&s));
    // Rebuilt without the cache, every feature again, while that kernel stays out of use.
    s.model.no_cache = true;
    *s.model.cache.lock().unwrap() = Default::default();
    run(&mut s, "timeline.roll_to", json!({"position": 0}));
    run(&mut s, "timeline.roll_to", json!({}));
    assert_same("rebuilt from the start", &after, &built(&s));
    drop(held);
    // And the body goes on from there, back on the first kernel.
    run(&mut s, "solid.box", json!({"corner": [-5, 0, 0], "length": 5, "width": 5, "height": 5, "operation": "join"}));
    let mut more = all.clone();
    more.push(("solid.box", json!({"corner": [-5, 0, 0], "length": 5, "width": 5, "height": 5, "operation": "join"})));
    assert_same("a further join", &built(&design(&more)), &built(&s));
}

/// A design rebuilt while other work keeps kernels of the pool busy on other threads builds what
/// it builds alone, whichever kernels its calls happen to get.
#[test]
fn a_design_rebuilt_beside_other_work_builds_the_same() {
    let f = features();
    let alone = built(&design(&f));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let busy: Vec<_> = (0..3)
        .map(|_| {
            let (done, f) = (done.clone(), f.clone());
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    design(&f);
                }
            })
        })
        .collect();
    for _ in 0..3 {
        assert_same("beside other work", &alone, &built(&design(&f)));
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    for b in busy {
        b.join().unwrap();
    }
}

/// A copy that replaces its own design leaves the design it was copied from going: the token over
/// that design's work is stopped only when that design itself closes or is replaced.
#[test]
fn a_copy_replacing_its_design_does_not_stop_the_design_it_came_from() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let token = s.stop_token();
    let new = [("file.new".to_string(), json!({}))];
    assert!(s.ahead().replies(&new).iter().all(Result::is_ok));
    assert!(!token.is_stopped(), "a copy evaluating ahead");
    let _ = s.scratch().compute_commands(&new, None);
    assert!(!token.is_stopped(), "a copy computing a dialog's commands");
    assert!(s.preview(&new).is_ok());
    assert!(!token.is_stopped(), "a preview");
    // It still works, and the design's own replacement does stop it.
    run(&mut s, "solid.box", json!({"corner": [20, 0, 0], "length": 10, "width": 10, "height": 10}));
    run(&mut s, "file.new", json!({}));
    assert!(token.is_stopped(), "the design itself replaced");
    assert!(!s.stop_token().is_stopped());
}
