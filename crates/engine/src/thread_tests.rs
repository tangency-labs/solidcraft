//! Modelled threads: the ISO metric profile cut along a helix, inside and outside,
//! including fine pitches on large shafts and the sample flange.

use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// Volume from the body's mesh: exact for the faceted threaded body, and quicker than a full
/// measurement of its thousands of faces.
fn volume(s: &mut Session, b: &str) -> f64 {
    let st = s.model.state();
    st.body(b).map(|mb| mb.body.tessellate(1e-3).unwrap().measure().volume).unwrap_or(0.0)
}

const SIZES: [(f64, f64); 9] = [(3.0, 0.5), (4.0, 0.7), (5.0, 0.8), (6.0, 1.0), (8.0, 1.25), (10.0, 1.5), (12.0, 1.75), (16.0, 2.0), (20.0, 2.5)];

/// Keep the original 48-station reproduction of the shell error even though new fine
/// threads now use more stations. The same cut polygons must rebuild as a closed solid.
#[test]
fn faceted_flange_thread_preserves_thin_crest_boundaries() {
    use crate::geom::{Plane, Vec2, Vec3};
    use crate::kernel;

    let mut s = Session::default();
    let sample = crate::sample::samples().into_iter().find(|s| s.name == "Bolted Flange").unwrap();
    s.run_script(&(sample.script)()).unwrap();
    let state = s.model.state();
    let body = &state.body("Flange").unwrap().body;
    let face = Vec3::new(-19.034766861657907, -11.03075929038123, 19.574640342897094);
    let cylinder = kernel::cylinder_face_at(body, face).unwrap();
    let pitch = 1.5;
    let height = pitch * 3f64.sqrt() / 2.0;
    let root = 22.0 - 5.0 * height / 8.0;
    let outer = 22.0 + 0.05 * pitch;
    let half_width = (pitch / 4.0 + 2.0 * (outer - root) * (PI / 6.0).tan()) / 2.0;
    let section = [Vec2::new(root, -pitch / 8.0), Vec2::new(outer, -half_width), Vec2::new(outer, half_width), Vec2::new(root, pitch / 8.0)];
    let (start, end) = (cylinder.start - pitch, cylinder.end + pitch);
    let base = cylinder.axis_point + cylinder.axis * start;
    let plane = Plane { origin: base, x: cylinder.axis.cross(Vec3::new(1.0, 0.0, 0.0)).normalized().unwrap(), y: *cylinder.axis };
    let groove = kernel::sweep_helix_mesh(&plane, &section, base, *cylinder.axis, pitch, (end - start) / pitch, 48).unwrap();
    let cut = kernel::faceted_boolean(body, &groove, kernel::BoolOp::Cut, pitch * 0.005).unwrap().unwrap();
    assert!(cut.validity().is_empty(), "{:?}", cut.validity());
    let before = body.tessellate(0.001).unwrap().measure().volume;
    let after = cut.tessellate(0.001).unwrap().measure().volume;
    assert!(before - after > 700.0 && before - after < 1800.0, "removed {}", before - after);
}

#[test]
fn modelled_full_length_m44_flange_thread() {
    let mut s = Session::default();
    let sample = crate::sample::samples().into_iter().find(|s| s.name == "Bolted Flange").unwrap();
    s.run_script(&(sample.script)()).unwrap();
    let before = volume(&mut s, "Flange");
    run(
        &mut s,
        "solid.thread",
        json!({
            "face": [-19.034766861657907, -11.03075929038123, 19.574640342897094],
            "designation": "M44x1.5", "modeled": true
        }),
    );
    let state = s.model.state();
    let body = &state.body("Flange").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let removed = before - volume(&mut s, "Flange");
    assert!(removed > 700.0 && removed < 1800.0, "a full-length helical cut removes material: {removed}");
}

#[test]
fn modelled_full_length_fine_threads_on_large_shafts() {
    for (d, p, len) in [(44.0, 1.0, 5.0), (44.0, 1.5, 16.0), (100.0, 1.5, 6.0)] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius":d / 2.0,"height":len,"body_name":"Shaft"}));
        let before = volume(&mut s, "Shaft");
        run(&mut s, "solid.thread", json!({"face":[d / 2.0,0,len / 2.0],"designation":format!("M{d}x{p}"),"modeled":true}));
        let state = s.model.state();
        let body = &state.body("Shaft").unwrap().body;
        assert!(body.validity().is_empty(), "M{d}x{p}: {:?}", body.validity());
        let root = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
        let want = per_turn(root, d / 2.0, |r| p / 4.0 + 2.0 * (r - root) * (PI / 6.0).tan()) * len / p;
        let got = before - volume(&mut s, "Shaft");
        assert!((got - want).abs() < want * 0.04, "M{d}x{p}: removed {got}, ISO profile {want}");
    }
}

