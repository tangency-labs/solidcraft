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
    st.body(b).map(|mb| mb.body.mesh(&solidcraft_kernel::MeshTolerance::chord(1e-3)).unwrap().measure().volume).unwrap_or(0.0)
}

const SIZES: [(f64, f64); 9] = [(3.0, 0.5), (4.0, 0.7), (5.0, 0.8), (6.0, 1.0), (8.0, 1.25), (10.0, 1.5), (12.0, 1.75), (16.0, 2.0), (20.0, 2.5)];

fn assert_tapped_depth(state: &solidcraft_doc::ModelState, entry: Vec3, inward: Vec3, offset: f64, depth: f64) {
    assert_eq!(state.threads.len(), 1);
    let thread = &state.threads[0];
    assert!(thread.internal);
    let a = (thread.axis_point + thread.axis * thread.start - entry).dot(inward);
    let b = (thread.axis_point + thread.axis * thread.end - entry).dot(inward);
    assert!((a.min(b) - offset).abs() < 1e-6, "thread entry {} mm, expected {offset} mm", a.min(b));
    assert!((a.max(b) - offset - depth).abs() < 1e-6, "thread end {} mm, expected {} mm", a.max(b), offset + depth);
}

#[test]
fn tapped_thread_depth_keeps_reference_edge_placement_after_reopen() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":60,"width":60,"height":12}));
    run(
        &mut s,
        "solid.hole",
        json!({"position":[30,30,12],"thread":"M6","depth":8,"thread_depth":3,
            "references":[{"edge":[0,25,12],"distance":"10 mm"},{"edge":[25,0,12],"distance":"12 mm"}]}),
    );
    let inward = Vec3::new(0.0, 0.0, -1.0);
    let entry = Vec3::new(10.0, 12.0, 12.0);
    assert_tapped_depth(&s.model.state(), entry, inward, 0.0, 3.0);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_tapped_depth(&reopened.model.state(), entry, inward, 0.0, 3.0);
    let solidcraft_doc::FeatureKind::Hole { references, thread_depth, .. } = &reopened.doc.features.last().unwrap().kind else {
        panic!("expected a referenced tapped hole");
    };
    assert_eq!(references.len(), 2);
    assert_eq!(thread_depth.as_ref().unwrap().to_string(), "3");
    assert!(!reopened.model.state().bodies[0].body.contains(Vec3::new(10.0, 12.0, 10.0)));
}

#[test]
fn tapped_thread_depth_is_independent_through_preview_redefine_undo_and_reload() {
    for (height, mut params, full) in
        [(12.0, json!({"depth":8,"type":"drilled"}), 8.0), (12.0, json!({}), 12.0), (12.0, json!({"to":[30,30,5]}), 7.0)]
    {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":60,"width":60,"height":height,"body_name":"Plate"}));
        params["position"] = json!([30, 30, height]);
        params["direction"] = json!([0, 0, -1]);
        params["thread"] = json!("M6");
        let hole = run(&mut s, "solid.hole", params.clone())["feature"].as_u64().unwrap();
        let entry = Vec3::new(30.0, 30.0, height);
        let inward = Vec3::new(0.0, 0.0, -1.0);
        assert_tapped_depth(&s.model.state(), entry, inward, 0.0, full);
        let before = s.doc.to_json();
        let physical_volume = volume(&mut s, "Plate");

        // Version 29 never saved this field: its default remains the full wall.
        let mut legacy: Value = serde_json::from_str(&before).unwrap();
        legacy["format"] = json!("solidcraft/29");
        let reopened = Session::new(solidcraft_doc::Document::from_json(&legacy.to_string()).unwrap());
        assert_tapped_depth(&reopened.model.state(), entry, inward, 0.0, full);

        params["thread_depth"] = json!(4);
        let redefine = json!({"feature":hole,"command":"solid.hole","params":params});
        let preview = s.preview(&[("timeline.redefine".into(), redefine.clone())]).unwrap();
        assert_tapped_depth(&preview.after, entry, inward, 0.0, 4.0);
        assert_eq!(s.doc.to_json(), before, "preview must not change the design");
        run(&mut s, "timeline.redefine", redefine);
        assert_tapped_depth(&s.model.state(), entry, inward, 0.0, 4.0);
        assert!((volume(&mut s, "Plate") - physical_volume).abs() < 1e-6, "thread depth must not change the drill geometry");
        let saved = s.doc.to_json();
        let reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
        assert_tapped_depth(&reopened.model.state(), entry, inward, 0.0, 4.0);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        assert_tapped_depth(&s.model.state(), entry, inward, 0.0, full);
    }
}

#[test]
fn tapped_thread_depth_uses_the_cylinder_beyond_recesses_and_in_shallow_holes() {
    for (height, mut params, offset, full) in [
        (12.0, json!({"thread":"M6","type":"counterbore","cb_diameter":12,"cb_depth":5,"depth":9}), 5.0, 4.0),
        (12.0, json!({"thread":"G 1/4","type":"counterbore","cb_diameter":20,"cb_depth":5,"depth":9}), 5.0, 4.0),
        (12.0, json!({"thread":"1/4-20 UNC","type":"counterbore","cb_diameter":12,"cb_depth":5,"depth":9}), 5.0, 4.0),
        (12.0, json!({"thread":"M6","type":"countersink","cs_diameter":11,"cs_angle":"90 deg","depth":9}), 3.0, 6.0),
        (12.0, json!({"thread":"M36","type":"drilled","depth":3}), 0.0, 3.0),
        (5.0, json!({"thread":"M36"}), 0.0, 5.0),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":60,"width":60,"height":height,"body_name":"Plate"}));
        params["position"] = json!([30, 30, height]);
        params["direction"] = json!([0, 0, -1]);
        let hole = run(&mut s, "solid.hole", params.clone())["feature"].as_u64().unwrap();
        let entry = Vec3::new(30.0, 30.0, height);
        let inward = Vec3::new(0.0, 0.0, -1.0);
        assert_tapped_depth(&s.model.state(), entry, inward, offset, full);
        params["thread_depth"] = json!(2);
        run(&mut s, "timeline.redefine", json!({"feature":hole,"command":"solid.hole","params":params}));
        assert_tapped_depth(&s.model.state(), entry, inward, offset, 2.0);
    }
}

#[test]
fn tapped_thread_depth_starts_at_the_entry_for_both_axis_directions() {
    for (entry, inward) in [(Vec3::new(0.0, 10.0, 6.0), Vec3::new(1.0, 0.0, 0.0)), (Vec3::new(20.0, 10.0, 6.0), Vec3::new(-1.0, 0.0, 0.0))] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":20,"width":20,"height":12}));
        run(
            &mut s,
            "solid.hole",
            json!({"position":[entry.x,entry.y,entry.z],"direction":[inward.x,inward.y,inward.z],"thread":"M6","depth":8,"thread_depth":3}),
        );
        assert_tapped_depth(&s.model.state(), entry, inward, 0.0, 3.0);
    }
}

#[test]
fn tapped_thread_depth_rejects_invalid_or_excessive_values_atomically() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
    let before = s.doc.to_json();
    for depth in [json!(0), json!(-1), json!(20), json!(true), json!(""), json!("NaN mm"), json!("1e999 mm"), json!("3 deg")] {
        let result = s.execute("solid.hole", &json!({"position":[10,10,10],"thread":"M6","depth":8,"thread_depth":depth}));
        assert!(result.is_err(), "invalid thread depth {depth}: {result:?}");
        assert_eq!(s.doc.to_json(), before);
        assert!(s.model.state().threads.is_empty());
    }
    assert!(s.execute("solid.hole", &json!({"position":[10,10,10],"diameter":5,"thread_depth":3})).is_err());
    assert_eq!(s.doc.to_json(), before);
}

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
    let before = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap().measure().volume;
    let after = cut.mesh(&solidcraft_kernel::MeshTolerance::chord(0.001)).unwrap().measure().volume;
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

