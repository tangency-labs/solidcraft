//! Modelled threads: the ISO metric profile cut along a helix, inside and outside,
//! including fine pitches on large shafts and the sample flange.

use solidcraft_geom::prelude::*;
use std::f64::consts::PI;

use serde_json::{Value, json};
use solidcore_record::Vec3;

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

/// Inch fastener threads use the same command path as scripts/MCP, retain their callout and
/// class, and round-trip the new form. All geometry is synthetic cylindrical stock.
#[test]
fn unified_shaft_threads_keep_their_size_class_and_form_after_reopen() {
    use solidcraft_doc::thread_forms::ThreadForm;
    for check in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        for (text, major, tpi) in
            [("1/4-20 UNC-2A", 6.35, 20.0), ("#10-32 UNF", 4.826, 32.0), ("0.25-32 UNEF-3A", 6.35, 32.0), ("3/8-20 UN-1A", 9.525, 20.0)]
        {
            let mut s = Session { param_check: check, ..Session::default() };
            run(&mut s, "solid.cylinder", json!({"radius":major / 2.0,"height":10}));
            let reply = run(&mut s, "solid.thread", json!({"face":[major / 2.0,0,5],"designation":text}));
            assert_eq!(reply["thread"]["designation"], text);
            assert_eq!(reply["thread"]["form"], "unified");
            let reported = run(&mut s, "model.threads", json!({}));
            assert_eq!(reported["threads"][0]["form"], "unified");
            let assert_size = |session: &Session| {
                let state = session.model.state();
                let thread = &state.threads[0];
                assert_eq!(thread.designation, text);
                assert_eq!(thread.form, ThreadForm::Unified);
                assert!((thread.major_diameter - major).abs() < 1e-12);
                assert_eq!(thread.pitch, 25.4 / tpi);
                assert!(!thread.internal);
            };
            assert_size(&s);
            let saved = s.doc.to_json();
            let value: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(value["features"].as_array().unwrap().last().unwrap()["form"], "unified");
            assert_size(&Session::new(solidcraft_doc::Document::from_json(&saved).unwrap()));
        }
    }
}

/// Before Unified support, an inch callout without a stored form was refused as metric.
/// Opening it must not opt in; creating or explicitly editing that Thread records Unified.
#[test]
fn unified_threads_require_a_saved_form_but_new_and_edited_threads_record_it() {
    use solidcraft_doc::thread_forms::ThreadForm;
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":3.175,"height":10}));
    let params = json!({"face":[3.175,0,5],"designation":"1/4-20 UNC"});
    let made = run(&mut s, "solid.thread", params.clone());
    let feature = made["feature"].as_u64().unwrap();
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    assert_eq!(saved["features"].as_array().unwrap().last().unwrap()["form"], "unified");
    let assert_unified = |session: &Session| {
        assert!(session.model.results.last().unwrap().error.is_none());
        let state = session.model.state();
        assert_eq!(state.threads.len(), 1);
        assert_eq!(state.threads[0].form, ThreadForm::Unified);
        assert_eq!(state.threads[0].designation, "1/4-20 UNC");
        assert_eq!((state.threads[0].major_diameter, state.threads[0].pitch), (6.35, 1.27));
    };
    assert_unified(&Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap()));

    // Match the file before this change: no Thread form and no Unified pending-format marker.
    let mut legacy = saved;
    legacy["features"].as_array_mut().unwrap().last_mut().unwrap().as_object_mut().unwrap().remove("form");
    // A released format names no pending changes, so there is no marker to take out then
    // (`get_mut`: indexing would add a null `format_pending`, which a file may not have).
    if let Some(pending) = legacy.get_mut("format_pending").and_then(Value::as_array_mut) {
        pending.retain(|change| change.as_str() != Some("unified-thread-form"));
    }
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&legacy.to_string()).unwrap());
    let error = reopened.model.results.last().unwrap().error.as_deref().unwrap();
    assert!(error.contains("`1/4-20 UNC` is a unified designation, not metric"), "{error}");
    assert!(reopened.model.state().threads.is_empty());

    // Other forms keep their historical inference from a designation with no stored form.
    let mut trapezoidal = legacy;
    trapezoidal["features"].as_array_mut().unwrap().last_mut().unwrap()["designation"] = json!("Tr6.35x1");
    let trapezoidal = Session::new(solidcraft_doc::Document::from_json(&trapezoidal.to_string()).unwrap());
    assert!(trapezoidal.model.results.last().unwrap().error.is_none());
    assert_eq!(trapezoidal.model.state().threads[0].form, ThreadForm::Trapezoidal);

    run(&mut reopened, "timeline.redefine", json!({"feature":feature,"command":"solid.thread","params":params}));
    assert_unified(&reopened);
    let saved = reopened.doc.to_json();
    let value: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(value["features"].as_array().unwrap().last().unwrap()["form"], "unified");
    assert_unified(&Session::new(solidcraft_doc::Document::from_json(&saved).unwrap()));
}

#[test]
fn unified_thread_takes_a_drilled_hole_and_refuses_the_wrong_side_class() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":20,"width":20,"height":10}));
    // The catalogue's #7 cutting-tap drill for 1/4-20 UNC is 0.201 in = 5.1054 mm.
    run(&mut s, "solid.hole", json!({"position":[10,10,10],"diameter":5.1054}));
    let before = s.doc.to_json();
    for modeled in [false, true] {
        let error =
            s.execute("solid.thread", &json!({"face":[12.5527,10,5],"designation":"1/4-20 UNC-2A","modeled":modeled})).unwrap_err().to_string();
        assert!(error.contains("hole needs 1B, 2B or 3B"), "{error}");
        assert_eq!(s.doc.to_json(), before);
    }
    for designation in ["1/4-20 UNC", "1/4-20 UNC-2B"] {
        for modeled in [false, true] {
            let mut trial = Session::new(solidcraft_doc::Document::from_json(&before).unwrap());
            run(&mut trial, "solid.thread", json!({"face":[12.5527,10,5],"designation":designation,"modeled":modeled}));
            let state = trial.model.state();
            if modeled {
                assert!(state.bodies[0].body.validity().is_empty());
                // The internal 60° basic groove runs from the #7 pilot to the major radius:
                // 10/P turns times 2π∫ r w(r) dr, with minor radius d/2 - 5 P √3/16.
                let pitch = 1.27;
                let minor = 3.175 - 5.0 * pitch * 3f64.sqrt() / 16.0;
                let pilot_volume = 4000.0 - PI * (5.1054_f64 / 2.0).powi(2) * 10.0;
                let removed = 10.0 / pitch * form_turn(pitch, pitch / 4.0, minor, 30.0, 30.0, 5.1054 / 2.0, 3.175, true);
                let actual = exact_volume(&trial, &state.bodies[0].name);
                assert!((actual - (pilot_volume - removed)).abs() < removed * 1e-4, "{designation}: {actual}, removed {removed}");
            } else {
                assert_eq!(state.threads[0].designation, designation);
                assert_eq!(state.threads[0].pitch, 1.27);
                assert!(state.threads[0].internal);
            }
        }
    }
}

#[test]
fn unified_threads_refuse_mismatched_shafts_and_suggest_a_catalogued_size() {
    for modeled in [false, true] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius":5,"height":10}));
        let before = s.doc.to_json();
        let error = s.execute("solid.thread", &json!({"face":[5,0,5],"designation":"1/4-20 UNC","modeled":modeled})).unwrap_err().to_string();
        assert!(error.contains("does not fit a 10.00 mm shaft") && error.contains("this shaft takes 3/8-16 UNC"), "{error}");
        assert_eq!(s.doc.to_json(), before);
        let error = s.execute("solid.thread", &json!({"face":[5,0,5],"designation":"3/8-16 UNC-2B","modeled":modeled})).unwrap_err().to_string();
        assert!(error.contains("class 2B") && error.contains("shaft needs 1A, 2A or 3A"), "{error}");
        assert_eq!(s.doc.to_json(), before);
    }
}

/// The two places an ISO metric thread is tried with a class: an 8 mm shaft 6 mm long, and a 6.8
/// mm hole (the tap maker's M8 drill) through a 6 mm plate. The point on the face, the face's side.
fn metric_class_stock(s: &mut Session, hole: bool) -> ([f64; 3], bool) {
    if hole {
        run(s, "solid.box", json!({"length":20,"width":20,"height":6}));
        run(s, "solid.hole", json!({"position":[10,10,6],"diameter":6.8}));
        ([13.4, 10.0, 3.0], true)
    } else {
        run(s, "solid.cylinder", json!({"radius":4,"height":6}));
        ([4.0, 0.0, 3.0], false)
    }
}

/// An ISO metric designation keeps its ISO 965-1 tolerance class in the callout, through a save
/// and reopen, under either parameter check; the size is the designation's without the class.
#[test]
fn metric_shaft_and_hole_threads_keep_their_class_after_reopen() {
    for check in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        for (hole, given, shown, major, pitch) in [
            (false, "M8-6g", "M8-6g", 8.0, 1.25),
            (false, "M8x1.25-5g6g", "M8-5g6g", 8.0, 1.25),
            (false, "M8 – 4h", "M8-4h", 8.0, 1.25),
            (true, "M8-6H", "M8-6H", 8.0, 1.25),
            (true, "M8x1-5H6H", "M8x1-5H6H", 8.0, 1.0),
        ] {
            let mut s = Session { param_check: check, ..Session::default() };
            let (face, internal) = metric_class_stock(&mut s, hole);
            let reply = run(&mut s, "solid.thread", json!({"face":face,"designation":given}));
            assert_eq!(reply["thread"]["designation"], shown, "{given}");
            assert_eq!(run(&mut s, "model.threads", json!({}))["threads"][0]["designation"], shown);
            let assert_thread = |session: &Session| {
                assert!(session.model.results.iter().all(|r| r.error.is_none()), "{given}");
                let state = session.model.state();
                let thread = &state.threads[0];
                assert_eq!(thread.designation, shown);
                assert_eq!((thread.major_diameter, thread.pitch, thread.internal), (major, pitch, internal), "{given}");
            };
            assert_thread(&s);
            let saved = s.doc.to_json();
            let value: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(value["features"].as_array().unwrap().last().unwrap()["designation"], given, "saved as given");
            // The class is part of format 73 since the 0.51.1 release, so it is never pending,
            // whatever other changes are.
            if let Some(pending) = value.get("format_pending").and_then(Value::as_array) {
                assert!(!pending.iter().any(|c| c == "metric-thread-class"), "{pending:?}");
            }
            assert_thread(&Session::new(solidcraft_doc::Document::from_json(&saved).unwrap()));
        }
    }
}

/// A lowercase class is a shaft's and an uppercase one a hole's (ISO 965-1); the other side's is
/// refused, cosmetic or modelled, and the design is left as it was.
#[test]
fn a_metric_class_for_the_other_side_is_refused() {
    for (hole, given, wants) in [(false, "M8-6H", "this shaft needs e, f, g or h"), (true, "M8-6g", "this hole needs G or H")] {
        for modeled in [false, true] {
            let mut s = Session::default();
            let (face, _) = metric_class_stock(&mut s, hole);
            let before = s.doc.to_json();
            let error = s.execute("solid.thread", &json!({"face":face,"designation":given,"modeled":modeled})).unwrap_err().to_string();
            assert!(error.contains(&format!("class {}", &given[3..])) && error.contains(wants), "{error}");
            assert_eq!(s.doc.to_json(), before);
        }
    }
    // A size that doesn't fit is refused with the size that does, not with the class added.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":5,"height":10}));
    let error = s.execute("solid.thread", &json!({"face":[5,0,5],"designation":"M8-6g"})).unwrap_err().to_string();
    assert!(error.contains("an M8-6g thread does not fit a 10.00 mm shaft; this shaft takes M10"), "{error}");
}

/// The class is an annotation: a modelled thread with it is the thread without it, record for
/// record (the native B-rep encoding, compared byte for byte, two sessions without a class first
/// to show the build is repeatable), and a cosmetic one has the same numbers, bit for bit.
#[test]
fn a_metric_class_changes_no_geometry() {
    let build = |hole: bool, designation: &str, modeled: bool| {
        let mut s = Session::default();
        let (face, _) = metric_class_stock(&mut s, hole);
        run(&mut s, "solid.thread", json!({"face":face,"designation":designation,"modeled":modeled}));
        let state = s.model.state();
        let body = solidcraft_kernel::encode_native_body(&state.bodies[0].body).unwrap();
        // A modelled thread is the groove itself and leaves no cosmetic marking.
        assert_eq!(state.threads.len(), usize::from(!modeled));
        let marking = state.threads.first().map(|t| {
            let numbers =
                [t.major_diameter, t.radius, t.pitch, t.axis_point.x, t.axis_point.y, t.axis_point.z, t.axis.x, t.axis.y, t.axis.z, t.start, t.end]
                    .map(f64::to_bits);
            (numbers, t.internal, t.form, t.hand, t.starts)
        });
        (body, marking)
    };
    for (hole, class) in [(false, "M8-6g"), (true, "M8-6H")] {
        for modeled in [false, true] {
            let plain = build(hole, "M8", modeled);
            assert!(build(hole, "M8", modeled) == plain, "the build repeats (hole {hole}, modelled {modeled})");
            assert!(build(hole, class, modeled) == plain, "{class} (modelled {modeled}) changed the geometry");
        }
    }
}

#[test]
fn automatic_thread_sizing_stays_metric_until_unified_is_selected() {
    for (form, designation, major, pitch) in [(None, "M6", 6.0, 1.0), (Some("metric"), "M6", 6.0, 1.0), (Some("unified"), "1/4-20 UNC", 6.35, 1.27)] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius":3.175,"height":10}));
        let mut params = json!({"face":[3.175,0,5]});
        if let Some(form) = form {
            params["form"] = json!(form);
        }
        run(&mut s, "solid.thread", params);
        let state = s.model.state();
        let thread = &state.threads[0];
        assert_eq!(thread.designation, designation);
        assert_eq!((thread.major_diameter, thread.pitch), (major, pitch));
    }
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
    assert_eq!(reopened.model.state().bodies[0].body.classify_point(Vec3::new(10.0, 12.0, 10.0)).unwrap(), solidcraft_kernel::PointClass::Outside);
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
    use crate::kernel;
    use solidcore_record::{Plane, Vec2, Vec3};

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
        // Each thread form names itself in its refusal: `unknown metric thread`.
        assert!(e.contains("unknown metric thread"), "{designation}: {e}");
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
fn the_dialog_fills_in_a_size_with_its_pitch_past_the_coarse_range() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 75, "height": 10, "body_name": "Shaft"}));
    let state = s.model.state();
    let size = crate::cmd::large_thread_size(&state, Vec3::new(75.0, 0.0, 5.0)).unwrap();
    assert_eq!(size, "M150x4");
    run(&mut s, "solid.thread", json!({"face": [75, 0, 5], "designation": size}));
    // A cylinder the coarse table covers is sized automatically, so there is nothing to fill in.
    run(&mut s, "solid.cylinder", json!({"base":[300,0,0],"radius": 10, "height": 10, "body_name": "Small", "operation": "new"}));
    assert_eq!(crate::cmd::large_thread_size(&s.model.state(), Vec3::new(310.0, 0.0, 5.0)), None);
}

