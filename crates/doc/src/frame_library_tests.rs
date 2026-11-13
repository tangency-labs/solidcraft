//! Profile library tests. Placement points are checked against positions worked out by hand from
//! each profile's dimensions, its bounding box centred on the origin.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn profile(json: serde_json::Value) -> Profile {
    serde_json::from_value(json).unwrap()
}

fn spec(json: serde_json::Value) -> ProfileSpec {
    serde_json::from_value(json).unwrap()
}

fn at(points: &[(&'static str, Vec2)], name: &str) -> Vec2 {
    points.iter().find(|(n, _)| *n == name).unwrap_or_else(|| panic!("no point {name}")).1
}

fn near(got: Vec2, want: (f64, f64), what: &str) {
    assert!((got.x - want.0).abs() <= 1e-12 && (got.y - want.1).abs() <= 1e-12, "{what}: got ({}, {}), want {want:?}", got.x, got.y);
}

fn entry(section: LibrarySection) -> LibraryProfile {
    LibraryProfile {
        id: "lp1".into(),
        name: "SHS 40x40x3".into(),
        category: vec!["Steel".into(), "SHS".into()],
        section,
        points: Vec::new(),
        default_point: None,
        material: None,
        description: None,
        stock_length: None,
    }
}

fn mm(_: &str, e: &Expr, _: crate::expr::Kind) -> Result<f64> {
    e.trim().parse::<f64>().map_err(|_| DocError::Invalid(format!("`{e}` is not a number")))
}

#[test]
fn an_angles_heel_and_toes_sit_where_its_legs_put_them() {
    // 50 × 30 × 5: the heel at (−25, −15); the inside faces meet 5 mm in from both outer faces.
    let p = generated_points(&profile(serde_json::json!({"kind": "angle", "width": 50, "height": 30, "thickness": 5})));
    near(at(&p, "heel"), (-25.0, -15.0), "heel");
    near(at(&p, "heel_inside"), (-20.0, -10.0), "heel_inside");
    near(at(&p, "toe_x"), (25.0, -12.5), "toe_x: the x leg's tip, mid-thickness");
    near(at(&p, "toe_y"), (-22.5, 15.0), "toe_y: the y leg's tip, mid-thickness");
    // The inside faces' middles: x from 5 to 50 of the heel's frame, y from 5 to 30.
    near(at(&p, "leg_x_inside"), (27.5 - 25.0, -10.0), "leg_x_inside");
    near(at(&p, "leg_y_inside"), (-20.0, 17.5 - 15.0), "leg_y_inside");
}

#[test]
fn a_channels_web_and_flange_points_follow_its_dimensions() {
    // 100 high, flanges 50 long and 8 thick, web 6 thick, open towards +x.
    let p = generated_points(&profile(serde_json::json!({"kind": "channel", "height": 100, "width": 50, "web": 6, "flange": 8})));
    near(at(&p, "web_back"), (-25.0, 0.0), "web_back");
    near(at(&p, "web_inside"), (-19.0, 0.0), "web_inside");
    near(at(&p, "web_inside_top"), (-19.0, 42.0), "web_inside_top");
    near(at(&p, "web_inside_bottom"), (-19.0, -42.0), "web_inside_bottom");
    near(at(&p, "web_mid"), (-22.0, 0.0), "web_mid");
    near(at(&p, "flange_tip_top"), (25.0, 46.0), "flange_tip_top");
    near(at(&p, "flange_tip_bottom"), (25.0, -46.0), "flange_tip_bottom");
}

#[test]
fn an_i_beams_flange_and_web_faces_follow_its_dimensions() {
    let p = generated_points(&profile(serde_json::json!({"kind": "i_beam", "height": 200, "width": 100, "web": 6, "flange": 10})));
    near(at(&p, "flange_top_inside"), (0.0, 90.0), "flange_top_inside");
    near(at(&p, "flange_bottom_inside"), (0.0, -90.0), "flange_bottom_inside");
    near(at(&p, "flange_top_mid"), (0.0, 95.0), "flange_top_mid");
    near(at(&p, "web_left"), (-3.0, 0.0), "web_left");
    near(at(&p, "web_right"), (3.0, 0.0), "web_right");
    near(at(&p, "flange_tip_bottom_right"), (50.0, -95.0), "flange_tip_bottom_right");
}

#[test]
fn tubes_name_their_inside_faces_and_mid_walls() {
    let p = generated_points(&profile(serde_json::json!({"kind": "rect_tube", "width": 60, "height": 40, "wall": 3})));
    near(at(&p, "inner_top_right"), (27.0, 17.0), "inner_top_right");
    near(at(&p, "inner_bottom_left"), (-27.0, -17.0), "inner_bottom_left");
    near(at(&p, "inner_left"), (-27.0, 0.0), "inner_left");
    near(at(&p, "wall_top"), (0.0, 18.5), "wall_top");
    near(at(&p, "wall_right"), (28.5, 0.0), "wall_right");
    let p = generated_points(&profile(serde_json::json!({"kind": "round_tube", "diameter": 60, "wall": 4})));
    near(at(&p, "inner_top"), (0.0, 26.0), "inner_top");
    near(at(&p, "wall_left"), (-28.0, 0.0), "wall_left");
    assert!(generated_points(&profile(serde_json::json!({"kind": "bar", "width": 10, "height": 5}))).is_empty());
}

#[test]
fn every_kinds_points_lie_in_its_bounding_box_with_names_of_their_own() {
    for k in frame::profile_kinds() {
        let (w, h) = k.example.size();
        let region = k.example.region().unwrap();
        let all = placement_points(std::slice::from_ref(&region), Some(&k.example), &[]);
        let names: std::collections::HashSet<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names.len(), all.len(), "{}: every point has its own name", k.kind);
        for (name, p) in &all {
            assert!(p.x.abs() <= w / 2.0 + 1e-9 && p.y.abs() <= h / 2.0 + 1e-9, "{} {name} lies in the bounding box", k.kind);
        }
        // The common points are the anchors, in order.
        for (name, anchor) in COMMON_POINTS.iter().zip(ANCHORS) {
            let want = frame::anchor_point(std::slice::from_ref(&region), *anchor);
            let got = find_point(&all, name).unwrap();
            assert_eq!((got.x, got.y), (want.x, want.y), "{} {name}", k.kind);
        }
    }
}

#[test]
fn a_built_in_entry_resolves_its_dimensions_and_adds_its_named_points() {
    let mut e = entry(LibrarySection::BuiltIn { profile: spec(serde_json::json!({"kind": "rect_tube", "width": 40, "height": 40, "wall": 3})) });
    e.points.push(NamedPoint { name: "bolt_line".into(), at: [0.0, 10.0] });
    e.check().unwrap();
    let r = e.resolve(mm).unwrap();
    assert_eq!(r.regions.len(), 1);
    assert_eq!(r.builtin.as_ref().map(Profile::kind), Some("rect_tube"));
    near(find_point(&r.points, "inner_top_left").unwrap(), (-17.0, 17.0), "a generated point");
    near(find_point(&r.points, "bolt_line").unwrap(), (0.0, 10.0), "a named point");
    near(find_point(&r.points, "BOLT_LINE").unwrap(), (0.0, 10.0), "names match ignoring case");
    let err = find_point(&r.points, "nowhere").unwrap_err().to_string();
    assert!(err.contains("no placement point `nowhere`") && err.contains("bolt_line"), "{err}");
    let names: Vec<&str> = e.exprs().into_iter().map(|(label, _)| label).collect();
    assert_eq!(names, ["Height", "Wall", "Width"]);
}

#[test]
fn a_drawn_entry_keeps_its_regions_and_has_the_common_points() {
    let square = profile(serde_json::json!({"kind": "bar", "width": 20, "height": 10})).region().unwrap();
    let e = entry(LibrarySection::Drawn { regions: vec![square] });
    e.check().unwrap();
    let r = e.resolve(mm).unwrap();
    assert!(r.builtin.is_none());
    near(find_point(&r.points, "top_right").unwrap(), (10.0, 5.0), "top_right");
    assert_eq!(r.points.len(), COMMON_POINTS.len());
}

#[test]
fn entries_refuse_bad_names_points_and_sections() {
    let base = entry(LibrarySection::BuiltIn { profile: spec(serde_json::json!({"kind": "round_bar", "diameter": 10})) });
    let refused = |e: LibraryProfile, want: &str| {
        let err = e.check().unwrap_err().to_string();
        assert!(err.contains(want), "{err} (wanted {want})");
    };
    let mut e = base.clone();
    e.name = " padded ".into();
    refused(e, "without spaces");
    let mut e = base.clone();
    e.name = "x".repeat(MAX_LIBRARY_NAME + 1);
    refused(e, "1…96 bytes");
    let mut e = base.clone();
    e.category = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    refused(e, "category levels");
    let mut e = base.clone();
    e.points = vec![NamedPoint { name: "top".into(), at: [0.0, 0.0] }];
    refused(e, "every profile has already");
    let mut e = base.clone();
    e.points = vec![NamedPoint { name: "p".into(), at: [0.0, 0.0] }, NamedPoint { name: "P".into(), at: [1.0, 0.0] }];
    refused(e, "named twice");
    let mut e = base.clone();
    e.points = vec![NamedPoint { name: "has space".into(), at: [0.0, 0.0] }];
    refused(e, "letters, digits");
    let mut e = base.clone();
    e.points = vec![NamedPoint { name: "far".into(), at: [f64::NAN, 0.0] }];
    refused(e, "within");
    let mut e = base.clone();
    e.material = Some("   ".into());
    refused(e, "`material`");
    refused(entry(LibrarySection::Drawn { regions: Vec::new() }), "1…64 regions");
    let mut huge = profile(serde_json::json!({"kind": "bar", "width": 20, "height": 10})).region().unwrap();
    if let Some(Seg2::Line { a, .. }) = huge.outer.segs.first_mut() {
        a.x = f64::INFINITY;
    }
    refused(entry(LibrarySection::Drawn { regions: vec![huge] }), "within");
}

#[test]
fn entries_save_and_read_back_and_ids_are_never_reused() {
    let mut e = entry(LibrarySection::BuiltIn { profile: spec(serde_json::json!({"kind": "angle", "width": 50, "height": 50, "thickness": "t"})) });
    e.default_point = Some("heel_inside".into());
    e.stock_length = Some(Expr::new("6 m"));
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(json["section"]["source"], "built_in");
    assert_eq!(json["section"]["profile"]["thickness"], "t");
    let back: LibraryProfile = serde_json::from_value(json).unwrap();
    assert_eq!(back, e);
    assert_eq!(back.references(), ["t"]);
    let mut lib = vec![e.clone()];
    assert_eq!(next_id(&lib), "lp2");
    lib[0].id = "lp7".into();
    assert_eq!(next_id(&lib), "lp8");
    assert_eq!(find(&lib, "shs 40X40x3").map(|p| p.id.as_str()), Some("lp7"));
    assert_eq!(find(&lib, "lp7").map(|p| p.name.as_str()), Some("SHS 40x40x3"));
}