/// A modelled thread on a shaft thicker than it turns the shaft down to the major diameter
/// over the threaded length, then cuts the thread: an M6 over the top 8 mm of an 8 mm shaft
/// leaves a 6 mm threaded section on an 8 mm stub, its turned face named for later features.
/// A shaft of the thread's own size is cut as it is.
#[test]
fn a_shaft_thicker_than_a_modelled_thread_is_turned_down_to_it() {
    let (p, len) = (1.0, 8.0);
    let root = 3.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let groove = per_turn(root, 3.0, |r| p / 4.0 + 2.0 * (r - root) * (PI / 6.0).tan()) * len / p;
    for (shaft, turned) in [(8.0, true), (6.0, false)] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": shaft / 2.0, "height": 12, "body_name": "Shaft"}));
        run(&mut s, "solid.thread", json!({"face": [shaft / 2.0, 0, 11], "designation": "M6", "length": len, "modeled": true}));
        let ring = PI * (shaft * shaft / 4.0 - 9.0) * len;
        let want = PI * shaft * shaft / 4.0 * 12.0 - ring - groove;
        let got = volume(&mut s, "Shaft");
        assert!((got - want).abs() < groove * 0.03, "M6 on {shaft} mm: {got} mm³, want {want}");
        let st = s.model.state();
        let Some(b) = st.body("Shaft") else { panic!("no shaft") };
        assert!(b.body.validity().is_empty(), "{shaft} mm: {:?}", b.body.validity());
        // Nothing past the major diameter over the threaded length; the stub keeps its size.
        let round = |z: f64, r: f64| {
            (0..24).filter(|i| b.body.contains(Vec3::new(r * (PI * *i as f64 / 12.0).cos(), r * (PI * *i as f64 / 12.0).sin(), z))).count()
        };
        assert_eq!(round(8.0, 3.05), 0, "{shaft} mm: the threaded part is turned to 6 mm");
        assert_eq!(round(2.0, shaft / 2.0 - 0.05), 24, "{shaft} mm: the stub is whole");
        let names = solidcraft_doc::naming::face_names(b);
        let fid = s.doc.features.last().map(|f| f.id).unwrap_or(0);
        let named = |role: &str| names.iter().any(|n| n.starts_with(&format!("F{fid}:{role}")));
        assert_eq!(named("turned"), turned, "{shaft} mm: {names:?}");
        assert_eq!(named("shoulder:"), turned, "{shaft} mm: {names:?}");
    }

    // A 9 mm shaft is close enough to M8 to be offered it; it was refused as too thick (#2688).
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 4.5, "height": 10, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4.5, 0, 5], "designation": "M8", "modeled": true}));
    let root = 4.0 - 5.0 * 1.25 * 3f64.sqrt() / 16.0;
    let groove = per_turn(root, 4.0, |r| 1.25 / 4.0 + 2.0 * (r - root) * (PI / 6.0).tan()) * 10.0 / 1.25;
    let (want, got) = (PI * 16.0 * 10.0 - groove, volume(&mut s, "Shaft"));
    assert!((got - want).abs() < groove * 0.03, "M8 on 9 mm: {got} mm³, want {want}");
}

/// Issue 2705: an M14 over the top 10 mm of a 14.6 mm boss on a plate. Turned to 14 mm first:
/// 1600 + π 7.3² 15 − π (7.3² − 7²) 10 − 5 turns of 50.189 mm³ (tests/kernel-cases/threads/2705).
#[test]
fn an_m14_thread_on_a_14_6_mm_boss_turns_it_to_14_mm() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 4, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"radius": 7.3, "height": 15, "base": [0, 0, 4], "operation": "join", "targets": ["Part"]}));
    run(&mut s, "solid.thread", json!({"face": [7.3, 0, 17], "designation": "M14", "length": 10, "modeled": true}));
    let got = volume(&mut s, "Part");
    assert!((got - 3725.5127).abs() < 3725.5127 * 0.01, "{got} mm³");
}

