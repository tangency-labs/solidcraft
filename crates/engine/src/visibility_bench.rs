//! Visibility cost benchmark (prototype only, `#[ignore]`d, not part of CI):
//! `SOLIDCRAFT_BENCH_PARTS=500 cargo test --profile ci -p solidcraft-engine visibility_bench -- --ignored --nocapture`
//! 500 parts = 2000 bodies. Prints `BENCH <label> <microseconds per call>` lines.

use serde_json::json;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use crate::Session;

/// Best of 7 batches of `reps` calls, in microseconds per call (the minimum shrugs off a busy machine).
fn us(reps: u32, mut f: impl FnMut()) -> f64 {
    f(); // warm up
    let mut best = f64::MAX;
    for _ in 0..7 {
        let t = Instant::now();
        for _ in 0..reps {
            f();
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e6 / reps as f64);
    }
    best
}

fn line(label: &str, v: f64) {
    println!("BENCH {label:<46} {v:>12.1} us");
}

fn hash_of(f: impl Fn(&mut std::collections::hash_map::DefaultHasher)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f(&mut h);
    h.finish()
}

#[test]
#[ignore = "benchmark"]
fn visibility_bench() {
    let parts = std::env::var("SOLIDCRAFT_BENCH_PARTS").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    let t = Instant::now();
    let mut s = Session::new(crate::large_assembly_tests::generated_assembly(parts));
    println!("BENCH open (Session::new)                          {:>12.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    println!("BENCH bodies {}", names.len());
    let keep = vec![names[0].clone()];
    let occ = s.doc.occurrences.first().map(|o| o.id).expect("an occurrence");

    // The old `ui.isolate` recompute: every body but the selection.
    line(
        "old ui.isolate recompute (world_state+filter)",
        us(20, || {
            let v: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).filter(|b| !keep.contains(b)).collect();
            std::hint::black_box(v);
        }),
    );
    s.visibility.hidden_bodies = names.iter().filter(|b| !keep.contains(b)).cloned().collect();
    s.revision += 1;
    let n_hidden = s.visibility.hidden_bodies.len();
    println!("BENCH hidden_bodies len {n_hidden}");

    // Render-cache hashing of the hidden lists (viewport scene key, pick key, model key).
    line(
        "hash scene key lists (viewport.rs:325)",
        us(200, || {
            let v = &s.visibility;
            std::hint::black_box(hash_of(|h| {
                v.hidden_bodies.hash(h);
                v.hidden_occurrences.hash(h);
                v.hidden_sketches.hash(h);
                v.shown_sketches.hash(h);
                v.hidden_folders.hash(h);
            }));
        }),
    );
    line(
        "hash model/pick key lists (bodies+occ)",
        us(200, || {
            let v = &s.visibility;
            std::hint::black_box(hash_of(|h| {
                v.hidden_bodies.hash(h);
                v.hidden_occurrences.hash(h);
            }));
        }),
    );
    line(
        "hash 3 keys per frame (scene+pick+model+origin)",
        us(200, || {
            let v = &s.visibility;
            std::hint::black_box(hash_of(|h| {
                v.hidden_bodies.hash(h);
                v.hidden_occurrences.hash(h);
                v.hidden_sketches.hash(h);
                v.shown_sketches.hash(h);
                v.hidden_folders.hash(h);
                v.hidden_origin.hash(h);
                v.hidden_bodies.hash(h);
                v.hidden_occurrences.hash(h);
                v.hidden_bodies.hash(h);
                v.hidden_occurrences.hash(h);
            }));
        }),
    );

    // Readers with a long hidden list.
    line(
        "body_visible x all bodies (1999 hidden)",
        us(5, || {
            let mut n = 0;
            for b in &names {
                n += s.body_visible(b) as usize;
            }
            std::hint::black_box(n);
        }),
    );

    // Save / dirty comparisons.
    line(
        "Session::design() (visibility non-empty)",
        us(20, || {
            std::hint::black_box(s.design());
        }),
    );
    line(
        "Session::is_dirty() (visibility differs)",
        us(200, || {
            std::hint::black_box(s.is_dirty());
        }),
    );
    s.mark_saved();
    line(
        "Session::is_dirty() (equal, 1999 names)",
        us(200, || {
            std::hint::black_box(s.is_dirty());
        }),
    );
    line(
        "Visibility clone (capture-like)",
        us(200, || {
            std::hint::black_box(s.visibility.clone());
        }),
    );

    // Toggling one body when 1999 are hidden.
    let one = names[5].clone();
    line(
        "toggle one body via browser.visibility cmd",
        us(20, || {
            s.execute("browser.visibility", &json!({"items": [one.clone()], "visible": true})).unwrap();
            s.execute("browser.visibility", &json!({"items": [one.clone()], "visible": false})).unwrap();
        }) / 2.0,
    );
    line(
        "toggle one body raw (contains+push/retain)",
        us(200, || {
            let v = &mut s.visibility;
            if !v.hidden_bodies.contains(&one) {
                v.hidden_bodies.push(one.clone());
            }
            v.hidden_bodies.retain(|b| *b != one);
            v.hidden_bodies.push(one.clone());
        }),
    );
    // Show all.
    line(
        "browser.visibility folder:bodies visible:true",
        us(10, || {
            s.visibility.hidden_bodies = names.iter().filter(|b| !keep.contains(b)).cloned().collect();
            s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
        }),
    );
    // Occurrence isolate.
    line(
        "browser.visibility paths isolate:true",
        us(5, || {
            // A repeated identical isolation is refused (it would add a layer that hides nothing).
            s.execute("display.unisolate_all", &json!({})).unwrap();
            s.execute("browser.visibility", &json!({"paths": [[occ]], "visible": true, "isolate": true})).unwrap();
        }),
    );
    extra(&mut s, &names, &keep);
}

