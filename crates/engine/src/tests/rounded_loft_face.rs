//! A sketch on an end face of a loft whose side edges are rounded (#8284). The rounds' fitted edges
//! end a few 1e-5 mm short of the face's other edges, and where a round dies away at a corner the
//! mesh keeps an edge from a point to itself; the face's outline still closes into one profile.

use solidcore_record::Vec3;

use super::*;

/// A 40 mm square at z = 0 lofted to a circle of radius 25 at z = 40, its four side edges rounded.
fn rounded_loft(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [-20, -20], "p1": [20, 20]}));
    run(s, "sketch.finish", json!({}));
    run(s, "sketch.create", json!({"plane": "XY", "offset": 40, "name": "Circle"}));
    run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 25}));
    run(s, "sketch.finish", json!({}));
    let section = |name: &str| json!({"sketch": name, "profiles": [{"point": [0, 0]}]});
    run(s, "solid.loft", json!({"sections": [section("Square"), section("Circle")], "operation": "new", "body_name": "Loft"}));
    // Each side edge runs from a corner of the square to the circle; pick it halfway up.
    let mid = (20.0 + 25.0 * std::f64::consts::FRAC_1_SQRT_2) / 2.0;
    let edges: Vec<Value> = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)].iter().map(|(x, y)| json!([x * mid, y * mid, 20])).collect();
    run(s, "solid.fillet", json!({"edges": edges, "radius": 3}));
}

#[test]
fn a_sketch_on_either_end_of_a_rounded_loft_takes_the_whole_face() {
    let mut s = Session::default();
    rounded_loft(&mut s);
    for up in [-1.0, 1.0] {
        let world = s.world_state().unwrap();
        let loft = world.body("Loft").unwrap();
        let face = loft.body.faces(0.01).unwrap().into_iter().find(|f| f.plane_normal.is_some_and(|n| n.dot(Vec3::Z * up) > 0.999)).unwrap();
        let pick = json!({"body": "Loft", "index": face.index, "face": [face.centroid.x, face.centroid.y, face.centroid.z]});
        let id = run(&mut s, "sketch.create", json!({"plane": pick}))["sketch"].as_u64().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        let mut s2 = Session::new((*s.doc).clone());
        run(&mut s2, "solid.extrude", json!({"sketch": id, "distance": 5, "operation": "new", "body_name": "Pad"}));
        let state = s2.model.state();
        let pad = state.bodies.iter().find(|b| b.name == "Pad").unwrap();
        let v = pad.body.mesh(&crate::kernel::MeshTolerance::chord(1e-2)).unwrap().measure().volume;
        // The projected outline fits the round with splines, so it is close to the face, not exact.
        assert!(rel(v, face.area * 5.0) < 1e-2, "end {up}: {v} vs {}", face.area * 5.0);
    }
}

/// #17016: the report's cyclic C2 fit splines have twelve loft wall faces. A 1 mm
/// round at the cylinder's foot crosses the narrow top face's two neighbouring joints.
/// Replaying the commands keeps the actual spline solve, loft and geometry pick in scope.
#[test]
fn a_cylinder_foot_round_on_the_reported_six_section_loft_builds() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":-50,"name":"Sec0"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[2.070552,15.170371],[5.656854,16.464466],[7.727407,18.705905],[7.727407,21.294095],[5.656854,23.535534],[2.070552,24.829629],[-2.070552,24.829629],[-5.656854,23.535534],[-7.727407,21.294095],[-7.727407,18.705905],[-5.656854,16.464466],[-2.070552,15.170371]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":-30,"name":"Sec1"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[4.753197,10.144903],[13.641449,12.421417],[17.739175,17.359335],[17.739175,22.640665],[13.641449,27.578583],[4.753197,29.855097],[-4.753197,29.855097],[-13.641449,27.578583],[-17.739175,22.640665],[-17.739175,17.359335],[-13.641449,12.421417],[-4.753197,10.144903]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":-10,"name":"Sec2"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[6.390064,6.088642],[19.048813,8.888193],[23.848042,16.272463],[23.848042,23.727537],[19.048813,31.111807],[6.390064,33.911358],[-6.390064,33.911358],[-19.048813,31.111807],[-23.848042,23.727537],[-23.848042,16.272463],[-19.048813,8.888193],[-6.390064,6.088642]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":10,"name":"Sec3"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[6.537189,5.361726],[18.243501,9.053899],[24.397123,16.077686],[24.397123,23.922314],[18.243501,30.946101],[6.537189,34.638274],[-6.537189,34.638274],[-18.243501,30.946101],[-24.397123,23.922314],[-24.397123,16.077686],[-18.243501,9.053899],[-6.537189,5.361726]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":30,"name":"Sec4"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[5.311867,9.096713],[15.614184,11.412199],[19.824158,17.078473],[19.824158,22.921527],[15.614184,28.587801],[5.311867,30.903287],[-5.311867,30.903287],[-15.614184,28.587801],[-19.824158,22.921527],[-19.824158,17.078473],[-15.614184,11.412199],[-5.311867,9.096713]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.create", json!({"plane":"YZ","offset":50,"name":"Sec5"}));
    run(
        &mut s,
        "sketch.spline.fit_point",
        json!({"points":[[2.329371,14.204445],[6.363961,15.757359],[8.693332,18.447086],[8.693332,21.552914],[6.363961,24.242641],[2.329371,25.795555],[-2.329371,25.795555],[-6.363961,24.242641],[-8.693332,21.552914],[-8.693332,18.447086],[-6.363961,15.757359],[-2.329371,14.204445]],"periodic":true}),
    );
    run(&mut s, "sketch.finish", json!({}));
    run(
        &mut s,
        "solid.loft",
        json!({"sections":[{"sketch":"Sec0"},{"sketch":"Sec1"},{"sketch":"Sec2"},{"sketch":"Sec3"},{"sketch":"Sec4"},{"sketch":"Sec5"}],"name":"Loft1","body_name":"LoftBody","operation":"new"}),
    );
    run(&mut s, "solid.cylinder", json!({"base":[0,0,25],"axis":[0,0,1],"radius":6,"height":14,"operation":"join","targets":["LoftBody"]}));
    let before = volume(&mut s);
    let state = s.model.state();
    let body = &state.body("LoftBody").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    assert_eq!(body.face_count(), 16, "the reported loft and boss must retain twelve wall faces");
    drop(state);
    run(&mut s, "solid.fillet", json!({"edges": [[6, 0, 33]], "radius": 1, "name": "Foot", "body": "LoftBody"}));
    let state = s.model.state();
    assert_eq!(state.bodies.len(), 1);
    let body = &state.body("LoftBody").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    assert!(body.face_count() > 16, "the round must add its own faces");
    // A concave foot round fills the gap between its two supports.
    assert!(volume(&mut s) > before);
    let bytes = crate::io::write_design_container(&s.doc).unwrap();
    let reopened = Session::new(crate::io::read_design(&bytes).unwrap());
    let state = reopened.model.state();
    let body = &state.body("LoftBody").unwrap().body;
    assert!(body.validity().is_empty(), "{:?}", body.validity());
}
