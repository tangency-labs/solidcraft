use super::*;

fn point(sk: &solidcraft_sketch::Sketch, id: &str) -> Vec2 {
    sk.point(sk.resolve_point(id).unwrap()).unwrap()
}

#[test]
fn circular_centered_six_holes_span_ninety_degrees_and_flip_about_the_original() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":1,"id":"seed"}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["seed"],"center":[0,0],"count":6,"angle":90,"alignment":"center"}));
    let copies = ids(&out["curves"]);
    let check = |s: &Session, angles: &[f64], seed: Vec2, center: Vec2| {
        assert!(point(&sketch(s), "seed.center").dist(seed) < 1e-6);
        for (id, angle) in copies.iter().zip(angles) {
            let (sin, cos) = angle.to_radians().sin_cos();
            let d = seed - center;
            let expected = center + Vec2::new(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
            assert!(point(&sketch(s), &format!("{id}.center")).dist(expected) < 1e-6, "angle {angle}");
        }
        check_relations(s);
    };
    check(&s, &[18., -18., 36., -36., 54.], Vec2::new(20., 0.), Vec2::ZERO);
    let before = s.doc.to_json();
    let flipped = run(&mut s, "sketch.pattern.circular", json!({"pattern":out["pattern"],"reversed":true}));
    assert_eq!(ids(&flipped["curves"]), copies);
    check(&s, &[-18., 18., -36., 36., -54.], Vec2::new(20., 0.), Vec2::ZERO);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    let mut s = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    s.refresh();
    // Omitting placement settings must retain them after reopening.
    run(&mut s, "sketch.pattern.circular", json!({"pattern":out["pattern"],"angle":90}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":[out["center"]]}));
    run(&mut s, "sketch.drag", json!({"entity":"seed.center","from":[20,0],"to":[25,2]}));
    check(&s, &[-18., 18., -36., 36., -54.], Vec2::new(25., 2.), Vec2::ZERO);
    run(&mut s, "sketch.constraint.fix", json!({"entities":["seed"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":[out["center"]],"fixed":false}));
    run(&mut s, "sketch.drag", json!({"entity":out["center"],"from":[0,0],"to":[2,3]}));
    check(&s, &[-18., 18., -36., 36., -54.], Vec2::new(25., 2.), Vec2::new(2., 3.));
}

#[test]
fn circular_alignment_handles_odd_arcs_one_sided_rotation_and_full_turns() {
    for (count, angle, alignment, reversed, expected) in [
        (5, 90., "center", false, vec![22.5_f64, -22.5, 45., -45.]),
        (3, 90., "right", false, vec![45., 90.]),
        (3, 90., "right", true, vec![-45., -90.]),
        (3, 90., "left", false, vec![-45., -90.]),
        (3, -90., "center", true, vec![45., -45.]),
        (6, 360., "center", false, vec![60., -60., 120., -120., 180.]),
        (6, 360., "center", true, vec![-60., 60., -120., 120., -180.]),
    ] {
        let mut s = new_sketch();
        run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":1,"id":"seed"}));
        let out = run(
            &mut s,
            "sketch.pattern.circular",
            json!({"entities":["seed"],"center":[0,0],"count":count,"angle":angle,"alignment":alignment,"reversed":reversed}),
        );
        let mut positions = vec![Vec2::new(20., 0.)];
        for (id, a) in ids(&out["curves"]).iter().zip(expected) {
            let pos = point(&sketch(&s), &format!("{id}.center"));
            let (sin, cos) = a.to_radians().sin_cos();
            assert!(pos.dist(Vec2::new(20. * cos, 20. * sin)) < 1e-6);
            assert!(positions.iter().all(|p| pos.dist(*p) > 1.));
            positions.push(pos);
        }
        assert_eq!(positions.len(), count);
        check_relations(&s);
    }
}

#[test]
fn circular_invalid_alignment_and_flip_roll_back_and_old_saved_defaults_still_load() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":1,"id":"seed"}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["seed"],"center":[0,0],"count":3,"angle":90}));
    let before = s.doc.to_json();
    for extra in [json!({"reversed":"true"}), json!({"reversed":null}), json!({"alignment":"unknown"}), json!({"alignment":null})] {
        let mut params = json!({"pattern":out["pattern"]});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.execute("sketch.pattern.circular", &params).is_err(), "{extra}");
        assert_eq!(s.doc.to_json(), before);
    }
    let mut saved: Value = serde_json::from_str(&before).unwrap();
    let kind = saved["features"][0]["sketch"]["patterns"][0]["kind"].as_object_mut().unwrap();
    kind.remove("alignment");
    kind.remove("reversed");
    let mut reopened =
        Session { doc: solidcraft_doc::format::read(&saved.to_string()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    reopened.refresh();
    run(&mut reopened, "sketch.pattern.circular", json!({"pattern":out["pattern"],"count":3}));
    assert!(point(&sketch(&reopened), &format!("{}.center", out["curves"][1].as_str().unwrap())).dist(Vec2::new(0., 20.)) < 1e-6);
}

#[test]
fn centered_two_direction_grid_keeps_original_fixed_and_follows_live_guides_after_reopen() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[5,5],"radius":2,"id":"seed"}));
    run(&mut s, "sketch.line", json!({"points":[[0,-10],[10,-10]],"ids":["guide1"]}));
    run(&mut s, "sketch.line", json!({"points":[[0,-30],[6,-22]],"ids":["guide2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide1","guide2"]}));
    let out = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({
            "entities":["seed"],"direction":"guide1","direction2":"guide2",
            "count":3,"spacing":20,"count2":3,"spacing2":10,"alignment":"center","alignment2":"center"
        }),
    );
    let expected = [(11., 13.), (-1., -3.), (25., 5.), (31., 13.), (19., -3.), (-15., 5.), (-9., 13.), (-21., -3.)];
    for (curve, (x, y)) in out["curves"].as_array().unwrap().iter().zip(expected) {
        assert!(point(&sketch(&s), &format!("{}.center", curve.as_str().unwrap())).dist(Vec2::new(x, y)) < 1e-6);
    }
    assert!(point(&sketch(&s), "seed.center").dist(Vec2::new(5., 5.)) < 1e-6);
    run(&mut s, "sketch.drag", json!({"entity":"seed.center","from":[5,5],"to":[7,8]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["seed"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide2"],"fixed":false}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide2.start"]}));
    run(&mut s, "sketch.drag", json!({"entity":"guide2.end","from":[6,-22],"to":[8,-24]}));
    check_relations(&s);
    let far = out["curves"][7].as_str().unwrap();
    assert!(point(&sketch(&s), &format!("{far}.center")).dist(Vec2::new(-21., 2.)) < 1e-6);
    let saved = s.doc.to_json();
    let mut s = Session { doc: solidcraft_doc::format::read(&saved).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    s.refresh();
    let before = s.doc.to_json();
    let edited = run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"alignment":"left","alignment2":"right","count":4}));
    assert_eq!(edited["curves"][7], far);
    assert!(point(&sketch(&s), &format!("{far}.center")).dist(Vec2::new(-17., 20.)) < 1e-6);
    check_relations(&s);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn left_and_right_alignments_are_independent_and_invalid_alignment_values_roll_back() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[5,5],"radius":2,"id":"seed"}));
    let out = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({
            "entities":["seed"],"count":3,"spacing":20,"count2":2,"spacing2":10,"alignment":"left","alignment2":"right"
        }),
    );
    let far = out["curves"][4].as_str().unwrap();
    assert!(point(&sketch(&s), &format!("{far}.center")).dist(Vec2::new(-35., 15.)) < 1e-6);
    run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"alignment":"right","alignment2":"left"}));
    assert!(point(&sketch(&s), &format!("{far}.center")).dist(Vec2::new(45., -5.)) < 1e-6);
    let before = s.doc.to_json();
    for extra in [json!({"reversed":"true"}), json!({"reversed2":null}), json!({"alignment":"unknown"}), json!({"alignment2":null})] {
        let mut params = json!({"pattern":out["pattern"]});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.execute("sketch.pattern.rectangular", &params).is_err(), "{extra}");
        assert_eq!(s.doc.to_json(), before);
    }
    let mut broken: Value = serde_json::from_str(&before).unwrap();
    broken["features"][0]["sketch"]["patterns"][0]["kind"]["reversed2"] = json!("true");
    assert!(solidcraft_doc::format::read(&broken.to_string()).is_err());
}