/// Issue 2705: an M14 over the top 10 mm of a 14.6 mm boss on a plate. A modelled thread never
/// changes the size of the shaft it is on (#1018, #2853): it is refused with the size to make
/// the shaft, and the part is unchanged (tests/kernel-cases/threads/2705).
#[test]
fn an_m14_thread_on_a_14_6_mm_boss_is_refused_with_the_size_to_make_it() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 4, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"radius": 7.3, "height": 15, "base": [0, 0, 4], "operation": "join", "targets": ["Part"]}));
    let before = volume(&mut s, "Part");
    let thread = json!({"face": [7.3, 0, 17], "designation": "M14", "length": 10, "modeled": true});
    let e = s.execute("solid.thread", &thread).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("this 14.6 mm shaft is too thick for a modelled M14 thread; make the shaft 14 mm for M14"), "{e}");
    assert!((volume(&mut s, "Part") - before).abs() < 1e-9);
}

/// A shaft thinner than the thread's minor diameter is refused, and so is a thread on a shaft
/// far thicker than it.
#[test]
fn a_shaft_far_off_the_threads_size_is_refused() {
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

    let e = refused(&mut s, "Shaft", json!({"face": [4, 0, 5], "designation": "M6", "modeled": true}));
    assert!(e.contains("an M6 thread does not fit a 8.00 mm shaft"), "{e}");
}

#[test]
fn a_thread_without_a_coarse_pitch_suggests_explicit_pitches() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 14, "height": 10, "body_name": "Shaft"}));
    let saved = s.doc.to_json();
    for designation in ["M28", "m28", " M28 "] {
        let e = s.execute("solid.thread", &json!({"face": [14, 0, 5], "designation": designation})).unwrap_err().to_string();
        assert!(e.contains("has no coarse ISO pitch") && e.contains("M28x2") && e.contains("M28x1.5"), "{e}");
        assert_eq!(s.doc.to_json(), saved);
    }
    for designation in ["MNaN", "Minf", "M-28", "M0", "M1e9", "M1e999", "M28x", "M28xNaN"] {
        let e = s.execute("solid.thread", &json!({"face": [14, 0, 5], "designation": designation})).unwrap_err().to_string();
        assert!(e.contains("unknown thread"), "{designation}: {e}");
        assert_eq!(s.doc.to_json(), saved);
    }
    run(&mut s, "solid.thread", json!({"face": [14, 0, 5], "designation": "M28x2"}));
}

#[test]
fn automatic_sizing_past_the_coarse_range_suggests_a_size_with_its_pitch() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 37.5, "height": 10, "body_name": "Shaft"}));
    let saved = s.doc.to_json();
    let e = s.execute("solid.thread", &json!({"face": [37.5, 0, 5]})).unwrap_err().to_string();
    assert!(e.contains("beyond the coarse thread range (up to M56)") && e.contains("M75x3 or M75x2"), "{e}");
    assert_eq!(s.doc.to_json(), saved);
    run(&mut s, "solid.thread", json!({"face": [37.5, 0, 5], "designation": "M75x2"}));
}

#[test]
fn a_threaded_hole_without_a_coarse_pitch_suggests_explicit_pitches() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 10}));
    let saved = s.doc.to_json();
    let e = s.execute("solid.hole", &json!({"position": [20, 20, 10], "thread": "M28"})).unwrap_err().to_string();
    assert!(e.contains("has no coarse ISO pitch") && e.contains("M28x2") && e.contains("M28x1.5"), "{e}");
    assert_eq!(s.doc.to_json(), saved);
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
/// what to drill, as a hole that is too wide does; and the same for a shaft that is too thick.
#[test]
fn a_hole_too_narrow_or_a_shaft_too_thick_says_which_and_what_fits() {
    let mut s = hex_nut(6.0);
    let before = volume(&mut s, "Nut");
    let e = s.execute("solid.thread", &json!({"face": [3.0, 0, 2], "designation": "M8", "modeled": true})).err().map(|e| e.to_string());
    let e = e.unwrap_or_default();
    assert!(e.contains("this 6 mm hole is too narrow for a modelled M8 thread") && e.contains("drill 6.8 mm for M8"), "{e}");
    assert!(!e.contains("too far"), "{e}");
    assert!((volume(&mut s, "Nut") - before).abs() < 1e-9);

    let mut s = Session::default();
    // 9 mm is close enough to M8 to be offered it, too thick to cut it.
    run(&mut s, "solid.cylinder", json!({"radius": 4.5, "height": 10, "body_name": "Shaft"}));
    let e = s.execute("solid.thread", &json!({"face": [4.5, 0, 5], "designation": "M8", "modeled": true})).err().map(|e| e.to_string());
    let e = e.unwrap_or_default();
    assert!(e.contains("this 9 mm shaft is too thick for a modelled M8 thread") && e.contains("make the shaft 8 mm for M8"), "{e}");
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

/// Issue 602: rounding the edges of a hex boss on a threaded shaft froze the app. The thread's
/// thousands of facets were healed pair by pair, once per feature after it, and the fillet's
/// preview then compared each of them with the whole old body. With SMLib the thread is exact (a
/// few dozen faces). The 4 mm thread starts at the shaft's free bottom end: the hex's corners
/// (2.31 mm out, past the M5 root at 2.07 mm) stand in the groove's way at the top end.
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

/// The SMLib volume of a body (exact surfaces), for closed-form checks.
fn exact_volume(s: &Session, b: &str) -> f64 {
    let st = s.model.state();
    st.body(b).map(|mb| solidcraft_kernel::measure(&mb.body).unwrap().volume).unwrap_or(0.0)
}

/// One turn of the ISO 68-1 external groove inside a shaft of radius `rs` (its section clipped at
/// the surface): 2π∫ r g(r) dr from the root, g = P/4 + 2 (r - root) tan 30°.
fn ext_turn(d: f64, p: f64, rs: f64) -> f64 {
    let root = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let (a, b) = (p / 4.0 - 2.0 * root * (PI / 6.0).tan(), 2.0 * (PI / 6.0).tan());
    let f = |r: f64| a * r * r / 2.0 + b * r * r * r / 3.0;
    2.0 * PI * (f(rs) - f(root))
}

/// The groove's half width at its widest: for an external thread at the tool's outer edge,
/// 0.05 P past the crest (the section reaches past the surface so the cut leaves no skin).
fn ext_half_width(d: f64, p: f64) -> f64 {
    let root = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    (p / 4.0 + 2.0 * (d / 2.0 + 0.05 * p - root) * (PI / 6.0).tan()) / 2.0
}

/// A d10 shaft (z 10..35) on a d16 collar (z 0..10), as ladder 06/004.
fn shaft_on_collar() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 8, "height": 10, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 10], "radius": 5, "height": 25, "operation": "join", "targets": ["Part"]}));
    s
}

