//! A modelled thread on a part with large curved faces (#2470): the faceted cut built a tree
//! deeper than the window thread's stack on Windows and the app closed. It must answer, on a
//! stack that small, with a result or an error.

use serde_json::json;

use crate::Session;

/// A bore through a Ø24 sphere, threaded M8. Every face of the body is faceted to the thread's
/// fineness, so the sphere alone makes a tree thousands of nodes deep.
#[test]
fn modelled_thread_through_a_sphere_answers_on_a_small_stack() {
    let r = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut s = Session::default();
            s.execute("solid.sphere", &json!({"radius": 12, "center": [0, 0, 0], "operation": "new", "body_name": "Ball"})).unwrap();
            s.execute("solid.cylinder", &json!({"base": [0, 0, -30], "radius": 3.4, "height": 60, "operation": "cut", "targets": ["Ball"]})).unwrap();
            s.execute("solid.thread", &json!({"face": [3.4, 0, 0], "designation": "M8", "modeled": true})).map(|_| ()).map_err(|e| e.to_string())
        })
        .unwrap()
        .join()
        .unwrap();
    // Either is fine here; what matters is that the process is still running.
    if let Err(e) = r {
        assert!(e.contains("too deep") || e.contains("too detailed"), "{e}");
    }
}
