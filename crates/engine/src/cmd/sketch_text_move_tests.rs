use super::*;

fn run(s: &mut Session, cmd: &str, params: Value) -> Value {
    s.execute(cmd, &params).unwrap()
}

fn setup() -> (Session, String, String) {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let result = run(&mut s, "sketch.text", json!({"text":"II","at":[3,4],"height":10,"angle":15}));
    (s, result["link"].as_str().unwrap().into(), result["curves"][0].as_str().unwrap().into())
}

#[test]
fn text_move_updates_the_source_once_preserves_ids_and_roundtrips() {
    let (mut s, link, curve) = setup();
    let before = s.doc.to_json();
    let ids: Vec<_> = s.doc.sketch(1).unwrap().curves.iter().map(|c| c.id.clone()).collect();
    let undo = s.undo.len();
    run(&mut s, "sketch.move", json!({"entities":[curve,ids[1]],"translate":[10,-2],"center":[3,4],"angle":30}));
    let sk = s.doc.sketch(1).unwrap();
    assert_eq!(sk.curves.iter().map(|c| c.id.clone()).collect::<Vec<_>>(), ids);
    let LinkSource::Text { at, angle, text, height, font, .. } = &sk.link(&link).unwrap().source else { panic!() };
    assert!(at.dist(Vec2::new(13.0, 2.0)) < 1e-9);
    assert!((*angle - 45_f64.to_radians()).abs() < 1e-12);
    assert_eq!((text.as_str(), *height, font), ("II", 10.0, &None));
    assert_eq!(s.undo.len(), undo + 1);
    let moved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&moved).unwrap();
    assert_eq!(reopened.to_json(), moved);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.to_json(), moved);
}

#[test]
fn text_copy_is_a_new_editable_source_and_original_is_unchanged() {
    let (mut s, link, curve) = setup();
    let original = s.doc.sketch(1).unwrap().link(&link).unwrap().source.clone();
    let result = run(&mut s, "sketch.move", json!({"entities":[curve],"translate":[20,0],"copy":true}));
    let copied_curve = result["curves"][0].as_str().unwrap();
    let new_link = text_link(s.doc.sketch(1).unwrap(), copied_curve).unwrap().to_owned();
    assert_ne!(new_link, link);
    run(&mut s, "sketch.edit_text", json!({"link":new_link,"text":"O"}));
    assert_eq!(s.doc.sketch(1).unwrap().link(&link).unwrap().source, original);
}

#[test]
fn text_drag_moves_all_letters_and_rebuilds_downstream_on_release() {
    let (mut s, link, curve) = setup();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":3}));
    run(&mut s, "sketch.edit", json!({"sketch":1}));
    let sk = s.doc.sketch(1).unwrap();
    let before = sk.link_points(&link).into_iter().map(|i| (i, sk.point(i).unwrap())).collect::<Vec<_>>();
    for frame in 0..3 {
        let result = run(&mut s, "sketch.drag", json!({"entity":curve,"from":[frame,0],"to":[frame+1,1],"live":true}));
        assert_eq!(result["reached"], 1.0);
        assert!(s.model.is_stale());
    }
    run(&mut s, "sketch.drag_end", json!({}));
    assert!(!s.model.is_stale());
    assert!(s.model.result(2).is_some_and(|r| r.error.is_none()));
    let sk = s.doc.sketch(1).unwrap();
    for (i, old) in before {
        assert!(sk.point(i).unwrap().dist(old + Vec2::new(3.0, 3.0)) < 1e-8);
    }
    assert!(matches!(sk.link(&link).unwrap().source, LinkSource::Text { .. }));
}