/// Rig 602, ladder 06/004: a thread on a shaft ran a pitch past both ends of its face, into the
/// collar below as into the air above. At a shoulder it stops with its widest part clear of it:
/// the collar keeps its whole π 8² 10, and the groove takes (25 - w)/P turns, w its half width
/// (a square stop inside material takes exactly its length over P, the section being symmetric).
#[test]
fn a_thread_stops_clear_of_a_shoulder() {
    let mut s = shaft_on_collar();
    run(&mut s, "solid.thread", json!({"face": [5, 0, 25], "designation": "M10", "modeled": true}));
    let (p, w) = (1.5, ext_half_width(10.0, 1.5));
    let want = PI * 64.0 * 10.0 + PI * 25.0 * 25.0 - (25.0 - w) / p * ext_turn(10.0, p, 5.0);
    let got = exact_volume(&s, "Part");
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
}

/// A d10 boss (z t..t+16) on a plate t thick, threaded M10: the plate's top is the shoulder.
/// The groove, with or without its runout, stays on the boss: the plate keeps all of itself
/// (what lies below z = t after the boss is cut away), and without a runout the boss loses
/// (16 - w)/P turns, as at a collar. The owner's bug: built as the app builds it (a rounded
/// plate, a circle sketched on its top face and extruded with Join), a plate thinner than 1.5 P
/// had no material where the free-end test looked (1 to 1.5 pitches past the face), so its top
/// was taken for a free end and the overrun cut a pitch into it.
#[test]
fn a_thread_on_a_boss_never_cuts_into_the_plate_below() {
    let boss = |t: f64, extra: Value| {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.rectangle.rounded", json!({"p0": [-20, -20], "p1": [20, 20], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": t}));
        run(&mut s, "sketch.create", json!({"plane": {"face": [0, 0, t]}}));
        run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 16, "operation": "join"}));
        assert_eq!(s.model.state().bodies.len(), 1);
        let mut p = json!({"face": [5, 0, t + 8.0], "modeled": true});
        if let (Some(p), Value::Object(e)) = (p.as_object_mut(), extra) {
            p.extend(e);
        }
        run(&mut s, "solid.thread", p);
        s
    };
    let plate_left = |mut s: Session, t: f64| {
        run(
            &mut s,
            "solid.box",
            json!({"corner": [-30, -30, t], "length": 60, "width": 60, "height": 30, "body_name": "Above", "operation": "new_body"}),
        );
        run(&mut s, "solid.combine", json!({"target": "Body1", "tools": ["Above"], "operation": "cut"}));
        exact_volume(&s, "Body1")
    };
    let plate = |t: f64| (40.0 * 40.0 - (4.0 - PI) * 25.0) * t;
    let (p, w) = (1.5, ext_half_width(10.0, 1.5));
    for t in [6.0, 1.0] {
        let s = boss(t, json!({}));
        let want = plate(t) + PI * 25.0 * 16.0 - (16.0 - w) / p * ext_turn(10.0, p, 5.0);
        let got = exact_volume(&s, "Body1");
        assert!((got - want).abs() < want * 2e-4, "plate {t}: {got} vs {want}");
        let left = plate_left(s, t);
        assert!((left - plate(t)).abs() < 1e-6 * plate(t), "plate {t}: lost {} mm³", plate(t) - left);
    }
    for extra in [json!({"ease_out": "standard"}), json!({"ease_out": "short", "tip": "both"})] {
        let s = boss(1.0, extra.clone());
        assert!(s.model.state().body("Body1").unwrap().body.validity().is_empty(), "{extra}");
        let left = plate_left(s, 1.0);
        assert!((left - plate(1.0)).abs() < 1e-6 * plate(1.0), "{extra}: the plate below the boss lost {} mm³", plate(1.0) - left);
    }
}

/// Rig 602: "from the end nearest the pick" took the head end for a pick a little above the
/// middle. A thread of limited length starts at the face's only free end, wherever it is picked.
#[test]
fn a_thread_of_some_length_starts_at_the_only_free_end() {
    let mut s = shaft_on_collar();
    let th = run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "length": 6}));
    let (a, b) = (th["thread"]["start"].as_f64().unwrap(), th["thread"]["end"].as_f64().unwrap());
    // The axis points up (+z) from the shaft's own axis point; the free end is the top, z = 35.
    let k = th["thread"]["axis"][2].as_f64().unwrap();
    let z = |t: f64| th["thread"]["axis_point"][2].as_f64().unwrap() + k * t;
    let (z0, z1) = (z(a).min(z(b)), z(a).max(z(b)));
    assert!((z0 - 29.0).abs() < 1e-6 && (z1 - 35.0).abs() < 1e-6, "{th}");
}

/// Ladder 06/003: a pick in the middle of a face whose two ends are both free was a coin toss
/// (it fell to whichever end rounding favoured). It is refused, naming `from`; with `from` it runs
/// from that end, and a square stop inside the face takes exactly length / P turns.
#[test]
fn a_mid_face_pick_between_two_free_ends_needs_from() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Part"}));
    let e = s.execute("solid.thread", &json!({"face": [5, 0, 15], "designation": "M10", "length": 15, "modeled": true}));
    let e = e.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("both ends of the face are free") && e.contains("`from`"), "{e}");
    run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "length": 15, "from": [0, 0, 30], "modeled": true}));
    let want = PI * 25.0 * 30.0 - 10.0 * ext_turn(10.0, 1.5, 5.0);
    let got = exact_volume(&s, "Part");
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
    // The groove is all in the top half: just under the surface, the bottom 15 mm (less the
    // stop's half width) is whole and the top has the groove's turns.
    let st = s.model.state();
    let body = &st.body("Part").unwrap().body;
    let ring = |z: f64| (0..36).map(|i| (i as f64 * 10.0).to_radians()).map(move |a| Vec3::new(4.9 * a.cos(), 4.9 * a.sin(), z));
    let rings = |z0: f64, z1: f64| (0..=40).map(move |i| z0 + (z1 - z0) * i as f64 / 40.0).flat_map(ring);
    assert!(rings(0.5, 14.0).all(|q| body.contains(q)), "the bottom half is cut");
    assert!(rings(16.0, 29.5).filter(|q| !body.contains(*q)).count() > 100, "the top half is not threaded");
}

/// Up to a plane, or to a point's place along the axis, from the end given.
#[test]
fn a_thread_runs_up_to_a_plane_or_a_point() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "body_name": "Shaft"}));
    let z = |th: &Value| {
        let t = &th["thread"];
        let at = |u: &str| t["axis_point"][2].as_f64().unwrap() + t["axis"][2].as_f64().unwrap() * t[u].as_f64().unwrap();
        let (a, b) = (at("start"), at("end"));
        (a.min(b), a.max(b))
    };
    let th = run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "up_to": {"origin": [0, 0, 12], "normal": [0, 0, 1]}, "from": [0, 0, 0]}));
    let (z0, z1) = z(&th);
    assert!(z0.abs() < 1e-6 && (z1 - 12.0).abs() < 1e-6, "{th}");
    let th = run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "extent": "up_to", "up_to": [4, 0, 5], "from": [0, 0, 20]}));
    let (z0, z1) = z(&th);
    assert!((z0 - 5.0).abs() < 1e-6 && (z1 - 20.0).abs() < 1e-6, "{th}");
    // An extent that contradicts the values given is refused.
    let e = s.execute("solid.thread", &json!({"face": [4, 0, 10], "extent": "full", "length": 5})).err().map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("extent: full"));
}