/// Measurements of the isolation layers (absent from the baseline and refactor runs).
fn extra(s: &mut Session, names: &[String], keep: &[String]) {
    s.execute("display.unisolate_all", &json!({})).unwrap();
    s.visibility = Default::default();
    s.mark_saved();
    let keep_json = json!({ "items": keep });
    line(
        "display.isolate one body (then unisolate)",
        us(10, || {
            s.execute("display.isolate", &keep_json).unwrap();
            s.execute("display.unisolate", &json!({})).unwrap();
        }) / 2.0,
    );
    s.execute("display.unisolate_all", &json!({})).unwrap();
    // A base state with 500 bodies hidden by hand, then an isolation on top of it.
    s.visibility.hidden_bodies = names[1500..].to_vec();
    s.mark_saved();
    s.execute("display.isolate", &keep_json).unwrap();
    line("Session::design() while isolated (500 hand-hidden base) (1999 hidden)", us(20, || drop(std::hint::black_box(s.design()))));
    line(
        "Session::is_dirty() while isolated",
        us(200, || {
            std::hint::black_box(s.is_dirty());
        }),
    );
    s.execute("display.isolate", &json!({ "items": &names[..10] })).unwrap();
    line(
        "display.isolate 10 bodies on level 2 (+exit)",
        us(10, || {
            s.execute("display.isolate", &json!({ "items": &names[..5] })).unwrap();
            s.execute("display.unisolate", &json!({})).unwrap();
        }) / 2.0,
    );
    line(
        "display.unisolate_all from level 2",
        us(1, || {
            s.execute("display.unisolate_all", &json!({})).unwrap();
            s.execute("display.isolate", &keep_json).unwrap();
            s.execute("display.isolate", &json!({ "items": &names[..10] })).unwrap();
        }),
    );
}

/// What the Browser's Display States folder pays on every frame it is drawn: finding the active
/// state compares the visibility (and the appearances) with every saved state.
#[test]
#[ignore = "benchmark"]
fn active_display_state_bench() {
    let parts: usize = std::env::var("SOLIDCRAFT_BENCH_PARTS").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    let mut s = Session::new(crate::large_assembly_tests::generated_assembly(parts));
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    for state in 0..20 {
        // Each state hides a different quarter of the bodies, so the comparison has real lists to walk.
        s.execute("browser.visibility", &json!({"folder": "bodies", "visible": true})).unwrap();
        s.execute("browser.visibility", &json!({"items": &names[state * 5..state * 5 + names.len() / 4], "visible": false})).unwrap();
        s.execute("display.save", &json!({ "name": format!("State {state}") })).unwrap();
    }
    line(
        "active_display_state(), 20 states of a quarter of the bodies hidden, none matching",
        us(200, || {
            std::hint::black_box(s.active_display_state());
        }),
    );
    s.execute("display.apply", &json!({"state": "State 19"})).unwrap();
    line(
        "active_display_state(), the last of 20 matches",
        us(200, || {
            std::hint::black_box(s.active_display_state());
        }),
    );
}