#[test]
fn even_centered_totals_flip_the_extra_side_independently_and_remain_associative() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[5,5],"radius":2,"id":"seed"}));
    run(&mut s, "sketch.line", json!({"points":[[0,-10],[10,-10]],"ids":["guide1"]}));
    run(&mut s, "sketch.line", json!({"points":[[0,-30],[6,-22]],"ids":["guide2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide1","guide2"]}));
    let out = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({
            "entities":["seed"],"direction":"guide1","direction2":"guide2",
            "count":4,"spacing":20,"count2":2,"spacing2":10,"alignment":"center","alignment2":"center"
        }),
    );
    let copy_ids = ids(&out["curves"]);
    let expected = [(11., 13.), (25., 5.), (31., 13.), (-15., 5.), (-9., 13.), (45., 5.), (51., 13.)];
    for (id, (x, y)) in copy_ids.iter().zip(expected) {
        assert!(point(&sketch(&s), &format!("{id}.center")).dist(Vec2::new(x, y)) < 1e-6);
    }
    let before = s.doc.to_json();
    let flipped = run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"reversed":true}));
    assert_eq!(ids(&flipped["curves"]), copy_ids);
    assert!(point(&sketch(&s), &format!("{}.center", copy_ids[6])).dist(Vec2::new(-29., 13.)) < 1e-6);
    run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"reversed2":true}));
    assert!(point(&sketch(&s), &format!("{}.center", copy_ids[6])).dist(Vec2::new(-41., -3.)) < 1e-6);
    assert!(point(&sketch(&s), "seed.center").dist(Vec2::new(5., 5.)) < 1e-6);
    check_relations(&s);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    let mut s = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    s.refresh();
    run(&mut s, "sketch.drag", json!({"entity":"seed.center","from":[5,5],"to":[7,8]}));
    assert!(point(&sketch(&s), &format!("{}.center", copy_ids[6])).dist(Vec2::new(-39., 0.)) < 1e-6);
    run(&mut s, "sketch.constraint.fix", json!({"entities":["seed"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide2"],"fixed":false}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["guide2.start"]}));
    run(&mut s, "sketch.drag", json!({"entity":"guide2.end","from":[6,-22],"to":[8,-24]}));
    assert!(point(&sketch(&s), &format!("{}.center", copy_ids[6])).dist(Vec2::new(-41., 2.)) < 1e-6);
    let edited = run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"spacing":10}));
    assert_eq!(ids(&edited["curves"]), copy_ids);
    assert!(point(&sketch(&s), &format!("{}.center", copy_ids[6])).dist(Vec2::new(-21., 2.)) < 1e-6);
    check_relations(&s);
}