/// A blind tapped hole: the thread runs out of the entry and stops clear of the bottom, the
/// groove taking (depth - w)/P turns of 2π∫ r g(r) dr over the wall, w its widest half width (at
/// the tool's inner edge, 0.1 P inside the minor diameter).
#[test]
fn a_blind_tapped_hole_stops_clear_of_its_bottom() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.hole", json!({"position": [0, 0, 20], "diameter": 8.5, "depth": 12}));
    let before = exact_volume(&s, "Block");
    run(&mut s, "solid.thread", json!({"face": [4.25, 0, 14], "designation": "M10", "modeled": true}));
    let (d, p, rh) = (10.0, 1.5, 4.25);
    let t = (PI / 6.0).tan();
    let minor = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let w = (p / 8.0 + 2.0 * (d / 2.0 - (minor - 0.1 * p)) * t) / 2.0;
    // g(r) = P/8 + 2 (d/2 - r) tan 30°, over the wall r 4.25..5.
    let f = |r: f64| (p / 8.0 + d * t) * r * r / 2.0 - 2.0 * t * r * r * r / 3.0;
    let turn = 2.0 * PI * (f(d / 2.0) - f(rh));
    let want = before - (12.0 - w) / p * turn;
    let got = exact_volume(&s, "Block");
    assert!((got - want).abs() < before * 1e-4, "{got} vs {want}");
}

/// A form's groove width at radius r between its root and crest: the pitch less the tooth, the
/// tooth `flat` wide at the crest radius `crest` and widening by tan a + tan b per unit of depth.
fn form_turn(p: f64, flat: f64, crest: f64, a: f64, b: f64, r0: f64, r1: f64, internal: bool) -> f64 {
    let t = a.to_radians().tan() + b.to_radians().tan();
    let n = 2000;
    let w = |r: f64| p - flat - (if internal { r - crest } else { crest - r }) * t;
    (0..n).map(|i| r0 + (r1 - r0) * (i as f64 + 0.5) / n as f64).map(|r| 2.0 * PI * r * w(r) * (r1 - r0) / n as f64).sum()
}

/// Every standard form cut into a shaft of its major diameter over six turns, right- and
/// left-handed: the groove takes 2π∫ r w(r) dr per turn (Pappus on the section, screw motion),
/// w from the standard's profile, from its external minor diameter to the shaft's surface.
#[test]
fn every_form_cuts_its_standard_profile() {
    let inch = 25.4;
    // (form, designation, d, P, crest flat, flanks, external minor radius)
    let tr_ac = 0.25;
    let acme_a = 0.020 * inch;
    let t3 = 3f64.to_radians().tan() + 30f64.to_radians().tan();
    let t7 = 7f64.to_radians().tan() + 1.0;
    let cases = [
        ("trapezoidal", "Tr20x4", 20.0, 4.0, 2.0 * (1.0 - 15f64.to_radians().tan()), (15.0, 15.0), 10.0 - 2.0 - tr_ac),
        (
            "acme",
            "3/4-6 ACME",
            0.75 * inch,
            inch / 6.0,
            inch / 12.0 * (1.0 - 14.5f64.to_radians().tan()),
            (14.5, 14.5),
            0.375 * inch - inch / 12.0 - acme_a / 2.0,
        ),
        ("buttress", "S20x4", 20.0, 4.0, (4.0 - 3.0 * t3) / 2.0, (3.0, 30.0), 10.0 - 0.86777 * 4.0),
        (
            "buttress_asme",
            "1.5-8 BUTT",
            1.5 * inch,
            inch / 8.0,
            (inch / 8.0 - 0.6 * inch / 8.0 * t7) / 2.0,
            (7.0, 45.0),
            0.75 * inch - 0.6 * inch / 8.0,
        ),
    ];
    for (form, designation, d, p, flat, (a, b), r3) in cases {
        let mut volumes = Vec::new();
        for hand in ["right", "left"] {
            let mut s = Session::default();
            let len = 6.0 * p;
            run(&mut s, "solid.cylinder", json!({"radius": d / 2.0, "height": len, "body_name": "Shaft"}));
            let before = exact_volume(&s, "Shaft");
            let th = run(
                &mut s,
                "solid.thread",
                json!({"face": [d / 2.0, 0, len / 2.0], "form": form, "designation": designation, "hand": hand, "modeled": true}),
            );
            assert!(th["thread"].is_null(), "{th}");
            let st = s.model.state();
            assert!(st.body("Shaft").unwrap().body.validity().is_empty(), "{designation} {hand}");
            let want = 6.0 * form_turn(p, flat, d / 2.0, a, b, r3, d / 2.0, false);
            let got = before - exact_volume(&s, "Shaft");
            assert!((got - want).abs() < want * 1e-4, "{designation} {hand}: removed {got}, the standard's profile {want}");
            volumes.push(got);
        }
        assert!((volumes[0] - volumes[1]).abs() < volumes[0] * 1e-5, "{designation}: right {} left {}", volumes[0], volumes[1]);
    }
}

/// A left-handed groove turns the other way: on the helix z = P θ / 2π (right-handed) the shaft
/// is cut once a turn, on z = -P θ / 2π it is cut all along. Both: a right- and a left-handed
/// groove, crossing.
#[test]
fn a_left_handed_thread_turns_the_other_way() {
    let (d, p, len) = (10.0, 1.5, 9.0);
    let r = (d / 2.0 + d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0) / 2.0;
    let on = |s: &Session, sign: f64| -> usize {
        let st = s.model.state();
        let body = &st.body("Shaft").unwrap().body;
        // Along each helix sense from every groove point on the z = 4.5 circle, count how often
        // the line stays in the groove: a groove lies on one of them.
        (0..36)
            .filter(|i| {
                let a0 = *i as f64 * 10f64.to_radians();
                let z0 = 4.5;
                let probe = |a: f64| Vec3::new(r * (a0 + a).cos(), r * (a0 + a).sin(), z0 + sign * p * a / std::f64::consts::TAU);
                !body.contains(probe(0.0)) && (1..12).all(|k| !body.contains(probe(k as f64 * 0.5)))
            })
            .count()
    };
    let mut volumes = Vec::new();
    for hand in ["right", "left", "both"] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": d / 2.0, "height": len, "body_name": "Shaft"}));
        let before = exact_volume(&s, "Shaft");
        run(&mut s, "solid.thread", json!({"face": [5, 0, 4.5], "designation": "M10", "hand": hand, "modeled": true}));
        assert!(s.model.state().body("Shaft").unwrap().body.validity().is_empty(), "{hand}");
        let (right, left) = (on(&s, 1.0), on(&s, -1.0));
        match hand {
            "right" => assert!(right > 0 && left == 0, "right: {right} {left}"),
            "left" => assert!(left > 0 && right == 0, "left: {right} {left}"),
            _ => assert!(left > 0 && right > 0, "both: {right} {left}"),
        }
        volumes.push(before - exact_volume(&s, "Shaft"));
    }
    let want = 6.0 * ext_turn(d, p, d / 2.0);
    assert!((volumes[0] - want).abs() < want * 1e-4 && (volumes[1] - want).abs() < want * 1e-4, "{volumes:?} vs {want}");
    // Crossing, the two grooves share some of their volume.
    assert!(volumes[2] > want * 1.2 && volumes[2] < 2.0 * want, "{volumes:?}");
}