#[test]
fn live_text_resize_follows_each_frame_and_rebuilds_downstream_on_release() {
    let (mut s, link, _) = setup();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance":3}));
    run(&mut s, "sketch.edit", json!({"sketch":1}));
    let curves = s.doc.sketch(1).unwrap().link_curves(&link).len();
    for (frame, height) in [12.0, 16.0, 20.0].into_iter().enumerate() {
        let at = Vec2::new(3.0 - frame as f64, 4.0);
        run(&mut s, "sketch.edit_text", json!({"link":link,"at":[at.x,at.y],"height":height,"angle":-10,"live":true}));
        assert!(s.model.is_stale(), "a live frame leaves the features after the sketch as they were");
        let sk = s.doc.sketch(1).unwrap();
        let LinkSource::Text { at: now, height: h, angle, text, .. } = &sk.link(&link).unwrap().source else { panic!() };
        assert!(now.dist(at) < 1e-12);
        assert_eq!((*h, text.as_str()), (height, "II"));
        assert!((*angle + 10_f64.to_radians()).abs() < 1e-12);
        // Same letters, same outlines: what references them keeps working.
        assert_eq!(sk.link_curves(&link).len(), curves);
        let shown = s.world_state();
        assert!(shown.sketch(1).unwrap().profiles.len() >= 2, "the sketch shown follows the frame");
    }
    run(&mut s, "sketch.drag_end", json!({}));
    assert!(!s.model.is_stale());
    assert!(s.model.result(2).is_some_and(|r| r.error.is_none()));
}

#[test]
fn live_text_edit_rejects_bad_values_and_leaves_the_design_as_it_was() {
    let (mut s, link, _) = setup();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for params in [
        json!({"link":link,"height":0,"live":true}),
        json!({"link":link,"height":-3,"live":true}),
        json!({"link":link,"height":2e5,"live":true}),
        json!({"link":"no-such-link","height":5,"live":true}),
        json!({"height":5,"live":true}),
    ] {
        assert!(s.execute("sketch.edit_text", &params).is_err(), "{params}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

#[test]
fn text_move_in_a_rotated_component_preserves_font_and_local_coordinates() {
    let mut s = Session::default();
    let component = run(&mut s, "component.create", json!({"name":"Label","grounded":false}));
    run(&mut s, "occurrence.move", json!({"occurrence":component["occurrence"],"translate":[30,20,10],"axis":[0,0,1],"angle":90}));
    let created = run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let sid = created["sketch"].as_u64().unwrap();
    let result = run(&mut s, "sketch.text", json!({"text":"I","at":[0,0],"height":10,"font":solidcraft_sketch::DEFAULT_FONT_FAMILY}));
    let link = result["link"].as_str().unwrap();
    let before = s.world_state().sketch(sid).unwrap().plane;
    run(&mut s, "sketch.move", json!({"entities":[result["curves"][0]],"translate":[5,2]}));
    let sk = s.doc.sketch(sid).unwrap();
    let LinkSource::Text { at, font, .. } = &sk.link(link).unwrap().source else { panic!() };
    assert!(at.dist(Vec2::new(5.0, 2.0)) < 1e-9);
    assert_eq!(font.as_deref(), Some(solidcraft_sketch::DEFAULT_FONT_FAMILY));
    let state = s.world_state();
    assert!(state.sketch(sid).unwrap().plane.to_world(*at).dist(before.to_world(Vec2::new(5.0, 2.0))) < 1e-9);
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.doc.sketch(sid).unwrap().link(link).unwrap().source, sk.link(link).unwrap().source);
}

#[test]
fn text_move_rejects_malformed_values_without_changing_history() {
    let (mut s, _, curve) = setup();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for params in [
        json!({"entities":[curve],"translate":[1]}),
        json!({"entities":[curve],"angle":"invalid"}),
        json!({"entity":curve,"from":[0,0],"to":["invalid",2]}),
    ] {
        let cmd = if params.get("entity").is_some() { "sketch.drag" } else { "sketch.move" };
        assert!(s.execute(cmd, &params).is_err());
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.undo.len(), undo);
    }
}

/// The box around the solved outlines of text `link` in sketch 1: (min, max).
fn text_box(s: &Session, link: &str) -> (Vec2, Vec2) {
    let state = s.world_state();
    let sk = &state.sketch(1).unwrap().sketch;
    let pts: Vec<Vec2> = sk.link_curves(link).into_iter().flat_map(|ci| sk.polyline(ci)).collect();
    let lo = pts.iter().fold(Vec2::new(f64::MAX, f64::MAX), |m, p| Vec2::new(m.x.min(p.x), m.y.min(p.y)));
    let hi = pts.iter().fold(Vec2::new(f64::MIN, f64::MIN), |m, p| Vec2::new(m.x.max(p.x), m.y.max(p.y)));
    (lo, hi)
}

#[test]
fn text_on_a_path_follows_its_curve_when_the_curve_moves() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    // (Away from the origin, which would hold a line's end.)
    let line = run(&mut s, "sketch.line", json!({"points":[[0,-30],[80,-30]]}))["curves"][0].as_str().unwrap().to_string();
    let t = run(&mut s, "sketch.text", json!({"text":"HH","at":[5,5],"height":10,"angle":30,"path":line,"align":"center"}));
    let link = t["link"].as_str().unwrap().to_string();
    // Centred on the line's middle, standing on it; `at` and the angle do not place it.
    let (lo, hi) = text_box(&s, &link);
    assert!(((lo.x + hi.x) / 2.0 - 40.0).abs() < 0.05 && (lo.y + 30.0).abs() < 1e-6 && (hi.y + 20.0).abs() < 0.05, "{lo:?} {hi:?}");
    // The line moves up 20 mm: the text with it.
    run(&mut s, "sketch.move", json!({"entities":[line],"translate":[0,20]}));
    let (lo, hi) = text_box(&s, &link);
    assert!((lo.y + 10.0).abs() < 1e-6 && ((lo.x + hi.x) / 2.0 - 40.0).abs() < 0.05, "{lo:?} {hi:?}");
    // It is moved with its curve, not on its own.
    let outline = s.doc.sketch(1).unwrap().curves.iter().find(|c| c.link.as_deref() == Some(link.as_str())).unwrap().id.clone();
    let e = s.execute("sketch.move", &json!({"entities":[outline],"translate":[5,0]})).unwrap_err().to_string();
    assert!(e.contains("follows its curve"), "{e}");
    // Off the path again: at `at`, turned by its angle.
    run(&mut s, "sketch.edit_text", json!({"link":link,"path":"","align":"left"}));
    let (lo, _) = text_box(&s, &link);
    assert!(lo.x > 0.0 && lo.y > 4.0 && lo.y < 6.0, "{lo:?}");
    // A design with styled text reads back the same.
    let saved = s.doc.to_json();
    assert_eq!(solidcraft_doc::Document::from_json(&saved).unwrap().to_json(), saved);
}

