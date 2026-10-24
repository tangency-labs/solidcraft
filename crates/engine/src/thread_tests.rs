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

#[test]
fn thread_handedness_cosmetic_defaults_and_save_reopen_keep_axis_and_extent() {
    use solidcraft_doc::{Document, FeatureKind, ThreadHandedness};
    for hole in [false, true] {
        let mut reference = None;
        for hand in [None, Some("right"), Some("left")] {
            let mut s = Session::default();
            let (command, mut params) = if hole {
                run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
                ("solid.hole", json!({"position":[10,10,10],"thread":"M6","depth":6}))
            } else {
                run(&mut s, "solid.cylinder", json!({"radius":3,"height":6}));
                ("solid.thread", json!({"face":[3,0,2],"designation":"M6","length":4}))
            };
            if let Some(hand) = hand {
                params["handedness"] = json!(hand);
            }
            run(&mut s, command, params);
            let expected = if hand == Some("left") { ThreadHandedness::Left } else { ThreadHandedness::Right };
            let info = s.model.state().threads[0].clone();
            assert_eq!(info.handedness, expected);
            let dimensions = (info.axis_point, info.axis, info.start, info.end, info.major_diameter, info.pitch);
            assert_eq!(*reference.get_or_insert(dimensions), dimensions, "direction must not change axis, length or size");
            let mut kind = serde_json::to_value(&s.doc.features.last().unwrap().kind).unwrap();
            assert_eq!(kind.get("handedness"), (expected == ThreadHandedness::Left).then(|| json!("left")).as_ref());
            // An old feature with no field still deserializes to Right, for both feature types.
            kind.as_object_mut().unwrap().remove("handedness");
            let old: FeatureKind = serde_json::from_value(kind).unwrap();
            assert!(matches!(
                old,
                FeatureKind::Hole { handedness: ThreadHandedness::Right, .. } | FeatureKind::Thread { handedness: ThreadHandedness::Right, .. }
            ));
            let reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
            assert_eq!(reopened.model.state().threads[0], info);
        }
    }
}

#[test]
fn thread_handedness_invalid_values_fail_without_changing_the_document() {
    for hole in [false, true] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius":3,"height":6}));
        let saved = s.doc.to_json();
        for hand in [json!("clockwise"), json!("LEFT"), json!(null), json!(true), json!(1)] {
            let (command, params) = if hole {
                ("solid.hole", json!({"position":[0,0,6],"thread":"M6","handedness":hand}))
            } else {
                ("solid.thread", json!({"face":[3,0,2],"designation":"M6","handedness":hand}))
            };
            assert!(s.execute(command, &params).unwrap_err().to_string().contains("handedness"));
            assert_eq!(s.doc.to_json(), saved);
        }
        if hole {
            assert!(s.execute("solid.hole", &json!({"position":[0,0,6],"diameter":2,"handedness":"left"})).is_err());
            assert_eq!(s.doc.to_json(), saved);
        }
    }
}

#[test]
fn thread_handedness_modelled_internal_and_external_keep_chirality_and_bounded_volume() {
    use solidcraft_doc::Document;
    for internal in [false, true] {
        let mut bodies = Vec::new();
        for hand in ["right", "left"] {
            let mut s = Session::default();
            run(&mut s, "solid.cylinder", json!({"radius":if internal {6.0} else {3.0},"height":4,"body_name":"Stock"}));
            if internal {
                run(&mut s, "solid.hole", json!({"position":[0,0,4],"diameter":5}));
            }
            let face = json!([if internal { 2.5 } else { 3.0 }, 0, 3.5]);
            run(&mut s, "solid.thread", json!({"face":face,"designation":"M6x1","length":3,"modeled":true,"handedness":hand}));
            let state = s.model.state();
            let body = state.body("Stock").unwrap().body.clone();
            assert!(body.validity().is_empty(), "{internal}/{hand}: {:?}", body.validity());
            let before = volume(&mut s, "Stock");
            let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
            assert!((volume(&mut reopened, "Stock") - before).abs() < 1e-8);
            let brep_volume = crate::kernel::measure(&body).unwrap().volume;
            bodies.push((body, before, brep_volume));
        }
        let [(right, vr, br), (left, vl, bl)] = bodies.as_slice() else { panic!("two directions") };
        eprintln!("handedness internal={internal}: mesh_mm3={vr}/{vl}, brep_mm3={br}/{bl}");
        for body in [right, left] {
            let mesh = body.tessellate(0.001).unwrap();
            let low = mesh.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
            let high = mesh.positions.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max);
            assert!(low.abs() < 1e-8 && (high - 4.0).abs() < 1e-8, "physical axial bounds: {low}/{high}");
            for z in [0.25, 1.5, 2.5, 3.75] {
                assert_eq!(body.contains(Vec3::new(0.0, 0.0, z)), !internal, "axis stays solid in a shaft and open in a bore");
                if internal {
                    for i in 0..24 {
                        let a = 2.0 * PI * i as f64 / 24.0;
                        assert!(body.contains(Vec3::new(4.5 * a.cos(), 4.5 * a.sin(), z)), "the outer wall remains whole");
                    }
                }
            }
        }
        let radius = if internal { 2.75 } else { 2.65 };
        let mut different = 0;
        for z in [0.4, 1.1, 1.4, 1.8, 2.2, 2.6, 3.1, 3.5] {
            for i in 0..24 {
                let angle = 2.0 * PI * (i as f64 + 0.17) / 24.0;
                let p = Vec3::new(radius * angle.cos(), radius * angle.sin(), z);
                let reflected = Vec3::new(-p.x, p.y, p.z);
                assert_eq!(right.contains(p), left.contains(reflected), "actual mirrored helix: {internal} at {p:?}");
                different += usize::from(right.contains(p) != left.contains(p));
                if z < 1.0 {
                    assert!(right.contains(p), "the 3 mm thread must leave the lower stub unchanged");
                }
            }
        }
        assert!(different > 20, "Left must change real geometry, not just metadata: {internal}: {different}");
        // The sweep itself is an exact reflection (doc test). The existing Boolean first
        // facets the curved stock within pitch * .005 mm. A nominal stock-faceting
        // regression envelope uses radial annuli [r-d, r+d], with flat caps unchanged:
        // pi*h*((r+d)^2-(r-d)^2) = 4*pi*h*r*d, summed over the outer and bore cylinders.
        // Exact-set cutting would not enlarge the stock's symmetric difference. The BSP
        // and rebuild also use numerical epsilons, so this conservative secondary ceiling
        // is not a rigorous bound on every Boolean error, nor a precision/equal-volume proof.
        let delta = 1.0 * 0.005;
        let radii = if internal { 6.0 + 2.5 } else { 3.0 };
        let budget = 4.0 * PI * 4.0 * radii * delta;
        eprintln!(
            "handedness internal={internal}: post_boolean_delta_mm3={}, stock_faceting_budget_mm3={budget}, relative_delta={}",
            (br - bl).abs(),
            (br - bl).abs() / br
        );
        assert!(
            (br - bl).abs() <= budget,
            "post-Boolean volume exceeds the configured stock faceting envelope: {internal}: {br}/{bl}, budget {budget}"
        );
    }
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