/// A left-hand thread and tapped hole saved by v0.24.0 (format 39, `handedness: "left"`) open
/// left-handed; a tapped hole takes `hand` like a thread, never `both`, and only with a thread.
#[test]
fn a_format_39_left_hand_thread_and_tapped_hole_open_left_handed() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 60, "height": 12, "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [20, 30, 12], "thread": "M6", "depth": 8, "hand": "left"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 9, "base": [80, 0, 0], "body_name": "Shaft", "operation": "new"}));
    run(&mut s, "solid.thread", json!({"face": [85, 0, 4.5], "designation": "M10", "hand": "left"}));
    let hands = |s: &Session| -> Vec<solidcraft_doc::thread_forms::ThreadHand> { s.model.state().threads.iter().map(|t| t.hand).collect() };
    let left = solidcraft_doc::thread_forms::ThreadHand::Left;
    assert_eq!(hands(&s), [left, left]);
    // What format 39 wrote: the direction as `handedness`, no pending changes.
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    v["format"] = json!("solidcraft/39");
    v.as_object_mut().unwrap().remove("format_pending");
    for f in v["features"].as_array_mut().unwrap() {
        if let Some(h) = f.as_object_mut().unwrap().remove("hand") {
            f["handedness"] = h;
        }
    }
    let reopened = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    assert_eq!(hands(&reopened), [left, left]);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 60, "height": 12}));
    for (params, why) in [
        (json!({"position": [20, 30, 12], "thread": "M6", "depth": 8, "hand": "both"}), "unknown hand"),
        (json!({"position": [20, 30, 12], "depth": 8, "hand": "left"}), "needs `thread`"),
    ] {
        let err = s.execute("solid.hole", &params).unwrap_err().to_string();
        assert!(err.contains(why), "{err}");
    }
}

/// A trapezoidal nut: the groove from the drilled minor diameter D1 = d - P out to the
/// internal root D4 = d + 2 ac (ISO 2904), 2π∫ r w(r) dr per turn.
#[test]
fn an_internal_trapezoidal_thread_cuts_out_to_its_root() {
    let (d, p, ac) = (20.0, 4.0, 0.25);
    let d1 = d - p;
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 6.0 * p, "body_name": "Nut"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -1], "radius": d1 / 2.0, "height": 6.0 * p + 2.0, "body_name": "Bore"}));
    run(&mut s, "solid.combine", json!({"target": "Nut", "tools": ["Bore"], "operation": "cut"}));
    let before = exact_volume(&s, "Nut");
    run(&mut s, "solid.thread", json!({"face": [d1 / 2.0, 0, 3.0 * p], "designation": "Tr20x4", "modeled": true}));
    let flat = p / 2.0 * (1.0 - 15f64.to_radians().tan());
    let want = 6.0 * form_turn(p, flat, d1 / 2.0, 15.0, 15.0, d1 / 2.0, d / 2.0 + ac, true);
    let got = before - exact_volume(&s, "Nut");
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
}

/// No current standard defines a square thread: it is refused, saying what to use.
#[test]
fn a_square_thread_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20}));
    let e = s.execute("solid.thread", &json!({"face": [10, 0, 10], "form": "square", "designation": "20x4"})).err().map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("no current standard defines a square thread"));
}

/// Ladder 06/008: n starts are n grooves of lead n P, each a turn's n-th further round: over the
/// same length they take n x L/(nP) turns, the volume of one start of that pitch, and do not
/// meet. A designation that gives the lead gives the starts.
#[test]
fn a_multi_start_thread_takes_the_volume_of_one_start() {
    let (d, p, len) = (10.0, 1.5, 30.0);
    let want = len / p * ext_turn(d, p, d / 2.0);
    for (starts, designation) in [(2, "M10"), (3, "M10"), (1, "M10xPh3P1.5")] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": len, "body_name": "Shaft"}));
        let before = exact_volume(&s, "Shaft");
        run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": designation, "starts": starts, "modeled": true}));
        let st = s.model.state();
        assert!(st.body("Shaft").unwrap().body.validity().is_empty(), "{designation} x{starts}");
        let got = before - exact_volume(&s, "Shaft");
        assert!((got - want).abs() < want * 1e-4, "{designation} x{starts}: removed {got}, one start {want}");
    }
    // The cosmetic thread says it: two starts, lead 3.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20, "height": 40}));
    let th = run(&mut s, "solid.thread", json!({"face": [20, 0, 20], "designation": "Tr40x14(P7)"}));
    assert_eq!(th["thread"]["designation"], "Tr40x14(P7)", "{th}");
    assert_eq!(th["thread"]["starts"], 2, "{th}");
    let e = s.execute("solid.thread", &json!({"face": [20, 0, 20], "designation": "Tr40x14(P7)", "starts": 3})).err().map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("has 2 start(s), not 3"));
}

/// The stock's volume after a modelled M10 thread, over sections z0..z1 whose outer radius is
/// `rmax(z)`: 2π∫∫ r m(r) dr dz, m the material fraction per pitch (1 under the root, 1 - w/P
/// in the groove's band). The groove's screw motion holds every meridian half-plane alike, and
/// an axisymmetric cut after it (a chamfer) only lowers rmax.
fn m10_stock(z0: f64, z1: f64, rmax: impl Fn(f64) -> f64) -> f64 {
    let (d, p) = (10.0, 1.5);
    let root = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let t = (PI / 6.0).tan();
    // ∫_0^R r m(r) dr in closed form: R²/2 to the root, then m = a + b r (1 - (P/4 + 2 (r - root)
    // tan 30°)/P) beyond it.
    let ring = |rr: f64| {
        let inner = rr.min(root);
        let mut v = inner * inner / 2.0;
        if rr > root {
            let (a, b) = (1.0 - 0.25 + 2.0 * root * t / p, -2.0 * t / p);
            let f = |r: f64| a * r * r / 2.0 + b * r * r * r / 3.0;
            v += f(rr) - f(root);
        }
        v
    };
    let n = 20000;
    (0..n).map(|i| z0 + (z1 - z0) * (i as f64 + 0.5) / n as f64).map(|z| 2.0 * PI * ring(rmax(z)) * (z1 - z0) / n as f64).sum()
}

/// ISO 4753's chamfered end on both free ends of an M10 stud: a 45° cone down to the external
/// minor diameter d3 (ISO 724), cut after the groove; the volume as the stock's chamfered section
/// with the thread's material fraction (as ladder 06/006). d3 lies below the modelled root, so
/// each end face is the whole disk of d3, with the cone a whole ring around it, not pinched
/// where the groove reaches the end.
#[test]
fn a_chamfered_tip_tapers_the_last_teeth() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Stud"}));
    run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "tip": "both", "modeled": true}));
    let st = s.model.state();
    let stud = &st.body("Stud").unwrap().body;
    assert!(stud.validity().is_empty());
    let rf = 5.0 - 17.0 / 24.0 * 1.5 * 3f64.sqrt() / 2.0;
    let ends: Vec<f64> =
        stud.faces(1e-4).unwrap().into_iter().filter(|f| f.plane_normal.is_some_and(|n| n.z.abs() > 0.999)).map(|f| f.area).collect();
    assert_eq!(ends.len(), 2);
    for a in ends {
        assert!((a - PI * rf * rf).abs() < 1e-3 * a, "each end is a whole disk of d3: {a} vs {}", PI * rf * rf);
    }
    let want = m10_stock(0.0, 30.0, |z| 5f64.min(rf + z).min(rf + 30.0 - z));
    let got = exact_volume(&s, "Stud");
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
    // 30° to the axis and 0.5 mm deep, at the one free end of a shaft on a collar.
    let mut s = shaft_on_collar();
    run(
        &mut s,
        "solid.thread",
        json!({"face": [5, 0, 25], "designation": "M10", "tip": "both", "chamfer_angle": "30 deg", "chamfer_depth": 0.5, "modeled": true}),
    );
    let w = ext_half_width(10.0, 1.5);
    let rf = 4.5;
    let slope = 30f64.to_radians().tan();
    let want = PI * 64.0 * 10.0 + PI * 25.0 * w + m10_stock(10.0 + w, 35.0, |z| 5f64.min(rf + (35.0 - z) * slope));
    let got = exact_volume(&s, "Part");
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
    // Internal threads take a countersink instead.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Nut"}));
    run(&mut s, "solid.hole", json!({"position": [0, 0, 10], "diameter": 8.5}));
    let e =
        s.execute("solid.thread", &json!({"face": [4.25, 0, 5], "designation": "M10", "tip": "both", "modeled": true})).err().map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("countersink"));
    // ISO 4753 is for ISO metric threads: another form names its chamfer depth.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 40, "body_name": "Screw"}));
    let e = s
        .execute("solid.thread", &json!({"face": [10, 0, 20], "designation": "Tr20x4", "tip": "both", "modeled": true}))
        .err()
        .map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("chamfer depth"));
    run(&mut s, "solid.thread", json!({"face": [10, 0, 20], "designation": "Tr20x4", "tip": "both", "chamfer_depth": 2.5, "modeled": true}));
    assert!(s.model.state().body("Screw").unwrap().body.validity().is_empty());
}