/// A shaft thinner than the thread's minor diameter is still refused, and so is a cosmetic
/// thread on a shaft thicker than it (a note changes nothing to turn). A shaft the part goes on
/// round (standing against a wall) is not turned into the wall.
#[test]
fn a_thin_shaft_or_one_the_part_surrounds_is_refused() {
    let refused = |s: &mut Session, body: &str, p: Value| {
        let before = volume(s, body);
        let e = s.execute("solid.thread", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!((volume(s, body) - before).abs() < 1e-9, "{p}: {e}");
        e
    };
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 2.4, "height": 10, "body_name": "Shaft"}));
    let e = refused(&mut s, "Shaft", json!({"face": [2.4, 0, 5], "designation": "M6", "modeled": true}));
    assert!(e.contains("does not fit a 4.80 mm shaft") || e.contains("too thin"), "{e}");

    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 10, "body_name": "Shaft"}));
    let e = refused(&mut s, "Shaft", json!({"face": [4, 0, 5], "designation": "M6"}));
    assert!(e.contains("an M6 thread does not fit a 8.00 mm shaft"), "{e}");

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [2, -10, 0], "length": 10, "width": 20, "height": 10, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 10, "operation": "join", "targets": ["Part"]}));
    let e = refused(&mut s, "Part", json!({"face": [-4, 0, 5], "designation": "M6", "modeled": true}));
    assert!(e.contains("this 8 mm shaft is too thick for a modelled M6 thread") && e.contains("part around it"), "{e}");
}

#[test]
fn non_finite_thread_designations_are_refused_without_changing_the_design() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":10}));
    let saved = s.doc.to_json();
    for designation in ["Minfx1", "M8xinf", "MNaNx1", "M8xNaN", "M1e999x1"] {
        let result = s.execute("solid.thread", &json!({"face":[4,0,2],"designation":designation}));
        assert!(result.is_err(), "{designation}: {result:?}");
        assert_eq!(s.doc.to_json(), saved);
    }
}

/// ∫ 2π r · width(r) dr over [a, b]: the volume one turn of the groove takes out (a screw
/// motion of a section in an axial plane sweeps 2π∫r dA per turn).
fn per_turn(a: f64, b: f64, width: impl Fn(f64) -> f64) -> f64 {
    let n = 400;
    (0..n).map(|i| a + (b - a) * (i as f64 + 0.5) / n as f64).map(|r| 2.0 * PI * r * width(r) * (b - a) / n as f64).sum()
}

#[test]
fn modelled_external_threads_m3_to_m20() {
    let h = |p: f64| p * 3f64.sqrt() / 2.0;
    let slope = (PI / 6.0).tan();
    for (d, p) in SIZES {
        let mut s = Session::default();
        let len = 4.0 * p;
        run(&mut s, "solid.cylinder", json!({"radius": d / 2.0, "height": len + 2.0, "body_name": "Shaft"}));
        let before = volume(&mut s, "Shaft");
        let th = run(&mut s, "solid.thread", json!({"face": [d / 2.0, 0, 1], "length": len, "modeled": true}));
        assert!(th["thread"].is_null(), "a modelled thread is real geometry, not an annotation: {th}");
        let r_root = d / 2.0 - 5.0 * h(p) / 8.0;
        let want = per_turn(r_root, d / 2.0, |r| p / 4.0 + 2.0 * (r - r_root) * slope) * len / p;
        let got = before - volume(&mut s, "Shaft");
        assert!((got - want).abs() < want * 0.03, "M{d}: removed {got}, ISO profile {want}");
    }
}

#[test]
fn modelled_internal_threads_m3_to_m20() {
    let h = |p: f64| p * 3f64.sqrt() / 2.0;
    let slope = (PI / 6.0).tan();
    for (d, p) in SIZES {
        let mut s = Session::default();
        let len = 4.0 * p;
        let d1 = d - 2.0 * 5.0 * h(p) / 8.0;
        run(&mut s, "solid.box", json!({"corner": [-d, -d, 0], "length": 2.0 * d, "width": 2.0 * d, "height": len, "body_name": "Nut"}));
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, -1], "radius": d1 / 2.0, "height": len + 2.0, "body_name": "Bore"}));
        run(&mut s, "solid.combine", json!({"target": "Nut", "tools": ["Bore"], "operation": "cut"}));
        let before = volume(&mut s, "Nut");
        let th = run(&mut s, "solid.thread", json!({"face": [d1 / 2.0, 0, len / 2.0], "modeled": true}));
        assert!(th["thread"].is_null(), "{th}");
        // Bolt-shaped space: P/8 flat at the major diameter, widening towards the bore.
        let want = per_turn(d1 / 2.0, d / 2.0, |r| p / 8.0 + 2.0 * (d / 2.0 - r) * slope) * len / p;
        let got = before - volume(&mut s, "Nut");
        assert!((got - want).abs() < want * 0.03, "M{d}: removed {got}, ISO profile {want}");
    }
}