#[test]
fn two_picked_pattern_lines_remain_live_through_drag_reopen_and_edit() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,5],[15,5]],"ids":["seed"]}));
    run(&mut s, "sketch.line", json!({"points":[[0,-20],[10,-20]],"ids":["dir1"]}));
    run(&mut s, "sketch.line", json!({"points":[[0,-40],[6,-32]],"ids":["dir2"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["seed","dir1.start","dir2.start"]}));
    let out = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({"entities":["seed"],"direction":"dir1","direction2":"dir2","count":3,"spacing":20,"count2":2,"spacing2":10}),
    );
    let copy = out["curves"][0].as_str().unwrap();
    let far = out["curves"][4].as_str().unwrap();
    assert!(point(&sketch(&s), &format!("{copy}.start")).dist(Vec2::new(16.0, 13.0)) < 1e-6);
    assert!(point(&sketch(&s), &format!("{far}.start")).dist(Vec2::new(56.0, 13.0)) < 1e-6);
    run(&mut s, "sketch.drag", json!({"entity":"dir2.end","from":[6,-32],"to":[8,-34]}));
    check_relations(&s);
    assert!(point(&sketch(&s), &format!("{copy}.start")).dist(Vec2::new(18.0, 11.0)) < 1e-6);
    let saved = s.doc.to_json();
    let mut s = Session { doc: solidcraft_doc::format::read(&saved).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    s.refresh();
    let edited = run(&mut s, "sketch.pattern.rectangular", json!({"pattern":out["pattern"],"count":4,"spacing2":-10}));
    assert_eq!(edited["curves"][0], copy);
    assert!(point(&sketch(&s), &format!("{copy}.start")).dist(Vec2::new(2.0, -1.0)) < 1e-6);
    run(&mut s, "sketch.constraint.fix", json!({"entities":["dir2"]}));
    run(&mut s, "sketch.drag", json!({"entity":"dir1.end","from":[10,-20],"to":[0,-10]}));
    check_relations(&s);
    assert!(point(&sketch(&s), &format!("{far}.start")).dist(Vec2::new(2.0, 39.0)) < 1e-6);
    let before = s.doc.to_json();
    run(&mut s, "sketch.delete", json!({"entities":["dir2"]}));
    assert!(sketch(&s).patterns.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn mirror_and_both_patterns_keep_selected_line_chains_driven_by_the_source() {
    for mode in [0, 1, 2] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,0],[15,0],[15,5]],"ids":["l1","l2"]}));
        run(&mut s, "sketch.line", json!({"points":[[0,-20],[0,20]],"ids":["axis"]}));
        run(&mut s, "sketch.constraint.fix", json!({"entities":["axis"]}));
        let (cmd, params) = match mode {
            0 => ("sketch.mirror", json!({"entities":["l1","l2"],"line":"axis"})),
            1 => (
                "sketch.pattern.rectangular",
                json!({"entities":["l1","l2"],"dir":[1,0],"dir2":[0.6,0.8],"count":2,"spacing":30,"count2":2,"spacing2":10}),
            ),
            _ => ("sketch.pattern.circular", json!({"entities":["l1","l2"],"center":"origin","count":4})),
        };
        let out = run(&mut s, cmd, params);
        run(&mut s, "sketch.drag", json!({"entity":"l1.end","from":[15,0],"to":[18,2]}));
        check_relations(&s);
        let sk = sketch(&s);
        let original = point(&sk, "l1.end");
        assert!(original.dist(Vec2::new(18.0, 2.0)) < 1e-6);
        let expected = match mode {
            0 => Vec2::new(-original.x, original.y),
            1 => original + Vec2::new(6.0, 8.0),
            _ => Vec2::new(-original.y, original.x),
        };
        let a = out["curves"][0].as_str().unwrap();
        let b = out["curves"][1].as_str().unwrap();
        // Reflection reverses each curve's traversal; the shared corner is still one point.
        let (end, start) = if mode == 0 { ("start", "end") } else { ("end", "start") };
        assert!(point(&sk, &format!("{a}.{end}")).dist(expected) < 1e-6, "{cmd}");
        assert_eq!(sk.resolve_point(&format!("{a}.{end}")), sk.resolve_point(&format!("{b}.{start}")));
    }
}

#[test]
fn malformed_two_direction_patterns_roll_back_and_saved_references_are_validated() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["seed"]}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":2,"id":"circle"}));
    let before = s.doc.to_json();
    for extra in [
        json!({"dir2":[0,0]}),
        json!({"dir2":[1e308,1e308]}),
        json!({"direction2":"missing"}),
        json!({"direction":"circle"}),
        json!({"direction2":42}),
    ] {
        let mut params = json!({"entities":["seed"],"count":2,"spacing":10,"count2":2});
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert!(s.execute("sketch.pattern.rectangular", &params).is_err(), "{extra}");
        assert_eq!(s.doc.to_json(), before);
    }
    run(&mut s, "sketch.pattern.rectangular", json!({"entities":["seed"],"direction":"seed","count":2,"spacing":10}));
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    for invalid in [json!([[usize::MAX, [10, 0]], null]), json!([[0, [1e308, 0]], null])] {
        let mut broken = saved.clone();
        let constraint =
            broken["features"][0]["sketch"]["constraints"].as_array_mut().unwrap().iter_mut().find(|c| c["type"] == "pattern_point").unwrap();
        constraint["directions"] = invalid;
        assert!(solidcraft_doc::format::read(&broken.to_string()).is_err());
    }
}

