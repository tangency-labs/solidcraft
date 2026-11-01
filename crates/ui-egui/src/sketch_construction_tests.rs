use crate::scenario::Harness;
use serde_json::json;
use solidcraft_engine::sketch::CurveKind;

fn sketch() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"ids":["a"],"infer":false})).unwrap();
    h.app.run("sketch.line", json!({"points":[[0,0],[0,20]],"ids":["b"],"infer":false})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[40,0],"radius":5,"id":"c"})).unwrap();
    h.app.run("sketch.circle.center", json!({"center":[60,0],"radius":5,"id":"d"})).unwrap();
    h.settle();
    h
}

fn steps(h: &mut Harness, id: &str, clicks: &[[f64; 2]]) {
    h.step(&json!({"start":id})).unwrap();
    for q in clicks {
        h.step(&json!({"click":{"sketch":q}})).unwrap();
    }
}

#[test]
fn every_construction_variant_is_interactive_and_stays_infinite() {
    let cases: &[(&str, &[[f64; 2]])] = &[
        ("sketch.construction_line", &[[5., 10.], [15., 15.]]),
        ("sketch.construction_line.horizontal", &[[5., 10.]]),
        ("sketch.construction_line.vertical", &[[5., 10.]]),
        ("sketch.construction_line.angle", &[[5., 10.], [15., 20.]]),
        ("sketch.construction_line.parallel", &[[10., 0.], [5., 10.]]),
        ("sketch.construction_line.perpendicular", &[[10., 0.], [5., 10.]]),
        ("sketch.construction_line.tangent_external", &[[40., 5.], [50., 10.], [50., 15.]]),
        ("sketch.construction_line.tangent_at", &[[40., 5.], [45., 0.]]),
        ("sketch.construction_line.tangent_two", &[[40., 5.], [60., 5.], [50., 8.]]),
        ("sketch.construction_line.bisector", &[[10., 0.], [0., 10.], [10., 10.]]),
    ];
    for (id, clicks) in cases {
        let mut h = sketch();
        crate::sketch_palette::set_mode(&mut h.app, 2);
        let sid = h.app.session.active_sketch.unwrap();
        let before = h.app.session.doc.sketch(sid).unwrap().curves.len();
        steps(&mut h, id, clicks);
        let sk = h.app.session.doc.sketch(sid).unwrap();
        assert_eq!(sk.curves.len(), before + 1, "{id}: {:?}", h.app.tool);
        let curve = sk.curves.last().unwrap();
        assert!(curve.infinite && curve.construction && !curve.centerline, "{id}: {curve:?}");
        assert!(matches!(curve.kind, CurveKind::Line { .. }));
    }
}

#[test]
fn world_axes_are_not_mistaken_for_sketch_axes() {
    // The world Y axis is sketch X on YZ. Sending its name as local "y" would
    // silently create a perpendicular reference, so only sketch curves are picked.
    let mut h = Harness::new();
    h.app.run("sketch.create", json!({"plane":"YZ"})).unwrap();
    for id in ["sketch.construction_line.parallel", "sketch.construction_line.perpendicular", "sketch.construction_line.bisector"] {
        let mut tool = crate::sketch_tools::tool_for(id).unwrap();
        super::on_click(&mut h.app, &mut tool, &[crate::viewport::Hit::Axis { name: "Y".into() }], None);
        assert!(tool.picks.is_empty(), "{id} must not reinterpret a world axis as a local axis");
    }
}

#[test]
fn construction_angle_accepts_inline_value_and_undo_is_one_step() {
    let mut h = sketch();
    steps(&mut h, "sketch.construction_line.angle", &[[5., 10.]]);
    h.step(&json!({"move":{"sketch":[15,15]}})).unwrap();
    h.settle();
    let tool = h.app.tool.as_mut().unwrap();
    tool.dims = crate::sketch_dims::boxes_for(tool.cmd, 1);
    let dim = tool.dims.first_mut().unwrap();
    dim.locked = true;
    dim.value = "30 deg".into();
    let mut tool = h.app.tool.take().unwrap();
    super::place_point(&mut h.app, &mut tool, (solidcraft_engine::geom::Vec2::new(15., 15.), None));
    let sid = h.app.session.active_sketch.unwrap();
    let sk = h.app.session.doc.sketch(sid).unwrap();
    let curve = sk.curves.last().unwrap();
    let CurveKind::Line { a, b } = curve.kind else { panic!("line") };
    assert!(((sk.point(b).unwrap() - sk.point(a).unwrap()).angle().to_degrees() - 30.).abs() < 1e-6);
    let count = sk.curves.len();
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.sketch(sid).unwrap().curves.len(), count - 1);
}

#[test]
fn construction_family_menu_and_preference() {
    for enabled in [true, false] {
        let mut h = sketch();
        h.app.preferences.sketch_construction_lines = enabled;
        h.settle();
        h.step(&json!({"click":{"handle":"panel:SKETCH:CREATE"}})).unwrap();
        h.frames(4);
        assert_eq!(crate::scenario::handle_at("create_family:Construction Line").is_some(), enabled);
        if enabled {
            h.step(&json!({"click":{"handle":"create_family:Construction Line"}})).unwrap();
            h.frames(4);
            let family = crate::sketch_families::family("sketch.construction_line").unwrap();
            for id in family.commands {
                assert!(crate::scenario::handle_at(&format!("dropdown:{id}")).is_some(), "{id}");
            }
        }
    }
}