/// A path placed by a parameter: the text on it follows in the same evaluation. (Texts are laid
/// out with the sketch's links, before the solve that moves the path: the text came one
/// evaluation late, where the path had been before the change.)
#[test]
fn text_on_a_path_follows_the_path_when_a_parameter_moves_it() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"W","expression":"20 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let line = run(&mut s, "sketch.line", json!({"points":[[20,15],[80,15]],"construction":true}))["curves"][0].as_str().unwrap().to_string();
    let start = format!("{line}.start");
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"line":line,"mode":"horizontal"}));
    run(&mut s, "sketch.dimension", json!({"entities":[start,"origin"],"type":"horizontal","value":"W"}));
    run(&mut s, "sketch.dimension", json!({"entities":[start,"origin"],"type":"vertical","value":"15 mm"}));
    run(&mut s, "sketch.dimension", json!({"entities":[line],"type":"length","value":"60 mm"}));
    let link =
        run(&mut s, "sketch.text", json!({"text":"AB-12","at":[0,0],"height":8,"path":line,"align":"center"}))["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.finish", json!({}));
    let middle = |s: &Session| {
        let (lo, hi) = text_box(s, &link);
        (lo.x + hi.x) / 2.0
    };
    let drawn = middle(&s);
    assert!((drawn - 50.0).abs() < 1.0, "{drawn}");
    for w in [300.0, 20.0, 140.0] {
        run(&mut s, "parameters.change", json!({"name":"W","expression":format!("{w} mm")}));
        let moved = middle(&s) - drawn;
        assert!((moved - (w - 20.0)).abs() < 1e-6, "W {w}: the text moved {moved}");
    }
}