/// `tip: start` chamfers only the end the thread starts from: on a stud started from its top, the
/// top is chamfered down to d3 and the bottom stays as cut. A start against a shoulder is no free
/// end and is refused; `both` there chamfers the one free end.
#[test]
fn a_tip_at_the_start_chamfers_only_that_end() {
    let rf = 5.0 - 17.0 / 24.0 * 1.5 * 3f64.sqrt() / 2.0;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Stud"}));
    run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "tip": "start", "from": [5, 0, 30], "modeled": true}));
    assert!(s.model.state().body("Stud").unwrap().body.validity().is_empty());
    let want = m10_stock(0.0, 30.0, |z| 5f64.min(rf + 30.0 - z));
    let got = exact_volume(&s, "Stud");
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
    let mut s = shaft_on_collar();
    let e = s.execute("solid.thread", &json!({"face": [5, 0, 25], "designation": "M10", "tip": "start", "from": [5, 0, 10], "modeled": true}));
    assert!(e.err().map(|e| e.to_string()).unwrap_or_default().contains("not a free end"));
    run(&mut s, "solid.thread", json!({"face": [5, 0, 25], "designation": "M10", "tip": "both", "from": [5, 0, 10], "modeled": true}));
    let e = s.execute("solid.thread", &json!({"face": [5, 0, 25], "designation": "M10", "tip": "chamfer", "modeled": true}));
    assert!(e.err().map(|e| e.to_string()).unwrap_or_default().contains("now `both`"));
}

/// M10's groove section moved `d` out from the axis, clipped at the shaft's radius 5: the cut per
/// radian of turn, ∫ r w(r - d) dr from its moved root out to 5 (w the section's own width at
/// its radius, P/4 + 2 (ρ - root) tan 30°). A cubic in d until the root reaches the surface.
fn m10_cut_per_radian(d: f64) -> f64 {
    let p = 1.5;
    let root = 5.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let q1 = 2.0 * (PI / 6.0).tan();
    let q0 = p / 4.0 - q1 * root - q1 * d;
    let a = root + d;
    if a >= 5.0 {
        return 0.0;
    }
    let f = |r: f64| q0 * r * r / 2.0 + q1 * r * r * r / 3.0;
    f(5.0) - f(a)
}

/// The mean cut per radian over a runout whose offset runs linearly 0..h: Simpson on the cubic up
/// to where the root leaves the shaft (exact), nothing beyond.
fn m10_runout_mean(h: f64) -> f64 {
    let p = 1.5;
    let top = 5.0 * p * 3f64.sqrt() / 16.0;
    let n = 64;
    let step = top / n as f64;
    let simpson: f64 = (0..n)
        .map(|i| {
            let (a, b) = (i as f64 * step, (i + 1) as f64 * step);
            (m10_cut_per_radian(a) + 4.0 * m10_cut_per_radian((a + b) / 2.0) + m10_cut_per_radian(b)) * step / 6.0
        })
        .sum();
    simpson / h
}

/// ISO 3508 runouts on a shaft between two collars: the thread starts at the lower collar and
/// its inner end fades out over 1.25 P (short) or 2.5 P (standard) before the upper one, its
/// section moving out past the surface as it turns, reaching nothing where its section clears the
/// collar. The cut is the integral over the turn of the clipped section's 2π∫ r dA (Pappus with
/// the section moving): full turns at f(0), the runout at the mean of f over the offsets it
/// passes. A fade shorter than a turn is cut too; the start never fades (`ease_in` is refused).
#[test]
fn a_runout_fades_the_groove_out_before_the_shoulder() {
    let (p, w) = (1.5, ext_half_width(10.0, 1.5));
    let root = 5.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let h = 5.0 - root + 0.05 * p;
    let turns = (35.0 - 10.0 - 2.0 * w) / p;
    for (ease, r_out) in [(json!("short"), 1.25), (json!("standard"), 2.5), (json!("1 mm"), 1.0 / 1.5)] {
        let mut s = shaft_on_collar();
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, 35], "radius": 8, "height": 10, "operation": "join", "targets": ["Part"]}));
        let before = exact_volume(&s, "Part");
        run(&mut s, "solid.thread", json!({"face": [5, 0, 20], "designation": "M10", "ease_out": ease, "from": [0, 0, 10], "modeled": true}));
        assert!(s.model.state().body("Part").unwrap().body.validity().is_empty(), "{ease}");
        let want = 2.0 * PI * (m10_cut_per_radian(0.0) * (turns - r_out) + r_out * m10_runout_mean(h));
        let got = before - exact_volume(&s, "Part");
        // Where the section moves, the sweep's walls lie within its tolerance (1e-4 P) of the
        // true surfaces, not on them: up to that times the walls (about 64 mm² a turn) of the
        // runout and of a turn more. Measured: 0.018 (short), 0.012 (standard), 0.011 mm³ (1
        // mm), not growing with the runout's length; full-depth turns are exact radially.
        let bound = 1e-4 * p * 64.0 * (r_out + 1.0);
        assert!((got - want).abs() < bound, "{ease}: {got} vs {want}");
    }
    // Longer runouts than the thread are refused, and so is a runout at the start.
    let mut s = shaft_on_collar();
    let e = s.execute("solid.thread", &json!({"face": [5, 0, 20], "designation": "M10", "ease_out": 30, "modeled": true}));
    assert!(e.err().map(|e| e.to_string()).unwrap_or_default().contains("longer than the thread"));
    let e = s.execute("solid.thread", &json!({"face": [5, 0, 20], "designation": "M10", "ease_in": "standard", "modeled": true}));
    assert!(e.err().map(|e| e.to_string()).unwrap_or_default().contains("`ease_in` is gone"));
}