#[test]
fn qualified_automatic_sizing_past_the_coarse_range_ignores_another_bodys_valid_size() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base":[33.5,0,0],"radius":4,"height":10,"body_name":"Small","operation":"new"}));
    run(&mut s, "solid.cylinder", json!({"radius":37.5,"height":10,"body_name":"Large","operation":"new"}));
    let saved = s.doc.to_json();
    // Both walls pass through this point: the selected body's diameter must decide the suggestion.
    for faces in [json!([{"body":"Large","point":[37.5,0,5]}]), json!([{"body":"Small","point":[37.5,0,5]},{"body":"Large","point":[37.5,0,5]}])] {
        let e = s.execute("solid.thread", &json!({"faces":faces})).unwrap_err().to_string();
        assert!(e.contains("beyond the coarse thread range (up to M56)") && e.contains("M75x3 or M75x2"), "{e}");
        assert_eq!(s.doc.to_json(), saved);
        assert!(s.model.state().threads.is_empty());
    }
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
        let th = run(&mut s, "solid.thread", json!({"face": [d1 / 2.0, 0, len / 2.0], "designation": format!("M{d}"), "modeled": true}));
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
fn a_modelled_thread_that_would_cut_nothing_narrows_the_hole_instead() {
    // #11609: the hole is narrowed to the thread's minor diameter, and the reply says so.
    let mut s = hex_nut(5.0);
    run(&mut s, "solid.thread", json!({"face": [2.5, 0, 2], "designation": "M5"}));
    let before = volume(&mut s, "Nut");
    let made = run(&mut s, "solid.thread", json!({"face": [2.5, 0, 2], "designation": "M5", "modeled": true}));
    let warning = s.model.result(made["feature"].as_u64().unwrap()).and_then(|r| r.warning.clone()).unwrap_or_default();
    assert!(warning.contains("narrowed the 5 mm hole to 4.1") && warning.contains("M5"), "{warning}");
    assert_eq!(made["warning"], json!(warning), "the reply says so too");
    assert!(volume(&mut s, "Nut") > before + 1.0);

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
/// M5x0.5) is cut as drilled, a wider one is narrowed to the thread's minor diameter first.
#[test]
fn the_cut_nothing_limit_holds_for_coarse_and_fine_pitch() {
    for (m, hole, refused) in [
        ("M5", 4.5, None),
        ("M5", 4.75, None),
        ("M5", 4.8, Some("narrowed the 4.8 mm hole")),
        ("M5x0.5", 4.5, None),
        ("M5x0.5", 4.85, None),
        ("M5x0.5", 4.9, Some("narrowed the 4.9 mm hole")),
        ("M5x0.5", 5.0, Some("narrowed the 5 mm hole")),
    ] {
        let mut s = hex_nut(hole);
        let before = volume(&mut s, "Nut");
        let made = s.execute("solid.thread", &json!({"face": [hole / 2.0, 0, 2], "designation": m, "modeled": true}));
        let cut = before - volume(&mut s, "Nut");
        let warning = made.ok().and_then(|v| v["feature"].as_u64()).and_then(|f| s.model.result(f)).and_then(|r| r.warning.clone());
        match (refused, warning) {
            (None, None) => assert!(cut > 0.05, "{m} in {hole} mm: removed {cut}"),
            (Some(want), Some(w)) => {
                assert!(w.contains(want) && w.contains(m), "{m} in {hole} mm: {w}");
                assert!(cut < -0.05, "{m} in {hole} mm: the bore was filled in, removed {cut}");
            }
            (want, got) => panic!("{m} in {hole} mm: want narrowed {want:?}, got {got:?}"),
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
/// collar below as into the air above. It runs down to the shoulder and is trimmed at its plane
/// (#11610): the collar keeps its whole π 8² 10, and the groove takes 25/P turns, each slice of
/// the shaft along the axis losing the same.
#[test]
fn a_thread_runs_down_to_a_shoulder_and_leaves_it_whole() {
    let mut s = shaft_on_collar();
    run(&mut s, "solid.thread", json!({"face": [5, 0, 25], "designation": "M10", "modeled": true}));
    let want = PI * 64.0 * 10.0 + PI * 25.0 * 25.0 - 25.0 / 1.5 * ext_turn(10.0, 1.5, 5.0);
    let got = exact_volume(&s, "Part");
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
}

/// The volume of a body's part between two heights along Z, within a quadrant of X and Y when
/// given (their signs).
fn volume_between(s: &Session, b: &str, (z0, z1): (f64, f64), quadrant: Option<(bool, bool)>) -> f64 {
    let st = s.model.state();
    let mut part = st.body(b).unwrap().body.clone();
    let on = |origin: Vec3, normal: Vec3| solidcore_record::Plane::from_normal(origin, normal.normalized().unwrap()).unwrap();
    let z = Vec3::new(0.0, 0.0, 1.0);
    let mut cuts = vec![(on(Vec3::new(0.0, 0.0, z0), z), true), (on(Vec3::new(0.0, 0.0, z1), z), false)];
    if let Some((x, y)) = quadrant {
        cuts.push((on(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)), x));
        cuts.push((on(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0)), y));
    }
    for (plane, positive) in cuts {
        part = solidcraft_kernel::half_space(&part, &plane, positive).unwrap().0.unwrap();
    }
    solidcraft_kernel::measure(&part).unwrap().volume
}

/// #11610: a boss standing on a flat plate. The groove reaches the plate at every angle of the
/// last turn, wherever the helix lands on it (each height a different gap under a pitch): a thin
/// slice on the plate has lost as much as any slice of the thread, a pitch's slice as much in each
/// quadrant (the uncut block of the old stop sat in some of them), and the plate is whole. Before,
/// the last pitch kept up to a whole turn's groove uncut.
#[test]
fn a_thread_reaches_a_flat_shoulder_at_every_angle_and_leaves_it_whole() {
    let (p, t, turn) = (1.5, 6.0, ext_turn(10.0, 1.5, 5.0));
    let per_mm = PI * 25.0 - turn / p;
    for (h, extra) in [
        (16.0, json!({})),
        (16.45, json!({})),
        (16.6, json!({})),
        (16.9, json!({})),
        (17.4, json!({})),
        (16.0, json!({"hand": "left"})),
        (16.6, json!({"tip": "both"})),
    ] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": t, "body_name": "Part"}));
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, t], "radius": 5, "height": h, "operation": "join", "targets": ["Part"]}));
        let mut ask = json!({"face": [5, 0, t + h / 2.0], "designation": "M10", "modeled": true});
        if let (Some(a), Value::Object(e)) = (ask.as_object_mut(), &extra) {
            a.extend(e.clone());
        }
        run(&mut s, "solid.thread", ask);
        let name = format!("h {h} {extra}");
        assert!(s.model.state().body("Part").unwrap().body.validity().is_empty(), "{name}");
        let plate = volume_between(&s, "Part", (-1.0, t), None);
        assert!((plate - 1600.0 * t).abs() < 1e-6 * 1600.0 * t, "{name}: the plate lost {} mm³", 1600.0 * t - plate);
        let thin = volume_between(&s, "Part", (t, t + 0.1 * p), None);
        let want = per_mm * 0.1 * p;
        assert!((thin - want).abs() < want * 2e-3, "{name}: the slice on the plate is {thin}, not {want}");
        for quadrant in [(true, true), (true, false), (false, true), (false, false)] {
            let got = volume_between(&s, "Part", (t, t + p), Some(quadrant));
            let want = per_mm * p / 4.0;
            assert!((got - want).abs() < want * 2e-3, "{name}: {quadrant:?} of the last pitch is {got}, not {want}");
        }
    }
}

/// #11610: where a shoulder is not flat. A fillet or a chamfer at the shaft's base stays whole, the
/// groove trimmed where the face ends above it. A shoulder that rises back along the face (a
/// slanted plate, a boss on a round body) would be cut by a groove running to the plane of the
/// face's lowest point, so the groove keeps its square stop there. Nothing here fails.
#[test]
fn a_thread_at_a_shoulder_that_is_not_flat_still_builds() {
    let (p, turn) = (1.5, ext_turn(10.0, 1.5, 5.0));
    let plate = |s: &mut Session| {
        run(s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 6, "body_name": "Part"}));
        run(s, "solid.cylinder", json!({"base": [0, 0, 6], "radius": 5, "height": 16, "operation": "join", "targets": ["Part"]}));
    };
    for finish in [("solid.fillet", json!({"edges": [[5, 0, 6]], "radius": 1})), ("solid.chamfer", json!({"edges": [[5, 0, 6]], "distance": 1}))] {
        let mut s = Session::default();
        plate(&mut s);
        run(&mut s, finish.0, finish.1);
        let (base, shaft) = (volume_between(&s, "Part", (-1.0, 7.0), None), exact_volume(&s, "Part"));
        run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "modeled": true}));
        assert!(s.model.state().body("Part").unwrap().body.validity().is_empty(), "{}", finish.0);
        let left = volume_between(&s, "Part", (-1.0, 7.0), None);
        assert!((left - base).abs() < 1e-6 * base, "{}: the base lost {} mm³", finish.0, base - left);
        let (got, want) = (exact_volume(&s, "Part"), shaft - 15.0 / p * turn);
        assert!((got - want).abs() < want * 2e-4, "{}: {got} vs {want}", finish.0);
    }
    let rising = [
        vec![("solid.cylinder", json!({"base": [0, 0, 0], "axis": [0.2, 0, 1], "radius": 20, "height": 6, "body_name": "Part"}))],
        vec![("solid.cylinder", json!({"base": [0, -20, 0], "axis": [0, 1, 0], "radius": 10, "height": 40, "body_name": "Part"}))],
    ];
    for cmds in rising {
        let mut s = Session::default();
        for (c, ask) in cmds {
            run(&mut s, c, ask);
        }
        run(&mut s, "solid.cylinder", json!({"base": [0, 0, 2], "radius": 5, "height": 26, "operation": "join", "targets": ["Part"]}));
        let before = exact_volume(&s, "Part");
        run(&mut s, "solid.thread", json!({"face": [5, 0, 20], "designation": "M10", "modeled": true}));
        assert!(s.model.state().body("Part").unwrap().body.validity().is_empty());
        assert!(exact_volume(&s, "Part") < before);
    }
}

/// #11610: the trim is for a thread that runs the whole face. A length stopping short of the
/// shoulder still stops there, however near.
#[test]
fn a_thread_of_some_length_stops_where_it_is_told_short_of_a_shoulder() {
    let (p, w) = (1.5, ext_half_width(10.0, 1.5));
    let mut s = shaft_on_collar();
    let before = exact_volume(&s, "Part");
    run(&mut s, "solid.thread", json!({"face": [5, 0, 25], "designation": "M10", "length": 24.6, "modeled": true}));
    // Its end is within half a pitch of the shoulder: the stop stays clear of it.
    let want = before - (24.6f64).min(25.0 - w) / p * ext_turn(10.0, p, 5.0);
    let got = exact_volume(&s, "Part");
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
}