#[test]
fn text_style_parameters_are_checked() {
    let (mut s, link, _) = setup();
    for (params, error) in [
        (json!({"link":link,"bold":true}), "no bold face"),
        (json!({"link":link,"bold":"yes"}), "`bold` must be true or false"),
        (json!({"link":link,"align":"justify"}), "`align` must be left, center or right"),
        (json!({"link":link,"line_spacing":0}), "`line_spacing` must be a positive factor"),
        (json!({"link":link,"letter_spacing":"wide"}), "`letter_spacing` must be a length"),
        (json!({"link":link,"path":"no-such-curve"}), "not a curve of this sketch"),
    ] {
        let before = s.doc.to_json();
        let e = s.execute("sketch.edit_text", &params).unwrap_err().to_string();
        assert!(e.contains(error), "{params}: {e}");
        assert_eq!(s.doc.to_json(), before);
    }
    run(&mut s, "sketch.edit_text", json!({"link":link,"italic":true,"align":"right","letter_spacing":1.5,"line_spacing":1.2}));
    let LinkSource::Text { style, text, .. } = &s.doc.sketch(1).unwrap().link(&link).unwrap().source else { panic!() };
    assert_eq!(text, "II");
    assert!(style.italic && style.align == solidcraft_sketch::TextAlign::Right && style.letter_spacing == 1.5 && style.line_spacing == 1.2);
    // Left out, a style stays.
    run(&mut s, "sketch.edit_text", json!({"link":link,"text":"I"}));
    let LinkSource::Text { style, .. } = &s.doc.sketch(1).unwrap().link(&link).unwrap().source else { panic!() };
    assert!(style.italic && style.letter_spacing == 1.5);
}