/// Issue 995: an M5 modelled thread in a 5 mm hole cut nothing (the groove lay in the hole, from
/// the minor diameter out to the 5 mm major one) and only left the wall faceted. A modelled thread
/// that would cut next to nothing is refused, with the drill to use and the size that fits;
/// a cosmetic one is still only a note.
#[test]
fn a_modelled_thread_that_would_cut_nothing_is_refused() {
    let mut s = hex_nut(5.0);
    let before = volume(&mut s, "Nut");
    let e = s.execute("solid.thread", &json!({"face": [2.5, 0, 2], "designation": "M5", "modeled": true})).err().map(|e| e.to_string());
    let e = e.unwrap_or_default();
    assert!(e.contains("this 5 mm hole") && e.contains("drill 4.2 mm for M5") && e.contains("thread this hole M6"), "{e}");
    assert!((volume(&mut s, "Nut") - before).abs() < 1e-9);
    run(&mut s, "solid.thread", json!({"face": [2.5, 0, 2], "designation": "M5"}));

    // A shaft no thicker than the thread's root (17.29 mm for M20) is refused the same way.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 8.6, "height": 10, "body_name": "Shaft"}));
    let e = s.execute("solid.thread", &json!({"face": [8.6, 0, 5], "designation": "M20", "modeled": true})).err().map(|e| e.to_string());
    let e = e.unwrap_or_default();
    assert!(e.contains("this 17.2 mm shaft") && e.contains("make the shaft 20 mm for M20") && e.contains("thread this shaft M18"), "{e}");
}

/// Issue 2688: an M8 modelled thread in a 6 mm hole (narrower than its 6.65 mm minor diameter)
/// was refused as "the face is too far from the thread's diameter". It says what is wrong and
/// what to drill, as a hole that is too wide does. (A shaft too thick is turned down.)
#[test]
fn a_hole_too_narrow_says_so_and_what_fits() {
    let mut s = hex_nut(6.0);
    let before = volume(&mut s, "Nut");
    let e = s.execute("solid.thread", &json!({"face": [3.0, 0, 2], "designation": "M8", "modeled": true})).err().map(|e| e.to_string());
    let e = e.unwrap_or_default();
    assert!(e.contains("this 6 mm hole is too narrow for a modelled M8 thread") && e.contains("drill 6.75 mm for M8"), "{e}");
    assert!(!e.contains("too far"), "{e}");
    assert!((volume(&mut s, "Nut") - before).abs() < 1e-9);
}

/// The limit is a quarter of the basic depth 5H/8: a hole up to 4.78 mm for M5 (4.86 mm for
/// M5x0.5) still takes a modelled thread, a wider one is refused with that pitch's drill.
#[test]
fn the_cut_nothing_limit_holds_for_coarse_and_fine_pitch() {
    for (m, hole, refused) in [
        ("M5", 4.5, None),
        ("M5", 4.75, None),
        ("M5", 4.8, Some("drill 4.2 mm for M5, or thread this hole M6")),
        ("M5x0.5", 4.5, None),
        ("M5x0.5", 4.85, None),
        ("M5x0.5", 4.9, Some("drill 4.5 mm for M5x0.5, or thread this hole M6")),
        ("M5x0.5", 5.0, Some("drill 4.5 mm for M5x0.5, or thread this hole M6")),
    ] {
        let mut s = hex_nut(hole);
        let before = volume(&mut s, "Nut");
        let made = s.execute("solid.thread", &json!({"face": [hole / 2.0, 0, 2], "designation": m, "modeled": true}));
        let cut = before - volume(&mut s, "Nut");
        match (refused, made) {
            (None, Ok(_)) => assert!(cut > 0.05, "{m} in {hole} mm: removed {cut}"),
            (Some(want), Err(e)) => {
                let e = e.to_string();
                assert!(e.contains(&format!("this {hole} mm hole")) && e.contains(want), "{m} in {hole} mm: {e}");
                assert!(cut.abs() < 1e-9, "{m} in {hole} mm: removed {cut}");
            }
            (want, got) => panic!("{m} in {hole} mm: want refused {want:?}, got {:?}", got.map(|_| ())),
        }
    }
}