/// A d10 boss (z t..t+16) on a plate t thick, threaded M10: the plate's top is the shoulder.
/// The groove, with or without its runout, stays on the boss: the plate keeps all of itself
/// (what lies below z = t after the boss is cut away), and without a runout the boss loses
/// 16/P turns, as at a collar. The owner's bug: built as the app builds it (a rounded
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
    let p = 1.5;
    for t in [6.0, 1.0] {
        let s = boss(t, json!({}));
        let want = plate(t) + PI * 25.0 * 16.0 - 16.0 / p * ext_turn(10.0, p, 5.0);
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
    assert!(rings(0.5, 14.0).all(|q| body.classify_point(q).unwrap() == solidcraft_kernel::PointClass::Inside), "the bottom half is cut");
    assert!(
        rings(16.0, 29.5).filter(|q| body.classify_point(*q).unwrap() == solidcraft_kernel::PointClass::Outside).count() > 100,
        "the top half is not threaded"
    );
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

/// The span a thread's reply gives, as world z from its axis point (low, high).
fn thread_z(th: &Value) -> (f64, f64) {
    let t = &th["thread"];
    let at = |u: &str| t["axis_point"][2].as_f64().unwrap() + t["axis"][2].as_f64().unwrap() * t[u].as_f64().unwrap();
    let (a, b) = (at("start"), at("end"));
    (a.min(b), a.max(b))
}

/// #23145: `offset` starts the thread that far in from the end it starts at, and the extent is
/// measured from there. On a d10 × 30 cylinder (z 0..30), offset 5: length 15 from the top is
/// z 25 down to 10, from the bottom 5 up to 20; Full from the top runs on to the far end (0..25);
/// up to the plane z = 12 from the top is 12..25; a length past the far end stops there (0..25).
#[test]
fn an_offset_thread_starts_that_far_in_from_its_end() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Rod"}));
    let top = json!([0, 0, 30]);
    for (extra, want) in [
        (json!({"length": 15, "from": top}), (10.0, 25.0)),
        (json!({"length": 15, "from": [0, 0, 0]}), (5.0, 20.0)),
        (json!({"from": top}), (0.0, 25.0)),
        (json!({"up_to": {"origin": [0, 0, 12], "normal": [0, 0, 1]}, "from": top}), (12.0, 25.0)),
        (json!({"length": 40, "from": top}), (0.0, 25.0)),
    ] {
        let mut p = json!({"face": [5, 0, 15], "designation": "M10", "offset": "5 mm"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let th = run(&mut s, "solid.thread", p);
        let (z0, z1) = thread_z(&th);
        assert!((z0 - want.0).abs() < 1e-9 && (z1 - want.1).abs() < 1e-9, "{extra}: {th}");
        // `model.threads` reports the same span as the reply.
        let listed = run(&mut s, "model.threads", json!({}));
        let feature = th["feature"].as_u64().unwrap();
        let mine = listed["threads"].as_array().unwrap().iter().find(|t| t["feature"].as_u64() == Some(feature)).unwrap();
        assert_eq!(mine, &th["thread"]);
    }
}

/// A modelled M10 on the d10 × 30 cylinder, offset 5 and length 15 from the top: both ends of the
/// groove are square stops inside the face (5 mm from the top, 10 mm from the bottom, each more
/// than half a pitch), so it takes exactly 15 / P = 10 turns. A turn's volume is `ext_turn`, the
/// closed form 2π∫ r g(r) dr of the ISO 68-1 groove section (a screw motion keeps volume, so one
/// lead of the sweep has the volume of the section revolved), from the standard's profile alone.
/// Just under the surface the 5 mm at the top and the 10 mm at the bottom stay whole, short of the
/// groove's widest half width w (`ext_half_width`, 0.70 mm for M10) at each stop.
#[test]
fn a_modelled_offset_thread_leaves_its_start_plain() {
    let (p, w) = (1.5, ext_half_width(10.0, 1.5));
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Rod"}));
    let before = exact_volume(&s, "Rod");
    run(&mut s, "solid.thread", json!({"face": [5, 0, 15], "designation": "M10", "offset": 5, "length": 15, "from": [0, 0, 30], "modeled": true}));
    let want = before - 15.0 / p * ext_turn(10.0, p, 5.0);
    let got = exact_volume(&s, "Rod");
    assert!((got - want).abs() < want * 2e-4, "{got} vs {want}");
    let st = s.model.state();
    let body = &st.body("Rod").unwrap().body;
    assert!(body.validity().is_empty());
    let ring = |z: f64| (0..36).map(|i| (i as f64 * 10.0).to_radians()).map(move |a| Vec3::new(4.9 * a.cos(), 4.9 * a.sin(), z));
    let rings = |z0: f64, z1: f64| (0..=40).map(move |i| z0 + (z1 - z0) * i as f64 / 40.0).flat_map(ring);
    let inside = |q: Vec3| body.classify_point(q).unwrap() == solidcraft_kernel::PointClass::Inside;
    assert!(rings(25.0 + w + 0.1, 29.9).all(inside), "the offset at the top is cut");
    assert!(rings(0.1, 10.0 - w - 0.1).all(inside), "the bottom past the thread is cut");
    assert!(rings(11.0, 24.0).filter(|q| !inside(*q)).count() > 100, "the band 10..25 is not threaded");
}

/// The runout stays at the inner end, away from the offset start: the plain top stays whole and
/// the fade goes on below z = 10, where the square stop would have ended.
#[test]
fn an_offset_threads_runout_stays_at_its_inner_end() {
    let w = ext_half_width(10.0, 1.5);
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Rod"}));
    let p = json!({"face": [5, 0, 15], "designation": "M10", "offset": 5, "length": 15, "from": [0, 0, 30], "modeled": true, "ease_out": "standard"});
    run(&mut s, "solid.thread", p);
    let st = s.model.state();
    let body = &st.body("Rod").unwrap().body;
    assert!(body.validity().is_empty());
    let ring = |z: f64| (0..36).map(|i| (i as f64 * 10.0).to_radians()).map(move |a| Vec3::new(4.9 * a.cos(), 4.9 * a.sin(), z));
    let rings = |z0: f64, z1: f64| (0..=20).map(move |i| z0 + (z1 - z0) * i as f64 / 20.0).flat_map(ring);
    let inside = |q: Vec3| body.classify_point(q).unwrap() == solidcraft_kernel::PointClass::Inside;
    assert!(rings(25.0 + w + 0.1, 29.9).all(inside), "the offset at the top is cut");
    assert!(rings(8.0, 10.0 - w - 0.1).filter(|q| !inside(*q)).count() > 10, "no runout below the threaded length");
}

/// A blind tapped hole (8.5 mm, M10's tap drill, 12 deep from the top of a 20 mm block), threaded
/// Full with an offset of 3 from its mouth, its only free end. The groove's start is a square stop
/// 3 mm down; at the bottom it stops with its section clear of the bottom, its centre the widest
/// half width w inside it, as in `a_blind_tapped_hole_stops_clear_of_its_bottom`: the section is
/// ISO 68-1's internal groove, g(r) = P/8 + 2 (d/2 - r) tan 30° (the flat P/8 at the major
/// diameter), reaching 0.1 P inside the minor radius d/2 - 5 P √3 / 16 so it crosses the wall,
/// so w = (P/8 + 2 (d/2 - (minor - 0.1 P)) tan 30°) / 2. That leaves 12 - 3 - w of square-ended
/// sweep: (12 - 3 - w) / P turns of 2π∫ r g(r) dr over the wall, r 4.25..5.
#[test]
fn an_offset_tapped_thread_starts_below_the_holes_mouth() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-10, -10, 0], "length": 20, "width": 20, "height": 20, "body_name": "Block"}));
    run(&mut s, "solid.hole", json!({"position": [0, 0, 20], "diameter": 8.5, "depth": 12}));
    let before = exact_volume(&s, "Block");
    let th = run(&mut s, "solid.thread", json!({"face": [4.25, 0, 14], "designation": "M10", "offset": 3, "modeled": true}));
    assert_eq!(th.get("warning"), None, "{th}");
    let (d, p, rh) = (10.0, 1.5, 4.25);
    let t = (PI / 6.0).tan();
    let minor = d / 2.0 - 5.0 * p * 3f64.sqrt() / 16.0;
    let w = (p / 8.0 + 2.0 * (d / 2.0 - (minor - 0.1 * p)) * t) / 2.0;
    let f = |r: f64| (p / 8.0 + d * t) * r * r / 2.0 - 2.0 * t * r * r * r / 3.0;
    let turn = 2.0 * PI * (f(d / 2.0) - f(rh));
    let want = before - (12.0 - 3.0 - w) / p * turn;
    let got = exact_volume(&s, "Block");
    assert!((got - want).abs() < before * 1e-4, "{got} vs {want}");
    // Just outside the bore, the 3 mm below the mouth (less the stop's half width) is whole.
    let st = s.model.state();
    let body = &st.body("Block").unwrap().body;
    let ring = |z: f64| (0..36).map(|i| (i as f64 * 10.0).to_radians()).map(move |a| Vec3::new(4.4 * a.cos(), 4.4 * a.sin(), z));
    assert!(
        (0..=10)
            .map(|i| 19.9 - (2.9 - w - 0.1) * i as f64 / 10.0)
            .flat_map(ring)
            .all(|q| body.classify_point(q).unwrap() == solidcraft_kernel::PointClass::Inside)
    );
}

/// Each face of one Thread takes the offset from its own start end; a face shorter than the offset
/// refuses the feature, naming its length.
#[test]
fn an_offset_applies_to_each_face_from_its_own_end() {
    let shafts = |second_height: f64| {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "A"}));
        run(&mut s, "solid.cylinder", json!({"base": [30, 0, 0], "radius": 5, "height": second_height, "body_name": "B"}));
        s
    };
    let params = |h: f64| {
        json!({"faces": [{"body": "A", "point": [5, 0, 15]}, {"body": "B", "point": [35, 0, h / 2.0]}],
            "designation": "M10", "offset": 5, "length": 10, "from": [0, 0, 30]})
    };
    let mut s = shafts(20.0);
    let made = run(&mut s, "solid.thread", params(20.0));
    let spans: Vec<(f64, f64)> = made["threads"].as_array().unwrap().iter().map(|t| thread_z(&json!({"thread": t}))).collect();
    assert_eq!(spans.len(), 2, "{made}");
    // A runs 25 down to 15; B (z 0..20), started from its top as `from` is nearer it, 15 down to 5.
    for (got, want) in spans.iter().zip([(15.0, 25.0), (5.0, 15.0)]) {
        assert!((got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9, "{got:?} vs {want:?}: {made}");
    }
    let mut s = shafts(4.0);
    let before = s.doc.to_json();
    let e = s.execute("solid.thread", &params(4.0)).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("leaves nothing to thread") && e.contains("(4 mm)"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

/// The refusals, each with its reason, leaving the design as it was: an offset below 0 (typed, or
/// an expression a parameter change makes negative, with one message), one as long as the face
/// or longer, one with a chamfered tip at its start, an `up_to` limit inside the offset, and an
/// offset on a taper pipe thread.
#[test]
fn an_offset_is_refused_with_its_reason() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Rod"}));
    let base = json!({"face": [5, 0, 15], "designation": "M10", "from": [0, 0, 30]});
    let with = |extra: Value| {
        let mut p = base.clone();
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    for (extra, reason) in [
        (json!({"offset": -1}), solidcraft_doc::THREAD_OFFSET_NEGATIVE),
        (json!({"offset": 30}), "leaves nothing to thread"),
        (json!({"offset": 31}), "leaves nothing to thread"),
        (json!({"offset": 2, "modeled": true, "tip": "start"}), solidcraft_doc::THREAD_OFFSET_WITH_TIP),
        (json!({"offset": 5, "up_to": {"origin": [0, 0, 27], "normal": [0, 0, 1]}}), "past its offset"),
    ] {
        let before = s.doc.to_json();
        let e = s.execute("solid.thread", &with(extra.clone())).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(reason), "{extra}: {e}");
        assert_eq!(s.doc.to_json(), before, "{extra}");
    }
    // An expression: the same reason when a parameter change makes it negative.
    run(&mut s, "parameters.add", json!({"name": "lead_in", "expression": "2 mm"}));
    run(&mut s, "solid.thread", with(json!({"offset": "lead_in", "name": "Lead-in"})));
    let changed = s.execute("parameters.change", &json!({"name": "lead_in", "expression": "-1 mm"}));
    let errors: Vec<String> = s.model.results.iter().filter_map(|r| r.error.clone()).chain(changed.err().map(|e| e.to_string())).collect();
    assert!(errors.iter().any(|e| e.contains(solidcraft_doc::THREAD_OFFSET_NEGATIVE)), "{errors:?}");
    // A taper pipe thread's standard diameter is set at the pipe's end: no offset (cosmetic too).
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 20.955 / 2.0, "height": 30, "body_name": "Pipe"}));
    let e = s.execute("solid.thread", &json!({"face": [20.955 / 2.0, 0, 15], "designation": "R 1/2", "offset": 2, "from": [0, 0, 30]}));
    let e = e.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("takes no offset"), "{e}");
}

/// An offset of 0 is no offset: the modelled body is the same to the byte, and the cosmetic thread
/// the same record. A thread saved without one has no `offset` key, as before this change.
#[test]
fn a_zero_offset_builds_exactly_what_no_offset_does() {
    let build = |modeled: bool, offset: Option<&str>| {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 30, "body_name": "Rod"}));
        let mut p = json!({"face": [5, 0, 15], "designation": "M10", "length": 15, "from": [0, 0, 30], "modeled": modeled});
        if let Some(o) = offset {
            p["offset"] = json!(o);
        }
        run(&mut s, "solid.thread", p);
        s
    };
    let bytes = |s: &Session| solidcraft_kernel::encode_native_body(&s.model.state().body("Rod").unwrap().body).unwrap();
    let (plain, zero) = (build(true, None), build(true, Some("0 mm")));
    assert!(bytes(&plain) == bytes(&zero), "a zero offset changed the modelled body");
    let (mut plain, mut zero) = (build(false, None), build(false, Some("0 mm")));
    assert_eq!(run(&mut plain, "model.threads", json!({})), run(&mut zero, "model.threads", json!({})));
    assert!(zero.doc.to_json().contains("\"offset\""), "the offset given is saved");
    assert!(!plain.doc.to_json().contains("\"offset\""), "a thread without an offset saves none");
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
        ("unified", "1/4-20 UNC-2A", 6.35, 1.27, 1.27 / 8.0, (30.0, 30.0), 3.175 - 5.0 * 1.27 * 3f64.sqrt() / 16.0),
        ("unified", "#10-32 UNF", 4.826, inch / 32.0, inch / 256.0, (30.0, 30.0), 2.413 - 5.0 * (inch / 32.0) * 3f64.sqrt() / 16.0),
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
        // The 90° custom profile: H = P / (2 tan 45°) = P/2, depth 5H/8 = 0.3125 P, flat P/8 at
        // the crest, so g(r) = P/4 + 2 (r - r3) from the root r3 = 5 - 0.46875 = 4.53125. One turn
        // is 2π∫ r g(r) dr from r3 to 5 = 2π [a r²/2 + 2 r³/3] with a = P/4 - 2 r3 = -8.6875:
        // 2π (1.9020081 mm³) = 11.950669 mm³; six turns 71.704 mm³.
        ("custom", "C10x1.5x90", 10.0, 1.5, 1.5 / 8.0, (45.0, 45.0), 5.0 - 0.3125 * 1.5),
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
                body.classify_point(probe(0.0)).unwrap() == solidcraft_kernel::PointClass::Outside
                    && (1..12).all(|k| body.classify_point(probe(k as f64 * 0.5)).unwrap() == solidcraft_kernel::PointClass::Outside)
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

/// A tapped hole saves its thread's tolerance class (#16336) as given, only with a thread that takes
/// it; an edit that doesn't give one keeps it while the thread still takes it, and drops it when the
/// thread changes to one that doesn't. A hole without a class saves no key, and a hand-edited class
/// the thread doesn't take fails the hole with the reason. The class changes no geometry.
#[test]
fn a_tapped_holes_class_is_saved_and_kept_through_an_edit() {
    let class_of = |s: &Session, id: u64| match &s.doc.feature(id).unwrap().kind {
        solidcraft_doc::FeatureKind::Hole { thread_class, .. } => thread_class.clone(),
        _ => panic!("expected a hole"),
    };
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 60, "height": 12, "body_name": "Plate"}));
    let plain = json!({"position": [20, 30, 12], "thread": "M8", "depth": 10});
    let hole = run(&mut s, "solid.hole", plain.clone())["feature"].as_u64().unwrap();
    assert!(!s.doc.to_json().contains("thread_class"), "no class, no key: older designs read the same");
    let untouched = volume(&mut s, "Plate");
    let classed = json!({"position": [20, 30, 12], "thread": "M8", "depth": 10, "thread_class": "6H"});
    run(&mut s, "timeline.redefine", json!({"feature": hole, "command": "solid.hole", "params": classed}));
    assert_eq!(class_of(&s, hole).as_deref(), Some("6H"));
    assert!(s.doc.to_json().contains(r#""thread_class": "6H""#), "saved as given (the design file is pretty-printed)");
    assert!((volume(&mut s, "Plate") - untouched).abs() < 1e-6, "the class changes no geometry");
    // The Hole window sends no class: an edit without one keeps it while the thread takes it.
    let deeper = json!({"position": [20, 30, 12], "thread": "M8", "depth": 11});
    run(&mut s, "timeline.redefine", json!({"feature": hole, "command": "solid.hole", "params": deeper}));
    assert_eq!(class_of(&s, hole).as_deref(), Some("6H"), "kept through an edit");
    let unified = json!({"position": [20, 30, 12], "thread": "1/4-20 UNC", "depth": 11});
    run(&mut s, "timeline.redefine", json!({"feature": hole, "command": "solid.hole", "params": unified}));
    assert_eq!(class_of(&s, hole), None, "a 1/4-20 UNC hole doesn't take 6H: dropped with the size");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()));
    for (params, why) in [
        (json!({"position": [40, 30, 12], "depth": 10, "diameter": 6, "thread_class": "6H"}), "`thread_class` needs `thread` (a tapped hole)"),
        (json!({"position": [40, 30, 12], "depth": 10, "thread": "M8", "thread_class": "6g"}), "an ISO metric tapped hole's class is"),
        // The class goes in `thread_class`, not after the hole's thread size (as before).
        (json!({"position": [40, 30, 12], "depth": 10, "thread": "M8-6H"}), "unknown tapped thread `M8-6H`"),
        (json!({"position": [40, 30, 12], "depth": 10, "thread": "1/4-20 UNC", "thread_class": "2A"}), "class is 1B, 2B or 3B"),
        (json!({"position": [40, 30, 12], "depth": 10, "thread": "G 1/4", "thread_class": "2B"}), "has no tolerance class"),
    ] {
        let err = s.execute("solid.hole", &params).unwrap_err().to_string();
        assert!(err.contains(why), "{params}: {err}");
    }
    // A hand-edited class the thread doesn't take fails the hole, saying why.
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let saved = v["features"].as_array_mut().unwrap().iter_mut().find(|f| f["id"] == json!(hole)).unwrap();
    saved["thread_class"] = json!("6H");
    let edited = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    let failed: Vec<&String> = edited.model.results.iter().filter_map(|r| r.error.as_ref()).collect();
    assert!(failed.iter().any(|e| e.contains("a Unified tapped hole's class is 1B, 2B or 3B")), "{failed:?}");
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