/// A modelled M10 thread's flanks are several B-spline faces per turn (spans of one sweep); where
/// two of them meet with one tangent plane and one curvature the join is left out of the shaded
/// view (still pickable and in the wireframe), so the flank reads as one surface. The normal angle across each join is measured
/// independently (`seam_diagnostics`, projecting stations onto both surfaces).
#[test]
fn a_modelled_threads_flank_joins_are_not_drawn() {
    use solidcraft_geom::brep::Surface;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5.0, "height": 12, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [5.0, 0, 6.0], "designation": "M10x1.5", "modeled": true}));
    let st = s.model.state();
    let body = &st.body("Shaft").unwrap().body;
    let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
    let record = body.record();
    let free_form = |f: u32| {
        record
            .faces
            .get(f as usize)
            .is_some_and(|f| f.nurbs.is_none() && matches!(f.surface, Surface::Spline { .. } | Surface::RationalSpline { .. }))
    };
    let tolerances = solidcraft_kernel::ContinuityTolerances { gap_mm: 1e-3, angle_deg: 0.5, curvature_per_mm: 1.0, ..Default::default() };
    let report = solidcraft_kernel::seam_diagnostics(body, &[], 33, tolerances).unwrap();
    let mut flank_joins = 0;
    let mut worst: f64 = 0.0;
    for seam in &report.seams {
        let e = seam.edge;
        let uses = &record.edges[e].uses;
        if uses.len() != 2 || !uses.iter().all(|u| free_form(u.face)) {
            continue;
        }
        let angle = seam.normal_angle_deg.value;
        if angle < 0.5 {
            flank_joins += 1;
            worst = worst.max(angle);
            assert!(mesh.smooth_edges[e] && !mesh.shaded_edge_visible(e), "join {e} at {angle} deg is drawn");
            assert!(mesh.edge_visible(e) && mesh.edge_pickable(e), "join {e} stays in the wireframe and pickable");
        } else {
            assert!(mesh.shaded_edge_visible(e), "crease {e} at {angle} deg is hidden");
        }
    }
    // SMLib's flank pieces meet with normals about 1.5e-6 degrees apart (measured).
    assert!(flank_joins >= 32 && worst < 1e-4, "{flank_joins} flank joins, the worst {worst} deg apart");
}

/// A pipe thread's profile for [`pipe_material`]: (flank half angle, thread height h, flat at crest
/// and root), each per the pitch: ISO 228-1/7-1 55° h 0.640327 P of H 0.960491 P (parallel) or
/// 0.960237 P (taper), ASME B1.20.1 60° h 0.8 P of H 0.866025 P.
fn pipe_profile(form: &str) -> (f64, f64, f64) {
    let (a, big_h, h) = match form {
        "bspp" | "rp" => (27.5f64, 0.960491, 0.640327),
        "bspt" => (27.5, 0.960237, 0.640327),
        _ => (30.0, 0.866025, 0.8),
    };
    (a.to_radians(), h, (big_h - h) * a.to_radians().tan())
}

/// The material a pipe thread leaves, over z0..z1, as 2π∫∫ r m dr dz with m the material
/// fraction per pitch at height y over the external thread's root, 1 - (P - flat - 2 y tan a)/P
/// (the tooth's width over the pitch), its major radius `crest(z)` (a taper's grows 1/32 per mm)
/// and the stock between `inner` and `outer`. External: full material under the root, the
/// envelope's cone taking all above the crest. Internal: the nut's teeth, crest at crest(z) - h,
/// nothing inside that (the tap's minor cone).
fn pipe_material(form: &str, p: f64, internal: bool, (z0, z1): (f64, f64), inner: f64, outer: f64, crest: impl Fn(f64) -> f64) -> f64 {
    let (a, h, flat) = pipe_profile(form);
    let (h, flat) = (h * p, flat * p);
    let tooth = |y: f64| ((p - flat) * p.recip() - 2.0 * y * a.tan() / p).clamp(0.0, 1.0);
    let (nz, nr) = (4000, 400);
    let mut v = 0.0;
    for i in 0..nz {
        let z = z0 + (z1 - z0) * (i as f64 + 0.5) / nz as f64;
        let rc = crest(z);
        let (root, top) = if internal { (rc, rc - h) } else { (rc - h, rc) };
        let mut ring = 0.0;
        // Exactly the full annuli, the band in fine steps.
        let (b0, b1) = (root.min(top).clamp(inner, outer), root.max(top).clamp(inner, outer));
        if internal {
            ring += (outer * outer - b1 * b1) / 2.0;
        } else {
            ring += (b0 * b0 - inner * inner) / 2.0;
        }
        for j in 0..nr {
            let r = b0 + (b1 - b0) * (j as f64 + 0.5) / nr as f64;
            let y = if internal { root - r } else { r - root };
            ring += r * tooth(y) * (b1 - b0) / nr as f64;
        }
        v += 2.0 * PI * ring * (z1 - z0) / nz as f64;
    }
    v
}

/// ISO 7-1 and ASME B1.20.1 taper threads, ISO 228-1 parallel: an R 1/2 and a 1/2-14 NPT on pipe
/// stubs of their pipes' outside diameters (21.3 mm, 0.840 in), an Rc 1/2 through a plate and a
/// G 1/2, each cut from the top across the whole face (a free end past each end, so every
/// meridian half-plane averages the same profile). The taper puts the external thread's gauge
/// diameter its gauge length in from the small end (R 1/2: 20.955 at 8.2 mm; NPT: E1 + h at L1)
/// and the internal one's at its face.
#[test]
fn pipe_threads_cut_their_standard_profiles_and_tapers() {
    let inch = 25.4;
    // (designation, form, internal, stock radius, crest radius at z for a 12 mm stock threaded from z = 12)
    let r_half: f64 = 20.955 / 2.0 - 8.2 / 32.0;
    let npt_half: f64 = ((0.75843 + 0.32 / 16.0) + 0.8 / 14.0) * inch / 2.0 - 0.32 * inch / 32.0;
    // ISO 7-1's Rp, parallel and internal, in a hole drilled to its minor diameter.
    let rp_minor = 20.955 / 2.0 - 0.640327 * inch / 14.0;
    let cases: [(&str, &str, bool, f64, Box<dyn Fn(f64) -> f64>); 5] = [
        ("R 1/2", "bspt", false, 10.65, Box::new(move |z| r_half + (12.0 - z) / 32.0)),
        ("1/2-14 NPT", "npt", false, 0.42 * inch, Box::new(move |z| npt_half + (12.0 - z) / 32.0)),
        ("G 1/2", "bspp", false, 20.955 / 2.0, Box::new(|_| 20.955 / 2.0)),
        ("Rc 1/2", "bspt", true, 9.2, Box::new(|z| 20.955 / 2.0 - (12.0 - z) / 32.0)),
        ("Rp 1/2", "rp", true, rp_minor + 1e-4, Box::new(|_| 20.955 / 2.0)),
    ];
    for (designation, form, internal, rs, crest) in cases {
        let stock = || {
            let mut s = Session::default();
            if internal {
                run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 12, "body_name": "Part"}));
                run(&mut s, "solid.cylinder", json!({"base": [0, 0, -1], "radius": rs, "height": 14, "body_name": "Bore"}));
                run(&mut s, "solid.combine", json!({"target": "Part", "tools": ["Bore"], "operation": "cut"}));
            } else {
                run(&mut s, "solid.cylinder", json!({"radius": rs, "height": 12, "body_name": "Part"}));
            }
            s
        };
        let face = [rs, 0.0, 6.0];
        let (mut s, mut cosmetic) = (stock(), stock());
        let out = run(&mut cosmetic, "solid.thread", json!({"face": face, "designation": designation, "from": [rs, 0, 12], "length": 12}));
        assert_eq!(out["thread"]["designation"], json!(designation));
        run(&mut s, "solid.thread", json!({"face": face, "designation": designation, "modeled": true, "from": [rs, 0, 12], "length": 12}));
        let st = s.model.state();
        assert!(st.body("Part").unwrap().body.validity().is_empty(), "{designation}");
        let p = inch / 14.0;
        let got = exact_volume(&s, "Part");
        let (want, stock) = if internal {
            let plate = 40.0 * 40.0 * 12.0;
            let hole = PI * rs * rs * 12.0;
            (plate - hole - (PI * (30.0 * 30.0 - rs * rs) * 12.0 - pipe_material(form, p, true, (0.0, 12.0), rs, 30.0, &crest)), plate - hole)
        } else {
            (pipe_material(form, p, false, (0.0, 12.0), 0.0, rs, &crest), PI * rs * rs * 12.0)
        };
        // A parallel thread's meridians are alike exactly. A taper's section moves off the axis
        // as it turns, so the meridians differ; the averaged profile met the cut within 0.12 % of
        // the removed volume here (R 1/2 0.06 %, NPT 0.12 %).
        let tol = if matches!(form, "bspp" | "rp") { 1e-5 } else { 2e-3 };
        assert!((got - want).abs() < (stock - want).abs() * tol, "{designation}: {got} vs {want} (stock {stock})");
    }
}