/// A 4 mm hex nut (inscribed radius 5) with a plain hole of this diameter through it.
fn hex_nut(hole: f64) -> Session {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 5, "sides": 6})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 4, "body_name": "Nut"})),
        ("solid.hole", json!({"position": [0, 0, 4], "diameter": hole})),
    ] {
        run(&mut s, id, p);
    }
    s
}

/// Two threaded screws crossing each other, cut and joined (#1983, the hang half of #2038): each
/// is thousands of flat faces, and the boolean went through every B-rep retry on them for many
/// minutes. It now takes the exact polygon route alone, with a result or a clear refusal, and
/// tries no B-rep boolean at all: counted, not timed, so a busy machine cannot fail it. A cross
/// hole through one of them (a curved tool) still takes the B-rep boolean, and is drilled.
#[test]
fn booleans_of_two_modelled_threads_finish() {
    let mut s = Session::default();
    for (id, p) in [
        ("solid.cylinder", json!({"radius": 2.5, "height": 10, "body_name": "A"})),
        ("solid.thread", json!({"face": [-2.5, 0, 5], "designation": "M5", "length": 4, "modeled": true})),
        ("solid.cylinder", json!({"radius": 2.5, "height": 10, "base": [3.5, 0, 2], "operation": "new", "body_name": "B"})),
        ("solid.thread", json!({"face": [6, 0, 7], "designation": "M5", "length": 4, "modeled": true})),
    ] {
        run(&mut s, id, p);
    }
    for b in ["A", "B"] {
        let faces = s.model.state().body(b).map(|b| b.body.face_count()).unwrap_or(0);
        assert!(faces > 1000, "{b} is faceted: {faces} faces");
    }
    for op in ["cut", "join"] {
        let (r, left) = solidcraft_kernel::with_attempt_budget(4, || {
            let r = s.execute("solid.combine", &json!({"target": "A", "tools": ["B"], "operation": op, "keep_tools": true}));
            (r, solidcraft_kernel::attempts_left())
        });
        assert_eq!(left, Some(4), "the {op} tried the B-rep boolean on two flat bodies");
        if let Err(e) = r {
            assert!(e.to_string().contains("not supported yet"), "{op}: {e}");
        }
    }
    let before = volume(&mut s, "B");
    run(&mut s, "solid.cylinder", json!({"radius": 0.8, "height": 10, "base": [-1.5, 0, 9], "axis": "X", "operation": "cut", "targets": ["B"]}));
    let after = volume(&mut s, "B");
    assert!(after < before - 1.0, "the cross hole is drilled: {before} -> {after}");
}

/// Issue 602: rounding the edges of a hex boss on a threaded shaft froze the app. The thread's
/// thousands of facets were healed pair by pair, once per feature after it, and the fillet's
/// preview then compared each of them with the whole old body.
#[test]
fn fillet_beside_a_modelled_thread_finishes() {
    let mut s = Session::default();
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.circle.center", json!({"center": [0, 0], "diameter": 5})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10, "body_name": "Screw"})),
        ("sketch.create", json!({"plane": {"face": [0, 0, 10]}})),
        ("sketch.polygon.circumscribed", json!({"center": [0, 0], "radius": 2, "sides": 6})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 3, "profiles": [{"point": [0, 0]}], "operation": "join", "targets": ["Screw"]})),
        ("solid.thread", json!({"face": [2.5, 0, 5], "designation": "M5", "length": 4, "modeled": true})),
    ] {
        run(&mut s, id, p);
    }
    let faces_before = s.model.state().body("Screw").map(|b| b.body.face_count()).unwrap_or(0);
    assert!(faces_before > 1000, "the thread is faceted: {faces_before} faces");
    let volume_before = volume(&mut s, "Screw");
    let top: Vec<Value> = (0..6)
        .map(|i| {
            let a = (30.0 + 60.0 * i as f64).to_radians();
            json!([2.0 * a.cos(), 2.0 * a.sin(), 13])
        })
        .collect();
    let started = std::time::Instant::now();
    run(&mut s, "solid.fillet", json!({"edges": top, "radius": 0.3}));
    assert!(started.elapsed().as_secs() < 30, "the fillet took {:?}", started.elapsed());
    let volume_after = volume(&mut s, "Screw");
    assert!(volume_after < volume_before && volume_after > volume_before * 0.99, "{volume_before} -> {volume_after}");
}