/// A custom thread keeps its designation in the callout and stores its form; it refuses a
/// chamfered tip and an angle past 90°. Its groove's volume: `every_form_cuts_its_standard_profile`.
#[test]
fn a_custom_thread_keeps_its_designation_and_refuses_a_tip() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 20, "body_name": "Shaft"}));
    let th = run(&mut s, "solid.thread", json!({"face": [5, 0, 10], "designation": "c10x1.5x90"}));
    assert_eq!(th["thread"]["designation"], "C10x1.5x90", "{th}");
    assert_eq!(th["thread"]["form"], "custom", "{th}");
    let tip = s.execute("solid.thread", &json!({"face": [5, 0, 10], "designation": "C10x1.5x90", "modeled": true, "tip": "both"}));
    assert!(tip.err().map(|e| e.to_string()).unwrap_or_default().contains("not offered for a custom thread"));
    let wide = s.execute("solid.thread", &json!({"face": [5, 0, 10], "form": "custom", "designation": "C10x1.5x95"}));
    assert!(wide.err().map(|e| e.to_string()).unwrap_or_default().contains("must be 20° to 90°"));
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
/// minor diameter d3 (ISO 724); the volume as the stock's chamfered section
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
    let rf = 4.5;
    let slope = 30f64.to_radians().tan();
    let want = PI * 64.0 * 10.0 + m10_stock(10.0, 35.0, |z| 5f64.min(rf + (35.0 - z) * slope));
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
    // Trimmed at the bottom collar's plane, stopped clear of the top one for its runout.
    let turns = (35.0 - 10.0 - w) / p;
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

#[test]
fn a_saved_segmented_thread_flank_pick_does_not_select_a_different_face() {
    use solidcore_record::brep::Surface;

    // Captured from the segmented build before continuous_flanks existed, including the old
    // groove-before-chamfer construction. Picks near both chamfered ends exercise its order.
    let cases = [
        ("none", vec![("F2:face:12", Vec3::new(4.7828951862188696, -0.0057530612776590315, 1.6606274291037717))]),
        (
            "both",
            vec![
                ("F2:face:41", Vec3::new(0.09242260275006192, 4.273234209721337, 0.23586874398281996)),
                ("F2:face:7", Vec3::new(0.9625365002593353, -4.6849682261915895, 6.262443362555148)),
                ("F2:face:51", Vec3::new(-0.0924226027501412, 4.273234209721336, 11.764131256017183)),
            ],
        ),
    ];
    for (tip, picks) in cases {
        let mut thread = json!({
            "designation": "M10x1.5", "face": [5, 0, 6], "face_names": ["F1:side"],
            "id": 2, "modeled": true, "name": "Thread1", "suppressed": false, "type": "thread"
        });
        if tip != "none" {
            thread["tip"] = json!(tip);
        }
        let mut saved: Value = serde_json::from_str(&solidcraft_doc::Document::default().to_json()).unwrap();
        saved["features"] = json!([
            {
                "axis": [0, 0, 1], "base": [0, 0, 0], "body_names": ["Shaft"], "height": "12",
                "id": 1, "name": "Cylinder1", "operation": "new_body", "param_names": ["d1", "d2"],
                "radius": "5", "suppressed": false, "type": "cylinder"
            }, thread
        ]);
        saved["next_id"] = json!(3);
        saved["params"] = json!([]);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved.to_string()).unwrap());
        let check_picks = |session: &Session| {
            let rebuilt = session.model.state();
            let shaft = rebuilt.body("Shaft").unwrap();
            for (name, pick) in &picks {
                let (face, point) = solidcraft_doc::naming::face_on_body(shaft, name, *pick).unwrap();
                assert!(point.dist(*pick) < 0.01, "{tip}: {name} must stay on its original flank: {point:?}");
                assert!(
                    matches!(
                        shaft.body.record().faces[face].surface,
                        Surface::Spline { .. } | Surface::RationalSpline { .. } | Surface::Sweep { .. }
                    ),
                    "{tip}: {name} must not select a cylinder"
                );
            }
        };
        check_picks(&reopened);
        run(
            &mut reopened,
            "timeline.redefine",
            json!({
                "feature": 2, "command": "solid.thread",
                "params": {"face": [5, 0, 6], "designation": "M10x1.5", "modeled": true, "tip": tip}
            }),
        );
        assert!(matches!(reopened.doc.feature(2).unwrap().kind, solidcraft_doc::FeatureKind::Thread { continuous_flanks: false, .. }));
        check_picks(&reopened);
        let saved_again = solidcraft_doc::Document::from_json(&reopened.doc.to_json()).unwrap();
        assert!(matches!(saved_again.feature(2).unwrap().kind, solidcraft_doc::FeatureKind::Thread { continuous_flanks: false, .. }));
    }
}

/// A modelled M10 thread keeps each flank as one B-spline face across all turns. Cylinder seams
/// must not split either flank into separate faces for selection, meshing or STEP export.
#[test]
fn a_modelled_thread_keeps_each_flank_as_one_face() {
    use solidcore_record::brep::Surface;
    // Include a partial final turn and a chamfered stock: neither may fragment the flanks.
    for (height, tip) in [(12.0, "none"), (12.345, "none"), (12.345, "both")] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 5.0, "height": height, "body_name": "Shaft"}));
        run(&mut s, "solid.thread", json!({"face": [5.0, 0, height / 2.0], "designation": "M10x1.5", "modeled": true, "tip": tip}));
        let st = s.model.state();
        let shaft = st.body("Shaft").unwrap();
        let body = &shaft.body;
        let mesh = body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)).unwrap();
        let record = body.record();
        let flanks: Vec<usize> = record
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                matches!(f.surface, Surface::Sweep { .. })
                    || (f.nurbs.is_none() && matches!(f.surface, Surface::Spline { .. } | Surface::RationalSpline { .. }))
            })
            .map(|(i, _)| i)
            .collect();
        assert_eq!(flanks.len(), 2, "one selectable face per flank, independent of the turn count");
        let names = solidcraft_doc::naming::face_names(shaft);
        let picks: Vec<_> = flanks
            .iter()
            .map(|&flank| {
                assert!(mesh.tri_face.iter().any(|f| *f as usize == flank), "flank {flank} must tessellate");
                (names[flank].clone(), solidcraft_doc::naming::face_spot(shaft, flank).unwrap().point)
            })
            .collect();
        // The named picks downstream face commands store must still find the same kind of face
        // after the history is serialized and rebuilt, including the reordered chamfer cuts.
        let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        let rebuilt = reopened.model.state();
        let shaft = rebuilt.body("Shaft").unwrap();
        for (name, point) in picks {
            let (face, found) = solidcraft_doc::naming::face_on_body(shaft, &name, point).unwrap();
            assert!(found.dist(point) < 1e-6, "the saved flank pick must remain on its named face: {name}");
            assert!(matches!(
                shaft.body.record().faces[face].surface,
                Surface::Spline { .. } | Surface::RationalSpline { .. } | Surface::Sweep { .. }
            ));
        }
    }
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

/// A tapped hole given no `diameter` drills the tap maker's drill for its size (M8: 6.8 mm, 1/4-20
/// UNC: 5.1054 mm); a given diameter is kept.
#[test]
fn a_tapped_hole_given_no_diameter_drills_the_tap_makers_drill() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":40,"width":20,"height":10,"body_name":"Plate"}));
    let mut drilled = |p: Value| {
        let id = run(&mut s, "solid.hole", p)["feature"].as_u64().unwrap();
        match &s.doc.feature(id).unwrap().kind {
            solidcraft_doc::FeatureKind::Hole { diameter, .. } => diameter.to_string(),
            k => panic!("{k:?}"),
        }
    };
    assert_eq!(drilled(json!({"position":[10,10,10],"thread":"M8"})), "6.8 mm");
    assert_eq!(drilled(json!({"position":[25,10,10],"thread":"1/4-20 UNC"})), "5.1054 mm");
    assert_eq!(drilled(json!({"position":[33,10,10],"thread":"M8","diameter":"6.9 mm"})), "6.9 mm");
}

/// A block with a bore of `dia` cut through it by a sketch circle.
fn block_with_bore(dia: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 20, "body_name": "Blk"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "H"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "diameter": dia}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "H", "through_all": true, "direction": "positive", "operation": "cut"}));
    s
}

/// #11609: a 12 mm hole took M14 (the size whose tap drill is 12 mm) and a modelled M12 in it was
/// refused. Automatic sizing now takes the size with the hole's nominal diameter first; a hole
/// near no nominal diameter is still matched by tap drill.
#[test]
fn a_hole_takes_the_size_of_its_nominal_diameter_first() {
    for (dia, want) in [(12.0, "M12"), (12.1, "M12"), (10.0, "M10"), (8.0, "M8"), (14.0, "M14"), (10.2, "M12"), (13.0, "M14"), (11.0, "M12")] {
        let mut s = block_with_bore(dia);
        let th = run(&mut s, "solid.thread", json!({"face": [dia / 2.0, 0, 10]}));
        assert_eq!(th["thread"]["designation"], json!(want), "a {dia} mm hole");
    }
    // A shaft is still its major diameter.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 6, "height": 30, "body_name": "Sh"}));
    let th = run(&mut s, "solid.thread", json!({"face": [6, 0, 15]}));
    assert_eq!(th["thread"]["designation"], json!("M12"));
}

/// #11609: M12 modelled in a 12 mm hole narrows the bore to the thread's minor diameter and cuts
/// the thread there, and says so; the cosmetic M12 is the same size on the same bore.
#[test]
fn a_modelled_thread_in_a_hole_as_wide_as_it_narrows_the_bore() {
    for (dia, d, p) in [(12.0, 12.0, 1.75), (14.0, 14.0, 2.0), (12.2, 12.0, 1.75), (13.0, 12.0, 1.75)] {
        let mut s = block_with_bore(dia);
        let before = volume(&mut s, "Blk");
        let made = run(&mut s, "solid.thread", json!({"face": [dia / 2.0, 0, 10], "designation": format!("M{d}"), "modeled": true}));
        let warning = s.model.result(made["feature"].as_u64().unwrap()).and_then(|r| r.warning.clone()).unwrap_or_default();
        assert!(warning.contains("narrowed the") && warning.contains(&format!("M{d}")), "{warning}");
        // The bore is now the minor diameter's, so there is material where there was air, but
        // less than a full sleeve (the groove cut into it).
        let h = p * 3f64.sqrt() / 2.0;
        let minor = d - 2.0 * 5.0 * h / 8.0;
        let sleeve = PI * ((dia / 2.0).powi(2) - (minor / 2.0).powi(2)) * 20.0;
        let added = volume(&mut s, "Blk") - before;
        assert!(added > 0.2 * sleeve && added < sleeve, "M{d} in {dia}: added {added}, a full sleeve is {sleeve}");
    }
    // Automatic and modelled, and cosmetic, on the 12 mm bore.
    let mut s = block_with_bore(12.0);
    let th = run(&mut s, "solid.thread", json!({"face": [6, 0, 10], "designation": "M12"}));
    assert_eq!(th["thread"]["designation"], json!("M12"));
    let mut s = block_with_bore(12.0);
    let made = run(&mut s, "solid.thread", json!({"face": [6, 0, 10], "modeled": true}));
    assert!(s.model.result(made["feature"].as_u64().unwrap()).and_then(|r| r.warning.clone()).unwrap_or_default().contains("M12"));
}

/// A hole that already has the wall for the groove is not narrowed, and a size nowhere near the
/// hole is refused with the size it does take.
#[test]
fn a_hole_with_wall_for_the_groove_is_left_as_drilled() {
    let mut s = block_with_bore(10.2);
    let made = run(&mut s, "solid.thread", json!({"face": [5.1, 0, 10], "designation": "M12", "modeled": true}));
    assert!(s.model.result(made["feature"].as_u64().unwrap()).and_then(|r| r.warning.clone()).is_none());
    let mut s = block_with_bore(20.0);
    let e = s.execute("solid.thread", &json!({"face": [10, 0, 10], "designation": "M12", "modeled": true})).unwrap_err().to_string();
    assert!(e.contains("does not fit a 20.00 mm hole") && e.contains("takes M20"), "{e}");
}

