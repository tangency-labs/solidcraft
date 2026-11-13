//! #2750: a negative extrude of the overlap between a circle and a dome outline.
use super::*;

#[test]
fn negative_lens_extrude_cuts_a_partial_sphere_and_reopens() {
    let mut s = Session::default();
    let plane = json!({"origin":[0,0,0],"x_dir":[1,0,0],"y_dir":[0,0,1]});
    run(&mut s, "sketch.create", json!({"name":"DomeProfile","plane":plane}));
    run(&mut s, "sketch.arc.three_point", json!({"start":[25,0],"through":[0,25],"end":[-25,0]}));
    run(&mut s, "sketch.line", json!({"points":[[-25,0],[25,0]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.revolve", json!({"sketch":"DomeProfile","axis":"x","angle":"90 deg","body_name":"Dome"}));
    // The same outline a face-attached sketch projects, and the intersecting circle.
    run(&mut s, "sketch.create", json!({"name":"CutProfile","plane":{"origin":[0,0,0],"x_dir":[-1,0,0],"y_dir":[0,0,1]}}));
    run(&mut s, "sketch.arc.three_point", json!({"start":[25,0],"through":[0,25],"end":[-25,0]}));
    run(&mut s, "sketch.line", json!({"points":[[-25,0],[25,0]]}));
    run(&mut s, "sketch.circle.center", json!({"center":[0,20],"radius":15}));
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.extrude",
        json!({"sketch":"CutProfile","profiles":[{"point":[0,15]}],"direction":"positive","distance":"-30 mm","operation":"cut"}),
    );
    let check = |s: &mut Session| {
        assert!((volume(s) - 8810.08973246).abs() < 2.);
        let state = s.model.state();
        assert_eq!(state.bodies.len(), 1);
        let body = &state.bodies[0].body;
        assert!(body.validity().is_empty(), "{:?}", body.validity());
        assert!(solidcraft_kernel::measure(body).unwrap().bbox.max.z < 20.01, "no lid across the opening");
        assert_eq!(body.classify_point(Vec3::new(0., -10., 2.)).unwrap(), solidcraft_kernel::PointClass::Inside);
        assert_eq!(body.classify_point(Vec3::new(0., -10., 15.)).unwrap(), solidcraft_kernel::PointClass::Outside);
    };
    check(&mut s);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    check(&mut reopened);
}
