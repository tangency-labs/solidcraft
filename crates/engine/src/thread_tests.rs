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

#[test]
fn thread_handedness_cosmetic_defaults_and_save_reopen_keep_axis_and_extent() {
    use solidcraft_doc::{Document, FeatureKind, ThreadHandedness};
    for (hole, designation) in [(false, "M6"), (true, "M6"), (true, "G 1/4"), (true, "1/4-20 UNC")] {
        let mut reference = None;
        for hand in [None, Some("right"), Some("left")] {
            let mut s = Session::default();
            let (command, mut params) = if hole {
                run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
                (
                    "solid.hole",
                    json!({"position":[10,10,10],"thread":designation,"depth":6,"thread_depth":3,
                        "references":[{"edge":[0,10,10],"distance":"7 mm"},{"edge":[10,0,10],"distance":"8 mm"}]}),
                )
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
            if hole {
                assert!((info.end - info.start - 3.0).abs() < 1e-8, "direction must retain the independent thread depth");
            }
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
        ("acme", "3/4-6 ACME", 0.75 * inch, inch / 6.0, inch / 12.0 * (1.0 - 14.5f64.to_radians().tan()), (14.5, 14.5), 0.375 * inch - inch / 12.0 - acme_a / 2.0),
        ("buttress", "S20x4", 20.0, 4.0, (4.0 - 3.0 * t3) / 2.0, (3.0, 30.0), 10.0 - 0.86777 * 4.0),
        ("buttress_asme", "1.5-8 BUTT", 1.5 * inch, inch / 8.0, (inch / 8.0 - 0.6 * inch / 8.0 * t7) / 2.0, (7.0, 45.0), 0.75 * inch - 0.6 * inch / 8.0),
    ];
    for (form, designation, d, p, flat, (a, b), r3) in cases {
        let mut volumes = Vec::new();
        for hand in ["right", "left"] {
            let mut s = Session::default();
            let len = 6.0 * p;
            run(&mut s, "solid.cylinder", json!({"radius": d / 2.0, "height": len, "body_name": "Shaft"}));
            let before = exact_volume(&s, "Shaft");
            let th = run(&mut s, "solid.thread", json!({"face": [d / 2.0, 0, len / 2.0], "form": form, "designation": designation, "hand": hand, "modeled": true}));
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

/// ISO 4753's chamfered end on both free ends of an M10 stud: a 45° cone down to the minor
/// diameter, cut after the groove; the volume as the stock's chamfered section with the thread's
/// material fraction (as ladder 06/006).
#[test]
fn a_chamfered_tip_tapers_the_last_teeth() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Stud"}));
    run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "tip": "chamfer", "modeled": true}));
    let st = s.model.state();
    assert!(st.body("Stud").unwrap().body.validity().is_empty());
    let rf = 5.0 - 5.0 * 1.5 * 3f64.sqrt() / 16.0;
    let want = m10_stock(0.0, 30.0, |z| 5f64.min(rf + z).min(rf + 30.0 - z));
    let got = exact_volume(&s, "Stud");
    assert!((got - want).abs() < want * 1e-4, "{got} vs {want}");
    // 30° to the axis and 0.5 mm deep, at the one free end of a shaft on a collar.
    let mut s = shaft_on_collar();
    run(&mut s, "solid.thread", json!({"face": [5, 0, 25], "designation": "M10", "tip": "chamfer", "chamfer_angle": "30 deg", "chamfer_depth": 0.5, "modeled": true}));
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
    let e = s.execute("solid.thread", &json!({"face": [4.25, 0, 5], "designation": "M10", "tip": "chamfer", "modeled": true})).err().map(|e| e.to_string());
    assert!(e.unwrap_or_default().contains("countersink"));
}