/// #14191: the fill goes all the way round the hole, so a hole whose wall is open along the
/// thread is not filled in: one that breaks out of the side of the part, one a cross hole meets,
/// one drilled into a slanted face. Each is refused with the drill to use, and the part is left as
/// it was. A blind hole and a chamfered one are still filled, and so are a counterbored hole and a
/// cross-drilled one where the thread stops short of the counterbore and the cross hole.
#[test]
fn a_hole_open_along_the_thread_is_not_filled_in() {
    let block = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [-20, -20, 0], "length": 40, "width": 40, "height": 20, "body_name": "Blk"}));
        s
    };
    let cut = |s: &mut Session, dia: f64, height: f64, base: [f64; 3], axis: &str| {
        run(s, "solid.cylinder", json!({"diameter": dia, "height": height, "base": base, "axis": axis, "operation": "cut"}));
    };
    let m12 = |face: [f64; 3]| json!({"face": face, "designation": "M12", "modeled": true});

    let mut side = block();
    cut(&mut side, 12.0, 20.0, [0.0, 16.0, 0.0], "Z");
    let mut cross = block_with_bore(12.0);
    cut(&mut cross, 4.0, 20.0, [-20.0, 0.0, 10.0], "X");
    let mut slanted = Session::default();
    run(&mut slanted, "sketch.create", json!({"plane": "XZ", "name": "P"}));
    // On XZ the sketch's second coordinate runs down: the top falls from z = 25 to z = 15.
    run(&mut slanted, "sketch.line", json!({"points": [[-20, 0], [20, 0], [20, -15], [-20, -25]], "closed": true}));
    run(&mut slanted, "sketch.finish", json!({}));
    run(&mut slanted, "solid.extrude", json!({"sketch": "P", "distance": 40, "direction": "symmetric", "body_name": "Blk"}));
    cut(&mut slanted, 12.0, 30.0, [0.0, 0.0, 0.0], "Z");
    for (what, mut s, face) in [("side", side, [-6.0, 16.0, 10.0]), ("cross hole", cross, [0.0, 6.0, 15.0]), ("slanted", slanted, [0.0, 6.0, 8.0])] {
        let before = volume(&mut s, "Blk");
        let e = s.execute("solid.thread", &m12(face)).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains("this 12 mm hole is too wide for a modelled M12 thread and can't be filled in"), "{what}: {e}");
        assert!(e.contains("drill 10.3 mm for M12"), "{what}: {e}");
        assert!((volume(&mut s, "Blk") - before).abs() < 1e-9, "{what}: the part changed");
    }

    let mut blind = block();
    cut(&mut blind, 12.0, 10.0, [0.0, 0.0, 10.0], "Z");
    let mut counterbored = block_with_bore(12.0);
    cut(&mut counterbored, 20.0, 5.0, [0.0, 0.0, 15.0], "Z");
    let mut chamfered = block_with_bore(12.0);
    run(&mut chamfered, "solid.chamfer", json!({"edges": [[6, 0, 20]], "distance": 1}));
    let mut cross_drilled = block_with_bore(12.0);
    cut(&mut cross_drilled, 4.0, 20.0, [-20.0, 0.0, 4.0], "X");
    // 10 mm of thread from the end at `from`.
    let part_way = |face: [f64; 3], from: [f64; 3]| {
        let mut p = m12(face);
        p["from"] = json!(from);
        p["length"] = json!(10);
        p
    };
    for (what, mut s, p) in [
        ("blind", blind, m12([6.0, 0.0, 15.0])),
        ("chamfered", chamfered, m12([6.0, 0.0, 10.0])),
        ("counterbored", counterbored, part_way([6.0, 0.0, 5.0], [6.0, 0.0, 0.0])),
        ("cross-drilled", cross_drilled, part_way([6.0, 0.0, 15.0], [6.0, 0.0, 20.0])),
    ] {
        let before = volume(&mut s, "Blk");
        let made = s.execute("solid.thread", &p).unwrap_or_else(|e| panic!("{what}: {e}"));
        let warning = made["feature"].as_u64().and_then(|f| s.model.result(f)).and_then(|r| r.warning.clone()).unwrap_or_default();
        assert!(warning.contains("narrowed the 12 mm hole"), "{what}: {warning}");
        assert!(volume(&mut s, "Blk") > before + 1.0, "{what}: the hole was not filled in");
    }
}

/// #11609: a design saved with an automatic thread keeps the size it had (a 12 mm hole stays
/// M14), while the same thread made now takes M12.
#[test]
fn a_saved_automatic_thread_keeps_its_old_size() {
    let mut s = block_with_bore(12.0);
    run(&mut s, "solid.thread", json!({"face": [6, 0, 10]}));
    let designations = |s: &Session| -> Vec<String> { s.model.state().threads.iter().map(|t| t.designation.clone()).collect() };
    assert_eq!(designations(&s), ["M12"]);
    // As the release before saved it: the last folded format, none of today's pending changes.
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    v["format"] = json!("solidcraft/49");
    v.as_object_mut().unwrap().remove("format_pending");
    let reopened = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    assert_eq!(designations(&reopened), ["M14"]);
    let saved: Value = serde_json::from_str(&reopened.doc.to_json()).unwrap();
    let thread = saved["features"].as_array().unwrap().iter().find(|f| f["type"] == json!("thread")).unwrap();
    assert_eq!(thread["old_auto_size"], json!(true));
}

/// Editing a saved automatic thread (which keeps its old size) does not change its size.
#[test]
fn editing_a_saved_automatic_thread_keeps_its_old_size() {
    let mut s = block_with_bore(12.0);
    let made = run(&mut s, "solid.thread", json!({"face": [6, 0, 10]}));
    let mut v: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    v["format"] = json!("solidcraft/49");
    v.as_object_mut().unwrap().remove("format_pending");
    let mut s = Session::new(solidcraft_doc::Document::from_json(&v.to_string()).unwrap());
    let designations = |s: &Session| -> Vec<String> { s.model.state().threads.iter().map(|t| t.designation.clone()).collect() };
    assert_eq!(designations(&s), ["M14"]);
    run(&mut s, "timeline.redefine", json!({"feature": made["feature"], "command": "solid.thread", "params": {"face": [6, 0, 10], "hand": "left"}}));
    assert_eq!(designations(&s), ["M14"]);
}

/// #11367: a modelled thread on one hole must not stop the next hole's wall from being found
/// as a cylinder to thread. The cut the modelled thread makes must not take the body's other
/// cylinders with it: the second hole still takes a thread, modelled or cosmetic.
#[test]
fn a_modelled_thread_leaves_the_next_hole_a_cylinder_to_thread() {
    for gap in [40.0, 20.0, 12.0, 8.0] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 80, "width": 80, "height": 20, "body_name": "Blk"}));
        run(&mut s, "solid.hole", json!({"position": [20, 40, 20], "diameter": 5, "direction": [0, 0, -1]}));
        run(&mut s, "solid.hole", json!({"position": [20.0 + gap, 40.0, 20.0], "diameter": 5, "direction": [0, 0, -1]}));
        // The first hole takes a modelled M6 thread.
        let t1 = s.execute("solid.thread", &json!({"face": [22.5, 40.0, 10.0], "designation": "M6", "modeled": true}));
        assert!(t1.is_ok(), "gap {gap}: the first hole takes its thread: {t1:?}");
        // The second hole's wall is still a cylinder: it takes a cosmetic thread...
        let t2 = s.execute("solid.thread", &json!({"face": [22.5 + gap, 40.0, 10.0], "designation": "M6"}));
        assert!(t2.is_ok(), "gap {gap}: the second hole still has a cylinder to thread: {t2:?}");
        // ...and a modelled one.
        let t3 = s.execute("solid.thread", &json!({"face": [22.5 + gap, 40.0, 10.0], "designation": "M6", "modeled": true}));
        assert!(t3.is_ok(), "gap {gap}: the second hole takes a modelled thread: {t3:?}");
    }
}

#[test]
fn grouped_thread_faces_keep_automatic_sizes_in_one_feature_through_reopen_and_undo() {
    for level in [crate::schema::ParamCheck::Off, crate::schema::ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"Small"}));
        run(&mut s, "solid.cylinder", json!({"base":[30,0,0],"radius":10,"height":8,"body_name":"Large"}));
        let before = s.doc.to_json();
        let feature_count = s.doc.features.len();
        let made = run(&mut s, "solid.thread", json!({"faces":[{"body":"Small","point":[4,0,2]},{"body":"Large","point":[40,0,2]}]}));
        let feature = made["feature"].as_u64().unwrap();
        assert_eq!(s.doc.features.len(), feature_count + 1);
        assert_eq!(made["threads"].as_array().unwrap().len(), 2);
        assert_eq!(made["thread"], made["threads"][0]);
        let check = |session: &Session| {
            let st = session.model.state();
            assert_eq!(st.threads.len(), 2);
            assert_eq!(st.threads.iter().map(|t| (t.body.as_str(), t.designation.as_str())).collect::<Vec<_>>(), [("Small", "M8"), ("Large", "M20")]);
            assert!(st.threads.iter().all(|t| t.feature == feature));
            let solidcraft_doc::FeatureKind::Thread { face, additional_faces, target_bodies, .. } = &session.doc.feature(feature).unwrap().kind
            else {
                panic!("expected one grouped Thread feature");
            };
            assert_eq!(face.0, Vec3::new(4.0, 0.0, 2.0));
            assert_eq!(additional_faces, &[Vec3::new(40.0, 0.0, 2.0).into()]);
            assert_eq!(target_bodies.iter().map(|n| n.as_ref()).collect::<Vec<&str>>(), ["Small", "Large"]);
        };
        check(&s);
        check(&Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap()));
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        assert!(s.model.state().threads.is_empty());
        run(&mut s, "edit.redo", json!({}));
        check(&s);
        run(&mut s, "timeline.redefine", json!({"feature":feature,"command":"solid.thread","params":{"faces":[{"body":"Large","point":[40,0,2]}]}}));
        assert_eq!(s.doc.features.len(), feature_count + 1);
        assert_eq!(s.model.state().threads.len(), 1);
        assert_eq!(s.model.state().threads[0].designation, "M20");
        run(&mut s, "edit.undo", json!({}));
        check(&s);
    }
}

#[test]
fn grouped_thread_faces_reject_invalid_plural_input_atomically() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"Shaft"}));
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let mut invalid = vec![
        json!({"face":[4,0,2],"faces":[[4,0,2]]}),
        json!({"faces":[]}),
        json!({"faces":null}),
        json!({"faces":true}),
        json!({"faces":"all"}),
        json!({"faces":[4,0,2]}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]},{"body":"Shaft","point":[0,0]}]}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]},{"body":"Shaft","point":[1e300,0,0]}]}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]},{"body":"Shaft","point":[null,0,0]}]}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]},{"body":"Shaft","point":[300,0,0]}],"designation":"M8"}),
        json!({"faces":[[4,0,2]]}),
        json!({"faces":[{"point":[4,0,2]}]}),
        json!({"faces":[{"body":"missing","point":[4,0,2]}]}),
        json!({"faces":[{"body":false,"point":[4,0,2]}]}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]}],"single_face_selection":"true"}),
        json!({"face":[4,0,2],"single_face_selection":true}),
        json!({"faces":[{"body":"Shaft","point":[4,0,2]},{"body":"Shaft","point":[4,0,2]}],"single_face_selection":true}),
    ];
    invalid.push(json!({"faces":vec![[4,0,2];1025]}));
    for params in invalid {
        let err = s.execute("solid.thread", &params).unwrap_err();
        assert!(!matches!(err, crate::EngineError::Internal(..)), "{params}: {err}");
        assert_eq!(s.doc.to_json(), before, "{params}: {err}");
        assert_eq!(s.undo.len(), undo);
        assert!(s.model.state().threads.is_empty());
    }
}

#[test]
fn a_single_thread_keeps_the_legacy_command_and_serialized_face() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8}));
    let made = run(&mut s, "solid.thread", json!({"face":{"point":[4,0,2]},"designation":"M8"}));
    assert_eq!(made["thread"]["designation"], "M8");
    assert!(made.get("threads").is_none());
    let value: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let thread = value["features"].as_array().unwrap().last().unwrap();
    assert_eq!(thread["face"], json!([4.0, 0.0, 2.0]));
    assert!(thread.get("additional_faces").is_none());
    assert!(thread.get("target_bodies").is_none());
    assert!(thread.get("single_face_selection").is_none());
    let reopened = Session::new(solidcraft_doc::Document::from_json(&value.to_string()).unwrap());
    assert_eq!(reopened.model.state().threads.len(), 1);
}

#[test]
fn grouped_thread_faces_map_all_picks_in_a_placed_component() {
    for active_part in [false, true] {
        let mut s = Session::default();
        let part = run(&mut s, "component.create", json!({"name":"Shafts","grounded":false}));
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":20,"body_name":"First"}));
        run(&mut s, "solid.cylinder", json!({"base":[20,0,0],"radius":4,"height":20,"body_name":"Second"}));
        run(&mut s, "component.activate", json!({"component":0}));
        run(
            &mut s,
            "occurrence.move",
            json!({"occurrence":part["occurrence"],"translate":[30,20,10],"axis":[1,0,0],"angle":"90 deg","capture":true}),
        );
        if active_part {
            run(&mut s, "component.activate", json!({"component":part["component"]}));
        }
        let made = run(
            &mut s,
            "solid.thread",
            json!({"faces":[{"body":"First","point":[34,10,10]},{"body":"Second","point":[54,10,10]}],"designation":"M8"}),
        );
        let feature = made["feature"].as_u64().unwrap();
        let solidcraft_doc::FeatureKind::Thread { face, additional_faces, .. } = &s.doc.feature(feature).unwrap().kind else {
            panic!("expected grouped Thread");
        };
        assert!(face.0.dist(Vec3::new(4.0, 0.0, 10.0)) < 1e-8);
        assert_eq!(additional_faces.len(), 1);
        assert!(additional_faces[0].0.dist(Vec3::new(24.0, 0.0, 10.0)) < 1e-8);
        let world = s.world_state().unwrap();
        assert_eq!(world.threads.len(), 2);
        for t in &world.threads {
            assert!(t.axis.dist(Vec3::new(0.0, -1.0, 0.0)) < 1e-8, "{:?}", t.axis);
        }
        let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        let after = reopened.world_state().unwrap();
        assert_eq!(after.threads.len(), 2);
        for (a, b) in world.threads.iter().zip(&after.threads) {
            assert!(a.axis_point.dist(b.axis_point) < 1e-8);
            assert!(a.axis.dist(*b.axis) < 1e-8);
        }
    }
}

#[test]
fn grouped_thread_faces_refuse_mixed_component_owners_before_mapping() {
    for placed in [false, true] {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name":"First part","grounded":false}));
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"First shaft"}));
        run(&mut s, "component.activate", json!({"component":0}));
        let second = run(&mut s, "component.create", json!({"name":"Second part","grounded":false}));
        let base = if placed { 0 } else { 30 };
        run(&mut s, "solid.cylinder", json!({"base":[base,0,0],"radius":4,"height":8,"body_name":"Second shaft"}));
        run(&mut s, "component.activate", json!({"component":0}));
        if placed {
            run(&mut s, "occurrence.move", json!({"occurrence":second["occurrence"],"translate":[30,0,0],"capture":true}));
        }
        assert_eq!(crate::frames::placed(&s), placed);
        let params = json!({"faces":[{"body":"First shaft","point":[4,0,2]},{"body":"Second shaft","point":[34,0,2]}],"designation":"M8"});
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let error = s.execute("solid.thread", &params).unwrap_err().to_string();
        assert!(error.contains("same component"), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        assert!(s.model.state().threads.is_empty());
        // The single-face API remains available; an attempted grouped edit is also atomic.
        let one = run(&mut s, "solid.thread", json!({"face":[4,0,2],"designation":"M8"}));
        let before = s.doc.to_json();
        let undo = s.undo.len();
        let error =
            s.execute("timeline.redefine", &json!({"feature":one["feature"],"command":"solid.thread","params":params})).unwrap_err().to_string();
        assert!(error.contains("same component"), "{error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
        assert_eq!(s.model.state().threads.len(), 1);
    }
}