#[test]
fn mirrored_chain_endpoints_on_the_axis_remain_on_it_during_source_edits() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,5],[10,5],[10,15],[0,15]],"ids":["a","b","c"]}));
    run(&mut s, "sketch.line", json!({"points":[[0,-10],[0,30]],"ids":["axis"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["axis"]}));
    let out = run(&mut s, "sketch.mirror", json!({"entities":["a","b","c"],"line":"axis"}));
    let copy = out["curves"][0].as_str().unwrap();
    assert_eq!(sketch(&s).resolve_point("a.start"), sketch(&s).resolve_point(&format!("{copy}.end")));
    run(&mut s, "sketch.drag", json!({"entity":"a.start","from":[0,5],"to":[4,8]}));
    assert!(point(&sketch(&s), "a.start").x.abs() < 1e-6);
    run(&mut s, "sketch.drag", json!({"entity":"b.start","from":[10,5],"to":[13,8]}));
    let original = point(&sketch(&s), "b.start");
    assert!(point(&sketch(&s), &format!("{copy}.start")).dist(Vec2::new(-original.x, original.y)) < 1e-6);
    check_relations(&s);
}

fn check_relations(s: &Session) {
    let sk = sketch(s);
    for (i, constraint) in sk.constraints.iter().enumerate() {
        assert!(solidcraft_sketch::constraint_residual(&sk, i).is_some_and(|r| r < 1e-6), "{} {:?}", constraint.id, constraint.kind);
    }
}

#[test]
fn default_second_direction_follows_the_first_line_and_legacy_vectors_keep_their_grid() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[5,5],"radius":2,"id":"seed"}));
    run(&mut s, "sketch.line", json!({"points":[[0,-10],[10,-10]],"ids":["guide"]}));
    run(&mut s, "sketch.constraint.fix", json!({"entities":["seed","guide.start"]}));
    let out =
        run(&mut s, "sketch.pattern.rectangular", json!({"entities":["seed"],"direction":"guide","count":2,"spacing":10,"count2":2,"spacing2":20}));
    run(&mut s, "sketch.drag", json!({"entity":"guide.end","from":[10,-10],"to":[0,0]}));
    let far = out["curves"][2].as_str().unwrap();
    assert!(point(&sketch(&s), &format!("{far}.center")).dist(Vec2::new(-15.0, 15.0)) < 1e-6);
    check_relations(&s);

    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[5,5],"radius":2,"id":"seed"}));
    run(&mut s, "sketch.pattern.rectangular", json!({"entities":["seed"],"dir":[0,1],"count":2,"spacing":10,"count2":2,"spacing2":20}));
    let mut saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    saved["format"] = json!("solidcraft/21");
    let sk = &mut saved["features"][0]["sketch"];
    for c in sk["constraints"].as_array_mut().unwrap() {
        c.as_object_mut().unwrap().remove("directions");
    }
    let settings = sk["patterns"][0]["kind"].as_object_mut().unwrap();
    for key in ["dir2", "direction", "direction2", "alignment", "alignment2", "reversed", "reversed2"] {
        settings.remove(key);
    }
    let doc = solidcraft_doc::format::read(&saved.to_string()).unwrap();
    let sk = doc.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(point(sk, "c3.center").dist(Vec2::new(-15.0, 15.0)) < 1e-6);
    assert_eq!(doc.format, solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT));
}

#[test]
fn circular_pattern_keeps_seed_copy_and_center_related_during_drags() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["l1"]}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["l1"],"center":"origin","count":4}));
    assert_eq!(out["sketch"]["dof"], 4);
    assert_eq!(ids(&out["created"]["constraints"]).len(), 6);
    run(&mut s, "sketch.drag", json!({"entity":"l2.start","from":[0,10],"to":[5,13]}));
    check_relations(&s);
    let sk = sketch(&s);
    assert!(point(&sk, "l1.start").dist(Vec2::new(13.0, -5.0)) < 1e-6);
    assert!(point(&sk, "l3.start").dist(Vec2::new(-13.0, 5.0)) < 1e-6);
    run(&mut s, "sketch.drag", json!({"entity":"l1.end","from":[20,0],"to":[24,3]}));
    check_relations(&s);
    assert!(point(&sketch(&s), "l2.end").dist(Vec2::new(-3.0, 24.0)) < 1e-6);
}

#[test]
fn constrained_seed_prevents_independent_copy_motion() {
    for (cmd, params) in [
        ("sketch.pattern.circular", json!({"entities":["l1"],"center":"origin","count":4})),
        ("sketch.pattern.rectangular", json!({"entities":["l1"],"count":4,"spacing":10})),
    ] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["l1"]}));
        run(&mut s, "sketch.constraint.fix", json!({"entities":["l1"]}));
        let out = run(&mut s, cmd, params);
        assert_eq!(out["sketch"]["dof"], 0);
        let before = point(&sketch(&s), "l2.start");
        run(&mut s, "sketch.drag", json!({"entity":"l2.start","from":before,"to":[7,17]}));
        assert!(point(&sketch(&s), "l2.start").dist(before) < 1e-6);
        check_relations(&s);
    }
}