/// A taper pipe thread's rules: it starts at a free end (not at a shoulder), ISO 7-1 names the
/// internal taper Rc and the external R (and Rp is internal only), a pipe thread has one start and
/// turns one way, and its chamfered tip is cut from the small end's major diameter.
#[test]
fn a_taper_pipe_thread_starts_at_a_free_end_and_is_named_for_its_side() {
    let part = || {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 16, "height": 10, "body_name": "Part"}));
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, 10], "radius": 10.65, "height": 25, "operation": "join", "targets": ["Part"]}));
        s
    };
    let mut s = part();
    let err = |s: &mut Session, p: Value| s.execute("solid.thread", &p).err().map(|e| e.to_string()).unwrap_or_default();
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "R 1/2", "modeled": true, "from": [10.65, 0, 10]}));
    assert!(e.contains("starts at a free end"), "{e}");
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "Rc 1/2", "modeled": true, "from": [10.65, 0, 35]}));
    assert!(e.contains("an external taper thread is R"), "{e}");
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "R 1/2", "starts": 2, "modeled": true}));
    assert!(e.contains("one start"), "{e}");
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "R 1/2", "hand": "both", "modeled": true, "from": [10.65, 0, 35]}));
    assert!(e.contains("turns one way"), "{e}");
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "Rp 1/2", "modeled": true, "from": [10.65, 0, 35]}));
    assert!(e.contains("Rp is an internal thread"), "{e}");
    // ISO 4753's chamfered end is metric: a pipe thread gives its depth.
    let e = err(&mut s, json!({"face": [10.65, 0, 20], "designation": "R 1/2", "tip": "start", "modeled": true, "from": [10.65, 0, 35]}));
    assert!(e.contains("chamfer depth"), "{e}");
    // From the free end it runs ISO 7-1's useful thread, 13.2 mm for R 1/2.
    let out = run(&mut s, "solid.thread", json!({"face": [10.65, 0, 20], "designation": "R 1/2", "from": [10.65, 0, 35]}));
    let t = &out["thread"];
    assert!((t["end"].as_f64().unwrap() - t["start"].as_f64().unwrap() - 13.2).abs() < 1e-9, "{t}");
    let mut plain = part();
    run(&mut plain, "solid.thread", json!({"face": [10.65, 0, 20], "designation": "R 1/2", "modeled": true, "from": [10.65, 0, 35]}));
    assert!(plain.model.state().body("Part").unwrap().body.validity().is_empty());
    // The chamfered tip of a taper thread: the cone reaches the small end's major diameter less
    // the depth, and cuts the first teeth.
    let p = 25.4 / 14.0;
    run(
        &mut s,
        "solid.thread",
        json!({"face": [10.65, 0, 20], "designation": "R 1/2", "modeled": true, "from": [10.65, 0, 35], "tip": "start", "chamfer_depth": 0.640327 * p}),
    );
    assert!(s.model.state().body("Part").unwrap().body.validity().is_empty());
    let (chamfered, threaded) = (exact_volume(&s, "Part"), exact_volume(&plain, "Part"));
    assert!(chamfered < threaded - 1e-3, "the tip cuts the first teeth: {chamfered} vs {threaded}");
}

/// A tapped hole's thread cut into its wall (`modeled`), as the thread feature cuts one: an M8
/// blind hole whose groove stops clear of the drill point's bottom, or fades out before it
/// (`ease_out`, ISO 3508), which leaves more of the wall than the full form there; an Rc 1/2
/// through a plate, drilled with the tap maker's drill and cut to its taper; each a valid body
/// that reopens the same. An Rc size with no tabulated drill is refused.
#[test]
fn a_modelled_tapped_hole_stops_clear_of_its_bottom_or_fades_out_before_it() {
    let blind = |extra: Value| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":30,"width":30,"height":20,"body_name":"Plate"}));
        let mut p = json!({"position":[15,15,20],"direction":[0,0,-1],"thread":"M8","depth":14,"type":"drilled"});
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        run(&mut s, "solid.hole", p);
        assert!(s.model.state().body("Plate").unwrap().body.validity().is_empty(), "{extra}");
        s
    };
    let (drilled, square, fade) = (blind(json!({})), blind(json!({"modeled":true})), blind(json!({"modeled":true,"ease_out":"standard"})));
    let (v0, v1, v2) = (exact_volume(&drilled, "Plate"), exact_volume(&square, "Plate"), exact_volume(&fade, "Plate"));
    assert!(v1 < v2 && v2 < v0, "drilled {v0}, square {v1}, fade {v2}");
    // The groove never reaches the drill point: the bottom's cone is the drilled one.
    assert!(square.model.state().threads.is_empty(), "a modelled thread is cut, not listed");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&fade.doc.to_json()).unwrap());
    assert!((exact_volume(&reopened, "Plate") - v2).abs() < 1e-6);

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":40,"height":12,"body_name":"Plate"}));
    let out = run(&mut s, "solid.hole", json!({"position":[20,20,12],"direction":[0,0,-1],"thread":"Rc 1/2"}));
    let drilled = exact_volume(&s, "Plate");
    // The tap maker's drill for a straight Rc 1/2 hole (Yamawa YEU23 p.709).
    let drill: f64 = 17.6;
    assert!((drilled - (40.0 * 40.0 * 12.0 - PI * (drill / 2.0).powi(2) * 12.0)).abs() < 1e-3, "{drilled}");
    let id = out["feature"].as_u64().unwrap();
    let redefine = json!({"position":[20,20,12],"direction":[0,0,-1],"thread":"Rc 1/2","modeled":true});
    run(&mut s, "timeline.redefine", json!({"feature":id,"command":"solid.hole","params":redefine}));
    assert!(s.model.state().body("Plate").unwrap().body.validity().is_empty());
    assert!(exact_volume(&s, "Plate") < drilled - 1.0);

    let err = |p: Value| Session::default().execute("solid.hole", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err(json!({"position":[0,0,0],"diameter":5,"modeled":true})).contains("`modeled` needs `thread`"));
    assert!(err(json!({"position":[0,0,0],"thread":"M8","ease_out":"standard"})).contains("modelled thread"));
    assert!(err(json!({"position":[0,0,0],"thread":"Rc 5"})).contains("no tap drill is tabulated"));
    // Another drill than the tap maker's (M8: 6.8 mm): from the minor diameter to 5% over it.
    let drill = |d: f64| {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length":30,"width":30,"height":20,"body_name":"Plate"}));
        s.execute("solid.hole", &json!({"position":[15,15,20],"thread":"M8","diameter":d})).err().map(|e| e.to_string()).unwrap_or_default()
    };
    let e = drill(7.5);
    assert!(e.contains("from 6.647 to 7.14 mm") && e.contains("too little of the thread"), "{e}");
    assert!(drill(6.5).contains("the tap cannot cut"));
    assert!(drill(7.1).is_empty() && drill(6.7).is_empty(), "inside the range");
}