/// A direct edit checks owners in the Thread's input, even after later features removed its bodies.
#[test]
fn grouped_thread_redefinition_checks_owners_before_later_body_removals() {
    for placed in [false, true] {
        for same_component in [false, true] {
            let mut s = Session::default();
            let first = run(&mut s, "component.create", json!({"name":"First part","grounded":false}));
            run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"First shaft"}));
            let second = if same_component {
                first.clone()
            } else {
                run(&mut s, "component.activate", json!({"component":0}));
                run(&mut s, "component.create", json!({"name":"Second part","grounded":false}))
            };
            run(&mut s, "solid.cylinder", json!({"base":[30,0,0],"radius":4,"height":8,"body_name":"Second shaft"}));
            run(&mut s, "component.activate", json!({"component":0}));
            if placed {
                run(&mut s, "occurrence.move", json!({"occurrence":first["occurrence"],"translate":[10,20,0],"capture":true}));
                if !same_component {
                    run(&mut s, "occurrence.move", json!({"occurrence":second["occurrence"],"translate":[10,20,0],"capture":true}));
                }
            }
            assert_eq!(crate::frames::placed(&s), placed);
            let faces = if placed { json!([[14, 20, 2], [44, 20, 2]]) } else { json!([[4, 0, 2], [34, 0, 2]]) };
            run(&mut s, "component.activate", json!({"component":first["component"]}));
            let made = run(&mut s, "solid.thread", json!({"face":faces[0],"designation":"M8"}));
            let id = made["feature"].as_u64().unwrap();
            run(&mut s, "component.activate", json!({"component":0}));
            let removed = run(&mut s, "solid.remove", json!({"bodies":["First shaft","Second shaft"]}))["feature"].as_u64().unwrap();
            assert!(s.model.state().bodies.is_empty(), "the final state cannot identify either picked owner");
            assert_eq!(s.model.state_before(id).bodies.len(), 2, "both bodies exist at the Thread input");
            let before = s.doc.to_json();
            let undo = s.undo.len();
            let features = s.doc.features.len();
            let reply = s.execute("timeline.redefine", &json!({"feature":id,"command":"solid.thread","params":{"faces":[{"body":"First shaft","point":faces[0]},{"body":"Second shaft","point":faces[1]}],"designation":"M8"}}));
            if same_component {
                reply.unwrap();
                assert_eq!(s.doc.features.len(), features);
                let solidcraft_doc::FeatureKind::Thread { additional_faces, .. } = &s.doc.feature(id).unwrap().kind else { panic!("Thread") };
                assert_eq!(additional_faces.len(), 1);
                let input_to_removal = s.model.state_before(removed);
                assert_eq!(input_to_removal.threads.len(), 2);
                assert!(input_to_removal.threads.iter().all(|thread| thread.feature == id));
                run(&mut s, "edit.undo", json!({}));
            } else {
                let error = reply.unwrap_err().to_string();
                assert!(error.contains("same component"), "{error}");
            }
            assert_eq!(s.doc.to_json(), before);
            assert_eq!(s.undo.len(), undo);
        }
    }
}

/// Explicit identities distinguish coincident bodies and follow a body rename and a copied reference.
#[test]
fn grouped_thread_keeps_ordered_body_identities_on_coincident_shafts() {
    let mut s = Session::default();
    for body in ["A", "B"] {
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":body,"operation":"new"}));
    }
    let made = run(&mut s, "solid.thread", json!({"faces":[{"body":"B","point":[4,0,2]},{"body":"A","point":[4,0,2]}],"designation":"M8"}));
    let id = made["feature"].as_u64().unwrap();
    assert_eq!(s.model.state().threads.iter().map(|t| t.body.as_str()).collect::<Vec<_>>(), ["B", "A"]);
    assert_eq!(s.doc.feature(id).unwrap().kind.input_bodies(), ["B", "A"]);
    let mut copied = s.doc.feature(id).unwrap().kind.clone();
    copied.remap_bodies(&std::collections::BTreeMap::from([("A".into(), "A2".into()), ("B".into(), "B2".into())]));
    assert_eq!(copied.input_bodies(), ["B2", "A2"]);
    run(&mut s, "body.rename", json!({"body":"B","name":"Renamed"}));
    assert_eq!(s.doc.feature(id).unwrap().kind.input_bodies(), ["Renamed", "A"]);
    let saved = s.doc.to_json();
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    assert!(reopened.model.results.iter().all(|r| r.error.is_none()));
    assert_eq!(reopened.model.state().threads.iter().map(|t| t.body.as_str()).collect::<Vec<_>>(), ["Renamed", "A"]);
    run(
        &mut reopened,
        "timeline.redefine",
        json!({"feature":id,"command":"solid.thread","params":{"faces":[{"body":"A","point":[4,0,2]},{"body":"Renamed","point":[4,0,2]}],"designation":"M8"}}),
    );
    assert_eq!(reopened.doc.feature(id).unwrap().kind.input_bodies(), ["A", "Renamed"]);
    run(&mut reopened, "edit.undo", json!({}));
    assert_eq!(reopened.doc.to_json(), saved);
}

/// A forged hint cannot broaden one target's domain to another component, including nested construction.
#[test]
fn grouped_thread_body_qualifiers_cannot_escape_their_component() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"Allowed"}));
    run(&mut s, "component.create", json!({"name":"Foreign"}));
    run(&mut s, "solid.cylinder", json!({"base":[30,0,0],"radius":4,"height":8,"body_name":"Foreign shaft"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let wrong =
        json!({"faces":[{"body":"Allowed","point":[34,0,2]}],"designation":"M8","participants":["Foreign shaft"],"target_bodies":["Foreign shaft"]});
    assert!(s.execute("solid.thread", &wrong).is_err());
    assert_eq!(s.doc.to_json(), before);
    assert_eq!(s.undo.len(), undo);
    let mixed = json!({"faces":[{"body":"Allowed","point":[4,0,2]},{"body":"Foreign shaft","point":[34,0,2]}],"designation":"M8"});
    assert!(crate::cmd::find_command("solid.thread").unwrap().run_nested(&mut s, &mixed).is_err());
    assert_eq!(s.doc.to_json(), before);
    let made = run(
        &mut s,
        "solid.thread",
        json!({"faces":[{"body":"Allowed","point":[4,0,2]}],"designation":"M8","participants":["Foreign shaft"],"target_bodies":["Foreign shaft"]}),
    );
    assert_eq!(made["thread"]["body"], "Allowed");
    assert_eq!(s.doc.features.last().unwrap().kind.input_bodies(), ["Allowed"]);
}

/// Old grouped files, including their formerly ignored participants, retain their empty-domain behavior.
#[test]
fn grouped_thread_without_saved_body_identities_keeps_legacy_behavior() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"A"}));
    run(&mut s, "solid.cylinder", json!({"base":[30,0,0],"radius":4,"height":8,"body_name":"B"}));
    run(&mut s, "solid.thread", json!({"faces":[{"body":"A","point":[4,0,2]},{"body":"B","point":[34,0,2]}],"designation":"M8"}));
    let mut old: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let thread = old["features"].as_array_mut().unwrap().last_mut().unwrap().as_object_mut().unwrap();
    thread.remove("target_bodies");
    thread.insert("participants".into(), json!(["A"]));
    let old = Session::new(solidcraft_doc::Document::from_json(&old.to_string()).unwrap());
    assert_eq!(old.model.state().threads.iter().map(|t| t.body.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    assert!(old.doc.features.last().unwrap().kind.input_bodies().is_empty());
    let saved: Value = serde_json::from_str(&old.doc.to_json()).unwrap();
    assert!(saved["features"].as_array().unwrap().last().unwrap().get("target_bodies").is_none());
}

#[test]
fn saved_grouped_thread_rejects_missing_or_misaligned_body_identities() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"A"}));
    run(&mut s, "solid.thread", json!({"faces":[{"body":"A","point":[4,0,2]},{"body":"A","point":[4,0,5]}],"designation":"M8"}));
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    for targets in [json!(["A"]), json!(["A", "missing"])] {
        let mut broken = saved.clone();
        broken["features"].as_array_mut().unwrap().last_mut().unwrap()["target_bodies"] = targets;
        let broken = Session::new(solidcraft_doc::Document::from_json(&broken.to_string()).unwrap());
        assert!(broken.model.results.last().unwrap().error.is_some());
        assert!(broken.model.state().threads.is_empty());
    }
}

/// Explicit viewport instance names map to definition identities while another component is active.
#[test]
fn grouped_thread_world_body_names_keep_definition_identities() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name":"Part","grounded":false}));
    run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"A"}));
    run(&mut s, "solid.cylinder", json!({"base":[20,0,0],"radius":4,"height":8,"body_name":"B"}));
    run(&mut s, "component.activate", json!({"component":0}));
    let copy = run(&mut s, "occurrence.copy", json!({"occurrence":part["occurrence"],"translate":[60,0,0]}));
    let occurrence = copy["occurrence"].as_u64().unwrap();
    let records = s.doc.body_frames_with_source(&s.model.state());
    let name =
        |definition: &str| records.iter().find(|(_, body, _, path, _)| body == definition && path.last() == Some(&occurrence)).unwrap().0.clone();
    let made =
        run(&mut s, "solid.thread", json!({"faces":[{"body":name("A"),"point":[64,0,2]},{"body":name("B"),"point":[84,0,2]}],"designation":"M8"}));
    let id = made["feature"].as_u64().unwrap();
    assert_eq!(s.doc.feature(id).unwrap().kind.input_bodies(), ["A", "B"]);
    assert_eq!(s.model.state().threads.iter().map(|t| t.body.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    assert_eq!(s.world_state().unwrap().threads.len(), 4, "the shared definition is threaded in both placements");
}

/// #13505: a tapped hole drilled across two slots has its wall in three pieces (z 30..40, 15..25
/// and 5..10). The thread runs along every piece the hole reaches, cosmetic or modelled, and a
/// thread depth runs on through the pieces from the first one's start.
#[test]
fn a_tapped_hole_across_slots_is_threaded_along_every_piece_of_its_wall() {
    let slotted = || {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"corner": [0, 0, 0], "length": 30, "width": 30, "height": 40, "body_name": "Part"}));
        for z in [25, 10] {
            run(&mut s, "solid.box", json!({"corner": [-1, -1, z], "length": 32, "width": 21, "height": 5, "operation": "cut", "targets": ["Part"]}));
        }
        s
    };
    let hole = |s: &mut Session, extra: Value| {
        let mut ask = json!({"position": [15, 10, 40], "direction": [0, 0, -1], "thread": "M4", "depth": 35});
        ask.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(s, "solid.hole", ask);
    };
    let spans = |s: &Session| {
        let mut v: Vec<(f64, f64)> = s
            .model
            .state()
            .threads
            .iter()
            .map(|t| {
                let (a, b) = ((t.axis_point + t.axis * t.start).z, (t.axis_point + t.axis * t.end).z);
                (a.min(b), a.max(b))
            })
            .collect();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        v
    };
    let near = |got: Vec<(f64, f64)>, want: &[(f64, f64)]| {
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(want) {
            assert!((g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6, "{got:?} vs {want:?}");
        }
    };
    let mut s = slotted();
    hole(&mut s, json!({}));
    near(spans(&s), &[(30.0, 40.0), (15.0, 25.0), (5.0, 10.0)]);
    // 22 mm from the entry: on through the first slot, stopping in the second piece.
    let mut s = slotted();
    hole(&mut s, json!({"thread_depth": 22}));
    near(spans(&s), &[(30.0, 40.0), (18.0, 25.0)]);
    // Modelled, every piece of the wall is grooved: each loses material the cosmetic one keeps.
    let mut s = slotted();
    hole(&mut s, json!({"modeled": true}));
    assert!(s.model.state().body("Part").unwrap().body.validity().is_empty());
    let mut plain = slotted();
    hole(&mut plain, json!({}));
    for (z0, z1) in [(30.0, 40.0), (15.0, 25.0), (5.0, 10.0)] {
        let (cut, drilled) = (volume_between(&s, "Part", (z0, z1), None), volume_between(&plain, "Part", (z0, z1), None));
        assert!(cut < drilled - 1.0, "z {z0}..{z1}: {cut} vs {drilled}");
    }
}