/// `base` with the keys of `extra` set (a null `extra` value is sent as null).
fn with(mut base: Value, extra: &Value) -> Value {
    if let (Some(base), Some(extra)) = (base.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

const AT: &str = "`at` must be the baseline start [x, y]";
const HEIGHT: &str = "`height` must be a length in mm";
const ANGLE: &str = "`angle` must be an angle in degrees";
const TEXT: &str = "`text` must be a string";

/// Inputs of `sketch.text` and `sketch.edit_text` that were once read as "not given": a height of
/// "H" drew the text 5 mm high, `at: ["H", 25]` put it at the origin. Each with its message.
fn not_understood() -> Vec<(Value, &'static str)> {
    vec![
        // A string is an expression now: one naming no parameter is refused as such.
        (json!({"height":"H"}), "unknown parameter `H`"),
        (json!({"height":true}), HEIGHT),
        (json!({"height":[10]}), HEIGHT),
        (json!({"height":1e10}), HEIGHT),
        (json!({"height":0}), "`height` must be positive"),
        (json!({"height":-3}), "`height` must be positive"),
        (json!({"angle":"A"}), "unknown parameter `A`"),
        (json!({"angle":[15]}), ANGLE),
        (json!({"angle":1e10}), ANGLE),
        (json!({"at":["H",25]}), "unknown parameter `H`"),
        (json!({"at":[1]}), AT),
        (json!({"at":[1,2,3]}), AT),
        (json!({"at":[1,null]}), AT),
        (json!({"at":[1e10,0]}), AT),
        (json!({"at":"origin"}), AT),
        (json!({"at":7}), AT),
        (json!({"text":5}), TEXT),
        (json!({"text":true}), TEXT),
        (json!({"text":["I"]}), TEXT),
    ]
}

#[test]
fn sketch_text_refuses_inputs_it_does_not_understand() {
    let (mut s, _, _) = setup();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    let base = json!({"text":"I","at":[0,0],"height":10});
    let mut cases = not_understood();
    // `at` is required: left out or null is refused too (null no longer means the origin).
    cases.push((json!({"at":null}), AT));
    for (extra, error) in cases {
        let params = with(base.clone(), &extra);
        let e = s.execute("sketch.text", &params).unwrap_err().to_string();
        assert!(e.contains(error), "{params}: {e}");
        assert_eq!(s.doc.to_json(), before, "{params}");
        assert_eq!(s.undo.len(), undo, "{params}");
    }
    let e = s.execute("sketch.text", &json!({"text":"I","height":10})).unwrap_err().to_string();
    assert!(e.contains(AT), "{e}");
    // No text at all is still the empty-text error.
    let e = s.execute("sketch.text", &json!({"at":[0,0]})).unwrap_err().to_string();
    assert!(e.contains("`text` must not be empty"), "{e}");
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn sketch_edit_text_refuses_inputs_it_does_not_understand() {
    let (mut s, link, _) = setup();
    let before = s.doc.to_json();
    let undo = s.undo.len();
    for (extra, error) in not_understood() {
        let params = with(json!({"link":link}), &extra);
        for live in [false, true] {
            let params = with(params.clone(), &json!({"live":live}));
            let e = s.execute("sketch.edit_text", &params).unwrap_err().to_string();
            assert!(e.contains(error), "{params}: {e}");
            assert_eq!(s.doc.to_json(), before, "{params}");
            assert_eq!(s.undo.len(), undo, "{params}");
        }
    }
}

fn text_source_of(s: &Session, link: &str) -> (String, Vec2, f64, f64) {
    let LinkSource::Text { text, at, height, angle, .. } = &s.doc.sketch(1).unwrap().link(link).unwrap().source else { panic!() };
    (text.clone(), *at, *height, *angle)
}

#[test]
fn sketch_text_inputs_left_out_or_null_keep_their_defaults() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    // Optional inputs left out, or null: the defaults (5 mm, no angle). `at` as {x, y} is fine.
    for (params, at) in [
        (json!({"text":"I","at":[4,6]}), Vec2::new(4.0, 6.0)),
        (json!({"text":"I","at":{"x":4,"y":6},"height":null,"angle":null,"font":null}), Vec2::new(4.0, 6.0)),
    ] {
        let t = run(&mut s, "sketch.text", params);
        let (text, got, height, angle) = text_source_of(&s, t["link"].as_str().unwrap());
        assert_eq!((text.as_str(), height, angle), ("I", 5.0, 0.0));
        assert!(got.dist(at) < 1e-12, "{got:?}");
    }
    // Given values are used, an angle of 0 and a negative position among them.
    let t = run(&mut s, "sketch.text", json!({"text":"I","at":[-4.5,6],"height":12.5,"angle":0}));
    let (_, at, height, angle) = text_source_of(&s, t["link"].as_str().unwrap());
    assert!(at.dist(Vec2::new(-4.5, 6.0)) < 1e-12 && height == 12.5 && angle == 0.0, "{at:?} {height} {angle}");
    let t = run(&mut s, "sketch.text", json!({"text":"I","at":[0,0],"angle":90}));
    assert!((text_source_of(&s, t["link"].as_str().unwrap()).3 - std::f64::consts::FRAC_PI_2).abs() < 1e-12);

    // An edit keeps what it leaves out, or sends as null, and changes what it gives.
    let (mut s, link, _) = setup();
    let old = text_source_of(&s, &link);
    run(&mut s, "sketch.edit_text", json!({"link":link,"text":null,"at":null,"height":null,"angle":null}));
    assert_eq!(text_source_of(&s, &link), old);
    run(&mut s, "sketch.edit_text", json!({"link":link,"height":20,"angle":0}));
    let (text, at, height, angle) = text_source_of(&s, &link);
    assert_eq!((text.as_str(), height, angle), ("II", 20.0, 0.0));
    assert!(at.dist(Vec2::new(3.0, 4.0)) < 1e-12, "{at:?}");
}

/// A text set on a spline stands on it, its descenders below it: the spline becomes
/// construction, so it doesn't cut the letters into pieces. Every profile is then the text's
/// alone (picking the text picks letters, not pieces of letters and curve), and one undo puts
/// the text and the curve back as they were.
#[test]
fn a_text_on_a_spline_turns_its_path_into_a_guide() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY"}));
    let spline = run(&mut s, "sketch.spline.fit_point", json!({"points":[[0,0],[30,15],[60,0],[90,10]]}))["created"]["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    let construction = |s: &Session| {
        let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
        sk.curves.iter().find(|c| c.id == spline).unwrap().construction
    };
    let t = run(&mut s, "sketch.text", json!({"text":"Phago","at":[0,0],"height":10,"path":spline}));
    assert_eq!(t["guide"], json!(spline));
    assert!(construction(&s));
    let state = s.world_state();
    let ss = state.sketch(s.active_sketch.unwrap()).unwrap();
    for (i, p) in ss.profiles.iter().enumerate() {
        let alone = p.curve_ids().iter().all(|id| ss.sketch.curves.iter().any(|c| &c.id == id && c.link.is_some()));
        assert!(alone, "profile {i} is bounded by the spline too");
    }
    // Taken off the curve, the text leaves it a guide; set on it again, nothing changes.
    let link = t["link"].as_str().unwrap().to_string();
    run(&mut s, "sketch.edit_text", json!({"link":link,"path":""}));
    assert!(construction(&s));
    assert_eq!(run(&mut s, "sketch.edit_text", json!({"link":link,"path":spline}))["guide"], json!(null));
    // One undo for each step, back to the text's first state, then to before it.
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert!(!construction(&s), "undoing the text gives the curve back");
}
