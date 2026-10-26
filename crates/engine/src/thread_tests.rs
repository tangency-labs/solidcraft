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

/// What the ISO groove of a modelled internal thread takes out per unit length of material from
/// radius `from` out: bolt-shaped, P/8 flat at the major diameter, widening towards the bore.
fn nut_groove(d: f64, p: f64, from: f64) -> f64 {
    let slope = (PI / 6.0).tan();
    if from >= d / 2.0 {
        return 0.0;
    }
    per_turn(from, d / 2.0, |r| p / 8.0 + 2.0 * (d / 2.0 - r) * slope) / p
}

/// And the external one up to radius `to`: P/4 flat at the root, widening outwards.
fn bolt_groove(d: f64, p: f64, to: f64) -> f64 {
    let slope = (PI / 6.0).tan();
    let r_root = d / 2.0 - 5.0 * (p * 3f64.sqrt() / 2.0) / 8.0;
    let to = to.min(d / 2.0);
    if to <= r_root {
        return 0.0;
    }
    per_turn(r_root, to, |r| p / 4.0 + 2.0 * (r - r_root) * slope) / p
}

/// What a modelled internal thread over `len` removes from a hole of diameter `hole` (negative:
/// adds): its groove, less what it fills in when the hole is filled to the tap drill. The fill
/// stops a twentieth of the pitch short of each end, where the hole keeps its size.
fn nut_removes(d: f64, p: f64, hole: f64, len: f64, filled: bool) -> f64 {
    let r = hole / 2.0;
    if !filled {
        return nut_groove(d, p, r) * len;
    }
    let (tap, lip) = ((d - p) / 2.0, 0.05 * p);
    (nut_groove(d, p, tap) - PI * (r * r - tap * tap)) * (len - 2.0 * lip) + nut_groove(d, p, r) * 2.0 * lip
}

/// The same for an external thread on a shaft of diameter `shaft`, built up to the major
/// diameter.
fn bolt_removes(d: f64, p: f64, shaft: f64, len: f64) -> f64 {
    let (r, lip) = (shaft / 2.0, 0.05 * p);
    (bolt_groove(d, p, d / 2.0) - PI * (d * d / 4.0 - r * r)) * (len - 2.0 * lip) + bolt_groove(d, p, r) * 2.0 * lip
}

/// Issues 995 and 1443: an M5 modelled thread in a 5 mm hole (M4 in a 4 mm one, as holes for
/// printed parts are drawn) would cut nothing: the groove lies in the hole, from the minor
/// diameter out to the major one. The hole is first filled in to the tap drill over the
/// threaded length, so the thread comes out as in a hole drilled to it, and a shaft too thin is
/// built up to the major diameter. A cosmetic thread is still only a note.
#[test]
fn a_hole_too_wide_for_a_modelled_thread_is_filled_to_the_tap_drill() {
    let mut drilled = hex_nut(4.2);
    run(&mut drilled, "solid.thread", json!({"face": [2.1, 0, 2], "designation": "M5", "modeled": true}));
    let mut s = hex_nut(5.0);
    run(&mut s, "solid.thread", json!({"face": [2.5, 0, 2], "designation": "M5", "modeled": true}));
    // Apart from the twentieth of a pitch at each end the fill stops short of.
    let lip = PI * (2.5f64.powi(2) - 2.1f64.powi(2)) * 4.0 + nut_removes(5.0, 0.8, 5.0, 4.0, true) - nut_removes(5.0, 0.8, 4.2, 4.0, false);
    let (want, got) = (volume(&mut drilled, "Nut") - lip, volume(&mut s, "Nut"));
    assert!((got - want).abs() < 0.05, "M5 in a 5 mm hole: {got} mm³, as drilled 4.2 mm {want} mm³");

    let mut s = hex_nut(5.0);
    let before = volume(&mut s, "Nut");
    run(&mut s, "solid.thread", json!({"face": [2.5, 0, 2], "designation": "M5"}));
    assert!((volume(&mut s, "Nut") - before).abs() < 1e-9);

    // A shaft under the thread's root (17.29 mm for M20) is built up to 20 mm.
    let mut turned = Session::default();
    run(&mut turned, "solid.cylinder", json!({"radius": 10, "height": 10, "body_name": "Shaft"}));
    run(&mut turned, "solid.thread", json!({"face": [10, 0, 5], "designation": "M20", "modeled": true}));
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 8.6, "height": 10, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [8.6, 0, 5], "designation": "M20", "modeled": true}));
    let lip = bolt_removes(20.0, 2.5, 17.2, 10.0) - bolt_groove(20.0, 2.5, 10.0) * 10.0 + PI * (100.0 - 8.6 * 8.6) * 10.0;
    let (want, got) = (volume(&mut turned, "Shaft") - lip, volume(&mut s, "Shaft"));
    assert!((got - want).abs() < 0.5, "M20 on a 17.2 mm shaft: {got} mm³, as turned to 20 mm {want} mm³");
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
/// M5x0.5) is cut as it is, the shallower thread a tap would cut in it; a wider one is first
/// filled in to that pitch's tap drill.
#[test]
fn the_fill_limit_holds_for_coarse_and_fine_pitch() {
    for (m, d, p, hole, filled) in [
        ("M5", 5.0, 0.8, 4.5, false),
        ("M5", 5.0, 0.8, 4.75, false),
        ("M5", 5.0, 0.8, 4.8, true),
        ("M5", 5.0, 0.8, 5.0, true),
        ("M5", 5.0, 0.8, 5.1, true),
        ("M5x0.5", 5.0, 0.5, 4.5, false),
        ("M5x0.5", 5.0, 0.5, 4.85, false),
        ("M5x0.5", 5.0, 0.5, 4.9, true),
        ("M5x0.5", 5.0, 0.5, 5.0, true),
        ("M5x0.7", 5.0, 0.7, 5.0, true),
        ("M4", 4.0, 0.7, 4.0, true),
        ("M3", 3.0, 0.5, 3.0, true),
    ] {
        let mut s = hex_nut(hole);
        let before = volume(&mut s, "Nut");
        run(&mut s, "solid.thread", json!({"face": [hole / 2.0, 0, 2], "designation": m, "modeled": true}));
        let removed = before - volume(&mut s, "Nut");
        let (want, groove) = (nut_removes(d, p, hole, 4.0, filled), nut_groove(d, p, hole.min(d - p) / 2.0) * 4.0);
        // The threaded body is faceted: chords a two-hundredth of the pitch inside a wall left as
        // it is keep a sliver of the hole (two thirds of the sag round it).
        let sliver = if filled { 0.0 } else { 2.0 / 3.0 * p * 0.005 * PI * hole * 4.0 };
        assert!((removed - want).abs() < groove * 0.03 + sliver, "{m} in {hole} mm: removed {removed}, want {want}");
    }
}