/// #16668: two components built alike each have a cylinder at the same place in their own
/// coordinates. A thread made in the second one is on the second one's body: cut into it when
/// modelled, and recorded on it when cosmetic. It went on the first component's body.
#[test]
fn a_thread_goes_on_a_body_of_its_own_component() {
    let studs = |modeled: bool| {
        let mut s = Session::default();
        for name in ["Stud A", "Stud B"] {
            run(&mut s, "component.activate", json!({"component": 0}));
            run(&mut s, "component.create", json!({"name": name}));
            run(&mut s, "solid.cylinder", json!({"diameter": 8, "height": 20}));
        }
        // Stud B is the active component.
        run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "designation": "M8x1.25", "modeled": modeled}));
        s
    };
    let body_of = |s: &Session, name: &str| {
        let comp = s.doc.components.iter().find(|c| c.name == name).map(|c| c.id).unwrap();
        s.model.state().bodies.iter().find(|b| b.component == comp).map(|b| b.name.clone()).unwrap()
    };
    let plain = PI * 16.0 * 20.0;
    let mut s = studs(true);
    let (a, b) = (body_of(&s, "Stud A"), body_of(&s, "Stud B"));
    assert_ne!(a, b);
    let (va, vb) = (volume(&mut s, &a), volume(&mut s, &b));
    assert!((va - plain).abs() < 0.5, "Stud A is left as it was: {va} against {plain}");
    assert!(vb < plain - 20.0, "Stud B is threaded: {vb} against {plain}");
    let s = studs(false);
    let on: Vec<String> = s.model.state().threads.iter().map(|t| t.body.clone()).collect();
    assert_eq!(on, [body_of(&s, "Stud B")]);
}
/// A plain `solid.move` (no copy) must carry the body's thread record with the body.
/// The record keeps the thread feature's id and its hand; a rigid turn does not flip that hand,
/// and a body left out of the move keeps the record it had. A copied move is covered above.
#[test]
fn a_plain_move_carries_the_bodys_thread_record() {
    // A modelled thread is cut into the body and moves with it; only a cosmetic one has a record to carry.
    {
        let modeled = false;
        let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"Shaft"}));
        let shaft_feature =
            run(&mut s, "solid.thread", json!({"face":[4,0,4],"designation":"M8-6g","hand":"left","modeled":modeled}))["feature"].clone();
        run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"base":[40,0,0],"body_name":"Peg","operation":"new"}));
        let peg_feature = run(&mut s, "solid.thread", json!({"face":[44,0,4],"designation":"M8-6g","modeled":modeled}))["feature"].clone();
        assert_ne!(shaft_feature, peg_feature, "modeled {modeled}, before the move: the two threads are different features");

        let (shaft_start, shaft_end, peg_start, peg_end) = {
            let listed = run(&mut s, "model.threads", json!({}));
            let threads = listed["threads"].as_array().unwrap_or_else(|| panic!("modeled {modeled}, before the move: {listed}"));
            let pick = |body: &str| {
                let found: Vec<_> = threads.iter().filter(|t| t["body"] == body).collect();
                assert_eq!(found.len(), 1, "modeled {modeled}, before the move: expected one {body} record, got {listed}");
                found[0]
            };
            let num = |t: &Value, body: &str, key: &str| {
                t[key].as_f64().unwrap_or_else(|| panic!("modeled {modeled}, before the move: {body} {key} is {t}"))
            };
            let shaft = pick("Shaft");
            let peg = pick("Peg");
            (num(shaft, "Shaft", "start"), num(shaft, "Shaft", "end"), num(peg, "Peg", "start"), num(peg, "Peg", "end"))
        };

        let coords_near = |got: &Value, want: [f64; 3], step: &str, what: &str| {
            for (i, want_i) in want.into_iter().enumerate() {
                let g = got.get(i).and_then(Value::as_f64);
                assert!(g.is_some_and(|g| (g - want_i).abs() < 1e-6), "modeled {modeled}, {step}: {what}[{i}] is {got}, expected {want:?}");
            }
        };
        let scalar_near = |got: &Value, want: f64, step: &str, what: &str| {
            let g = got.as_f64();
            assert!(g.is_some_and(|g| (g - want).abs() < 1e-6), "modeled {modeled}, {step}: {what} is {got}, expected {want}");
        };
        let expect = |s: &mut Session, step: &str, shaft_point: [f64; 3], shaft_axis: [f64; 3]| {
            let listed = run(s, "model.threads", json!({}));
            let threads = listed["threads"].as_array().unwrap_or_else(|| panic!("modeled {modeled}, {step}: {listed}"));
            assert_eq!(threads.len(), 2, "modeled {modeled}, {step}: expected one record each for Shaft and Peg, got {listed}");
            for (body, feature, point, axis, hand, start, end) in [
                ("Shaft", &shaft_feature, shaft_point, shaft_axis, "left", shaft_start, shaft_end),
                ("Peg", &peg_feature, [40.0, 0.0, 0.0], [0.0, 0.0, 1.0], "right", peg_start, peg_end),
            ] {
                let found: Vec<_> = threads.iter().filter(|t| t["body"] == body).collect();
                assert_eq!(found.len(), 1, "modeled {modeled}, {step}: expected one {body} record, got {listed}");
                let t = found[0];
                coords_near(&t["axis_point"], point, step, &format!("{body} axis_point"));
                coords_near(&t["axis"], axis, step, &format!("{body} axis"));
                assert_eq!(t["feature"], *feature, "modeled {modeled}, {step}: {body} feature id, got {t}");
                assert_eq!(t["body"], body, "modeled {modeled}, {step}: body, got {t}");
                assert_eq!(t["hand"], hand, "modeled {modeled}, {step}: {body} hand, got {t}");
                assert_eq!(t["designation"], "M8-6g", "modeled {modeled}, {step}: {body} designation, got {t}");
                scalar_near(&t["start"], start, step, &format!("{body} start"));
                scalar_near(&t["end"], end, step, &format!("{body} end"));
            }
        };
        let body_names = |s: &Session| {
            let mut names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
            names.sort();
            names
        };
        let at_origin = ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let translated = ([30.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        // Right-hand quarter turn about +X: (x, y, z) relative to the pivot maps to (x, -z, y).
        let turned = ([30.0, 0.0, 0.0], [0.0, -1.0, 0.0]);

        expect(&mut s, "before the move", at_origin.0, at_origin.1);

        let moved = run(&mut s, "solid.move", json!({"bodies":["Shaft"],"translate":[30,0,0]}));
        assert_eq!(moved["created_bodies"], json!([]), "modeled {modeled}, after the 30 mm move: a plain move must not create a copy: {moved}");
        assert_eq!(
            body_names(&s),
            ["Peg".to_string(), "Shaft".to_string()],
            "modeled {modeled}, after the 30 mm move: Shaft stays the moved body and Peg stays"
        );
        expect(&mut s, "after the 30 mm move", translated.0, translated.1);
        let contains = |at: [f64; 3]| {
            let state = s.model.state();
            let shaft = state.body("Shaft").unwrap_or_else(|| panic!("modeled {modeled}, after the 30 mm move: Shaft is gone"));
            shaft.body.classify_point(Vec3::new(at[0], at[1], at[2])).unwrap() == solidcraft_kernel::PointClass::Inside
        };
        assert!(contains([30.0, 0.0, 4.0]), "modeled {modeled}, after the 30 mm move: the shaft should contain [30, 0, 4]");
        assert!(!contains([0.0, 0.0, 4.0]), "modeled {modeled}, after the 30 mm move: the shaft should not contain [0, 0, 4]");

        let turned_reply = run(&mut s, "solid.move", json!({"bodies":["Shaft"],"axis":[1,0,0],"angle":"90 deg","origin":[30,0,0]}));
        assert_eq!(
            turned_reply["created_bodies"],
            json!([]),
            "modeled {modeled}, after the 90 deg turn: a plain move must not create a copy: {turned_reply}"
        );
        assert_eq!(body_names(&s), ["Peg".to_string(), "Shaft".to_string()], "modeled {modeled}, after the 90 deg turn: Shaft and Peg only");
        expect(&mut s, "after the 90 deg turn", turned.0, turned.1);

        run(&mut s, "edit.undo", json!({}));
        expect(&mut s, "after undoing the turn", translated.0, translated.1);
        run(&mut s, "edit.undo", json!({}));
        expect(&mut s, "after undoing the move", at_origin.0, at_origin.1);
        run(&mut s, "edit.redo", json!({}));
        run(&mut s, "edit.redo", json!({}));
        expect(&mut s, "after redoing both", turned.0, turned.1);

        let folder = crate::tests::Folder::new(&format!("plain-move-thread-record-{modeled}"));
        let path = folder.path("move.sc");
        run(&mut s, "file.save_as", json!({"path":path,"format":"container","cache":true}));
        let mut reopened = Session::default();
        run(&mut reopened, "doc.open", json!({"path":path,"read_only":true}));
        assert_eq!(reopened.model.last_recomputed, 0, "modeled {modeled}, reopened: reopen should reuse the saved model");
        expect(&mut reopened, "reopened", turned.0, turned.1);
        let mut rebuilt = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        expect(&mut rebuilt, "rebuilt", turned.0, turned.1);
        std::fs::remove_dir_all(&folder.0).unwrap();
    }
}