#[test]
fn circular_center_reference_follows_a_moved_circle_and_empty_space_center_is_constrainable() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[3,4],"radius":2,"id":"c1"}));
    run(&mut s, "sketch.line", json!({"points":[[13,4],[23,4]],"ids":["l1"]}));
    run(&mut s, "sketch.pattern.circular", json!({"entities":["l1"],"center":"c1.center","count":3,"angle":-180}));
    run(&mut s, "sketch.drag", json!({"entity":"c1.center","from":[3,4],"to":[8,9]}));
    check_relations(&s);
    let center = point(&sketch(&s), "c1.center");
    assert!(center.dist(Vec2::new(8.0, 9.0)) < 1e-6);
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["l1"],"center":[8,9],"count":2}));
    let center_id = out["center"].as_str().unwrap();
    run(&mut s, "sketch.constraint.coincident", json!({"a":center_id,"b":"c1.center"}));
    check_relations(&s);
}

#[test]
fn rectangular_pattern_preserves_signed_two_axis_offsets_and_circle_radii() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[10,5],"radius":2,"id":"c1"}));
    let out = run(&mut s, "sketch.pattern.rectangular", json!({"entities":["c1"],"dir":[0,1],"count":3,"spacing":-8,"count2":2,"spacing2":6}));
    assert_eq!(out["sketch"]["dof"], 3);
    run(&mut s, "sketch.drag", json!({"entity":"c1.center","from":[10,5],"to":[12,7]}));
    run(&mut s, "sketch.drag", json!({"entity":"c1","from":[14,7],"to":[16,7]}));
    check_relations(&s);
    let sk = sketch(&s);
    for id in ids(&out["curves"]) {
        assert!(close(sk.radius(sk.curve_index(&id).unwrap()).unwrap(), 4.0));
    }
    assert!(point(&sk, "c2.center").dist(Vec2::new(6.0, 7.0)) < 1e-6);
    assert!(point(&sk, "c6.center").dist(Vec2::new(6.0, -9.0)) < 1e-6);
}

#[test]
fn pattern_edits_retain_surviving_ids_and_work_after_undo_and_reopen() {
    for circular in [false, true] {
        let mut s = new_sketch();
        run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["l1"]}));
        let cmd = if circular { "sketch.pattern.circular" } else { "sketch.pattern.rectangular" };
        let params =
            if circular { json!({"entities":["l1"],"center":"origin","count":4}) } else { json!({"entities":["l1"],"count":4,"spacing":10}) };
        let original = run(&mut s, cmd, params);
        let pattern = original["pattern"].as_str().unwrap();
        let before = s.doc.to_json();
        let shrunk = run(&mut s, cmd, json!({"pattern":pattern,"count":2}));
        assert_eq!(ids(&shrunk["curves"]), vec!["l2"]);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc.to_json(), before);
        run(&mut s, "edit.redo", json!({}));
        let mut reopened =
            Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
        reopened.refresh();
        let grown = run(&mut reopened, cmd, json!({"pattern":pattern,"count":5,"spacing":12,"angle":-180}));
        assert_eq!(ids(&grown["curves"])[0], "l2");
        assert_eq!(ids(&grown["curves"]).len(), 4);
        assert!(!ids(&grown["curves"]).contains(&"l3".to_string()));
        check_relations(&reopened);
        let sk = sketch(&reopened);
        let expected = if circular { Vec2::new(10.0 / 2.0_f64.sqrt(), -10.0 / 2.0_f64.sqrt()) } else { Vec2::new(22.0, 0.0) };
        assert!(point(&sk, "l2.start").dist(expected) < 1e-6);
    }
}

#[test]
fn malformed_pattern_edits_roll_back_and_deleted_seed_does_not_panic() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["l1"]}));
    let out = run(&mut s, "sketch.pattern.rectangular", json!({"entities":["l1","l1"],"count":3,"spacing":10}));
    assert_eq!(ids(&out["curves"]).len(), 2);
    let before = s.doc.to_json();
    for params in [json!({"count":2.5}), json!({"count":501}), json!({"count2":500}), json!({"dir":[0,0]}), json!({"spacing":1e308})] {
        let mut params = params;
        params["pattern"] = out["pattern"].clone();
        assert!(s.execute("sketch.pattern.rectangular", &params).is_err());
        assert_eq!(s.doc.to_json(), before);
    }
    run(&mut s, "sketch.delete", json!({"entities":["l1"]}));
    assert!(s.execute("sketch.pattern.rectangular", &json!({"pattern":out["pattern"],"count":4})).is_err());
}

#[test]
fn pattern_seed_move_and_scale_preserve_shape_and_shared_points() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0],[20,10]],"ids":["l1","l2"]}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["l1","l2"],"center":"origin","count":3}));
    let sk = sketch(&s);
    assert_eq!(sk.resolve_point("l3.end"), sk.resolve_point("l4.start"));
    assert_eq!(ids(&out["created"]["constraints"]).len(), 6);
    run(&mut s, "sketch.move", json!({"entities":["l1","l2"],"translate":[5,3]}));
    assert!(point(&sketch(&s), "l1.start").dist(Vec2::new(15.0, 3.0)) < 1e-6);
    check_relations(&s);
    run(&mut s, "sketch.scale", json!({"entities":["l1","l2"],"base":"origin","factor":2}));
    assert!(point(&sketch(&s), "l1.start").dist(Vec2::new(30.0, 6.0)) < 1e-6);
    check_relations(&s);
}