/// A partial thread fills, or builds up, only its own length: the rest of a 4 mm hole stays
/// 4 mm, and the rest of a 14 mm boss (M16, on a block) keeps its size.
#[test]
fn a_partial_thread_fills_only_its_own_length() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.hole", json!({"position": [0, 0, 10], "diameter": 4}));
    let before = volume(&mut s, "Plate");
    run(&mut s, "solid.thread", json!({"face": [2, 0, 9], "designation": "M4", "length": 5, "modeled": true}));
    let removed = before - volume(&mut s, "Plate");
    let want = nut_removes(4.0, 0.7, 4.0, 5.0, true);
    // As above, for the 5 mm of the hole left as it is.
    let sliver = 2.0 / 3.0 * 0.7 * 0.005 * PI * 4.0 * 5.0;
    assert!((removed - want).abs() < nut_groove(4.0, 0.7, 1.65) * 5.0 * 0.03 + sliver, "removed {removed}, want {want}");
    let st = s.model.state();
    let Some(plate) = st.body("Plate") else { panic!("no plate") };
    // Teeth at 1.8 mm from the axis in the threaded half, none in the rest of the hole.
    let teeth = |z0: f64| (0..40).filter(|i| plate.body.contains(Vec3::new(1.8, 0.0, z0 + 4.0 * *i as f64 / 40.0))).count();
    assert!(teeth(5.5) > 5, "threaded: {}", teeth(5.5));
    assert_eq!(teeth(0.5), 0, "the rest of the hole is 4 mm");

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-15, -15, 0], "length": 30, "width": 30, "height": 10, "body_name": "Part"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 10], "radius": 7, "height": 15, "body_name": "Boss"}));
    run(&mut s, "solid.combine", json!({"target": "Part", "tools": ["Boss"], "operation": "join"}));
    let before = volume(&mut s, "Part");
    run(&mut s, "solid.thread", json!({"face": [7, 0, 24], "designation": "M16", "length": 8, "modeled": true}));
    let removed = before - volume(&mut s, "Part");
    let want = bolt_removes(16.0, 2.0, 14.0, 8.0);
    assert!((removed - want).abs() < bolt_groove(16.0, 2.0, 8.0) * 8.0 * 0.03, "removed {removed}, want {want}");
    let st = s.model.state();
    let Some(part) = st.body("Part") else { panic!("no part") };
    let teeth = |z0: f64| (0..40).filter(|i| part.body.contains(Vec3::new(7.5, 0.0, z0 + 5.0 * *i as f64 / 40.0))).count();
    assert!(teeth(19.0) > 5, "threaded: {}", teeth(19.0));
    assert_eq!(teeth(10.5), 0, "the rest of the boss is 14 mm");
}