/// #310: a plain `solid.move` (no copy) that lists the same body more than once applies that
/// one rigid motion once per listing, to the body and to each of that body's thread records.
/// The record ends where the body ends. Hand, feature id, designation, class, start and end
/// stay. A body left out of the move does not move.
#[test]
fn a_plain_move_that_names_a_body_more_than_once_carries_its_thread_record() {
    // A modelled thread is cut into the body and moves with it; only a cosmetic one has a record to carry.
    {
        let modeled = false;
        let prepare = || {
            let mut s = Session { param_check: crate::schema::ParamCheck::Strict, ..Session::default() };
            run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"body_name":"Shaft"}));
            let shaft_feature =
                run(&mut s, "solid.thread", json!({"face":[4,0,4],"designation":"M8-6g","hand":"left","modeled":modeled}))["feature"].clone();
            run(&mut s, "solid.cylinder", json!({"radius":4,"height":8,"base":[40,0,0],"body_name":"Peg","operation":"new"}));
            let peg_feature = run(&mut s, "solid.thread", json!({"face":[44,0,4],"designation":"M8-6g","modeled":modeled}))["feature"].clone();
            assert_ne!(shaft_feature, peg_feature, "modeled {modeled}, before the move: the two threads are different features");
            let (shaft_start, shaft_end, peg_start, peg_end) = {
                let listed = run(&mut s, "model.threads", json!({}));
                let threads = listed["threads"].as_array().unwrap_or_else(|| panic!("modeled {modeled}, before the move: {listed}"));
                let pick = |body: &str| {
                    let found: Vec<_> = threads.iter().filter(|t| t["body"] == body).collect();
                    assert_eq!(found.len(), 1, "modeled {modeled}, before the move: expected one {body} record, got {listed}");
                    found[0]
                };
                let num = |t: &Value, body: &str, key: &str| {
                    t[key].as_f64().unwrap_or_else(|| panic!("modeled {modeled}, before the move: {body} {key} is {t}"))
                };
                let shaft = pick("Shaft");
                let peg = pick("Peg");
                (num(shaft, "Shaft", "start"), num(shaft, "Shaft", "end"), num(peg, "Peg", "start"), num(peg, "Peg", "end"))
            };
            (s, shaft_feature, peg_feature, shaft_start, shaft_end, peg_start, peg_end)
        };
        let coords_near = |got: &Value, want: [f64; 3], step: &str, what: &str| {
            for (i, want_i) in want.into_iter().enumerate() {
                let g = got.get(i).and_then(Value::as_f64);
                assert!(g.is_some_and(|g| (g - want_i).abs() < 1e-6), "modeled {modeled}, {step}: {what}[{i}] is {got}, expected {want:?}");
            }
        };
        let scalar_near = |got: &Value, want: f64, step: &str, what: &str| {
            let g = got.as_f64();
            assert!(g.is_some_and(|g| (g - want).abs() < 1e-6), "modeled {modeled}, {step}: {what} is {got}, expected {want}");
        };
        let expect = |s: &mut Session,
                      step: &str,
                      shaft_feature: &Value,
                      peg_feature: &Value,
                      shaft_start: f64,
                      shaft_end: f64,
                      peg_start: f64,
                      peg_end: f64,
                      shaft_point: [f64; 3],
                      shaft_axis: [f64; 3]| {
            let listed = run(s, "model.threads", json!({}));
            let threads = listed["threads"].as_array().unwrap_or_else(|| panic!("modeled {modeled}, {step}: {listed}"));
            assert_eq!(threads.len(), 2, "modeled {modeled}, {step}: expected one record each for Shaft and Peg, got {listed}");
            for (body, feature, point, axis, hand, start, end) in [
                ("Shaft", shaft_feature, shaft_point, shaft_axis, "left", shaft_start, shaft_end),
                ("Peg", peg_feature, [40.0, 0.0, 0.0], [0.0, 0.0, 1.0], "right", peg_start, peg_end),
            ] {
                let found: Vec<_> = threads.iter().filter(|t| t["body"] == body).collect();
                assert_eq!(found.len(), 1, "modeled {modeled}, {step}: expected one {body} record, got {listed}");
                let t = found[0];
                coords_near(&t["axis_point"], point, step, &format!("{body} axis_point"));
                coords_near(&t["axis"], axis, step, &format!("{body} axis"));
                assert_eq!(t["feature"], *feature, "modeled {modeled}, {step}: {body} feature id, got {t}");
                assert_eq!(t["body"], body, "modeled {modeled}, {step}: body, got {t}");
                assert_eq!(t["hand"], hand, "modeled {modeled}, {step}: {body} hand, got {t}");
                assert_eq!(t["designation"], "M8-6g", "modeled {modeled}, {step}: {body} designation, got {t}");
                scalar_near(&t["start"], start, step, &format!("{body} start"));
                scalar_near(&t["end"], end, step, &format!("{body} end"));
            }
        };
        let class_at = |s: &Session, at: [f64; 3], step: &str| {
            let state = s.model.state();
            let shaft = state.body("Shaft").unwrap_or_else(|| panic!("modeled {modeled}, {step}: Shaft is gone"));
            shaft
                .body
                .classify_point(Vec3::new(at[0], at[1], at[2]))
                .unwrap_or_else(|err| panic!("modeled {modeled}, {step}: classify_point {at:?} failed: {err}"))
        };
        let at_origin = ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);

        // Two listings of +30 mm: the body and its record both end at x = 60.
        {
            let (mut s, shaft_feature, peg_feature, shaft_start, shaft_end, peg_start, peg_end) = prepare();
            expect(&mut s, "before the move", &shaft_feature, &peg_feature, shaft_start, shaft_end, peg_start, peg_end, at_origin.0, at_origin.1);
            let moved = run(&mut s, "solid.move", json!({"bodies":["Shaft","Shaft"],"translate":[30,0,0]}));
            assert_eq!(
                moved["created_bodies"],
                json!([]),
                "modeled {modeled}, after naming Shaft twice: a plain move must not create a copy: {moved}"
            );
            let step = "after naming Shaft twice";
            assert_eq!(
                class_at(&s, [60.0, 0.0, 4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should contain [60, 0, 4]"
            );
            assert_ne!(
                class_at(&s, [30.0, 0.0, 4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should not contain [30, 0, 4]"
            );
            assert_ne!(
                class_at(&s, [0.0, 0.0, 4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should not contain [0, 0, 4]"
            );
            expect(&mut s, step, &shaft_feature, &peg_feature, shaft_start, shaft_end, peg_start, peg_end, [60.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
            run(&mut s, "edit.undo", json!({}));
            expect(&mut s, "after one undo", &shaft_feature, &peg_feature, shaft_start, shaft_end, peg_start, peg_end, at_origin.0, at_origin.1);
        }

        // Three listings of +30 mm land at x = 90.
        {
            let (mut s, shaft_feature, peg_feature, shaft_start, shaft_end, peg_start, peg_end) = prepare();
            expect(
                &mut s,
                "before the three-name move",
                &shaft_feature,
                &peg_feature,
                shaft_start,
                shaft_end,
                peg_start,
                peg_end,
                at_origin.0,
                at_origin.1,
            );
            let moved = run(&mut s, "solid.move", json!({"bodies":["Shaft","Shaft","Shaft"],"translate":[30,0,0]}));
            assert_eq!(
                moved["created_bodies"],
                json!([]),
                "modeled {modeled}, after naming Shaft three times: a plain move must not create a copy: {moved}"
            );
            let step = "after naming Shaft three times";
            assert_eq!(
                class_at(&s, [90.0, 0.0, 4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should contain [90, 0, 4]"
            );
            assert_ne!(
                class_at(&s, [30.0, 0.0, 4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should not contain [30, 0, 4]"
            );
            expect(&mut s, step, &shaft_feature, &peg_feature, shaft_start, shaft_end, peg_start, peg_end, [90.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        }

        // Two right-hand quarter turns about +X: (x, y, z) -> (x, -z, y) each time,
        // so the axis [0, 0, 1] becomes [0, 0, -1] and the axis point stays at the origin.
        {
            let (mut s, shaft_feature, peg_feature, shaft_start, shaft_end, peg_start, peg_end) = prepare();
            expect(
                &mut s,
                "before the two turns",
                &shaft_feature,
                &peg_feature,
                shaft_start,
                shaft_end,
                peg_start,
                peg_end,
                at_origin.0,
                at_origin.1,
            );
            let moved = run(&mut s, "solid.move", json!({"bodies":["Shaft","Shaft"],"axis":[1,0,0],"angle":"90 deg","origin":[0,0,0]}));
            assert_eq!(moved["created_bodies"], json!([]), "modeled {modeled}, after two 90 deg turns: a plain move must not create a copy: {moved}");
            let step = "after two 90 deg turns about +X";
            // (0, 0, 4) -> (0, -4, 0) -> (0, 0, -4). One turn would still contain (0, -4, 0).
            assert_eq!(
                class_at(&s, [0.0, 0.0, -4.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should contain [0, 0, -4]"
            );
            assert_ne!(
                class_at(&s, [0.0, -4.0, 0.0], step),
                solidcraft_kernel::PointClass::Inside,
                "modeled {modeled}, {step}: the shaft should not contain [0, -4, 0]"
            );
            expect(&mut s, step, &shaft_feature, &peg_feature, shaft_start, shaft_end, peg_start, peg_end, [0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        }
    }
}

/// Ungrounded "Part" with Shaft local z 0..12, translated to world z 100..112, root active.
fn placed_shaft() -> Session {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 12, "body_name": "Shaft"}));
    run(&mut s, "occurrence.move", json!({"occurrence": part["occurrence"], "translate": [0, 0, 100]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let bounds = s.world_state().unwrap().body("Shaft").expect("Shaft in the world").mesh().bounds();
    assert!(
        (bounds.min.z - 100.0).abs() < 1e-6 && (bounds.max.z - 112.0).abs() < 1e-6,
        "Shaft should sit at world z 100..112, got {:?}..{:?}",
        bounds.min,
        bounds.max
    );
    s
}

/// `up_to` as `{x, y, z}` on a placed component stops where the array form does.
/// World z 108 is local z 8 on a shaft whose axis parameter runs 0..12.
#[test]
fn placed_thread_up_to_xyz_object_maps_like_an_array() {
    let mut array_session = placed_shaft();
    let array = run(&mut array_session, "solid.thread", json!({"face": [4, 0, 102], "up_to": [4, 0, 108]}));
    let array_end = array["thread"]["end"].as_f64().unwrap();
    assert!((array_end - 8.0).abs() < 1e-6, "array up_to end {array_end}, reply {array}");

    let mut s = placed_shaft();
    let reply = run(&mut s, "solid.thread", json!({"face": [4, 0, 102], "up_to": {"x": 4, "y": 0, "z": 108}}));
    let end = reply["thread"]["end"].as_f64().unwrap();
    assert!((end - 8.0).abs() < 1e-6, "xyz up_to end {end}, same as array {array_end}, reply {reply}");
}

/// `face: {point: [x, y, z]}` on a placed component finds the cylinder, as `face: [x, y, z]` does.
#[test]
fn placed_thread_face_point_wrapper_finds_the_cylinder() {
    let mut s = placed_shaft();
    let reply = s.execute("solid.thread", &json!({"face": {"point": [4, 0, 102]}}));
    assert!(reply.is_ok(), "{reply:?}");
}

/// How a shaft's component is placed for the placed-frame tests.
#[derive(Clone, Copy, Debug)]
enum Place {
    Translated,
    Rotated,
    Nested,
}

const PLACES: [Place; 3] = [Place::Translated, Place::Rotated, Place::Nested];

/// A shaft (radius 4, local z 0..12) in a component placed as `place`, root active.
fn shaft_placed(place: Place) -> Session {
    let mut s = Session::default();
    let shaft = |s: &mut Session| {
        run(s, "solid.cylinder", json!({"radius": 4, "height": 12, "body_name": "Shaft"}));
    };
    match place {
        Place::Translated | Place::Rotated => {
            let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
            shaft(&mut s);
            let turn = if matches!(place, Place::Rotated) { json!({"axis": [1, 0, 0], "angle": "90 deg"}) } else { json!({}) };
            let mut p = json!({"occurrence": part["occurrence"], "translate": [30, 40, 100]});
            for (k, v) in turn.as_object().into_iter().flatten() {
                p[k] = v.clone();
            }
            run(&mut s, "occurrence.move", p);
        }
        Place::Nested => {
            let outer = run(&mut s, "component.create", json!({"name": "Outer", "grounded": false}));
            let inner = run(&mut s, "component.create", json!({"name": "Inner", "grounded": false}));
            shaft(&mut s);
            run(&mut s, "occurrence.move", json!({"occurrence": inner["occurrence"], "translate": [5, 0, 30], "axis": [0, 1, 0], "angle": "90 deg"}));
            run(&mut s, "occurrence.move", json!({"occurrence": outer["occurrence"], "translate": [20, 10, 70]}));
        }
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    s
}

/// The world point and direction of the shaft's local ones.
fn shaft_world(s: &Session, local: [f64; 3]) -> Value {
    let frames = s.body_frames();
    let (_, _, m) = frames.iter().find(|(w, _, _)| w == "Shaft").expect("Shaft is placed");
    let p = solidcraft_doc::apply_point(m, Vec3::new(local[0], local[1], local[2]));
    json!([p.x, p.y, p.z])
}

fn shaft_world_dir(s: &Session, local: [f64; 3]) -> Value {
    let frames = s.body_frames();
    let (_, _, m) = frames.iter().find(|(w, _, _)| w == "Shaft").expect("Shaft is placed");
    let d = solidcraft_doc::apply_vector(m, Vec3::new(local[0], local[1], local[2]));
    json!([d.x, d.y, d.z])
}

fn thread_end_with(place: Place, limit: impl Fn(&Session) -> Value, what: &str) {
    let mut s = shaft_placed(place);
    let face = shaft_world(&s, [4.0, 0.0, 2.0]);
    let up_to = limit(&s);
    let reply = s.execute("solid.thread", &json!({"face": face, "up_to": up_to}));
    let reply = reply.unwrap_or_else(|e| panic!("{place:?} {what}: {e} (up_to {up_to})"));
    let end = reply["thread"]["end"].as_f64().expect("a thread end");
    assert!((end - 8.0).abs() < 1e-6, "{place:?} {what}: thread end {end}, want 8 (up_to {up_to})");
}

/// an `up_to` point on the axis or off the surface of a placed shaft, in every form.
#[test]
fn placed_thread_up_to_point_off_the_surface_maps_into_the_faces_frame() {
    for place in PLACES {
        let xyz = |v: &Value| json!({"x": v[0], "y": v[1], "z": v[2]});
        thread_end_with(place, |s| shaft_world(s, [4.0, 0.0, 8.0]), "on the surface, array");
        thread_end_with(place, |s| shaft_world(s, [0.0, 0.0, 8.0]), "on the axis, array");
        thread_end_with(place, |s| xyz(&shaft_world(s, [0.0, 0.0, 8.0])), "on the axis, xyz");
        thread_end_with(place, |s| shaft_world(s, [9.0, 0.0, 8.0]), "in the air, array");
        thread_end_with(place, |s| xyz(&shaft_world(s, [9.0, 0.0, 8.0])), "in the air, xyz");
        thread_end_with(place, |s| json!({"point": shaft_world(s, [0.0, 0.0, 8.0])}), "on the axis, point wrapper");
    }
}

/// a plane limit of a placed shaft with its origin off the surface, in every plane form.
#[test]
fn placed_thread_plane_limit_maps_an_origin_off_the_surface() {
    for place in PLACES {
        let normal_form = |s: &Session, origin: Value| json!({"origin": origin, "normal": shaft_world_dir(s, [0.0, 0.0, 1.0])});
        thread_end_with(place, |s| normal_form(s, shaft_world(s, [0.0, 0.0, 8.0])), "plane origin on the axis");
        thread_end_with(place, |s| normal_form(s, shaft_world(s, [9.0, 0.0, 8.0])), "plane origin in the air");
        thread_end_with(place, |s| normal_form(s, shaft_world(s, [4.0, 0.0, 8.0])), "plane origin on the surface");
        thread_end_with(
            place,
            |s| {
                json!({"origin": shaft_world(s, [0.0, 0.0, 8.0]),
                    "x_dir": shaft_world_dir(s, [1.0, 0.0, 0.0]), "y_dir": shaft_world_dir(s, [0.0, 1.0, 0.0])})
            },
            "plane from x_dir and y_dir",
        );
        thread_end_with(
            place,
            |s| {
                let o = shaft_world(s, [0.0, 0.0, 8.0]);
                normal_form(s, json!([o[0].as_f64().unwrap().to_string(), o[1].as_f64().unwrap().to_string(), o[2].as_f64().unwrap().to_string()]))
            },
            "plane origin as expressions",
        );
    }
}

/// an `up_to` named as a root construction plane, on a placed shaft.
#[test]
fn placed_thread_up_to_a_root_construction_plane_maps_into_the_faces_frame() {
    for place in PLACES {
        let mut s = shaft_placed(place);
        let origin = shaft_world(&s, [0.0, 0.0, 4.0]);
        let normal = shaft_world_dir(&s, [0.0, 0.0, 1.0]);
        run(&mut s, "construct.plane.offset", json!({"base": {"origin": origin, "normal": normal}, "offset": 0, "name": "Lim"}));
        let face = shaft_world(&s, [4.0, 0.0, 2.0]);
        let reply = run(&mut s, "solid.thread", json!({"face": face, "up_to": "Lim"}));
        let end = reply["thread"]["end"].as_f64().expect("a thread end");
        assert!((end - 4.0).abs() < 1e-6, "{place:?}: thread end {end}, want 4");
    }
    // The reported case: the root plane XY offset 104 over a shaft translated 100.
    let mut s = placed_shaft();
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 104, "name": "Lim"}));
    let reply = run(&mut s, "solid.thread", json!({"face": [4, 0, 102], "up_to": "Lim"}));
    assert!((reply["thread"]["end"].as_f64().unwrap() - 4.0).abs() < 1e-6, "{reply}");
}

/// a malformed `up_to` is refused with the forms it takes, and `{point}` is read as the point.
#[test]
fn thread_up_to_refusals_name_the_forms_it_takes() {
    let mut s = placed_shaft();
    for bad in [json!({"x": 4, "z": 108}), json!({}), json!({"x": "a", "y": 0, "z": 1})] {
        let e = s.execute("solid.thread", &json!({"face": [4, 0, 102], "up_to": bad})).expect_err("refused").to_string();
        assert!(e.contains("[x, y, z]") && !e.contains("plane needs"), "{bad}: {e}");
    }
}

/// a `face` object naming a body that does not exist is refused.
#[test]
fn thread_face_point_with_an_unknown_body_is_refused() {
    let mut s = placed_shaft();
    let e = s.execute("solid.thread", &json!({"face": {"point": [4, 0, 102], "body": "Nope"}})).expect_err("refused").to_string();
    assert!(e.contains("Nope"), "{e}");
    assert!(s.execute("solid.thread", &json!({"face": {"point": [4, 0, 102], "body": "Shaft"}})).is_ok());
}

/// A component's origin plane key names a plane of that part, so from the root it is already in
/// the placed shaft's frame: the thread runs up to the part's XY plane, as an explicit plane does.
#[test]
fn placed_thread_up_to_the_parts_origin_plane_key_ends_on_that_plane() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -4], "radius": 4, "height": 12, "body_name": "Shaft"}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [0, 0, 100]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let key = format!("o:{occurrence}/origin/XY");
    let reply = run(&mut s, "solid.thread", json!({"face": [4, 0, 102], "from": [4, 0, 108], "up_to": key}));
    // The shaft spans local z -4 to 8 (world 96 to 108); from the top end down to the part's XY
    // plane (local z 0) the thread is 8 mm long. Had the key been read in the root frame it would
    // be world z 0, 108 mm away, and the limit would be refused.
    let (a, b) = (reply["thread"]["start"].as_f64().unwrap(), reply["thread"]["end"].as_f64().unwrap());
    assert!(((b - a).abs() - 8.0).abs() < 1e-6, "ends on the part's XY plane, reply {reply}");
}

/// A plane key that names no plane of any part is refused with a reason, never passed through to
/// be read in the wrong frame.
#[test]
fn placed_thread_up_to_an_unmappable_plane_name_is_refused() {
    let mut s = Session::default();
    let part = run(&mut s, "component.create", json!({"name": "Part", "grounded": false}));
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 12, "body_name": "Shaft"}));
    let occurrence = part["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence": occurrence, "translate": [0, 0, 100]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let e = s.execute("solid.thread", &json!({"face": [4, 0, 102], "up_to": "o:9999/origin/XY"})).expect_err("refused").to_string();
    assert!(e.contains("cannot be mapped") && e.contains("explicit"), "{e}");
}

/// A cosmetic thread in a bore that a cross hole cut before it runs its asked length across the
/// cross hole, as it does when the thread comes first (#23507): the cut splits the bore's wall
/// into two coaxial faces, 0 to 10 - sqrt(3.675^2 - 2.1^2) = 6.984 mm and from 13.016 mm on.
#[test]
fn a_thread_through_an_earlier_cross_hole_keeps_its_length() {
    let block = |s: &mut Session| {
        run(s, "sketch.create", json!({"plane": "YZ", "name": "Section"}));
        run(s, "sketch.line", json!({"points": [[-10, -10], [10, -10], [10, 10], [-10, 10]], "closed": true}));
        run(s, "sketch.circle.center", json!({"center": [0, 0], "diameter": 4.2}));
        run(s, "sketch.finish", json!({}));
        run(s, "solid.extrude", json!({"distance": 30, "name": "Block", "body_name": "Block"}));
    };
    let cross = json!({"diameter": 7.35, "height": 100, "base": [10, -50, 0], "axis": "Y", "operation": "cut", "name": "CrossCut"});
    for (extent, want) in [(json!({"extent": "length", "length": 15}), (0.0, 15.0)), (json!({}), (0.0, 30.0))] {
        let mut thread = json!({"face": [0.5, 2.1, 0], "designation": "M5", "from": [0, 2.1, 0], "name": "Thread"});
        thread.as_object_mut().unwrap().extend(extent.as_object().unwrap().clone());
        let mut spans = Vec::new();
        for cut_first in [false, true] {
            let mut s = Session::default();
            block(&mut s);
            if cut_first {
                run(&mut s, "solid.cylinder", cross.clone());
            }
            run(&mut s, "solid.thread", thread.clone());
            if !cut_first {
                run(&mut s, "solid.cylinder", cross.clone());
            }
            let st = s.model.state();
            let t = &st.threads[0];
            let (a, b) = (t.start.min(t.end), t.start.max(t.end));
            let along = |v: f64| (t.axis_point + t.axis * v).x;
            spans.push((along(a).min(along(b)), along(a).max(along(b))));
        }
        for (lo, hi) in spans {
            assert!((lo - want.0).abs() < 1e-6 && (hi - want.1).abs() < 1e-6, "{thread}: thread spans x {lo}..{hi}, not {want:?}");
        }
    }
    // Both pieces picked: one thread, the whole bore.
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "solid.cylinder", cross);
    let faces = json!([{"body": "Block", "point": [0.5, 2.1, 0]}, {"body": "Block", "point": [20, 2.1, 0]}]);
    run(&mut s, "solid.thread", json!({"faces": faces, "designation": "M5"}));
    let st = s.model.state();
    assert_eq!(st.threads.len(), 1, "{:?}", st.threads);
    let length = (st.threads[0].end - st.threads[0].start).abs();
    assert!((length - 30.0).abs() < 1e-6, "{length}");
}