#[test]
fn malformed_saved_pattern_settings_and_relations_are_refused() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[10,0],[20,0]],"ids":["l1"]}));
    run(&mut s, "sketch.pattern.circular", json!({"entities":["l1"],"center":"origin","count":3}));
    let saved: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    for key in ["count", "index", "angle", "relation", "center"] {
        let mut value = saved.clone();
        let sk = &mut value["features"][0]["sketch"];
        match key {
            "count" => sk["patterns"][0]["kind"]["count"] = json!(usize::MAX),
            "index" => sk["patterns"][0]["instances"][0]["index"] = json!([usize::MAX, 0]),
            "angle" => sk["patterns"][0]["kind"]["angle"] = json!(1e308),
            "center" => sk["patterns"][0]["kind"]["center"] = json!("missing"),
            _ => sk["constraints"][0]["offset"] = json!([1e308, 0]),
        }
        assert!(solidcraft_doc::format::read(&value.to_string()).is_err(), "{key}");
    }
    let mut legacy = saved;
    legacy["format"] = json!("solidcraft/19");
    let sk = legacy["features"][0]["sketch"].as_object_mut().unwrap();
    sk.remove("patterns");
    sk.insert("constraints".into(), json!([]));
    let read = solidcraft_doc::format::read(&legacy.to_string()).unwrap();
    assert!(read.sketch(s.active_sketch.unwrap()).unwrap().patterns.is_empty());
}

#[test]
fn patterned_arc_ellipse_spline_and_conic_follow_seed_geometry_exactly() {
    for (cmd, params) in [
        ("sketch.arc.center_point", json!({"center":[0,0],"start":[10,0],"end":[0,10],"connect":false})),
        ("sketch.ellipse", json!({"center":[0,0],"major":[10,0],"minor_radius":4})),
        ("sketch.spline.fit_point", json!({"points":[[0,0],[6,8],[14,3],[20,0]],"connect":false})),
        ("sketch.conic", json!({"start":[0,0],"end":[20,0],"apex":[10,20],"rho":0.65,"connect":false})),
    ] {
        let mut s = new_sketch();
        let seed = ids(&run(&mut s, cmd, params)["curves"])[0].clone();
        let out = run(&mut s, "sketch.pattern.rectangular", json!({"entities":[seed],"count":3,"spacing":40}));
        run(&mut s, "sketch.scale", json!({"entities":[seed],"base":"origin","factor":1.5}));
        let sk = sketch(&s);
        if matches!(sk.curves[sk.curve_index(&seed).unwrap()].kind, CurveKind::Conic { .. }) {
            let from = sk.exact_segs(sk.curve_index(&seed).unwrap()).unwrap()[0].point_at(0.5);
            run(&mut s, "sketch.drag", json!({"entity":seed,"from":from,"to":[from.x,from.y+2.0]}));
        }
        check_relations(&s);
        let sk = sketch(&s);
        let ci = sk.curve_index(&seed).unwrap();
        let original = sk.exact_segs(ci).unwrap_or_else(|| sk.segs(ci));
        for (i, id) in ids(&out["curves"]).iter().enumerate() {
            let ci = sk.curve_index(id).unwrap();
            let copy = sk.exact_segs(ci).unwrap_or_else(|| sk.segs(ci));
            assert_eq!(copy.len(), original.len());
            for (a, b) in original.iter().zip(copy) {
                for t in [0.0, 0.3, 0.7, 1.0] {
                    assert!(b.point_at(t).dist(a.point_at(t) + Vec2::new(40.0 * (i + 1) as f64, 0.0)) < 1e-6, "{cmd}");
                }
            }
        }
        assert!(solidcraft_doc::format::read(&s.doc.to_json()).is_ok());
    }
}