/// A ring would also fill a notch or the mouth of a cross hole: a face that is not a whole band
/// is refused as before. A blind, counterbored or countersunk hole is a whole band, and filled.
#[test]
fn only_a_whole_band_is_filled() {
    let plate = |s: &mut Session| {
        run(s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 10, "body_name": "Plate"}));
    };
    let refused = |s: &mut Session, at: Value| {
        let before = volume(s, "Plate");
        let e = s.execute("solid.thread", &json!({"face": at, "designation": "M5", "modeled": true})).err().map(|e| e.to_string());
        let e = e.unwrap_or_default();
        assert!(e.contains("this 5 mm hole is too wide for a modelled M5 thread"), "{e}");
        assert!((volume(s, "Plate") - before).abs() < 1e-9);
    };
    // A half-round notch in the plate's side.
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.cylinder", json!({"base": [10, 0, -1], "radius": 2.5, "height": 12, "body_name": "Notch"}));
    run(&mut s, "solid.combine", json!({"target": "Plate", "tools": ["Notch"], "operation": "cut"}));
    refused(&mut s, json!([7.5, 0, 5]));
    // A 2 mm cross hole through the hole's wall.
    let mut s = Session::default();
    plate(&mut s);
    run(&mut s, "solid.hole", json!({"position": [0, 0, 10], "diameter": 5}));
    run(&mut s, "solid.hole", json!({"position": [10, 0, 5], "diameter": 2}));
    refused(&mut s, json!([0, 2.5, 7]));

    for hole in [
        json!({"position": [0, 0, 10], "diameter": 5, "depth": 6}),
        json!({"position": [0, 0, 10], "diameter": 5, "depth": 6, "type": "drilled"}),
        json!({"position": [0, 0, 10], "diameter": 5, "type": "counterbore", "cb_diameter": 9, "cb_depth": 3}),
        json!({"position": [0, 0, 10], "diameter": 5, "type": "countersink", "cs_diameter": 9, "cs_angle": 90}),
    ] {
        let mut s = Session::default();
        plate(&mut s);
        run(&mut s, "solid.hole", hole.clone());
        let before = volume(&mut s, "Plate");
        run(&mut s, "solid.thread", json!({"face": [2.5, 0, 5.5], "designation": "M5", "modeled": true}));
        let added = volume(&mut s, "Plate") - before;
        assert!(added > 1.0, "{hole}: added {added}");
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

/// Issue 524: a modelled thread down to the end of its face ran a pitch past it, as if past a
/// free end: into the part under a shoulder, the floor of a blind hole or the rim of a drill
/// point (where the cut could also fail). Where the part goes on past the end, the groove stops
/// short of it; past a free end it still runs out (the volumes above).
#[test]
fn a_thread_stops_short_of_a_shoulder_or_a_blind_bottom() {
    // How many of 24 points round the axis at height `z` and radius `r` are in the body.
    let solid = |s: &Session, body: &str, z: f64, r: f64| {
        let st = s.model.state();
        let Some(b) = st.body(body) else { panic!("no {body}") };
        (0..24).filter(|i| b.body.contains(Vec3::new(r * (PI * *i as f64 / 12.0).cos(), r * (PI * *i as f64 / 12.0).sin(), z))).count()
    };
    // Where the hole's (or pin's) wall ends below, before the thread.
    let bottom = |s: &Session, body: &str, at: Vec3| {
        let st = s.model.state();
        let c = st.body(body).and_then(|b| solidcraft_kernel::cylinder_face_at(&b.body, at));
        let Some(c) = c else { panic!("no cylinder at {at:?}") };
        (c.axis_point + c.axis * c.start).z.min((c.axis_point + c.axis * c.end).z)
    };

    // M8 over the whole of an 8 mm pin, down to the base it stands on.
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 6, "body_name": "Base"}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 6], "radius": 4, "height": 10, "body_name": "Pin"}));
    run(&mut s, "solid.combine", json!({"target": "Base", "tools": ["Pin"], "operation": "join"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 12], "designation": "M8", "modeled": true}));
    assert_eq!(solid(&s, "Base", 5.9, 3.7), 24, "the base under the shoulder is whole");
    assert!(solid(&s, "Base", 12.0, 3.7) < 24, "the pin is threaded");

    // M5 in blind holes drilled to its tap drill: with a drill point (this one failed to cut),
    // and flat.
    for kind in ["drilled", "simple"] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [-8, -8, 0], "length": 16, "width": 16, "height": 10, "body_name": "Block"}));
        run(&mut s, "solid.hole", json!({"position": [0, 0, 10], "diameter": 4.2, "depth": 7, "type": kind}));
        let z = bottom(&s, "Block", Vec3::new(2.1, 0.0, 6.0));
        run(&mut s, "solid.thread", json!({"face": [2.1, 0, 6], "designation": "M5", "modeled": true}));
        assert_eq!(solid(&s, "Block", z - 0.1, 2.3), 24, "{kind}: what is past the hole's wall is whole");
        assert!(solid(&s, "Block", 6.0, 2.3) < 24, "{kind}: the hole is threaded");
    }
}