#[test]
fn circular_quantity_expression_survives_parameter_edits_undo_and_reopen() {
    let mut s = new_sketch();
    run(&mut s, "parameters.add", json!({"name":"HoleAmount","expression":"5","unit":""}));
    run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":2,"id":"seed"}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["seed"],"center":"origin","count":"HoleAmount"}));
    let original = sketch(&s).patterns[0].instances.clone();
    assert_eq!(sketch(&s).curves.len(), 5);
    run(&mut s, "parameters.change", json!({"name":"HoleAmount","expression":"7"}));
    let expanded = sketch(&s);
    assert_eq!(expanded.curves.len(), 7);
    assert_eq!(expanded.patterns[0].count_expression.as_deref(), Some("HoleAmount"));
    for (before, after) in original.iter().zip(&expanded.patterns[0].instances) {
        assert_eq!(before.curves, after.curves);
        assert_eq!(before.points, after.points);
        assert_eq!(before.constraints, after.constraints);
    }
    let expected = Vec2::new(20.0 * (std::f64::consts::TAU / 7.0).cos(), 20.0 * (std::f64::consts::TAU / 7.0).sin());
    assert!(point(&expanded, &format!("{}.center", original[0].curves[0])).dist(expected) < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sketch(&s).curves.len(), 5);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(sketch(&s).curves.len(), 7);
    run(&mut s, "parameters.rename", json!({"name":"HoleAmount","new_name":"holes"}));
    assert_eq!(sketch(&s).patterns[0].count_expression.as_deref(), Some("holes"));
    assert!(s.execute("parameters.delete", &json!({"name":"holes"})).is_err());
    let saved = s.doc.to_json();
    // A cold evaluation must rebuild from the expression even when no engine value-edit
    // command has materialized the new copies into the document yet.
    let mut cold_doc = (*s.doc).clone();
    cold_doc.change_param("holes", "9", None, None).unwrap();
    let mut cold =
        Session { doc: solidcraft_doc::format::read(&cold_doc.to_json()).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    cold.refresh();
    assert_eq!(sketch(&cold).curves.len(), 9);
    let mut reopened = Session { doc: solidcraft_doc::format::read(&saved).unwrap().into(), active_sketch: s.active_sketch, ..Default::default() };
    reopened.refresh();
    assert_eq!(sketch(&reopened).curves.len(), 7);
    run(&mut reopened, "parameters.change", json!({"name":"holes","expression":"3"}));
    assert_eq!(sketch(&reopened).curves.len(), 3);
    run(&mut reopened, "sketch.pattern.circular", json!({"pattern":out["pattern"],"reversed":true}));
    assert_eq!(sketch(&reopened).patterns[0].count_expression.as_deref(), Some("holes"));
    run(&mut reopened, "sketch.pattern.circular", json!({"pattern":out["pattern"],"count":4}));
    assert!(sketch(&reopened).patterns[0].count_expression.is_none());
    run(&mut reopened, "parameters.change", json!({"name":"holes","expression":"6"}));
    assert_eq!(sketch(&reopened).curves.len(), 4);
}

#[test]
fn circular_quantity_rejects_invalid_expressions_and_parameter_changes_atomically() {
    let mut s = new_sketch();
    run(&mut s, "parameters.add", json!({"name":"holes","expression":"5","unit":""}));
    run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":2,"id":"seed"}));
    for expression in ["missing", "5 mm", "1", "0", "-3", "3.5", "501", "1e309"] {
        let before = s.doc.to_json();
        assert!(s.execute("sketch.pattern.circular", &json!({"entities":["seed"],"center":"origin","count":expression})).is_err(), "{expression}");
        assert_eq!(s.doc.to_json(), before);
    }
    run(&mut s, "sketch.pattern.circular", json!({"entities":["seed"],"center":"origin","count":"holes + 1e-10"}));
    for expression in ["1", "0", "-3", "3.5", "501", "5 mm", "1e309"] {
        let before = s.doc.to_json();
        assert!(s.execute("parameters.change", &json!({"name":"holes","expression":expression})).is_err(), "{expression}");
        assert_eq!(s.doc.to_json(), before, "rejected {expression} leaves the document unchanged");
        assert_eq!(sketch(&s).curves.len(), 5);
    }
}

#[test]
fn circular_quantity_changes_keep_partial_full_centered_and_reversed_placements() {
    for angle in [90.0_f64, -90.0, 360.0, -360.0] {
        for alignment in ["right", "left", "center"] {
            for reversed in [false, true] {
                let mut s = new_sketch();
                run(&mut s, "parameters.add", json!({"name":"holes","expression":"4","unit":""}));
                run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":2,"id":"seed"}));
                run(
                    &mut s,
                    "sketch.pattern.circular",
                    json!({"entities":["seed"],"center":"origin","count":"holes","angle":angle,"alignment":alignment,"reversed":reversed}),
                );
                for count in [6, 3] {
                    run(&mut s, "parameters.change", json!({"name":"holes","expression":count}));
                    let sk = sketch(&s);
                    let solidcraft_sketch::SketchPatternKind::Circular { alignment, .. } = sk.patterns[0].kind else { panic!("circular pattern") };
                    let step = angle / if angle.abs() == 360.0 { count as f64 } else { (count - 1) as f64 };
                    for instance in &sk.patterns[0].instances {
                        let a = (step * alignment.multiplier(instance.index[0], reversed)).to_radians();
                        let expected = Vec2::new(20.0 * a.cos(), 20.0 * a.sin());
                        assert!(point(&sk, &format!("{}.center", instance.curves[0])).dist(expected) < 1e-6);
                    }
                    sk.validate_patterns().unwrap();
                    check_relations(&s);
                }
            }
        }
    }
}

/// #5280: a pattern joins its copies into one group of unknowns, and a group larger than the
/// solver takes is refused saying so, not as an over-constraint with no constraint named.
#[test]
fn a_pattern_too_large_to_solve_says_why() {
    let hexagon = |s: &mut Session| {
        let r = run(s, "sketch.polygon.inscribed", json!({"center": [0, 0], "radius": 2.5, "sides": 6}));
        ids(&r["curves"])
    };
    // 15 × 22 hexagons: inside the limit.
    let mut s = new_sketch();
    let lines = hexagon(&mut s);
    run(&mut s, "sketch.pattern.rectangular", json!({"entities": lines, "count": 15, "spacing": 6.5, "count2": 22, "spacing2": 6.5}));
    // 17 × 20: past it.
    let mut s = new_sketch();
    let lines = hexagon(&mut s);
    let e = s
        .execute("sketch.pattern.rectangular", &json!({"entities": lines, "count": 17, "spacing": 6.5, "count2": 20, "spacing2": 6.5}))
        .unwrap_err()
        .to_string();
    assert!(e.contains("unknowns") && e.contains(&solidcraft_sketch::MAX_COMPONENT_VARS.to_string()) && !e.contains("conflicts with"), "{e}");
}

#[test]
fn rectangular_pattern_expressions_follow_parameters_and_survive_reopening() {
    let mut s = new_sketch();
    for (name, expression, unit) in [("pitch", "30", "mm"), ("copies", "3", "")] {
        run(&mut s, "parameters.add", json!({"name":name,"expression":expression,"unit":unit}));
    }
    run(&mut s, "sketch.circle.center", json!({"id":"seed","center":[5,5],"radius":1}));
    let out = run(
        &mut s,
        "sketch.pattern.rectangular",
        json!({"entities":["seed"],"count":"copies","spacing":"pitch","count2":"2","spacing2":"pitch / 2"}),
    );
    let first = ids(&out["curves"])[0].clone();
    assert!(!s.doc.param_users("pitch").is_empty());
    run(&mut s, "sketch.finish", json!({}));
    let mut s = Session { doc: solidcraft_doc::format::read(&s.doc.to_json()).unwrap().into(), ..Default::default() };
    s.refresh();
    run(&mut s, "parameters.change", json!({"name":"pitch","expression":"40"}));
    run(&mut s, "parameters.change", json!({"name":"copies","expression":"4"}));
    let state = s.world_state().unwrap();
    let sk = &state.sketch(1).unwrap().sketch;
    assert_eq!(sk.patterns[0].instances.len(), 7);
    assert_eq!(sk.patterns[0].instances[0].curves[0], first);
    assert!(point(sk, &format!("{first}.center")).dist(Vec2::new(5., 25.)) < 1e-6);
    let last = &sk.patterns[0].instances.last().unwrap().curves[0];
    assert!(point(sk, &format!("{last}.center")).dist(Vec2::new(125., 25.)) < 1e-6);
    run(&mut s, "parameters.change", json!({"name":"copies","expression":"2"}));
    assert_eq!(s.world_state().unwrap().sketch(1).unwrap().sketch.patterns[0].instances.len(), 3);
    run(&mut s, "parameters.change", json!({"name":"pitch","new_name":"step"}));
    assert_eq!(serde_json::to_value(&s.doc.sketch(1).unwrap().patterns[0]).unwrap()["expressions"]["spacing"], "step");
    assert!(s.execute("parameters.change", &json!({"delete":"step"})).is_err());
}

#[test]
fn circular_pattern_expressions_validate_inputs_and_keep_numeric_compatibility() {
    let mut s = new_sketch();
    for (name, expression, unit) in [("turn", "90", "deg"), ("copies", "3", "")] {
        run(&mut s, "parameters.add", json!({"name":name,"expression":expression,"unit":unit}));
    }
    run(&mut s, "sketch.circle.center", json!({"id":"seed","center":[20,0],"radius":1}));
    let out = run(&mut s, "sketch.pattern.circular", json!({"entities":["seed"],"center":"origin","count":"copies","angle":"turn"}));
    run(&mut s, "parameters.change", json!({"name":"turn","expression":"180"}));
    run(&mut s, "parameters.change", json!({"name":"copies","expression":"4"}));
    let sk = sketch(&s);
    let last = &sk.patterns[0].instances.last().unwrap().curves[0];
    assert!(point(&sk, &format!("{last}.center")).dist(Vec2::new(-20., 0.)) < 1e-6);
    for expression in ["2.5", "501", "0", "-1"] {
        let error = s.execute("parameters.change", &json!({"name":"copies","expression":expression})).unwrap_err().to_string();
        assert!(error.contains("whole number"), "{error}");
    }
    for (key, value, message) in [("count", json!("2.5"), "count"), ("count", json!("501"), "count"), ("angle", json!("missing + 1"), "missing")] {
        let mut p = json!({"pattern":out["pattern"]});
        p[key] = value;
        let error = s.execute("sketch.pattern.circular", &p).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
    let error = s.execute("sketch.pattern.rectangular", &json!({"entities":["seed"],"count":2,"spacing":"unknown_pitch"})).unwrap_err().to_string();
    assert!(error.contains("spacing") && error.contains("unknown_pitch"), "{error}");
    run(&mut s, "sketch.pattern.circular", json!({"pattern":out["pattern"],"count":3,"angle":90}));
    assert!(serde_json::to_value(&sketch(&s).patterns[0]).unwrap().get("expressions").is_none());
    let legacy = s.doc.to_json();
    // Numeric patterns omit the optional expression field, just as older files do.
    assert!(!s.doc.to_json().contains("\"expressions\""));
    let reopened = solidcraft_doc::format::read(&legacy).unwrap();
    assert_eq!(reopened.sketch(1).unwrap().patterns[0].instances.len(), 2);
    // A design with plain numbers saves back unchanged and evaluates to the same copies.
    assert_eq!(reopened.to_json(), legacy);
    let before = sketch(&s);
    let mut s = Session { doc: reopened.into(), ..Default::default() };
    s.refresh();
    let state = s.world_state().unwrap();
    let after = &state.sketch(1).unwrap().sketch;
    for (a, b) in before.points.iter().zip(&after.points) {
        assert!(a.id == b.id && a.pos.dist(b.pos) < 1e-9, "{} moved", a.id);
    }
}
